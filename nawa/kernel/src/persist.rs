//! Making SIJIL survive the power going out.
//!
//! Until this file existed, "what did my computer do while I slept?" had an
//! answer only until the machine was turned off — the journal was a ring in
//! RAM. Now every boot opens the log on disk, **verifies its whole chain**
//! before trusting a word of it, appends what happened this time, and leaves
//! it committed.
//!
//! Verification is not decoration. A hash chain turns the difference between
//! "the record I wrote" and "the record I am reading" into something the
//! kernel can detect instead of assume.

use core::fmt::Write;

use i3ml_gpt::Window;
use i3ml_journal::{Fault, Journal, Record, Sectors};
use nawa_core::serial::SerialWriter;
use nawa_sijil as sijil;
use nawa_virtio::block::{Block, SECTOR_SIZE};

/// The journal's view of the disk. The driver knows about sectors; the format
/// knows about records; neither needs to know about the other.
struct Disk {
    device: Block,
}

/// The partition type that says "the journal lives here". Written by the
/// image builder, read back by the kernel — neither side assumes an offset.
pub const JOURNAL_PARTITION: [u8; 16] = [
    0x6c, 0x6d, 0x33, 0x69, 0x53, 0x00, 0x4c, 0x4a, 0xa1, 0x00, 0x69, 0x33, 0x6d, 0x6c, 0x4f,
    0x53,
];

impl Sectors for Disk {
    fn sector_count(&self) -> u64 {
        // The last sector is the driver's scratch area; the journal stops
        // short of it rather than discovering the overlap the hard way.
        self.device.sectors.saturating_sub(1)
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> bool {
        out.len() >= SECTOR_SIZE && self.device.read_sector(sector, out)
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> bool {
        data.len() >= SECTOR_SIZE && self.device.write_sector(sector, data)
    }

    fn flush(&mut self) -> bool {
        self.device.flush()
    }
}

fn event_code(event: sijil::Event) -> u8 {
    match event {
        sijil::Event::Spawned => 0,
        sijil::Event::Delegated => 1,
        sijil::Event::Attenuated => 2,
        sijil::Event::Granted => 3,
        sijil::Event::Invoked => 4,
        sijil::Event::Denied => 5,
        sijil::Event::ApprovalRequested => 6,
        sijil::Event::Approved => 7,
        sijil::Event::Rejected => 8,
        sijil::Event::Remembered => 9,
        sijil::Event::Noted => 10,
        sijil::Event::Finished => 11,
        sijil::Event::BudgetExhausted => 12,
    }
}

/// Open (or start) the log, report what the last boot left behind, append
/// this boot's entries, and commit.
pub fn run(device: Block, out: &mut SerialWriter) {
    let mut disk = Disk { device };

    // Ask the disk where the journal belongs. On a partitioned image this
    // finds the partition the builder reserved; on a bare scratch disk there
    // is no table, and the whole device is ours.
    match i3ml_gpt::find(&mut disk, &JOURNAL_PARTITION) {
        Ok(partition) => {
            let _ = writeln!(
                out,
                "gpt: journal partition found — sectors {}..{} ({} MiB)",
                partition.first_lba,
                partition.last_lba,
                partition.sector_count() * 512 / (1024 * 1024)
            );
            // The last sector of our partition is scratch: the journal stops
            // one short of it, so proving the disk round-trips cannot
            // overwrite a record — and cannot reach the backup partition
            // table beyond the partition either.
            let mut scratch = partition;
            scratch.first_lba = partition.last_lba;
            round_trip(&mut Window::new(&mut disk, &scratch), out);

            let mut usable = partition;
            usable.last_lba = partition.last_lba.saturating_sub(1);
            let mut window = Window::new(&mut disk, &usable);
            journal(&mut window, out);
        }
        Err(fault) => {
            let _ = writeln!(out, "gpt: no journal partition ({fault:?}) — using the whole disk");
            round_trip(&mut Tail { inner: &mut disk }, out);
            journal(&mut disk, out);
        }
    }
}

/// A one-sector view of the very end of a device, for the round-trip check on
/// a disk with no partition table to tell us what is ours.
struct Tail<'a> {
    inner: &'a mut dyn Sectors,
}

impl Sectors for Tail<'_> {
    fn sector_count(&self) -> u64 {
        1
    }
    fn read(&mut self, sector: u64, out: &mut [u8]) -> bool {
        let last = self.inner.sector_count();
        sector == 0 && self.inner.read(last, out)
    }
    fn write(&mut self, sector: u64, data: &[u8]) -> bool {
        let last = self.inner.sector_count();
        sector == 0 && self.inner.write(last, data)
    }
    fn flush(&mut self) -> bool {
        self.inner.flush()
    }
}

