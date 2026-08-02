//! Our own GDT + TSS, replacing the firmware's the moment boot services end.
//! Long mode needs almost nothing from segmentation — a 64-bit code segment,
//! a data segment, and a TSS so the double-fault handler gets a known-good
//! stack (IST1) even if the main stack is the thing that broke.

use core::arch::asm;

use crate::cell::StaticCell;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x18;

/// IST index (1-based in IDT entries) for the double-fault stack.
pub const DOUBLE_FAULT_IST_INDEX: u8 = 1;

const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;
static DOUBLE_FAULT_STACK: StaticCell<[u8; DOUBLE_FAULT_STACK_SIZE]> =
    StaticCell::new([0; DOUBLE_FAULT_STACK_SIZE]);

#[repr(C, packed(4))]
struct TaskStateSegment {
    _reserved0: u32,
    rsp: [u64; 3],
    _reserved1: u64,
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    iomap_base: u16,
}

static TSS: StaticCell<TaskStateSegment> = StaticCell::new(TaskStateSegment {
    _reserved0: 0,
    rsp: [0; 3],
    _reserved1: 0,
    ist: [0; 7],
    _reserved2: 0,
    _reserved3: 0,
    iomap_base: core::mem::size_of::<TaskStateSegment>() as u16,
});

/// GDT: null, 64-bit kernel code, kernel data, TSS (16-byte descriptor).
static GDT: StaticCell<[u64; 5]> = StaticCell::new([0; 5]);

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

/// Build and load the GDT, reload all segment registers, load the TSS.
/// Called once, single-threaded, interrupts masked.
pub(crate) fn init() {
    unsafe {
        // Double-fault stack grows down from the top (16-byte aligned).
        let stack_top =
            (DOUBLE_FAULT_STACK.get() as u64 + DOUBLE_FAULT_STACK_SIZE as u64) & !0xf;
        (*TSS.get()).ist[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = stack_top;

        let tss_base = TSS.get() as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        // 16-byte system descriptor, type 0x9 = available 64-bit TSS.
        let tss_low = (tss_limit & 0xffff)
            | ((tss_base & 0xff_ffff) << 16)
            | (0x89u64 << 40)
            | ((tss_limit & 0xf_0000) << 32)
            | (((tss_base >> 24) & 0xff) << 56);
        let tss_high = tss_base >> 32;

        let gdt = GDT.get();
        (*gdt)[0] = 0;
        (*gdt)[1] = 0x00af_9b00_0000_ffff; // code: present, exec, L=1
        (*gdt)[2] = 0x00cf_9300_0000_ffff; // data: present, writable
        (*gdt)[3] = tss_low;
        (*gdt)[4] = tss_high;

        let pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<[u64; 5]>() - 1) as u16,
            base: gdt as u64,
        };
        asm!("lgdt [{}]", in(reg) &pointer, options(nostack, preserves_flags));

        // Reload CS with a far return, then the data segment registers.
        asm!(
            "push {code_sel}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {data_sel:x}",
            "mov es, {data_sel:x}",
            "mov ss, {data_sel:x}",
            "xor eax, eax",
            "mov fs, ax",
            "mov gs, ax",
            code_sel = in(reg) KERNEL_CODE_SELECTOR as u64,
            data_sel = in(reg) KERNEL_DATA_SELECTOR as u64,
            // `out`, NOT `lateout`: data_sel is still read after the lea
            // writes tmp, so they must never share a register (review
            // finding, M0 — lateout permits exactly that aliasing).
            tmp = out(reg) _,
            out("rax") _,
        );

        asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nostack, preserves_flags));
    }
}
