//! Building the distributable image: a GPT disk with an EFI system partition
//! the firmware can boot, and a second partition the kernel keeps its journal
//! in.
//!
//! Written from scratch — CRC-32, GPT, FAT32 — for the same reason the rest
//! of the project is: a build that needs `mkfs.vfat`, `sgdisk` and a Linux
//! host is a build most people cannot run. This one needs nothing but the
//! toolchain, and produces the identical bytes on every machine.

use std::path::Path;

const SECTOR: usize = 512;

/// Partition type GUID for an EFI System Partition (UEFI spec).
const ESP_TYPE: [u8; 16] = guid(
    0xc12a7328,
    0xf81f,
    0x11d2,
    [0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b],
);

/// Ours. A partition the kernel recognises as "the journal lives here" —
/// so it finds its storage by asking the disk, not by assuming an offset.
pub const JOURNAL_TYPE: [u8; 16] = guid(
    0x6933_6d6c,
    0x0053,
    0x4a4c,
    [0xa1, 0x00, 0x69, 0x33, 0x6d, 0x6c, 0x4f, 0x53],
);

/// GUIDs are little-endian in their first three fields and big-endian in the
/// rest — a layout that has confused every implementer at least once.
const fn guid(a: u32, b: u16, c: u16, rest: [u8; 8]) -> [u8; 16] {
    let a = a.to_le_bytes();
    let b = b.to_le_bytes();
    let c = c.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], rest[0], rest[1], rest[2], rest[3],
        rest[4], rest[5], rest[6], rest[7],
    ]
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

struct Image {
    bytes: Vec<u8>,
}

impl Image {
    fn new(sectors: usize) -> Image {
        Image { bytes: vec![0; sectors * SECTOR] }
    }

    fn sectors(&self) -> u64 {
        (self.bytes.len() / SECTOR) as u64
    }

    fn write(&mut self, offset: usize, data: &[u8]) {
        self.bytes[offset..offset + data.len()].copy_from_slice(data);
    }

    fn sector_mut(&mut self, lba: u64) -> &mut [u8] {
        let start = lba as usize * SECTOR;
        &mut self.bytes[start..start + SECTOR]
    }
}

struct PartitionSpec {
    type_guid: [u8; 16],
    first_lba: u64,
    last_lba: u64,
    name: &'static str,
}

/// A GUID that is stable across builds. Randomness would make the image
/// non-reproducible for no benefit — nothing here is a secret.
fn stable_guid(seed: u8) -> [u8; 16] {
    let mut guid = *b"i3mlOS-disk-guid";
    guid[15] = seed;
    guid
}

