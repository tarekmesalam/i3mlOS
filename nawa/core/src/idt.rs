//! Our own IDT. Exceptions are the CPU telling the truth about a bug; from
//! this module on, that truth lands on serial (and one day in SIJIL) instead
//! of a silent triple-fault reboot.

use core::arch::asm;
use core::fmt::Write;

use crate::cell::StaticCell;
use crate::{gdt, serial};

/// What the CPU pushes for an interrupt in long mode.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InterruptFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    options: u16,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}

const IDT_ENTRY_MISSING: IdtEntry =
    IdtEntry { offset_low: 0, selector: 0, options: 0, offset_mid: 0, offset_high: 0, _reserved: 0 };

impl IdtEntry {
    fn set(&mut self, handler_address: u64, ist_index: u8) {
        self.offset_low = handler_address as u16;
        self.offset_mid = (handler_address >> 16) as u16;
        self.offset_high = (handler_address >> 32) as u32;
        self.selector = gdt::KERNEL_CODE_SELECTOR;
        // present | interrupt gate (0xE) | IST index
        self.options = 0x8e00 | ist_index as u16;
    }
}

static IDT: StaticCell<[IdtEntry; 256]> = StaticCell::new([IDT_ENTRY_MISSING; 256]);

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

/// Vector numbers we install handlers for (Intel SDM vol. 3, ch. 6).
const DIVIDE_ERROR: usize = 0;
const BREAKPOINT: usize = 3;
const INVALID_OPCODE: usize = 6;
const DOUBLE_FAULT: usize = 8;
const GENERAL_PROTECTION: usize = 13;
const PAGE_FAULT: usize = 14;

/// Set by the supervisor: called from the timer interrupt, in interrupt
/// context, with interrupts still masked. Must be short and must not block.
static TIMER_HOOK: crate::cell::StaticCell<Option<fn()>> = crate::cell::StaticCell::new(None);

/// Register the scheduler's tick hook. Bring-up only, before interrupts are
/// enabled.
pub(crate) fn set_timer_hook(hook: fn()) {
    unsafe { *TIMER_HOOK.get() = Some(hook) }
}

/// Build and load the IDT. Called once, single-threaded, interrupts masked.
pub(crate) fn init() {
    unsafe {
        let idt = IDT.get();
        (*idt)[DIVIDE_ERROR].set(divide_error as *const () as u64, 0);
        (*idt)[BREAKPOINT].set(breakpoint as *const () as u64, 0);
        (*idt)[INVALID_OPCODE].set(invalid_opcode as *const () as u64, 0);
        (*idt)[DOUBLE_FAULT].set(double_fault as *const () as u64, gdt::DOUBLE_FAULT_IST_INDEX);
        (*idt)[GENERAL_PROTECTION].set(general_protection as *const () as u64, 0);
        (*idt)[PAGE_FAULT].set(page_fault as *const () as u64, 0);
        (*idt)[crate::apic::TIMER_VECTOR as usize].set(timer as *const () as u64, 0);

        let pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: idt as u64,
        };
        asm!("lidt [{}]", in(reg) &pointer, options(nostack, preserves_flags));
    }
}

/// The kernel's heartbeat. Rearm first so a slow hook cannot stall the clock,
/// then run the supervisor's tick, then acknowledge.
extern "x86-interrupt" fn timer(_frame: InterruptFrame) {
    crate::apic::record_tick();
    crate::apic::arm_next_deadline();
    let hook = unsafe { *TIMER_HOOK.get() };
    if let Some(hook) = hook {
        hook();
    }
    crate::apic::end_of_interrupt();
}

extern "x86-interrupt" fn divide_error(frame: InterruptFrame) {
    panic!("divide error at {:#x} (rsp {:#x})", frame.rip, frame.rsp);
}

/// The one exception that RESUMES: `int3` is our first proof that the CPU
/// takes our IDT seriously without killing the kernel.
extern "x86-interrupt" fn breakpoint(frame: InterruptFrame) {
    let mut out = serial::SerialWriter;
    let _ = writeln!(out, "int3: breakpoint handled at {:#x}", frame.rip);
}

extern "x86-interrupt" fn invalid_opcode(frame: InterruptFrame) {
    panic!("invalid opcode at {:#x}", frame.rip);
}

extern "x86-interrupt" fn double_fault(frame: InterruptFrame, _error_code: u64) -> ! {
    panic!("DOUBLE FAULT at {:#x} (rsp {:#x})", frame.rip, frame.rsp);
}

extern "x86-interrupt" fn general_protection(frame: InterruptFrame, error_code: u64) {
    if from_user(&frame) {
        kill_resident("general protection fault", frame.rip, error_code);
    }
    panic!("general protection fault at {:#x} (error {:#x})", frame.rip, error_code);
}

extern "x86-interrupt" fn page_fault(frame: InterruptFrame, error_code: u64) {
    let faulting_address: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) faulting_address, options(nomem, nostack, preserves_flags));
    }
    if from_user(&frame) {
        // Expected and load-bearing: a yard resident reached for memory it
        // does not have. The CPU refused before we did.
        let mut out = serial::SerialWriter;
        let _ = writeln!(
            out,
            "yard: resident faulted reaching {faulting_address:#x} from {:#x} (error {error_code:#x})",
            frame.rip
        );
        kill_resident("page fault", frame.rip, faulting_address);
    }
    panic!(
        "page fault: address {:#x}, error {:#x}, at {:#x}",
        faulting_address, error_code, frame.rip
    );
}

/// Did this fault come from ring 3? The CPU records the interrupted CS, and
/// its low two bits are the privilege level that was running.
fn from_user(frame: &InterruptFrame) -> bool {
    frame.cs & 3 == 3 && crate::yard::is_active()
}

/// Terminate the yard resident and return to kernel flow. The faulting
/// context is never resumed — that is what makes the isolation real rather
/// than advisory.
fn kill_resident(kind: &str, rip: u64, detail: u64) -> ! {
    let mut out = serial::SerialWriter;
    let _ = writeln!(out, "yard: killed resident — {kind} at {rip:#x} ({detail:#x})");
    // `leave` restores the caller's stack and registers itself.
    unsafe { crate::yard::leave() }
}
