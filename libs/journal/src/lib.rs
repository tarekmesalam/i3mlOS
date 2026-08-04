//! The SIJIL on-disk format — a hash-chained log.
//!
//! The property this format exists to provide: **you can tell whether the
//! record you are reading is the record that was written.** Each entry's hash
//! covers its own fields *and* the hash before it, so altering or removing
//! anything in the middle breaks every link after it. That does not stop an
//! attacker who owns the disk from rewriting the whole chain — a signature
//! would, and is a later milestone — but it does stop the far more common
//! case: a truncated write, a corrupted sector, a quiet edit.
//!
//! Records are fixed-size so that "entry N" is arithmetic rather than a scan,
//! and four fit exactly in a 512-byte sector.
//!
//! Host-testable first: everything here runs against a `Vec<u8>` in the test
//! suite before it ever meets a disk.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub const SECTOR_BYTES: usize = 512;
pub const RECORD_BYTES: usize = 128;
pub const RECORDS_PER_SECTOR: usize = SECTOR_BYTES / RECORD_BYTES;
pub const LABEL_BYTES: usize = 40;
pub const HASH_BYTES: usize = 32;

/// Sector 0 holds the superblock; records start at sector 1.
pub const SUPERBLOCK_SECTOR: u64 = 0;
pub const FIRST_RECORD_SECTOR: u64 = 1;

const MAGIC: [u8; 8] = *b"i3mlSJL1";

/// The most records this build will ever verify, whatever the disk claims
/// about its own size.
///
/// `open` walks the entire chain before trusting it, so the length of that
/// walk must be a number *we* chose. A device is free to report a capacity
/// near 2^64; believing it turns verification into an unbounded loop that a
/// cooperative liar can feed valid records forever.
pub const MAX_RECORDS: u64 = 1 << 20;

/// Where the storage lives. The kernel implements this over virtio-blk; the
/// tests implement it over a vector, which is why the format can be trusted
/// before the driver is.
pub trait Sectors {
    fn sector_count(&self) -> u64;
    fn read(&mut self, sector: u64, out: &mut [u8]) -> bool;
    fn write(&mut self, sector: u64, data: &[u8]) -> bool;
    fn flush(&mut self) -> bool;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    pub sequence: u64,
    pub at: u64,
    pub agent: u32,
    pub event: u8,
    pub detail: u64,
    pub label: [u8; LABEL_BYTES],
    pub label_length: u8,
}

impl Record {
    pub fn new(sequence: u64, at: u64, agent: u32, event: u8, detail: u64, label: &str) -> Record {
        let mut bytes = [0u8; LABEL_BYTES];
        let mut length = 0;
        for ch in label.chars() {
            let width = ch.len_utf8();
            if length + width > LABEL_BYTES {
                break;
            }
            ch.encode_utf8(&mut bytes[length..]);
            length += width;
        }
        Record { sequence, at, agent, event, detail, label: bytes, label_length: length as u8 }
    }

    pub fn label_str(&self) -> &str {
        core::str::from_utf8(&self.label[..self.label_length as usize]).unwrap_or("")
    }

    /// The bytes the hash covers: everything about the record except the hash
    /// itself.
    fn fields(&self, out: &mut [u8; 72]) {
        out[0..8].copy_from_slice(&self.sequence.to_le_bytes());
        out[8..16].copy_from_slice(&self.at.to_le_bytes());
        out[16..20].copy_from_slice(&self.agent.to_le_bytes());
        out[20] = self.event;
        out[21] = self.label_length;
        out[22] = 0;
        out[23] = 0;
        out[24..32].copy_from_slice(&self.detail.to_le_bytes());
        out[32..72].copy_from_slice(&self.label);
    }

    /// `hash(previous || fields)` — the link that makes the chain a chain.
    pub fn hash(&self, previous: &[u8; HASH_BYTES]) -> [u8; HASH_BYTES] {
        let mut fields = [0u8; 72];
        self.fields(&mut fields);
        let mut hasher = i3ml_sha256::Hasher::new();
        hasher.update(previous);
        hasher.update(&fields);
        hasher.finish()
    }

