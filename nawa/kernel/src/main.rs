//! The i3ml kernel. M1: the machine has a heartbeat (APIC/TSC-deadline) and
//! a supervisor whose schedulable unit is an **agent** — goal, lineage,
//! capabilities, budget — running through the AMAN gate with every crossing
//! recorded in SIJIL.
//!
//! Note what is NOT here: unsafe *operations*. The kernel builds on safe APIs
//! from the NAWA trusted core, and the framekernel rule keeps it that way.
//! (The single `unsafe(export_name)` below is an attribute, not an operation:
//! Rust flags it only because duplicate symbol names are a linker hazard, and
//! this image has exactly one `efi_main`.)

#![no_std]
#![no_main]
#![deny(unsafe_code)]

extern crate alloc;

mod agents;
mod banner;
mod logo;
mod resident;
mod persist;
mod tool;
mod toolmod;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use nawa_core::entry::{self, BootInfo};
use nawa_core::serial::SerialWriter;
use nawa_core::uefi::{EfiHandle, EfiStatus, EfiSystemTable};
use nawa_core::{apic, qemu, selftest, serial, yard};
use nawa_gate::{self as gate, Budget, State};
use nawa_sijil as sijil;

/// The line `cargo xtask test` (and CI) greps for on the serial port.
const HELLO: &str = "hello from the i3ml kernel";

#[allow(unsafe_code)] // the entry-symbol attribute; see module docs
#[unsafe(export_name = "efi_main")]
extern "efiapi" fn efi_main(image_handle: EfiHandle, system_table: *mut EfiSystemTable) -> EfiStatus {
    entry::boot(image_handle, system_table, gate::on_tick, kmain)
}

fn kmain(boot: BootInfo) -> ! {
    let mut out = SerialWriter;

    // Only printed when the full bring-up succeeded — the line means more
    // every milestone.
    serial::write_str(HELLO);
    serial::write_str("\n");

    let _ = writeln!(
        out,
        "mem: {} MiB usable, {} MiB managed by the frame allocator",
        boot.usable_bytes / (1024 * 1024),
        boot.managed_bytes / (1024 * 1024),
    );

    // Self-test 1: the CPU takes our IDT seriously (handler logs and resumes).
    selftest::breakpoint();

    // Self-test 2: the heap is real — allocate, grow, format.
    if boot.heap_ok {
        let mut squares: Vec<u64> = Vec::new();
        for n in 1..=10 {
            squares.push(n * n);
        }
        let mut rendered = String::new();
        let _ = write!(rendered, "{:?}", squares);
        let _ = writeln!(out, "heap: ok — squares {rendered}");
    } else {
        let _ = writeln!(out, "heap: FAILED to initialize");
    }

    if boot.tsc_per_microsecond != 0 {
        let _ = writeln!(
            out,
            "clock: {} MHz TSC, {} ticks in the first moments",
            boot.tsc_per_microsecond,
            apic::ticks()
        );
    }

    if boot.mapped_gibibytes != 0 {
        let _ = writeln!(out, "paging: {} GiB identity-mapped by our own tables", boot.mapped_gibibytes);
    }

    draw_boot_screen(&boot, &mut out);
    run_first_agents(&mut out);
    run_the_yard(&mut out);
    run_a_wasm_agent(&mut out);
    bring_up_devices(&mut out);

    serial::write_str("nawa: M5 complete, parking\n");
    qemu::exit(qemu::EXIT_SUCCESS);
    entry::park()
}

fn draw_boot_screen(boot: &BootInfo, out: &mut SerialWriter) {
    let Some(fb) = boot.framebuffer else { return };
    fb.clear(0x0b, 0x10, 0x21); // ink-dark blue
    let art = banner::banner();
    let scale = if fb.height >= art.height * 3 { 2 } else { 1 };
    const GAP: usize = 56;
    let stack = logo::MARK_HEIGHT + GAP + art.height * scale;
    // checked_sub: tiny framebuffers must degrade, not underflow.
    if let Some(margin) = fb.height.checked_sub(stack) {
        // The mark — three petals orbiting a center, the O in i3mlOS.
        let mark_top = margin / 2;
        let mark_left = (fb.width.saturating_sub(logo::MARK_WIDTH)) / 2;
        for petal in logo::mark() {
            fb.blit(&petal.bitmap, mark_left + petal.dx, mark_top + petal.dy, 1, petal.color);
        }
        let banner_top = mark_top + logo::MARK_HEIGHT + GAP;
        fb.blit_centered(&art, banner_top, scale, (0xf5, 0xf0, 0xe6)); // warm white
        let _ = writeln!(out, "fb: {}x{} mark + banner drawn — i3mlOS", fb.width, fb.height);
    }
}

