//! x86_64 privileged instructions, wrapped as *purposeful* safe APIs.
//!
//! Raw port I/O is deliberately `pub(crate)`: the trusted core exposes what
//! the kernel may do (serial, QEMU exit, halt), never raw hardware access.

use core::arch::asm;

#[inline]
pub(crate) fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub(crate) fn outl(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub(crate) fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Mask maskable interrupts. Exceptions still fire — that is the point:
/// nothing preempts bring-up, but bugs still land in our IDT.
pub(crate) fn disable_interrupts() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

/// Halt the CPU forever. The kernel's terminal state until there is a scheduler.
pub fn halt_forever() -> ! {
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