    fn encode(&self, previous: &[u8; HASH_BYTES], out: &mut [u8; RECORD_BYTES]) {
        let mut fields = [0u8; 72];
        self.fields(&mut fields);
        out[..72].copy_from_slice(&fields);
        out[72..104].copy_from_slice(&self.hash(previous));
        out[104..].fill(0);
    }

    fn decode(bytes: &[u8]) -> Option<(Record, [u8; HASH_BYTES])> {
        if bytes.len() < RECORD_BYTES {
            return None;
        }
        let mut label = [0u8; LABEL_BYTES];
        label.copy_from_slice(&bytes[32..72]);
        let label_length = bytes[21];
        if label_length as usize > LABEL_BYTES {
            return None;
        }
        let record = Record {
            sequence: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            at: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
            agent: u32::from_le_bytes(bytes[16..20].try_into().ok()?),
            event: bytes[20],
            detail: u64::from_le_bytes(bytes[24..32].try_into().ok()?),
            label,
            label_length,
        };
        let mut stored = [0u8; HASH_BYTES];
        stored.copy_from_slice(&bytes[72..104]);
        Some((record, stored))
    }
}

/// What sector 0 says about the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub count: u64,
    pub tip: [u8; HASH_BYTES],
}

impl Superblock {
    fn encode(&self, out: &mut [u8; SECTOR_BYTES]) {
        out.fill(0);
        out[0..8].copy_from_slice(&MAGIC);
        out[8..16].copy_from_slice(&self.count.to_le_bytes());
        out[16..48].copy_from_slice(&self.tip);
    }

    fn decode(bytes: &[u8]) -> Option<Superblock> {
        if bytes.len() < 48 || bytes[0..8] != MAGIC {
            return None;
        }
        let mut tip = [0u8; HASH_BYTES];
        tip.copy_from_slice(&bytes[16..48]);
        Some(Superblock { count: u64::from_le_bytes(bytes[8..16].try_into().ok()?), tip })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    /// The disk has no journal on it (or a different one).
    NoJournal,
    /// A record's hash does not match its contents or its predecessor.
    /// The index is where the chain broke.
    ChainBroken(usize),
    /// The device refused a read or a write.
    Device,
    /// The log has filled the space it was given.
    Full,
}

/// A hash-chained journal on a block device.
pub struct Journal {
    count: u64,
    tip: [u8; HASH_BYTES],
    capacity: u64,
}

impl Journal {
    /// Start a new log, overwriting whatever was there.
    pub fn create(storage: &mut dyn Sectors) -> Result<Journal, Fault> {
        let capacity = record_capacity(storage.sector_count());
        let journal = Journal { count: 0, tip: [0; HASH_BYTES], capacity };
        journal.write_superblock(storage)?;
        Ok(journal)
    }

