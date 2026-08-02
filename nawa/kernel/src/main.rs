//! The i3ml kernel. Milestone: boot as a UEFI application on QEMU/OVMF and
//! say hello on serial (the line CI asserts) and the firmware console.
//!
//! Note what is NOT here: unsafe *operations*. The kernel builds on safe APIs
//! from the NAWA trusted core, and the framekernel rule keeps it that way.
//! (The single `unsafe(export_name)` below is an attribute, not an operation:
//! Rust flags it only because duplicate symbol names are a linker hazard, and
//! this image has exactly one `efi_main`.)

#![no_std]
#![no_main]
#![deny(unsafe_code)]

use nawa_core::entry::{self, BootInfo};
use nawa_core::uefi::{EfiHandle, EfiStatus, EfiSystemTable};
use nawa_core::{qemu, serial};

/// The line `cargo xtask test` (and CI) greps for on the serial port.
const HELLO: &str = "hello from the i3ml kernel";

#[allow(unsafe_code)] // the entry-symbol attribute; see module docs
#[unsafe(export_name = "efi_main")]
extern "efiapi" fn efi_main(image_handle: EfiHandle, system_table: *mut EfiSystemTable) -> EfiStatus {
    entry::boot(image_handle, system_table, kmain)
}

fn kmain(boot: BootInfo) -> ! {
    serial::write_str(HELLO);
    serial::write_str("\n");

    if let Some(mut console) = boot.console {
        console.write_line("");
        console.write_line(HELLO);
        console.write_line("i3mlOS / NAWA 0.0.1 — every line ours.");
    }

    // Under `cargo xtask test` this ends the run with the success status;
    // in a normal run the port write is ignored and we park.
    qemu::exit(qemu::EXIT_SUCCESS);
    entry::park()
}
