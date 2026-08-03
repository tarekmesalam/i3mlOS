//! Our own page tables — the precondition for the yard.
//!
//! Until now the kernel ran on the firmware's identity map, where every page
//! is supervisor-only. Ring 3 needs pages marked user-accessible, and marking
//! them means owning the tables. So we build a fresh 4-level map:
//!
//! * **Identity, supervisor-only** (`U=0`) for all physical memory: the
//!   kernel keeps working exactly as before, and user code can never reach
//!   kernel memory *through* the identity map — the CPU refuses at the walk.
//! * **The yard** at a distant virtual base, `U=1` at every level, backed by
//!   frames the kernel allocates. Its PML4 entry is its own, so no user bit
//!   is ever shared with the identity map.
//!
//! 2 MiB pages for the identity map (six frames of tables for 4 GiB instead
//! of thousands), 4 KiB pages for the yard, where precision matters.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::cell::StaticCell;
use crate::mem;
use crate::uefi::{MemoryMap, PAGE_SIZE};

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER: u64 = 1 << 2;
const WRITE_THROUGH: u64 = 1 << 3;
const CACHE_DISABLE: u64 = 1 << 4;
const HUGE: u64 = 1 << 7;
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;

const ENTRIES: usize = 512;
const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

/// Physical memory at or above this line is device territory on a PC (local
/// APIC at 0xfee00000, IOAPIC, HPET, flash). Mapped cache-disabled so MMIO
/// register writes reach the device instead of sitting in a cache line.
const DEVICE_WINDOW: u64 = 0xf000_0000;

/// Virtual base of the yard: PML4 slot 32, far from the identity map so the
/// user bit is never shared with kernel mappings.
pub const YARD_BASE: u64 = 0x0000_1000_0000_0000;

static PML4_PHYSICAL: AtomicU64 = AtomicU64::new(0);
/// How much physical memory the identity map covers. Everything that pokes
/// hardware checks against this, so a bad address is refused, not wild.
static MAPPED_BYTES: AtomicU64 = AtomicU64::new(0);

fn allocate_table() -> Option<u64> {
    let frame = mem::allocate_frames(1)? as u64;
    // We are identity-mapped while building, so physical == virtual here.
    unsafe {
        core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE);
    }
    Some(frame)
}

fn entry(table: u64, index: usize) -> u64 {
    unsafe { (table as *const u64).add(index).read() }
}

fn set_entry(table: u64, index: usize, value: u64) {
    unsafe {
        (table as *mut u64).add(index).write(value);
    }
}

/// Build the kernel's address space and switch to it. Returns the number of
/// GiB identity-mapped, or 0 on failure (in which case CR3 is untouched and
/// the firmware map stays live).
pub(crate) fn init(map: MemoryMap, framebuffer_end: u64) -> u64 {
    // Cover all RAM, the framebuffer, and the device window — never less
    // than 4 GiB so the local APIC is always reachable.
    //
    // One PDPT holds 512 entries, so this map spans at most 512 GiB. Firmware
    // memory maps routinely describe regions far above installed RAM; without
    // the clamp their addresses would drive the loop past the end of the
    // table and corrupt whatever follows it.
    const MAX_GIB: u64 = ENTRIES as u64;
    let mut highest = 4 * GIB;
    for region in map.regions() {
        let end = region.physical_start.saturating_add(region.pages * PAGE_SIZE as u64);
        if end <= MAX_GIB * GIB {
            highest = highest.max(end);
        }
    }
    highest = highest.max(framebuffer_end.min(MAX_GIB * GIB));
    let gibibytes = highest.div_ceil(GIB).min(MAX_GIB);

    let Some(pml4) = allocate_table() else { return 0 };
    let Some(pdpt) = allocate_table() else { return 0 };
    set_entry(pml4, 0, pdpt | PRESENT | WRITABLE);

    for gibibyte in 0..gibibytes {
        let Some(directory) = allocate_table() else { return 0 };
        set_entry(pdpt, gibibyte as usize, directory | PRESENT | WRITABLE);
        for index in 0..ENTRIES {
            let physical = gibibyte * GIB + index as u64 * HUGE_PAGE_SIZE;
            let mut flags = PRESENT | WRITABLE | HUGE;
            if physical >= DEVICE_WINDOW {
                flags |= CACHE_DISABLE | WRITE_THROUGH;
            }
            set_entry(directory, index, physical | flags);
        }
    }

    PML4_PHYSICAL.store(pml4, Ordering::Relaxed);
    MAPPED_BYTES.store(gibibytes * GIB, Ordering::Relaxed);
    unsafe {
        asm!("mov cr3, {}", in(reg) pml4, options(nostack, preserves_flags));
    }
    gibibytes
}