    /// Open an existing log and **verify every link before trusting it**. A
    /// journal that does not verify is not a journal.
    pub fn open(storage: &mut dyn Sectors) -> Result<Journal, Fault> {
        let mut sector = [0u8; SECTOR_BYTES];
        if !storage.read(SUPERBLOCK_SECTOR, &mut sector) {
            return Err(Fault::Device);
        }
        let superblock = Superblock::decode(&sector).ok_or(Fault::NoJournal)?;
        let capacity = record_capacity(storage.sector_count());
        if superblock.count > capacity {
            return Err(Fault::ChainBroken(0));
        }

        let mut previous = [0u8; HASH_BYTES];
        for index in 0..superblock.count {
            let (record, stored) = read_record(storage, index)?;
            let expected = record.hash(&previous);
            if expected != stored {
                return Err(Fault::ChainBroken(index as usize));
            }
            previous = stored;
        }
        if previous != superblock.tip {
            return Err(Fault::ChainBroken(superblock.count as usize));
        }
        Ok(Journal { count: superblock.count, tip: superblock.tip, capacity })
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn tip(&self) -> [u8; HASH_BYTES] {
        self.tip
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Append one record and advance the chain.
    pub fn append(&mut self, storage: &mut dyn Sectors, record: &Record) -> Result<(), Fault> {
        if self.count >= self.capacity {
            return Err(Fault::Full);
        }
        let sector = FIRST_RECORD_SECTOR + self.count / RECORDS_PER_SECTOR as u64;
        let slot = (self.count % RECORDS_PER_SECTOR as u64) as usize;

        let mut buffer = [0u8; SECTOR_BYTES];
        // Read-modify-write: the other records in this sector are already
        // part of the chain and must survive.
        if slot != 0 && !storage.read(sector, &mut buffer) {
            return Err(Fault::Device);
        }
        let mut encoded = [0u8; RECORD_BYTES];
        record.encode(&self.tip, &mut encoded);
        buffer[slot * RECORD_BYTES..(slot + 1) * RECORD_BYTES].copy_from_slice(&encoded);
        if !storage.write(sector, &buffer) {
            return Err(Fault::Device);
        }

        self.tip = record.hash(&self.tip);
        self.count += 1;
        self.write_superblock(storage)
    }

    /// Read record `index` back.
    pub fn read(&self, storage: &mut dyn Sectors, index: u64) -> Result<Record, Fault> {
        if index >= self.count {
            return Err(Fault::ChainBroken(index as usize));
        }
        let (record, _) = read_record(storage, index)?;
        Ok(record)
    }

    /// Commit. Without this the log is a promise, not a record.
    pub fn flush(&self, storage: &mut dyn Sectors) -> bool {
        storage.flush()
    }

    fn write_superblock(&self, storage: &mut dyn Sectors) -> Result<(), Fault> {
        let mut sector = [0u8; SECTOR_BYTES];
        Superblock { count: self.count, tip: self.tip }.encode(&mut sector);
        if storage.write(SUPERBLOCK_SECTOR, &sector) {
            Ok(())
        } else {
            Err(Fault::Device)
        }
    }
}

fn record_capacity(sector_count: u64) -> u64 {
    sector_count
        .saturating_sub(FIRST_RECORD_SECTOR)
        .saturating_mul(RECORDS_PER_SECTOR as u64)
        .min(MAX_RECORDS)
}

fn read_record(storage: &mut dyn Sectors, index: u64) -> Result<(Record, [u8; HASH_BYTES]), Fault> {
    let sector = FIRST_RECORD_SECTOR + index / RECORDS_PER_SECTOR as u64;
    let slot = (index % RECORDS_PER_SECTOR as u64) as usize;
    let mut buffer = [0u8; SECTOR_BYTES];
    if !storage.read(sector, &mut buffer) {
        return Err(Fault::Device);
    }
    Record::decode(&buffer[slot * RECORD_BYTES..(slot + 1) * RECORD_BYTES])
        .ok_or(Fault::ChainBroken(index as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disk made of memory, so the format can be wrong loudly and cheaply.
    struct Ram {
        sectors: Vec<[u8; SECTOR_BYTES]>,
        flushed: usize,
    }

    impl Ram {
        fn new(count: usize) -> Ram {
            Ram { sectors: vec![[0; SECTOR_BYTES]; count], flushed: 0 }
        }
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
            self.flushed += 1;
            true
        }
    }

    fn entry(sequence: u64, label: &str) -> Record {
        Record::new(sequence, sequence * 100, 1, 4, sequence, label)
    }

    #[test]
    fn a_log_survives_being_closed_and_opened() {
        let mut disk = Ram::new(64);
        let mut journal = Journal::create(&mut disk).unwrap();
        for sequence in 1..=10 {
            journal.append(&mut disk, &entry(sequence, "invoked fs:read")).unwrap();
        }
        journal.flush(&mut disk);
        let tip = journal.tip();

        // The reboot: nothing but the disk survives.
        let reopened = Journal::open(&mut disk).expect("a verified journal");
        assert_eq!(reopened.count(), 10);
        assert_eq!(reopened.tip(), tip);
        assert_eq!(reopened.read(&mut disk, 3).unwrap().label_str(), "invoked fs:read");
        assert_eq!(reopened.read(&mut disk, 9).unwrap().sequence, 10);
    }

    #[test]
    fn a_record_edited_in_place_breaks_the_chain() {
        let mut disk = Ram::new(64);
        let mut journal = Journal::create(&mut disk).unwrap();
        for sequence in 1..=8 {
            journal.append(&mut disk, &entry(sequence, "sent 3 emails")).unwrap();
        }

        // Rewrite the label of record 2 as if nothing happened.
        let mut sector = [0u8; SECTOR_BYTES];
        disk.read(FIRST_RECORD_SECTOR, &mut sector);
        sector[2 * RECORD_BYTES + 32] = b'X';
        disk.write(FIRST_RECORD_SECTOR, &sector);

        assert_eq!(Journal::open(&mut disk).err(), Some(Fault::ChainBroken(2)));
    }

    #[test]
    fn a_truncated_log_is_detected_by_its_tip() {
        let mut disk = Ram::new(64);
        let mut journal = Journal::create(&mut disk).unwrap();
        for sequence in 1..=6 {
            journal.append(&mut disk, &entry(sequence, "granted fs:read")).unwrap();
        }

        // Claim there are fewer records than were written: the chain still
        // verifies internally, but the tip no longer matches.
        let mut sector = [0u8; SECTOR_BYTES];
        disk.read(SUPERBLOCK_SECTOR, &mut sector);
        sector[8..16].copy_from_slice(&4u64.to_le_bytes());
        disk.write(SUPERBLOCK_SECTOR, &sector);

        assert_eq!(Journal::open(&mut disk).err(), Some(Fault::ChainBroken(4)));
    }

    #[test]
    fn an_empty_disk_is_not_mistaken_for_an_empty_log() {
        let mut disk = Ram::new(64);
        assert_eq!(Journal::open(&mut disk).err(), Some(Fault::NoJournal));
    }

    #[test]
    fn a_disk_that_lies_about_its_size_cannot_set_the_work() {
        struct Enormous;
        impl Sectors for Enormous {
            fn sector_count(&self) -> u64 {
                u64::MAX
            }
            fn read(&mut self, _sector: u64, _out: &mut [u8]) -> bool {
                true
            }
            fn write(&mut self, _sector: u64, _data: &[u8]) -> bool {
                true
            }
            fn flush(&mut self) -> bool {
                true
            }
        }
        // Neither wrapped to something small nor believed at face value.
        let journal = Journal::create(&mut Enormous).unwrap();
        assert_eq!(journal.capacity(), MAX_RECORDS);
    }

    #[test]
    fn the_log_refuses_to_run_past_its_space() {
        let mut disk = Ram::new(3); // superblock + 2 sectors = 8 records
        let mut journal = Journal::create(&mut disk).unwrap();
        assert_eq!(journal.capacity(), 8);
        for sequence in 1..=8 {
            journal.append(&mut disk, &entry(sequence, "ok")).unwrap();
        }
        assert_eq!(journal.append(&mut disk, &entry(9, "one too many")), Err(Fault::Full));
    }

    #[test]
    fn records_sharing_a_sector_all_survive() {
        // Four records live in one sector; appending the fourth must not
        // erase the first three.
        let mut disk = Ram::new(8);
        let mut journal = Journal::create(&mut disk).unwrap();
        for sequence in 1..=4 {
            journal.append(&mut disk, &entry(sequence, "shared sector")).unwrap();
        }
        let reopened = Journal::open(&mut disk).unwrap();
        for index in 0..4 {
            assert_eq!(reopened.read(&mut disk, index).unwrap().sequence, index + 1);
        }
    }
}
