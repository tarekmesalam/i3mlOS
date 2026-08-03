//! Volatile access to physical memory — device registers and the buffers
//! devices read.
//!
//! This module exists so that drivers do not need `unsafe`. The framekernel
//! rule says raw memory belongs to the trusted core; a virtio driver is a
//! protocol, not a memory-safety argument, and should read like one.
//!
//! Every access is bounds-checked against what paging actually mapped, so a
//! wrong address is a refused write rather than a wild one. Volatility is not
//! optional here: on the other side of these words is a device that changes
//! them while we are not looking.

use crate::paging;

fn in_range(address: u64, length: usize) -> bool {
    let Some(end) = address.checked_add(length as u64) else {
        return false;
    };
    end <= paging::mapped_bytes() || paging::in_device_window(address, length)
}

pub fn read8(address: u64) -> u8 {
    if !in_range(address, 1) {
        return 0;
    }
    unsafe { (address as *const u8).read_volatile() }
}

pub fn read16(address: u64) -> u16 {
    if !in_range(address, 2) {
        return 0;
    }
    unsafe { (address as *const u16).read_volatile() }
}

pub fn read32(address: u64) -> u32 {
    if !in_range(address, 4) {
        return 0;
    }
    unsafe { (address as *const u32).read_volatile() }
}

pub fn write8(address: u64, value: u8) {
    if in_range(address, 1) {
        unsafe { (address as *mut u8).write_volatile(value) }
    }
}

pub fn write16(address: u64, value: u16) {
    if in_range(address, 2) {
        unsafe { (address as *mut u16).write_volatile(value) }
    }
}

pub fn write32(address: u64, value: u32) {
    if in_range(address, 4) {
        unsafe { (address as *mut u32).write_volatile(value) }
    }
}

/// Copy out of physical memory into a slice.
pub fn read_into(address: u64, out: &mut [u8]) {
    if !in_range(address, out.len()) {
        return;
    }
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = unsafe { (address as *const u8).add(index).read_volatile() };
    }
}

/// Copy a slice into physical memory.
pub fn write_from(address: u64, data: &[u8]) {
    if !in_range(address, data.len()) {
        return;
    }
    for (index, byte) in data.iter().enumerate() {
        unsafe { (address as *mut u8).add(index).write_volatile(*byte) }
    }
}

pub fn zero(address: u64, length: usize) {
    if !in_range(address, length) {
        return;
    }
    unsafe {
        core::ptr::write_bytes(address as *mut u8, 0, length);
    }
}

/// Write a plain-data value a device will read. `T` must be `repr(C)` with a
/// layout the device expects — that is the caller's protocol obligation, not
/// a memory-safety one.
pub fn write_struct<T: Copy>(address: u64, value: T) {
    if !in_range(address, core::mem::size_of::<T>()) {
        return;
    }
    unsafe {
        (address as *mut T).write_volatile(value);
    }
}

pub fn read_struct<T: Copy + Default>(address: u64) -> T {
    if !in_range(address, core::mem::size_of::<T>()) {
        return T::default();
    }
    unsafe { (address as *const T).read_volatile() }
}
