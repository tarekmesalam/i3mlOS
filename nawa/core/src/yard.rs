//! **SAHA (الساحة) — the yard.** Ring 3, and with it the moment two of this
//! project's central claims stop being properties of our type system and
//! become properties of the CPU:
//!
//! * *capabilities cannot be forged* — untrusted code cannot reach the
//!   capability table at all; its only way to ask for anything is `syscall`,
//!   which lands in the gate.
//! * *the journal cannot be falsified* — SIJIL lives in kernel memory that a
//!   yard resident cannot address, let alone write.
//!
//! M2 shape (master plan §3): one shared flat user address space for all
//! untrusted code, entered only through the gate. Per-agent address spaces
//! are a Phase 3 milestone, honestly costed there — not a retrofit, because
//! untrusted code is separately-loaded user code from this milestone onward.

use core::arch::{asm, naked_asm};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::cell::StaticCell;
use crate::{gdt, paging, serial};

const IA32_EFER: u32 = 0xc000_0080;
const IA32_STAR: u32 = 0xc000_0081;
const IA32_LSTAR: u32 = 0xc000_0082;
const IA32_FMASK: u32 = 0xc000_0084;
const EFER_SYSCALL_ENABLE: u64 = 1 << 0;
const RFLAGS_INTERRUPT_ENABLE: u64 = 1 << 9;

/// Where the yard's code and stack live in the user address space.
pub const CODE_ADDRESS: u64 = paging::YARD_BASE;
pub const STACK_ADDRESS: u64 = paging::YARD_BASE + 0x2000;
const STACK_TOP: u64 = STACK_ADDRESS + 0x1000 - 16;

/// State for the ring 0 <-> ring 3 transition. Single core: statics are the
/// whole story until SMP, where these become per-CPU.
///
/// The yard runs on a **dedicated kernel stack**, not on the caller's. That
/// is not tidiness: it means leaving the yard depends on nothing but these
/// four words, so no caller's frame layout, codegen choice, or return-address
/// assumption can affect whether the kernel comes back.
static KERNEL_RETURN_STACK: AtomicU64 = AtomicU64::new(0);
/// Where to resume in the kernel — captured explicitly, never inferred.
static KERNEL_RETURN_RIP: AtomicU64 = AtomicU64::new(0);
/// The caller's stack pointer, restored before we return to it.
static CALLER_STACK: AtomicU64 = AtomicU64::new(0);
static USER_STACK: AtomicU64 = AtomicU64::new(0);
static SYSCALL_STACK: StaticCell<[u8; 32 * 1024]> = StaticCell::new([0; 32 * 1024]);
static YARD_STACK: StaticCell<[u8; 32 * 1024]> = StaticCell::new([0; 32 * 1024]);
/// Top of the yard's dedicated kernel stack, materialized for the asm.
static YARD_STACK_TOP: AtomicU64 = AtomicU64::new(0);

/// What a yard resident asked for. Verb numbers follow the eight-verb ABI
/// (master plan §3); M2 implements the ones that need no capability table
/// entry of their own, and the rest return `Denied`.
static LAST_VERB: AtomicU64 = AtomicU64::new(u64::MAX);
static CROSSINGS: AtomicU64 = AtomicU64::new(0);

fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack, preserves_flags),
        );
    }
}

fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
    }
    (high as u64) << 32 | low as u64
}

/// Arm the `syscall`/`sysret` path. Called once during bring-up.
pub(crate) fn init() {
    write_msr(IA32_EFER, read_msr(IA32_EFER) | EFER_SYSCALL_ENABLE);
    // STAR: [63:48] = sysret selector base, [47:32] = syscall selector base.
    let star = ((gdt::STAR_USER_BASE as u64) << 48) | ((gdt::KERNEL_CODE_SELECTOR as u64) << 32);
    write_msr(IA32_STAR, star);
    write_msr(IA32_LSTAR, syscall_entry as *const () as u64);
    // Clear IF on entry: the gate runs with interrupts masked, so a timer
    // tick cannot land between the stack switch and the first saved register.
    write_msr(IA32_FMASK, RFLAGS_INTERRUPT_ENABLE);
}

