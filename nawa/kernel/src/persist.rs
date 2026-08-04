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

use i3ml_journal::{Fault, Journal, Record, Sectors};
use nawa_core::serial::SerialWriter;
use nawa_sijil as sijil;
use nawa_virtio::block::{Block, SECTOR_SIZE};

/// The journal's view of the disk. The driver knows about sectors; the format
/// knows about records; neither needs to know about the other.
struct Disk {
    device: Block,
}

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

    let mut journal = match Journal::open(&mut disk) {
        Ok(journal) => {
            let _ = writeln!(
                out,
                "sijil: {} entries recovered from disk, chain intact — the record outlived the reboot",
                journal.count()
            );
            if journal.count() > 0 {
                // Show the machine remembering something specific, not just a
                // number: the first thing it did last time.
                if let Ok(first) = journal.read(&mut disk, 0) {
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
            match Journal::create(&mut disk) {
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
        match journal.append(&mut disk, &record) {
            Ok(()) => written += 1,
            Err(Fault::Full) => full = true,
            Err(_) => full = true,
        }
    });
    journal.flush(&mut disk);

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