fn write_gpt(image: &mut Image, partitions: &[PartitionSpec]) {
    let last_lba = image.sectors() - 1;
    let entries_lba = 2u64;
    let entry_count = 128usize;
    let entry_size = 128usize;
    let entry_sectors = (entry_count * entry_size / SECTOR) as u64;

    // Protective MBR: one partition of type 0xEE spanning the disk, so tools
    // that only understand MBR see a disk that is fully in use rather than an
    // empty one they might helpfully reformat.
    let mut mbr = [0u8; SECTOR];
    mbr[446] = 0x00;
    mbr[450] = 0xee;
    mbr[454..458].copy_from_slice(&1u32.to_le_bytes());
    let span = u32::try_from(last_lba).unwrap_or(u32::MAX);
    mbr[458..462].copy_from_slice(&span.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    image.sector_mut(0).copy_from_slice(&mbr);

    // Partition entry array.
    let mut entries = vec![0u8; entry_count * entry_size];
    for (index, partition) in partitions.iter().enumerate() {
        let base = index * entry_size;
        entries[base..base + 16].copy_from_slice(&partition.type_guid);
        entries[base + 16..base + 32].copy_from_slice(&stable_guid(index as u8 + 1));
        entries[base + 32..base + 40].copy_from_slice(&partition.first_lba.to_le_bytes());
        entries[base + 40..base + 48].copy_from_slice(&partition.last_lba.to_le_bytes());
        // Attributes stay zero; the name is UTF-16LE.
        for (position, unit) in partition.name.encode_utf16().take(36).enumerate() {
            let at = base + 56 + position * 2;
            entries[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    let entries_crc = crc32(&entries);
    image.write(entries_lba as usize * SECTOR, &entries);

    let backup_entries_lba = last_lba - entry_sectors;
    image.write(backup_entries_lba as usize * SECTOR, &entries);

    let header = |current: u64, backup: u64, entries_at: u64| -> [u8; SECTOR] {
        let mut header = [0u8; SECTOR];
        header[0..8].copy_from_slice(b"EFI PART");
        header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        header[12..16].copy_from_slice(&92u32.to_le_bytes());
        // header[16..20] is the CRC, computed once the rest is filled in.
        header[24..32].copy_from_slice(&current.to_le_bytes());
        header[32..40].copy_from_slice(&backup.to_le_bytes());
        header[40..48].copy_from_slice(&(entries_lba + entry_sectors).to_le_bytes());
        header[48..56].copy_from_slice(&(backup_entries_lba - 1).to_le_bytes());
        header[56..72].copy_from_slice(&stable_guid(0));
        header[72..80].copy_from_slice(&entries_at.to_le_bytes());
        header[80..84].copy_from_slice(&(entry_count as u32).to_le_bytes());
        header[84..88].copy_from_slice(&(entry_size as u32).to_le_bytes());
        header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let checksum = crc32(&header[..92]);
        header[16..20].copy_from_slice(&checksum.to_le_bytes());
        header
    };

    let primary = header(1, last_lba, entries_lba);
    image.sector_mut(1).copy_from_slice(&primary);
    let backup = header(last_lba, 1, backup_entries_lba);
    image.sector_mut(last_lba).copy_from_slice(&backup);
}

/// Lay out a FAT32 filesystem containing exactly `/EFI/BOOT/BOOTX64.EFI`.
///
/// FAT32 rather than FAT16 because the UEFI spec asks for it, and one sector
/// per cluster because it makes the cluster count large enough to *be* FAT32
/// without inflating the partition.
fn write_fat32(image: &mut Image, first_lba: u64, sectors: u64, kernel: &[u8]) -> Result<(), String> {
    const RESERVED: u64 = 32;
    const FATS: u64 = 2;
    let clusters = sectors - RESERVED; // approximate; refined below
    // Each FAT entry is 4 bytes; solve for a FAT size that covers the data
    // area it leaves behind.
    let mut fat_sectors = (clusters * 4).div_ceil(SECTOR as u64);
    let mut data_sectors = sectors - RESERVED - FATS * fat_sectors;
    for _ in 0..8 {
        fat_sectors = ((data_sectors + 2) * 4).div_ceil(SECTOR as u64);
        data_sectors = sectors - RESERVED - FATS * fat_sectors;
    }
    let cluster_count = data_sectors;
    if cluster_count < 65_525 {
        return Err(format!(
            "the EFI partition is too small to be FAT32: {cluster_count} clusters, need 65525"
        ));
    }

    let base = first_lba as usize * SECTOR;

    // Boot sector / BIOS parameter block.
    let mut boot = [0u8; SECTOR];
    boot[0..3].copy_from_slice(&[0xeb, 0x58, 0x90]);
    boot[3..11].copy_from_slice(b"i3mlOS  ");
    boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    boot[13] = 1; // sectors per cluster
    boot[14..16].copy_from_slice(&(RESERVED as u16).to_le_bytes());
    boot[16] = FATS as u8;
    boot[17..19].copy_from_slice(&0u16.to_le_bytes()); // root entries: FAT32 uses a cluster chain
    boot[19..21].copy_from_slice(&0u16.to_le_bytes()); // total sectors 16: see the 32-bit field
    boot[21] = 0xf8; // fixed disk
    boot[22..24].copy_from_slice(&0u16.to_le_bytes()); // FAT size 16: unused on FAT32
    boot[24..26].copy_from_slice(&32u16.to_le_bytes());
    boot[26..28].copy_from_slice(&8u16.to_le_bytes());
    boot[28..32].copy_from_slice(&(first_lba as u32).to_le_bytes());
    boot[32..36].copy_from_slice(&(sectors as u32).to_le_bytes());
    boot[36..40].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    boot[40..42].copy_from_slice(&0u16.to_le_bytes()); // ext flags: mirror both FATs
    boot[42..44].copy_from_slice(&0u16.to_le_bytes()); // version
    boot[44..48].copy_from_slice(&2u32.to_le_bytes()); // root directory cluster
    boot[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo sector
    boot[50..52].copy_from_slice(&6u16.to_le_bytes()); // backup boot sector
    boot[64] = 0x80;
    boot[66] = 0x29; // extended boot signature
    boot[67..71].copy_from_slice(&0x1391_1391u32.to_le_bytes());
    boot[71..82].copy_from_slice(b"I3MLOS     ");
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55;
    boot[511] = 0xaa;
    image.write(base, &boot);
    image.write(base + 6 * SECTOR, &boot); // backup

    // FSInfo — advisory, but firmware reads it and a wrong one looks broken.
    let mut fsinfo = [0u8; SECTOR];
    fsinfo[0..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
    fsinfo[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
    fsinfo[488..492].copy_from_slice(&u32::MAX.to_le_bytes()); // free count unknown
    fsinfo[492..496].copy_from_slice(&u32::MAX.to_le_bytes()); // next free unknown
    fsinfo[508] = 0x55;
    fsinfo[509] = 0xaa;
    image.write(base + SECTOR, &fsinfo);

    // Cluster plan: 2 = root, 3 = /EFI, 4 = /EFI/BOOT, 5.. = the kernel.
    let kernel_clusters = (kernel.len() as u64).div_ceil(SECTOR as u64).max(1);
    let mut fat = vec![0u8; (fat_sectors * SECTOR as u64) as usize];
    let set = |cluster: u64, value: u32, fat: &mut Vec<u8>| {
        let at = cluster as usize * 4;
        fat[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    set(0, 0x0fff_fff8, &mut fat);
    set(1, 0x0fff_ffff, &mut fat);
    set(2, 0x0fff_ffff, &mut fat); // root: one cluster
    set(3, 0x0fff_ffff, &mut fat); // /EFI
    set(4, 0x0fff_ffff, &mut fat); // /EFI/BOOT
    for index in 0..kernel_clusters {
        let cluster = 5 + index;
        let value =
            if index + 1 == kernel_clusters { 0x0fff_ffff } else { (cluster + 1) as u32 };
        set(cluster, value, &mut fat);
    }
    for copy in 0..FATS {
        image.write(base + ((RESERVED + copy * fat_sectors) as usize * SECTOR), &fat);
    }

    let data_start = RESERVED + FATS * fat_sectors;
    let cluster_offset =
        |cluster: u64| base + ((data_start + cluster - 2) as usize) * SECTOR;

    let entry = |name: &[u8; 11], attributes: u8, cluster: u64, size: u32| -> [u8; 32] {
        let mut record = [0u8; 32];
        record[0..11].copy_from_slice(name);
        record[11] = attributes;
        record[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        record[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        record[28..32].copy_from_slice(&size.to_le_bytes());
        record
    };

    const DIRECTORY: u8 = 0x10;
    // Root directory.
    let root = cluster_offset(2);
    image.write(root, &entry(b"EFI        ", DIRECTORY, 3, 0));
    // /EFI
    let efi = cluster_offset(3);
    image.write(efi, &entry(b".          ", DIRECTORY, 3, 0));
    image.write(efi + 32, &entry(b"..         ", DIRECTORY, 0, 0));
    image.write(efi + 64, &entry(b"BOOT       ", DIRECTORY, 4, 0));
    // /EFI/BOOT
    let boot_dir = cluster_offset(4);
    image.write(boot_dir, &entry(b".          ", DIRECTORY, 4, 0));
    image.write(boot_dir + 32, &entry(b"..         ", DIRECTORY, 3, 0));
    image.write(boot_dir + 64, &entry(b"BOOTX64 EFI", 0x20, 5, kernel.len() as u32));
    // The kernel itself.
    image.write(cluster_offset(5), kernel);

    Ok(())
}

/// Build the whole image. Returns the path written.
pub fn build(output: &Path, kernel: &[u8], megabytes: u64) -> Result<(), String> {
    let total_sectors = (megabytes * 1024 * 1024 / SECTOR as u64) as usize;
    let mut image = Image::new(total_sectors);

    // Layout: GPT, then a 48 MiB ESP, then the journal partition to the end.
    let esp_first = 2048u64;
    let esp_sectors = 48 * 1024 * 1024 / SECTOR as u64;
    let esp_last = esp_first + esp_sectors - 1;
    let journal_first = esp_last + 1;
    let journal_last = image.sectors() - 34;
    if journal_last <= journal_first {
        return Err("image too small for both partitions".into());
    }

    write_gpt(&mut image, &[
        PartitionSpec {
            type_guid: ESP_TYPE,
            first_lba: esp_first,
            last_lba: esp_last,
            name: "EFI System",
        },
        PartitionSpec {
            type_guid: JOURNAL_TYPE,
            first_lba: journal_first,
            last_lba: journal_last,
            name: "i3ml journal",
        },
    ]);
    write_fat32(&mut image, esp_first, esp_sectors, kernel)?;

    std::fs::write(output, &image.bytes).map_err(|error| format!("writing image: {error}"))?;
    Ok(())
}