/// The syscall trampoline. `syscall` does not switch stacks — the CPU leaves
/// us on the *user's* stack with the return address in rcx and flags in r11 —
/// so the first job is to get off it before touching anything else.
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
        // Park the user stack, take the kernel's.
        "mov [rip + {user_stack}], rsp",
        "mov rsp, [rip + {syscall_stack}]",
        // Preserve what sysretq needs, plus the C-clobbered registers we use.
        "push rcx",
        "push r11",
        "push rdi",
        "push rsi",
        // Verb in rax, argument in rdi (the ABI is plain data: no pointers
        // cross the gate).
        "mov rsi, rdi",
        "mov rdi, rax",
        "call {dispatch}",
        "pop rsi",
        "pop rdi",
        "pop r11",
        "pop rcx",
        "mov rsp, [rip + {user_stack}]",
        "sysretq",
        user_stack = sym USER_STACK,
        syscall_stack = sym SYSCALL_STACK_TOP,
        dispatch = sym dispatch,
    )
}

/// Top of the syscall stack, materialized as a static so the trampoline can
/// load it with a single RIP-relative move.
static SYSCALL_STACK_TOP: AtomicU64 = AtomicU64::new(0);

/// Handle a gate crossing from ring 3. Returns the value the resident sees
/// in rax.
///
/// **`sysv64`, not `extern "C"`.** On `x86_64-unknown-uefi` the C ABI is
/// Microsoft x64: arguments arrive in rcx/rdx and the caller owes 32 bytes of
/// shadow space. The trampoline above hands over rdi/rsi, and rcx is the
/// user's return address — passing it as "the verb" is precisely the bug this
/// annotation prevents.
extern "sysv64" fn dispatch(verb: u64, argument: u64) -> u64 {
    CROSSINGS.fetch_add(1, Ordering::Relaxed);
    LAST_VERB.store(verb, Ordering::Relaxed);
    if let Some(handler) = unsafe { *CROSSING_HOOK.get() } {
        return handler(verb, argument);
    }
    0
}

/// The gate installs this: the kernel-side implementation of the eight verbs.
/// Kept as a hook so `nawa-core` never depends on the gate crate.
static CROSSING_HOOK: StaticCell<Option<fn(u64, u64) -> u64>> = StaticCell::new(None);

pub fn set_crossing_hook(hook: fn(u64, u64) -> u64) {
    unsafe { *CROSSING_HOOK.get() = Some(hook) }
}

/// Load a resident into the yard: copy its code into user-mapped memory and
/// give it a stack. Returns false if the mapping could not be made.
pub fn load(program: &[u8]) -> bool {
    let Some(code_frame) = paging::map_yard_page(CODE_ADDRESS, false) else {
        return false;
    };
    if paging::map_yard_page(STACK_ADDRESS, true).is_none() {
        return false;
    }
    if program.len() > crate::uefi::PAGE_SIZE {
        return false;
    }
    // The frame is identity-mapped for the kernel, so it can be written here
    // even though the yard sees it at YARD_BASE.
    unsafe {
        core::ptr::copy_nonoverlapping(program.as_ptr(), code_frame as *mut u8, program.len());
    }
    SYSCALL_STACK_TOP.store(SYSCALL_STACK.get() as u64 + 32 * 1024 - 16, Ordering::Relaxed);
    YARD_STACK_TOP.store(YARD_STACK.get() as u64 + 32 * 1024 - 16, Ordering::Relaxed);
    true
}

/// Drop to ring 3 and run the resident. Returns when the yard is left — by a
/// fault, or by the resident asking to exit.
///
/// The mechanism is a deliberate one-way door: `iretq` in, and the only ways
/// back are through the gate or through the CPU reporting a fault.
pub fn enter() {
    unsafe { enter_ring3() }
    // Reopened only once we are fully back on the caller's stack.
    crate::arch::enable_interrupts();
}