/// The first workload i3mlOS ever schedules. Four laws are demonstrated, and
/// each one is a claim the whole project rests on:
///   1. agents are the schedulable unit (not processes),
///   2. authority only ever narrows on delegation,
///   3. the irreversible parks for consent instead of proceeding,
///   4. a budget suspends rather than overspends.
fn run_first_agents(out: &mut SerialWriter) {
    gate::init();

    // --- Law 1: spawn an agent, not a process ---
    let filer = gate::spawn(
        "file invoices, draft reminders",
        0,
        Budget::new(1_000, 0), // 1000 micro-dollars
        1_000,
        agents::file_invoices,
    );
    let Some(read) = gate::grant(filer, agents::root_read()) else {
        let _ = writeln!(out, "agents: FAILED to grant read");
        return;
    };
    let _send = gate::grant(filer, agents::root_send());

    // --- Law 2: delegation attenuates. The child gets a narrower prefix... ---
    match gate::delegate(
        filer,
        "summarize the invoices",
        read,
        agents::narrowed_read(),
        200,
        0,
        agents::summarize,
    ) {
        Ok(child) => {
            let _ = writeln!(out, "aman: delegated to agent {child} under a narrower prefix");
        }
        Err(denied) => {
            let _ = writeln!(out, "aman: delegation refused: {denied:?}");
        }
    }
    // ...and a request that would WIDEN authority is refused. This is the
    // attenuation law, tested at runtime rather than asserted in a comment.
    match gate::attenuate(filer, read, agents::model_arabic()) {
        Err(nawa_aman::Denied::NotASubset) => {
            let _ = writeln!(out, "aman: refused to widen fs:read into model:arabic — correct");
        }
        other => {
            let _ = writeln!(out, "aman: ATTENUATION LAW BROKEN: {other:?}");
        }
    }
    // The subtler attack, found by review: a sibling path that merely shares
    // a textual prefix. "/inbox-archive" starts with "/inbox" but is not
    // inside it, and a string prefix test would have granted it.
    match gate::attenuate(filer, read, agents::sibling_read()) {
        Err(nawa_aman::Denied::NotASubset) => {
            let _ = writeln!(out, "aman: refused /inbox-archive as a child of /inbox — correct");
        }
        other => {
            let _ = writeln!(out, "aman: PATH BOUNDARY LAW BROKEN: {other:?}");
        }
    }

    // --- Law 4: a greedy agent on a small budget ---
    let greedy = gate::spawn("spend everything", 0, Budget::new(500, 0), 5_000, agents::spendthrift);
    let _ = gate::grant(greedy, agents::model_arabic());

    // Run until every agent is blocked, done, or suspended.
    let completed = gate::run_until_idle(64);
    let _ = writeln!(out, "sched: {completed} agents reached a terminal state");

    // --- Law 3: the human decides, once, about one thing ---
    for request in gate::pending_approvals() {
        let _ = writeln!(
            out,
            "consent: agent {} parked awaiting approval — kernel says \"{}\", agent says \"{}\"",
            request.agent,
            request.what.as_str(),
            request.note.as_str(),
        );
        // Stand-in for the consent surface; from Phase 2 only the kernel-owned
        // UI can call this.
        gate::answer_approval(request.id, true);
    }
    let completed = gate::run_until_idle(64);
    let _ = writeln!(out, "sched: {completed} more agents finished after consent");

    // The approval was spent on the send it named. Asking again with the same
    // capability must park again, not ride the answered request — the defect
    // an adversarial review found before any user could.
    if let Some(send) = gate::capability_of(filer, 1) {
        match gate::invoke(filer, send) {
            Err(nawa_aman::Denied::NeedsApproval) => {
                let _ = writeln!(out, "consent: second send refused — one yes, one action");
            }
            other => {
                let _ = writeln!(out, "consent: CONSENT LAW BROKEN: {other:?}");
            }
        }
    }

    // --- What the shell will one day show: `i3ml ps` ---
    for agent in gate::summaries() {
        let _ = writeln!(
            out,
            "agent {}: \"{}\" parent={} state={:?} crossings={} spent={}/{} µ$ caps={}",
            agent.id,
            agent.goal.as_str(),
            agent.parent,
            agent.state,
            agent.crossings,
            agent.spent_micro_dollars,
            agent.budget_micro_dollars,
            agent.capabilities,
        );
        if agent.state == State::Suspended {
            let _ = writeln!(out, "  ^ suspended by its budget — never overspent");
        }
    }

    // --- The journal: what the computer did, in its own words ---
    let _ = writeln!(out, "sijil: {} entries recorded", sijil::written());
    sijil::for_each(|entry| {
        let _ = writeln!(
            out,
            "  #{:<3} t={:>7}µs agent={} {:?}({}) {}",
            entry.sequence,
            entry.at,
            entry.agent,
            entry.event,
            entry.detail,
            entry.label.as_str(),
        );
    });
    let _ = writeln!(out, "agents: first agent scheduled by an original kernel — i3mel");
}

