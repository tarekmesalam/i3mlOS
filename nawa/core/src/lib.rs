//! NAWA trusted core.
//!
//! Framekernel rule (see docs/purity-charter.md): this crate is the ONLY place
//! in the tree where `unsafe` is legal. Everything above it — the kernel
//! binary, AMAN, SIJIL, DHAKIRA, libs, the yard — is safe Rust building on the
//! safe APIs exported here. `cargo xtask check` enforces this on every commit.
//!
//! The arch-specific parts live in `arch::x86_64` for now; they move behind a
//! HAL trait when the aarch64 port starts.

#![no_std]
#![feature(abi_x86_interrupt)]

#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod apic;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod arch;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod cell;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod entry;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod fb;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod gdt;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod heap;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
mod idt;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod mem;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod mmio;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod paging;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod pci;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
mod panic;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod qemu;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod selftest;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod serial;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod uefi;
pub mod vec_lite;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub mod yard;
