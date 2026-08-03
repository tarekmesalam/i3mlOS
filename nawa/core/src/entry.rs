//! Boot handoff. The kernel binary stays free of unsafe *operations* by
//! delegating the raw firmware pointers here: it declares `efi_main`, then
//! immediately calls [`boot`], which brings the machine up and hands a safe
//! [`BootInfo`] to `kmain`. From `ExitBootServices` onward, every instruction
//! that runs is ours.

use crate::{apic, arch, fb, gdt, heap, idt, mem, serial, uefi};

/// Everything the safe kernel gets to touch after bring-up.
pub struct BootInfo {
    pub framebuffer: Option<fb::Framebuffer>,
    /// Total EFI_CONVENTIONAL_MEMORY in the final map.
    pub usable_bytes: u64,
    /// Bytes actually managed by the frame allocator (largest region).
    pub managed_bytes: u64,
    pub heap_ok: bool,
    /// TSC ticks per microsecond (0 = no timer; the kernel runs untimed).
    pub tsc_per_microsecond: u64,
}

/// Timer period. 1 kHz: fine enough for budget accounting, coarse enough
/// that the tick itself costs nothing measurable.
const TICK_MICROSECONDS: u64 = 1_000;

/// Trusted-core boundary: `image_handle`/`system_table` must be the values
/// UEFI passed to `efi_main`, forwarded untouched.
///
/// Bring-up order matters and is the whole story of M0:
/// 1. serial (so every later step can report),
/// 2. GOP framebuffer (must be found while boot services live),
/// 3. `ExitBootServices` — the firmware is gone; the machine is ours,
/// 4. interrupts masked, our GDT + TSS, our IDT (exceptions now land on us),
/// 5. frame allocator over the final memory map, then the kernel heap.
pub fn boot(
    image_handle: uefi::EfiHandle,
    system_table: *mut uefi::EfiSystemTable,
    timer_hook: fn(),
    kmain: fn(BootInfo) -> !,
) -> ! {
    serial::init();
    serial::write_str("nawa: serial up\n");

    if let Some(mut console) = uefi::Console::from_system_table(system_table) {
        console.write_line("i3mlOS / NAWA — leaving the firmware behind...");
    }

    let framebuffer = uefi::locate_framebuffer(system_table);
    serial::write_str(if framebuffer.is_some() {
        "nawa: framebuffer located\n"
    } else {
        "nawa: framebuffer: none\n"
    });

    let Some(memory_map) = uefi::exit_boot_services(image_handle, system_table) else {
        panic!("ExitBootServices failed");
    };
    serial::write_str("nawa: exit_boot_services ok — the machine is ours\n");

    arch::disable_interrupts();
    gdt::init();
    idt::init();
    serial::write_str("nawa: gdt+tss+idt loaded\n");

    let stats = mem::init(memory_map);
    let heap_ok = heap::init();

    // The heartbeat comes last: nothing may preempt bring-up, and the
    // scheduler's hook must be installed before the first tick can land.
    idt::set_timer_hook(timer_hook);
    let tsc_per_microsecond = apic::init(TICK_MICROSECONDS);
    if tsc_per_microsecond != 0 {
        arch::enable_interrupts();
        serial::write_str("nawa: apic timer armed — the kernel has a heartbeat\n");
    } else {
        serial::write_str("nawa: no apic timer; running untimed\n");
    }

    kmain(BootInfo {
        framebuffer,
        usable_bytes: stats.usable_bytes,
        managed_bytes: stats.managed_bytes,
        heap_ok,
        tsc_per_microsecond,
    })
}

/// Terminal state for a kernel with no scheduler yet.
pub fn park() -> ! {
    arch::halt_forever()
}