/// The first real hardware i3mlOS drives. Two devices, chosen for what they
/// unlock rather than for difficulty: entropy the kernel cannot invent for
/// itself, and a disk — the thing that turns the journal from a ring in RAM
/// into a record that survives the power going out.
fn bring_up_devices(out: &mut SerialWriter) {
    let mut found = 0;
    nawa_core::pci::scan(|device| {
        if device.vendor == nawa_virtio::transport::VENDOR_VIRTIO {
            found += 1;
            let _ = writeln!(
                out,
                "pci: virtio device {:#06x} at {}:{}.{}",
                device.device, device.address.bus, device.address.slot, device.address.function
            );
        }
    });
    let _ = writeln!(out, "pci: {found} virtio device(s) on the bus");

    match nawa_virtio::entropy::Entropy::open() {
        Some(mut entropy) => {
            let mut bytes = [0u8; 16];
            match entropy.read(&mut bytes) {
                Some(count) if count > 0 => {
                    let _ = write!(out, "virtio-rng: {count} bytes of real entropy —");
                    for byte in bytes.iter().take(count.min(8)) {
                        let _ = write!(out, " {byte:02x}");
                    }
                    let _ = writeln!(out);
                }
                _ => {
                    let _ = writeln!(out, "virtio-rng: device answered with nothing");
                }
            }
        }
        None => {
            let _ = writeln!(out, "virtio-rng: absent");
        }
    }

    match nawa_virtio::block::Block::open() {
        Some(mut disk) => {
            let _ = writeln!(
                out,
                "virtio-blk: {} sectors ({} MiB)",
                disk.sectors,
                disk.sectors * 512 / (1024 * 1024)
            );
            // Write a sector, read it back, and compare. Until this round
            // trip works, nothing the kernel remembers outlives a reboot.
            //
            // The LAST sector, not the first: the journal owns the disk from
            // sector 0 upward, and a scratch write over its superblock is
            // exactly how a record silently stops surviving reboots.
            let scratch = disk.sectors.saturating_sub(1);
            let mut written = [0u8; nawa_virtio::block::SECTOR_SIZE];
            let greeting = b"i3mlOS scratch sector -- i3mel";
            written[..greeting.len()].copy_from_slice(greeting);
            written[greeting.len()] = boot_marker();

            if !disk.write_sector(scratch, &written) {
                let _ = writeln!(out, "virtio-blk: write REFUSED");
                return;
            }
            let mut read_back = [0u8; nawa_virtio::block::SECTOR_SIZE];
            if !disk.read_sector(scratch, &mut read_back) {
                let _ = writeln!(out, "virtio-blk: read REFUSED");
                return;
            }
            if read_back[..greeting.len()] == greeting[..] {
                let _ = writeln!(out, "virtio-blk: wrote and read back a sector — storage works");
            } else {
                let _ = writeln!(out, "virtio-blk: READ BACK MISMATCH");
            }
            if disk.flush() {
                let _ = writeln!(out, "virtio-blk: flushed — the write is on the disk, not in a promise");
            }
            persist::run(disk, out);
        }
        None => {
            let _ = writeln!(out, "virtio-blk: absent");
        }
    }
}

/// A byte that differs per boot, so a sector written now is distinguishable
/// from one written last time.
fn boot_marker() -> u8 {
    (apic::ticks() & 0xff) as u8
}

