//! The yard's first resident — the first code i3mlOS ever runs that the
//! kernel does not trust.
//!
//! Hand-assembled rather than compiled: a dozen instructions with no
//! relocations, no linker involvement, and nothing hidden. Every byte is
//! accounted for below. When the WASM interpreter lands (Phase 2), residents
//! become modules and this array retires.
//!
//! What it does, in order:
//!   1. crosses the gate with verb 5 (`journal`) — proving ring 3 can reach
//!      the kernel *only* through `syscall`,
//!   2. reads a kernel address — proving it cannot reach kernel memory at
//!      all. The CPU faults; the kernel kills it; the yard is left.
//!
//! Step 2 is the milestone. Until now "untrusted code cannot touch the
//! journal or the capability table" was a property of Rust's type system.
//! After it, it is a property of the page tables.

/// A kernel address that is certainly mapped supervisor-only: the identity
/// map covers all physical memory with `U=0`.
const KERNEL_ADDRESS: u32 = 0x0010_0000;

/// Verb 5 = `journal` in the eight-verb ABI.
const VERB_JOURNAL: u32 = 5;

#[rustfmt::skip]
pub static PROGRAM: [u8; 26] = [
    // mov rax, VERB_JOURNAL
    0x48, 0xc7, 0xc0,
    VERB_JOURNAL as u8, (VERB_JOURNAL >> 8) as u8, (VERB_JOURNAL >> 16) as u8, (VERB_JOURNAL >> 24) as u8,
    // mov rdi, 0x1391   (the "argument": a marker the kernel journals)
    0x48, 0xc7, 0xc7, 0x91, 0x13, 0x00, 0x00,
    // syscall           -> lands in yard::syscall_entry, returns here
    0x0f, 0x05,
    // mov rcx, KERNEL_ADDRESS
    0x48, 0xc7, 0xc1,
    KERNEL_ADDRESS as u8, (KERNEL_ADDRESS >> 8) as u8, (KERNEL_ADDRESS >> 16) as u8, (KERNEL_ADDRESS >> 24) as u8,
    // mov rcx, [rcx]    -> #PF: the last instruction this resident executes
    0x48, 0x8b, 0x09,
];

/// The marker the resident passes through the gate, echoed into SIJIL so the
/// journal shows a crossing that originated in ring 3.
pub const GREETING: u64 = 0x1391;
