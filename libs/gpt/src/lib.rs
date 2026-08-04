//! Reading a GUID partition table.
//!
//! The point is not the format; it is the habit. A kernel that writes its
//! journal at "sector 0 of the first disk" works until the day it boots from
//! that disk and eats its own boot sector — which is exactly what happened
//! here once already. Asking the disk where things are is one page of code
//! and removes a whole class of that mistake.
//!
//! Both headers are checked: a disk whose primary table is damaged still
//! carries a backup at the far end, and the reason GPT keeps one is that
//! disks do get damaged.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

use i3ml_journal::Sectors;

pub const SECTOR_BYTES: usize = 512;
const SIGNATURE: &[u8; 8] = b"EFI PART";
const HEADER_BYTES: usize = 92;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Partition {
    pub type_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
}

impl Partition {
    pub fn sector_count(&self) -> u64 {
        self.last_lba.saturating_sub(self.first_lba) + 1
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    /// No GPT here — an unpartitioned disk, or a different scheme.
    NoTable,
    /// The header's own checksum, or the partition array's, did not match.
    Corrupt,
    Device,
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_u64(bytes: &[u8], at: usize) -> u64 {
    let mut value = 0u64;
    for index in 0..8 {
        value |= (bytes[at + index] as u64) << (index * 8);
    }
    value
}

struct Header {
    entries_lba: u64,
    entry_count: u32,
    entry_size: u32,
    entries_crc: u32,
}

fn parse_header(sector: &[u8]) -> Option<Header> {
    if sector.len() < HEADER_BYTES || &sector[0..8] != SIGNATURE {
        return None;
    }
    let declared = read_u32(sector, 16);
    let size = read_u32(sector, 12) as usize;
    if size < HEADER_BYTES || size > sector.len() {
        return None;
    }
    // The checksum is computed with its own field zeroed.
    let mut copy = [0u8; SECTOR_BYTES];
    copy[..size].copy_from_slice(&sector[..size]);
    copy[16..20].fill(0);
    if crc32(&copy[..size]) != declared {
        return None;
    }
    let entry_size = read_u32(sector, 84);
    let entry_count = read_u32(sector, 80);
    // Sanity: an entry array bigger than this is a lie or a corruption, and
    // either way should not size a loop.
    if entry_size < 128 || entry_size > 4096 || entry_count > 512 {
        return None;
    }
    Some(Header {
        entries_lba: read_u64(sector, 72),
        entry_count,
        entry_size,
        entries_crc: read_u32(sector, 88),
    })
}

/// Find the first partition of a given type. Tries the primary table, then
/// the backup.
pub fn find(storage: &mut dyn Sectors, type_guid: &[u8; 16]) -> Result<Partition, Fault> {
    let mut sector = [0u8; SECTOR_BYTES];
    let total = storage.sector_count();

    let mut header = None;
    if storage.read(1, &mut sector) {
        header = parse_header(&sector);
    }
    let mut from_backup = false;
    if header.is_none() && total > 1 {
        if storage.read(total - 1, &mut sector) {
            header = parse_header(&sector);
            from_backup = header.is_some();
        }
    }
    let Some(header) = header else {
        return Err(Fault::NoTable);
    };
    let _ = from_backup;

    // Verify the whole entry array before reading a single partition out of
    // it: a table that does not check out is not a map.
    let entries_bytes = header.entry_count as usize * header.entry_size as usize;
    let entry_sectors = entries_bytes.div_ceil(SECTOR_BYTES);
    let mut checksum = Crc32::new();
    for index in 0..entry_sectors {
        if !storage.read(header.entries_lba + index as u64, &mut sector) {
            return Err(Fault::Device);
        }
        let remaining = entries_bytes - index * SECTOR_BYTES;
        checksum.update(&sector[..remaining.min(SECTOR_BYTES)]);
    }
    if checksum.finish() != header.entries_crc {
        return Err(Fault::Corrupt);
    }

    for index in 0..header.entry_count as usize {
        let byte = index * header.entry_size as usize;
        let lba = header.entries_lba + (byte / SECTOR_BYTES) as u64;
        let within = byte % SECTOR_BYTES;
        if !storage.read(lba, &mut sector) {
            return Err(Fault::Device);
        }
        let entry = &sector[within..within + 128];
        if &entry[0..16] == type_guid {
            let first_lba = read_u64(entry, 32);
            let last_lba = read_u64(entry, 40);
            if last_lba < first_lba || last_lba >= total {
                return Err(Fault::Corrupt);
            }
            let mut guid = [0u8; 16];
            guid.copy_from_slice(&entry[0..16]);
            return Ok(Partition { type_guid: guid, first_lba, last_lba });
        }
    }
    Err(Fault::NoTable)
}

/// Streaming CRC-32, so a large entry array can be checked a sector at a time
/// rather than buffered whole in a kernel with a small heap.
struct Crc32 {
    value: u32,
}

impl Crc32 {
    fn new() -> Crc32 {
        Crc32 { value: 0xffff_ffff }
    }

    fn update(&mut self, data: &[u8]) {
        for byte in data {
            self.value ^= *byte as u32;
            for _ in 0..8 {
                let mask = (self.value & 1).wrapping_neg();
                self.value = (self.value >> 1) ^ (0xedb8_8320 & mask);
            }
        }
    }

    fn finish(self) -> u32 {
        !self.value
    }
}

/// A window onto part of a disk. The journal writes sectors 0..n of *this*,
/// which are sectors `first_lba..` of the device — so the format never needs
/// to know it is living in a partition, and can be tested without one.
pub struct Window<'a> {
    storage: &'a mut dyn Sectors,
    first: u64,
    count: u64,
}

impl<'a> Window<'a> {
    pub fn new(storage: &'a mut dyn Sectors, partition: &Partition) -> Window<'a> {
        Window { storage, first: partition.first_lba, count: partition.sector_count() }
    }
}

impl Sectors for Window<'_> {
    fn sector_count(&self) -> u64 {
        self.count
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> bool {
        // Refused, not clamped: a read past the end of a partition is a bug
        // in the caller, and silently returning a neighbour's bytes is how
        // one component's mistake becomes another's corruption.
        sector < self.count && self.storage.read(self.first + sector, out)
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> bool {
        sector < self.count && self.storage.write(self.first + sector, data)
    }

    fn flush(&mut self) -> bool {
        self.storage.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram {
        sectors: Vec<[u8; SECTOR_BYTES]>,
    }

    impl Sectors for Ram {
        fn sector_count(&self) -> u64 {
            self.sectors.len() as u64
        }
        fn read(&mut self, sector: u64, out: &mut [u8]) -> bool {
            match self.sectors.get(sector as usize) {
                Some(data) => {
                    out[..SECTOR_BYTES].copy_from_slice(data);
                    true
                }
                None => false,
            }
        }
        fn write(&mut self, sector: u64, data: &[u8]) -> bool {
            match self.sectors.get_mut(sector as usize) {
                Some(slot) => {
                    slot.copy_from_slice(&data[..SECTOR_BYTES]);
                    true
                }
                None => false,
            }
        }
        fn flush(&mut self) -> bool {
            true
        }
    }

    const OUR_TYPE: [u8; 16] = *b"i3mlOS-part-type";

    /// Build the same structures the image builder writes, so the reader is
    /// tested against the format rather than against itself.
    fn disk_with_partition(total: usize, first: u64, last: u64) -> Ram {
        let mut disk = Ram { sectors: vec![[0; SECTOR_BYTES]; total] };
        let mut entries = vec![0u8; 128 * 128];
        entries[0..16].copy_from_slice(&OUR_TYPE);
        entries[32..40].copy_from_slice(&first.to_le_bytes());
        entries[40..48].copy_from_slice(&last.to_le_bytes());
        for (index, chunk) in entries.chunks(SECTOR_BYTES).enumerate() {
            disk.sectors[2 + index][..chunk.len()].copy_from_slice(chunk);
        }

        let mut header = [0u8; SECTOR_BYTES];
        header[0..8].copy_from_slice(SIGNATURE);
        header[12..16].copy_from_slice(&92u32.to_le_bytes());
        header[72..80].copy_from_slice(&2u64.to_le_bytes());
        header[80..84].copy_from_slice(&128u32.to_le_bytes());
        header[84..88].copy_from_slice(&128u32.to_le_bytes());
        header[88..92].copy_from_slice(&crc32(&entries).to_le_bytes());
        let checksum = crc32(&header[..92]);
        header[16..20].copy_from_slice(&checksum.to_le_bytes());
        disk.sectors[1] = header;
        disk
    }

    #[test]
    fn a_partition_is_found_by_its_type() {
        let mut disk = disk_with_partition(1024, 100, 199);
        let partition = find(&mut disk, &OUR_TYPE).unwrap();
        assert_eq!(partition.first_lba, 100);
        assert_eq!(partition.sector_count(), 100);
    }

    #[test]
    fn a_damaged_header_is_not_believed() {
        let mut disk = disk_with_partition(1024, 100, 199);
        disk.sectors[1][80] ^= 0xff; // change the entry count, break the CRC
        assert_eq!(find(&mut disk, &OUR_TYPE).err(), Some(Fault::NoTable));
    }

    #[test]
    fn a_tampered_entry_array_is_caught_by_its_checksum() {
        let mut disk = disk_with_partition(1024, 100, 199);
        disk.sectors[2][32] ^= 0xff; // move the partition's start
        assert_eq!(find(&mut disk, &OUR_TYPE).err(), Some(Fault::Corrupt));
    }

    #[test]
    fn an_unpartitioned_disk_says_so() {
        let mut disk = Ram { sectors: vec![[0; SECTOR_BYTES]; 64] };
        assert_eq!(find(&mut disk, &OUR_TYPE).err(), Some(Fault::NoTable));
    }

    #[test]
    fn a_window_cannot_reach_outside_its_partition() {
        let mut disk = disk_with_partition(1024, 100, 199);
        let partition = find(&mut disk, &OUR_TYPE).unwrap();
        let mut window = Window::new(&mut disk, &partition);
        assert_eq!(window.sector_count(), 100);

        let payload = [0xabu8; SECTOR_BYTES];
        assert!(window.write(0, &payload));
        assert!(window.write(99, &payload));
        // One past the end belongs to somebody else.
        assert!(!window.write(100, &payload));
        let mut out = [0u8; SECTOR_BYTES];
        assert!(!window.read(100, &mut out));

        // And the bytes really landed at the partition's offset.
        assert_eq!(disk.sectors[100], payload);
        assert_eq!(disk.sectors[199], payload);
    }
}
