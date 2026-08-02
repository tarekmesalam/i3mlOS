# devlog 001 — it boots | بيقلع

*Draft — publish with the repo.*

i3mlOS starts the only honest way an OS can: by booting in public.

What exists as of today:

- **NAWA 0.0.1** — a kernel that is itself a UEFI application, compiled by
  `rustc` straight to a PE binary. No bootloader crate, no Limine, no
  assembly stub, no third-party code in the image. It programs the COM1 UART
  and prints `hello from the i3ml kernel`, then greets the firmware console.
- **The framekernel rule, enforced from commit one** — `unsafe` is legal only
  in `nawa/core`; the kernel binary itself is `#![forbid(unsafe_code)]`, and
  `cargo xtask check` fails the build on any violation.
- **CI that boots the OS** — every commit, GitHub Actions boots the kernel
  headless under QEMU/OVMF, asserts the serial line, and requires the clean
  `isa-debug-exit` status. If it doesn't boot, it doesn't merge.

Next (M0, per [the plan](../../MASTER_PLAN.md) §5): exceptions and a real
IDT, the frame allocator, the heap — and the **إعمل** boot banner on the
framebuffer.

The verb is the vision: *don't operate your computer — tell it.* Every line
underneath the telling is ours.
