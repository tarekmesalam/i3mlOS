# From-Scratch Rust Kernel: 2026 State of the Art

## 1. Boot path without borrowed code

rustc has a built-in `x86_64-unknown-uefi` (and `aarch64-unknown-uefi`) target that emits PE binaries directly — no assembly stub, no external toolchain. [uefi-rs](https://github.com/rust-osdev/uefi-rs) proves the shape: a thin FFI layer (`uefi-raw`) plus safe wrappers over firmware protocols. Writing your own equivalent is cheap because a loader needs only ~6 protocols: `LocateProtocol`, Simple File System (read kernel ELF off the ESP), GOP (framebuffer), `AllocatePages`, `GetMemoryMap`, `ExitBootServices`. Budget **3–6 weeks** for a purity-clean loader: parse your kernel ELF, build 4-level page tables mapping the kernel higher-half (e.g. `0xffff_8000_...`) plus a full physical-memory offset map, enable NX/WP, call `ExitBootServices` with the memory-map dance (it can fail and force a re-fetch — the classic bug), jump with a hand-rolled boot-info struct (memory map, framebuffer, RSDP, initrd). Note phil-opp's UEFI-based third edition [stalled](https://os.phil-opp.com/status-update/); his second edition uses the `bootloader` crate — you'll replace that layer yourself. Do **not** adopt Limine — that's borrowed code.

## 2. Architecture: x86_64 first, aarch64 designed-for from day one

For VM-first, aarch64 is tempting (Apple Silicon dev machines run aarch64 guests at native speed via QEMU/HVF or UTM; Graviton/Axion/Cobalt cloud instances are cheap), but **start x86_64**: virtually all learning material (phil-opp, OSDev wiki), QEMU's best-debugged machine, Firecracker's primary target, and x64 GitHub runners expose `/dev/kvm` while [arm64 hosted runners do not](https://github.com/orgs/community/discussions/148648). Firecracker itself is Linux/KVM-only, so Apple-Silicon dev means QEMU/HVF locally, Firecracker on a Linux box/cloud. x86_64 costs you legacy ceremony (GDT/IDT, APIC init, trampoline for AP boot); aarch64 is cleaner (exception levels, GIC, PSCI) but needs device-tree parsing and has thinner tutorials. **Port cost later: 2–4 months** if — like Hermit and Asterinas' `ostd` — you isolate arch-specific code (boot, MMU formats, interrupt entry, context switch, timers ≈ 10–15% of the kernel) behind a HAL trait from commit one. Retrofitting the HAL later doubles that.

## 3. Core subsystem estimates (strong solo dev, full-time, post-ramp)

| Subsystem | Estimate | Notes |
|---|---|---|
| Physical frame allocator + paging + kernel heap | 6–10 weeks | Bitmap/buddy frames; mapping API; linked-list then slab heap |
| Interrupts/exceptions + APIC (or GIC) + timers | 4–8 weeks | x2APIC, TSC-deadline (or ARM generic timer); page-fault handler quality matters forever |
| SMP bring-up | 4–8 weeks | SIPI trampoline (or PSCI), per-CPU data, spinlocks, IPIs, TLB shootdown (the hard part) |
| Scheduler + context switch | 2–4 weeks basic | Round-robin → priorities; refined forever |
| User mode + syscalls | 4–8 weeks | `syscall`/`sysret` (or `svc`/EL0), user page tables, copy-in/out discipline |
| ELF loading (static) | 2–3 weeks | Custom format is barely cheaper; skip dynamic linking for years |

**Total: 9–18 months** to "multi-core kernel running user-mode Rust programs under QEMU." This matches observed reality (Redox's first booting demo took Soller roughly a year; Theseus ≈ one PhD's early years).

## 4. VirtIO from scratch

The [OASIS VirtIO 1.2/1.3 spec](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html) is genuinely readable. The shared core — split virtqueues + a transport — is **2–4 weeks**; build both PCI (QEMU) and MMIO (Firecracker; [PCI is only developer-preview there](https://github.com/firecracker-microvm/firecracker/releases)) transports early, it's cheap. Then per device:

- **virtio-rng**: days. **virtio-blk**: ~1 week — simplest real device, do it first. **virtio-console**: ~1 week.
- **virtio-net**: 2–4 weeks for the driver; the *TCP/IP stack* is the real bill — smoltcp is borrowed code (**explicit purity decision**), writing your own is 3–6+ months for something trustworthy.
- **virtio-vsock**: 2–3 weeks; ideal host↔agent control channel with no IP stack needed — strategically valuable for i3mlOS early.
- **virtio-gpu 2D**: 2–4 weeks (resource create/transfer/flush/scanout) — a framebuffer, honestly. **3D is a different universe**: guest-side [Venus/virgl](https://docs.mesa3d.org/drivers/venus.html) means reimplementing a Vulkan/GL userspace driver stack (Mesa-scale, team-years). Treat GPU-3D as out of scope; Firecracker has no GPU device at all.

## 5. Dev workflow

QEMU `-s -S` + gdb with kernel symbols is the daily driver; add `-d int` logging, QEMU monitor, and an exit device (`isa-debug-exit`) so tests report pass/fail as exit codes. Use phil-opp's custom-test-framework pattern for in-kernel `cargo test`. Structure aggressively so allocators, virtqueue/ELF parsers, and scheduler logic are `no_std` host-testable libraries — then **cargo-fuzz them on the host**, especially virtio descriptor parsing (a hostile hypervisor/device is your threat model). CI on GitHub Actions: install `qemu-system-x86_64`, boot `-display none -serial stdio`, assert expected serial lines under a timeout; KVM works on free x64 runners, and TCG emulation is fast enough for boot tests anyway. QEMU `savevm`/Firecracker snapshots later enable restore-based test parallelism — and SIJIL-style determinism experiments.

## 6. Curriculum, ranked

1. **[Writing an OS in Rust](https://os.phil-opp.com/)** (phil-opp, 2nd ed.) — do it end-to-end, 2–4 weeks. Still the canonical on-ramp despite the stalled 3rd edition.
2. **OSTEP** (free textbook) + **xv6-riscv** source (MIT 6.1810) in parallel — concepts plus a complete readable Unix, 1–2 months.
3. **OSDev wiki** — reference-while-building, not linear reading (C-centric but the hardware facts are language-neutral).
4. **[This Month in Rust OSDev](https://rust-osdev.com/this-month/2026-04/)** — track the ecosystem monthly.
5. **Source reading order**: Hermit (small, readable, multi-arch virtio) → Asterinas `ostd` (the unsafe-boundary discipline to copy) → Redox (microkernel at scale) → Theseus (ideas).

**Honest ramp**: a strong systems dev new to kernels needs **3–6 months** before original work is productive, and **~18–30 months** total to a self-designed kernel running agent workloads over virtio under QEMU/Firecracker. Expect the first three months to feel like drowning; that's normal.

## 7. What the reference projects prove

- **[Redox](https://www.redox-os.org/)** (2015, Soller solo start): one person gets a remarkable booting Rust microkernel + GUI in ~1–2 years — and eleven years later is still fighting bare-metal drivers. Proof the VM-first/VirtIO-first decision is the single highest-leverage scoping call i3mlOS has made.
- **[Hermit](https://hermit-os.org/)** (RWTH Aachen, handful of devs): pure-Rust, zero-C kernel on x86_64 + aarch64 with its own virtio drivers, still shipping (v0.13, Feb 2026). The closest existence proof for i3mlOS's shape: small team, VM-only, multi-arch is sustainable.
- **[Theseus](https://github.com/theseus-os/Theseus)** (≈1 PhD student + collaborators): an original-design, single-address-space kernel with an OSDI paper in ~4–5 years — solo-scale novelty is real; post-founder momentum loss is the cautionary tale (bus factor).
- **[Asterinas](https://github.com/asterinas/asterinas)** (2022, small funded team): 230+ Linux syscalls with Linux-comparable performance in ~4 years by targeting **VMs only**. Steal its framekernel pattern: one tiny audited `unsafe` core (`ostd`-equivalent), everything else — including AMAN/SIJIL/DHAKIRA logic — in safe Rust.

Sources: [uefi-rs](https://github.com/rust-osdev/uefi-rs) · [phil-opp status](https://os.phil-opp.com/status-update/) · [rust-osdev bootloader](https://github.com/rust-osdev/bootloader) · [GH arm64 runners](https://github.com/orgs/community/discussions/148648) · [GH KVM discussion](https://github.com/orgs/community/discussions/8305) · [Firecracker releases](https://github.com/firecracker-microvm/firecracker/releases) · [Firecracker design](https://github.com/firecracker-microvm/firecracker/blob/main/docs/design.md) · [Mesa Venus](https://docs.mesa3d.org/drivers/venus.html) · [virglrenderer state (Collabora)](https://www.collabora.com/news-and-blog/blog/2025/01/15/the-state-of-gfx-virtualization-using-virglrenderer/) · [QEMU virtio-gpu](https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html) · [Hermit](https://hermit-os.org/) · [Hermit releases](https://github.com/hermit-os/kernel/releases) · [Theseus](https://github.com/theseus-os/Theseus) · [Asterinas](https://github.com/asterinas/asterinas) · [Asterinas ATC '25](https://www.usenix.org/conference/atc25/presentation/peng-yuke) · [Rust OSDev monthlies](https://rust-osdev.com/this-month/2026-04/)