/// Write a sector, read it back, compare. Until this works, nothing the
/// kernel remembers outlives a reboot — so it is checked every boot rather
/// than assumed from the last one.
fn round_trip(storage: &mut dyn Sectors, out: &mut SerialWriter) {
    let mut written = [0u8; 512];
    let greeting = b"i3mlOS scratch sector -- i3mel";
    written[..greeting.len()].copy_from_slice(greeting);

    if !storage.write(0, &written) {
        let _ = writeln!(out, "virtio-blk: write REFUSED");
        return;
    }
    let mut read_back = [0u8; 512];
    if !storage.read(0, &mut read_back) {
        let _ = writeln!(out, "virtio-blk: read REFUSED");
        return;
    }
    if read_back[..greeting.len()] == greeting[..] {
        let _ = writeln!(out, "virtio-blk: wrote and read back a sector — storage works");
    } else {
        let _ = writeln!(out, "virtio-blk: READ BACK MISMATCH");
    }
    if storage.flush() {
        let _ = writeln!(out, "virtio-blk: flushed — the write is on the disk, not in a promise");
    }
}

/// Open (or start) the log on whatever storage it was given.
fn journal(disk: &mut dyn Sectors, out: &mut SerialWriter) {
    let mut journal = match Journal::open(disk) {
        Ok(journal) => {
            let _ = writeln!(
                out,
                "sijil: {} entries recovered from disk, chain intact — the record outlived the reboot",
                journal.count()
            );
            if journal.count() > 0 {
                // Show the machine remembering something specific, not just a
                // number: the first thing it did last time.
                if let Ok(first) = journal.read(disk, 0) {
                    let _ = writeln!(
                        out,
                        "  earliest surviving entry: #{} agent={} \"{}\"",
                        first.sequence,
                        first.agent,
                        first.label_str()
                    );
                }
            }
            journal
        }
        Err(Fault::NoJournal) => {
            let _ = writeln!(out, "sijil: no journal on this disk — starting one");
            match Journal::create(disk) {
                Ok(journal) => journal,
                Err(_) => {
                    let _ = writeln!(out, "sijil: could not create the journal");
                    return;
                }
            }
        }
        Err(fault) => {
            // A journal that does not verify is not a journal. Say so loudly
            // and refuse to build on top of it.
            let _ = writeln!(out, "sijil: REFUSING a journal that failed verification: {fault:?}");
            return;
        }
    };

    let mut written = 0u64;
    let mut full = false;
    sijil::for_each(|entry| {
        if full {
            return;
        }
        let record = Record::new(
            entry.sequence,
            entry.at,
            entry.agent,
            event_code(entry.event),
            entry.detail,
            entry.label.as_str(),
        );
        match journal.append(disk, &record) {
            Ok(()) => written += 1,
            Err(Fault::Full) => full = true,
            Err(_) => full = true,
        }
    });
    journal.flush(disk);

    let _ = writeln!(
        out,
        "sijil: {} entries persisted this boot, {} total on disk ({} capacity)",
        written,
        journal.count(),
        journal.capacity()
    );
    let tip = journal.tip();
    let _ = write!(out, "sijil: chain tip");
    for byte in tip.iter().take(8) {
        let _ = write!(out, " {byte:02x}");
    }
    let _ = writeln!(out, " — a record that can be checked, not just believed");
}