/// Run an agent that is a WebAssembly module. Two things are proven here:
/// the tool's manifest is resolved against capabilities *before* it runs, and
/// a module whose manifest asks for something it was not granted is refused
/// at load — it never reaches its first instruction.
fn run_a_wasm_agent(out: &mut SerialWriter) {
    let Ok(module) = i3ml_wasm::Module::decode(&toolmod::MODULE) else {
        let _ = writeln!(out, "wasm: module REJECTED by the decoder");
        return;
    };
    let _ = writeln!(out, "wasm: module decoded — manifest:");
    for requirement in i3ml_wasm::manifest(&module) {
        let _ = writeln!(out, "  needs {}::{}", requirement.module, requirement.name);
    }

    let agent = gate::spawn("read the inbox, as a wasm tool", 0, Budget::new(400, 0), 2_000, |_| {
        nawa_gate::Progress::Done
    });
    let Some(read) = gate::grant(agent, agents::root_read()) else {
        return;
    };

    // Refused first: the same module, with nothing granted for `invoke`.
    match tool::bind(&module, agent, &[]) {
        Err(tool::LoadError::UnauthorizedImport) => {
            let _ = writeln!(out, "wasm: refused to load — imports invoke, was granted nothing");
        }
        _ => {
            let _ = writeln!(out, "wasm: MANIFEST LAW BROKEN — loaded without a grant");
        }
    }

    // Now bound properly: `invoke` resolves to the fs:read capability.
    let Ok(mut host) = tool::bind(&module, agent, &[("invoke", read)]) else {
        let _ = writeln!(out, "wasm: bind FAILED with a grant present");
        return;
    };
    match tool::run(&module, &mut host, "run", 100_000) {
        Ok(Some(value)) => {
            let _ = writeln!(
                out,
                "wasm: tool ran — {} of 3 invocations allowed, {} crossings, {} refusals",
                value.as_i64(),
                host.crossings,
                host.refusals
            );
        }
        Ok(None) => {
            let _ = writeln!(out, "wasm: tool ran with no result");
        }
        Err(trap) => {
            let _ = writeln!(out, "wasm: tool trapped: {trap:?}");
        }
    }
    let _ = writeln!(out, "wasm: an agent is a module now — i3mel");
}

/// The kernel side of a gate crossing from ring 3. Untrusted code has no
/// capability table entry of its own yet, so M2 implements the one verb that
/// needs none — and journals it, which is the point: the record of what
/// untrusted code did is written by the kernel, in kernel memory the
/// resident cannot address.
fn on_yard_crossing(verb: u64, argument: u64) -> u64 {
    const VERB_JOURNAL: u64 = 5;
    if verb == VERB_JOURNAL {
        let label =
            if argument == resident::GREETING { "ring3:journal" } else { "ring3:journal-other" };
        sijil::record(0, sijil::Event::Invoked, argument, label);
        return 0;
    }
    sijil::record(0, sijil::Event::Denied, verb, "ring3:unknown-verb");
    u64::MAX
}

/// Enter the yard. Two claims become hardware facts here — the whole reason
/// this milestone exists.
fn run_the_yard(out: &mut SerialWriter) {
    if !nawa_core::paging::active() {
        let _ = writeln!(out, "yard: skipped — no page tables of our own");
        return;
    }
    yard::set_crossing_hook(on_yard_crossing);
    if !yard::load(&resident::PROGRAM) {
        let _ = writeln!(out, "yard: FAILED to load the resident");
        return;
    }
    let _ = writeln!(out, "yard: resident loaded at {:#x}, dropping to ring 3", yard::CODE_ADDRESS);

    // Returns when the resident faults or exits; it never resumes.
    yard::enter();

    let _ = writeln!(
        out,
        "yard: back in the kernel — {} gate crossing(s) from ring 3, last verb {}",
        yard::crossings(),
        yard::last_verb()
    );
    if yard::crossings() > 0 {
        let _ = writeln!(out, "yard: untrusted code reached the kernel only through the gate");
    }
    // The journal recorded the crossing, in memory ring 3 cannot address.
    let mut ring3_entries = 0;
    sijil::for_each(|entry| {
        if entry.label.as_str().starts_with("ring3:") {
            ring3_entries += 1;
            let _ = writeln!(
                out,
                "  sijil #{} {:?}({:#x}) {}",
                entry.sequence,
                entry.event,
                entry.detail,
                entry.label.as_str()
            );
        }
    });
    let _ = writeln!(out, "yard: {ring3_entries} ring-3 crossing(s) in the journal — isolation is hardware now");
}
