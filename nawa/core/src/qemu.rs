//! QEMU `isa-debug-exit` support. Under `cargo xtask test` the device sits at
//! port 0xf4 and QEMU's exit status becomes `(value << 1) | 1`. On real
//! hardware or a plain run the port write is a harmless no-op.

use crate::arch::outl;

const DEBUG_EXIT_PORT: u16 = 0xf4;

/// QEMU process exit status: (0x10 << 1) | 1 = 33. xtask asserts this.
pub const EXIT_SUCCESS: u32 = 0x10;
/// QEMU process exit status: (0x11 << 1) | 1 = 35.
pub const EXIT_PANIC: u32 = 0x11;

/// Ask QEMU to exit with the given code. Returns normally when the debug-exit
/// device is absent; callers must be prepared to continue.
pub fn exit(code: u32) {
    outl(DEBUG_EXIT_PORT, code);
}