/// Map one 4 KiB frame into the yard, user-accessible. Intermediate tables
/// are created on demand and carry the user bit — which is why the yard has
/// its own PML4 slot: no kernel mapping is ever exposed by these writes.
pub(crate) fn map_yard_page(virtual_address: u64, writable: bool) -> Option<u64> {
    let pml4 = PML4_PHYSICAL.load(Ordering::Relaxed);
    if pml4 == 0 {
        return None;
    }
    let indices = [
        (virtual_address >> 39) as usize & 0x1ff,
        (virtual_address >> 30) as usize & 0x1ff,
        (virtual_address >> 21) as usize & 0x1ff,
        (virtual_address >> 12) as usize & 0x1ff,
    ];

    let mut table = pml4;
    for level in 0..3 {
        let existing = entry(table, indices[level]);
        let next = if existing & PRESENT == 0 {
            let fresh = allocate_table()?;
            set_entry(table, indices[level], fresh | PRESENT | WRITABLE | USER);
            fresh
        } else {
            // Ensure the path stays user-walkable.
            set_entry(table, indices[level], existing | USER);
            existing & ADDRESS_MASK
        };
        table = next;
    }

    let frame = mem::allocate_frames(1)? as u64;
    unsafe {
        core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE);
    }
    let mut flags = PRESENT | USER;
    if writable {
        flags |= WRITABLE;
    }
    set_entry(table, indices[3], frame | flags);
    flush(virtual_address);
    Some(frame)
}

fn flush(virtual_address: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags));
    }
}

/// Device registers can live far outside RAM: a virtio BAR under OVMF lands
/// at 768 GiB, which no sane identity map reaches. Rather than mapping half a
/// terabyte, the kernel maps device windows on demand and remembers them, so
/// [`crate::mmio`] can still tell a legitimate register from a wild address.
const MAX_DEVICE_WINDOWS: usize = 8;
static DEVICE_WINDOWS: StaticCell<[(u64, u64); MAX_DEVICE_WINDOWS]> =
    StaticCell::new([(0, 0); MAX_DEVICE_WINDOWS]);
static DEVICE_WINDOW_COUNT: AtomicU64 = AtomicU64::new(0);

/// Identity-map a device's registers, cache-disabled. 2 MiB pages, allocating
/// whatever intermediate tables the address needs — a BAR may sit in a
/// PML4 slot the identity map never touched.
pub fn map_device(physical: u64, length: usize) -> bool {
    let pml4 = PML4_PHYSICAL.load(Ordering::Relaxed);
    if pml4 == 0 || length == 0 {
        return false;
    }
    let start = physical & !(HUGE_PAGE_SIZE - 1);
    let Some(end) = physical.checked_add(length as u64) else {
        return false;
    };
    let end = end.div_ceil(HUGE_PAGE_SIZE) * HUGE_PAGE_SIZE;

    let mut page = start;
    while page < end {
        let pml4_index = (page >> 39) as usize & 0x1ff;
        let pdpt_index = (page >> 30) as usize & 0x1ff;
        let directory_index = (page >> 21) as usize & 0x1ff;

        let existing = entry(pml4, pml4_index);
        let pdpt = if existing & PRESENT == 0 {
            let Some(fresh) = allocate_table() else { return false };
            set_entry(pml4, pml4_index, fresh | PRESENT | WRITABLE);
            fresh
        } else {
            existing & ADDRESS_MASK
        };

        let existing = entry(pdpt, pdpt_index);
        let directory = if existing & PRESENT == 0 {
            let Some(fresh) = allocate_table() else { return false };
            set_entry(pdpt, pdpt_index, fresh | PRESENT | WRITABLE);
            fresh
        } else {
            existing & ADDRESS_MASK
        };

        set_entry(
            directory,
            directory_index,
            page | PRESENT | WRITABLE | HUGE | CACHE_DISABLE | WRITE_THROUGH,
        );
        page += HUGE_PAGE_SIZE;
    }

    let count = DEVICE_WINDOW_COUNT.load(Ordering::Relaxed) as usize;
    if count < MAX_DEVICE_WINDOWS {
        unsafe {
            (*DEVICE_WINDOWS.get())[count] = (start, end);
        }
        DEVICE_WINDOW_COUNT.store(count as u64 + 1, Ordering::Relaxed);
    }
    // Reload CR3 rather than tracking every page: mapping devices happens
    // a handful of times at bring-up, and correctness beats cleverness.
    unsafe {
        let value: u64;
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
        asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags));
    }
    true
}

/// Is this range inside a device window the kernel deliberately mapped?
pub fn in_device_window(address: u64, length: usize) -> bool {
    let Some(end) = address.checked_add(length as u64) else {
        return false;
    };
    let count = DEVICE_WINDOW_COUNT.load(Ordering::Relaxed) as usize;
    let windows = unsafe { *DEVICE_WINDOWS.get() };
    windows.iter().take(count).any(|(start, stop)| address >= *start && end <= *stop)
}

/// Upper bound of the identity map, in bytes.
pub fn mapped_bytes() -> u64 {
    MAPPED_BYTES.load(Ordering::Relaxed)
}

pub fn active() -> bool {
    PML4_PHYSICAL.load(Ordering::Relaxed) != 0
}
