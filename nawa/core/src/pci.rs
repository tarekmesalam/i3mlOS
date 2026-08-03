//! PCI configuration space — how the kernel finds out what hardware exists.
//!
//! The legacy 0xCF8/0xCFC port pair rather than memory-mapped ECAM: it needs
//! no ACPI tables, no mappings, and works on every x86 machine and every
//! emulator. We only need to *find* devices; everything fast happens through
//! their BARs afterwards.

use crate::arch::{inl, outl};

const ADDRESS_PORT: u16 = 0xcf8;
const DATA_PORT: u16 = 0xcfc;

/// Where a device lives on the bus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Address {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

impl Address {
    fn selector(&self, offset: u8) -> u32 {
        0x8000_0000
            | (self.bus as u32) << 16
            | (self.slot as u32) << 11
            | (self.function as u32) << 8
            | (offset as u32 & 0xfc)
    }

    pub fn read32(&self, offset: u8) -> u32 {
        outl(ADDRESS_PORT, self.selector(offset));
        inl(DATA_PORT)
    }

    pub fn write32(&self, offset: u8, value: u32) {
        outl(ADDRESS_PORT, self.selector(offset));
        outl(DATA_PORT, value);
    }

    pub fn read16(&self, offset: u8) -> u16 {
        (self.read32(offset) >> ((offset as u32 & 2) * 8)) as u16
    }

    pub fn read8(&self, offset: u8) -> u8 {
        (self.read32(offset) >> ((offset as u32 & 3) * 8)) as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub address: Address,
    pub vendor: u16,
    pub device: u16,
}

const VENDOR_OFFSET: u8 = 0x00;
const COMMAND_OFFSET: u8 = 0x04;
const HEADER_TYPE_OFFSET: u8 = 0x0e;
const CAPABILITIES_POINTER_OFFSET: u8 = 0x34;
const BAR0_OFFSET: u8 = 0x10;

const COMMAND_MEMORY_SPACE: u32 = 1 << 1;
const COMMAND_BUS_MASTER: u32 = 1 << 2;

impl Device {
    /// Let the device read and write system memory (virtqueues live there)
    /// and respond to its memory-mapped registers. Firmware often leaves bus
    /// mastering off; a queue the device cannot read is a silent hang.
    pub fn enable(&self) {
        let command = self.address.read32(COMMAND_OFFSET);
        self.address
            .write32(COMMAND_OFFSET, command | COMMAND_MEMORY_SPACE | COMMAND_BUS_MASTER);
    }

    /// Physical address of a memory BAR, following the 64-bit form when the
    /// device uses one. Returns None for I/O-space or absent BARs.
    pub fn bar(&self, index: u8) -> Option<u64> {
        if index > 5 {
            return None;
        }
        let offset = BAR0_OFFSET + index * 4;
        let low = self.address.read32(offset);
        if low & 1 != 0 {
            return None; // I/O space
        }
        let base = (low & 0xffff_fff0) as u64;
        match (low >> 1) & 0b11 {
            0b00 => Some(base),
            0b10 => {
                let high = self.address.read32(offset + 4) as u64;
                Some(base | high << 32)
            }
            _ => None,
        }
    }

    /// Walk the capability list, handing each (id, offset) to `visit`.
    pub fn capabilities(&self, mut visit: impl FnMut(u8, u8)) {
        let mut offset = self.address.read8(CAPABILITIES_POINTER_OFFSET) & 0xfc;
        // Bounded: a device with a circular list must not hang the kernel.
        for _ in 0..48 {
            if offset == 0 {
                return;
            }
            let id = self.address.read8(offset);
            visit(id, offset);
            offset = self.address.read8(offset + 1) & 0xfc;
        }
    }
}

/// Visit every function present on bus 0. Enough for the virtual machines
/// i3mlOS targets; bridge recursion arrives with real hardware.
pub fn scan(mut visit: impl FnMut(Device)) {
    for slot in 0..32u8 {
        let base = Address { bus: 0, slot, function: 0 };
        let identity = base.read32(VENDOR_OFFSET);
        if identity == 0xffff_ffff {
            continue;
        }
        let multifunction = base.read8(HEADER_TYPE_OFFSET) & 0x80 != 0;
        let functions = if multifunction { 8 } else { 1 };
        for function in 0..functions {
            let address = Address { bus: 0, slot, function };
            let identity = address.read32(VENDOR_OFFSET);
            if identity == 0xffff_ffff {
                continue;
            }
            visit(Device {
                address,
                vendor: identity as u16,
                device: (identity >> 16) as u16,
            });
        }
    }
}
