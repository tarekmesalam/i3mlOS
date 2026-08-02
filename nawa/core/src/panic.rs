//! Kernel panic path: report on serial, ask QEMU to exit with the panic code,
//! otherwise halt. SIJIL will one day record this; serial is day-one truth.

use core::fmt::Write;
use core::panic::PanicInfo;

use crate::{arch, qemu, serial};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut out = serial::SerialWriter;
    let _ = write!(out, "\nKERNEL PANIC: {info}\n");
    qemu::exit(qemu::EXIT_PANIC);
    arch::halt_forever()
}