/// Takes no arguments on purpose. The entry point and stack are compile-time
/// constants of the yard's layout, so they are materialized inside the asm
/// rather than passed — a naked function is exactly where you do not want to
/// be debugging a register-passing mismatch.
#[unsafe(naked)]
unsafe extern "C" fn enter_ring3() {
    naked_asm!(
        // Preserve the caller's world on the caller's stack, then leave it
        // entirely: everything below runs on the yard's own stack, so the
        // path back cannot depend on the caller's frame.
        //
        // The register set is the **Windows x64** non-volatile set — rbx,
        // rbp, rdi, rsi, r12-r15 — because that is this target's C ABI.
        // Saving only the SysV set (no rdi/rsi) and then zeroing them for
        // ring 3 silently corrupted the caller's live values on return.
        "push rbp",
        "push rbx",
        "push rdi",
        "push rsi",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rip + {caller_stack}], rsp",
        // The continuation is chosen here, explicitly — never inferred from
        // a return address that a stack switch might invalidate.
        "lea rax, [rip + 2f]",
        "mov [rip + {return_rip}], rax",
        "mov rsp, [rip + {yard_stack}]",
        "mov [rip + {return_stack}], rsp",
        // Build the interrupt frame ring 3 expects, pushed in reverse pop
        // order: SS, RSP, RFLAGS, CS, RIP.
        "mov rax, {user_data}",
        "push rax",
        "mov rax, {stack_top}",
        "push rax",
        "push {rflags}",
        "mov rax, {user_code}",
        "push rax",
        "mov rax, {entry}",
        "push rax",
        // Registers a resident must not inherit from the kernel.
        "xor rax, rax",
        "xor rbx, rbx",
        "xor rcx, rcx",
        "xor rdx, rdx",
        "xor rsi, rsi",
        "xor rdi, rdi",
        "xor rbp, rbp",
        "iretq",
        // Reached only via `leave`, with rsp already back on the caller's
        // stack and interrupts still masked.
        "2:",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rsi",
        "pop rdi",
        "pop rbx",
        "pop rbp",
        "ret",
        caller_stack = sym CALLER_STACK,
        return_rip = sym KERNEL_RETURN_RIP,
        return_stack = sym KERNEL_RETURN_STACK,
        yard_stack = sym YARD_STACK_TOP,
        user_data = const gdt::USER_DATA_SELECTOR as u64,
        user_code = const gdt::USER_CODE_SELECTOR as u64,
        rflags = const RFLAGS_INTERRUPT_ENABLE,
        entry = const CODE_ADDRESS,
        stack_top = const STACK_TOP,
    )
}

/// Abandon the yard and resume kernel execution at [`enter`]'s caller.
///
/// Called from the fault handler: the user context is not resumed, so the
/// interrupt frame is simply dropped. Interrupts are re-enabled because we
/// are returning to ordinary kernel flow, not `iretq`-ing.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn leave() -> ! {
    naked_asm!(
        // Mark the yard closed first — but keep the value we need in a
        // register, so clearing it can never strand the jump.
        "mov rax, [rip + {caller_stack}]",
        "mov qword ptr [rip + {return_stack}], 0",
        // Back onto the caller's stack, then to the continuation inside
        // `enter_ring3`, which restores the callee-saved registers.
        "mov rsp, rax",
        "jmp qword ptr [rip + {return_rip}]",
        caller_stack = sym CALLER_STACK,
        return_stack = sym KERNEL_RETURN_STACK,
        return_rip = sym KERNEL_RETURN_RIP,
    )
}

/// Is the yard currently the thing that is running? Used by the fault
/// handlers to decide between "kill the resident" and "panic the kernel".
pub(crate) fn is_active() -> bool {
    KERNEL_RETURN_STACK.load(Ordering::Relaxed) != 0
}

pub fn crossings() -> u64 {
    CROSSINGS.load(Ordering::Relaxed)
}

pub fn last_verb() -> u64 {
    LAST_VERB.load(Ordering::Relaxed)
}

pub fn report(message: &str) {
    serial::write_str("yard: ");
    serial::write_str(message);
    serial::write_str("\n");
}
