//! COM1 (16550 UART) driver. Serial is the kernel's channel of record: CI
//! boots QEMU headless and asserts on what this module emits.

use crate::arch::{inb, outb};

const COM1: u16 = 0x3f8;

/// Program COM1: 115200 baud, 8N1, FIFOs on, interrupts off (we poll).
pub fn init() {
    outb(COM1 + 1, 0x00); // disable UART interrupts
    outb(COM1 + 3, 0x80); // DLAB on
    outb(COM1, 0x01); // divisor low: 115200 baud
    outb(COM1 + 1, 0x00); // divisor high
    outb(COM1 + 3, 0x03); // 8 bits, no parity, one stop; DLAB off
    outb(COM1 + 2, 0xc7); // FIFO on, clear, 14-byte threshold
    outb(COM1 + 4, 0x0b); // DTR + RTS + OUT2
}

fn transmit_ready() -> bool {
    inb(COM1 + 5) & 0x20 != 0
}

pub fn write_byte(byte: u8) {
    while !transmit_ready() {}
    outb(COM1, byte);
}

pub fn write_str(s: &str) {
    for byte in s.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }
        write_byte(byte);
    }
}

/// `core::fmt` adapter so `write!` works over serial (used by the panic path).
pub struct SerialWriter;

impl core::fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_str(s);
        Ok(())
    }
}
