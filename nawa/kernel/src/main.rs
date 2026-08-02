//! The i3ml kernel. M0: leave the firmware, own GDT/IDT with exception
//! handlers, a frame allocator and heap over the UEFI memory map, and the
//! **إعمل** banner on the framebuffer.
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

mod banner;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use nawa_core::entry::{self, BootInfo};
use nawa_core::serial::SerialWriter;
use nawa_core::uefi::{EfiHandle, EfiStatus, EfiSystemTable};
use nawa_core::{qemu, selftest, serial};

/// The line `cargo xtask test` (and CI) greps for on the serial port.
const HELLO: &str = "hello from the i3ml kernel";

#[allow(unsafe_code)] // the entry-symbol attribute; see module docs
#[unsafe(export_name = "efi_main")]
extern "efiapi" fn efi_main(image_handle: EfiHandle, system_table: *mut EfiSystemTable) -> EfiStatus {
    entry::boot(image_handle, system_table, kmain)
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

    // The banner: an original kernel greeting in its own language.
    if let Some(fb) = boot.framebuffer {
        fb.clear(0x0b, 0x10, 0x21); // ink-dark blue
        let art = banner::banner();
        let scale = if fb.height >= art.height * 3 { 2 } else { 1 };
        // checked_sub: tiny framebuffers must degrade, not underflow.
        if let Some(margin) = fb.height.checked_sub(art.height * scale) {
            fb.blit_centered(&art, margin / 2, scale, (0xf5, 0xf0, 0xe6)); // warm white
            let _ = writeln!(out, "fb: {}x{} banner drawn — i3mel", fb.width, fb.height);
        }
    }

    serial::write_str("nawa: M0 complete, parking\n");
    qemu::exit(qemu::EXIT_SUCCESS);
    entry::park()
}
