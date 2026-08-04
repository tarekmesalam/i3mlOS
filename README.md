<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
    <img src="assets/logo-light.svg" alt="i3mlOS" width="440">
  </picture>
</p>

# i3mlOS — إعمل

> **"Don't operate your computer. Tell it."** — *«ما تشغّلش الكمبيوتر… قوله يعمل إيه.»*
> **Written from scratch. Every line ours.** — *«مكتوب من الصفر. كل سطر بتاعنا.»*

**i3mlOS** (from the Arabic imperative **إعمل** — *i3mel*, "do!") is an operating system being written **literally from scratch**: an original Rust kernel (codename **NAWA / نواة**), zero Linux code, zero BSD code, no existing OS underneath. Its schedulable unit is not a process but an **AI agent** — goal + context + capabilities + budget. You don't operate the computer; you delegate to it, inside a trust fabric you own.

**i3mlOS** نظام تشغيل بيتكتب **من الصفر حرفيًا**: نواة Rust أصلية اسمها الكودي **NAWA (نواة)**، صفر كود Linux، مفيش أي نظام تحتينا. وحدة الجدولة فيه مش process — هي **AI Agent**: هدف + سياق + صلاحيات + ميزانية.

## The trust fabric | نسيج الثقة

- **AMAN (أمان)** — kernel capability broker: agents can never forge authority or hold credentials; every action is an `invoke` through the gate
- **SIJIL (سجل)** — gate-written flight recorder: agents cannot self-report, omit, or rewrite; reversible actions undo in one command, irreversible ones wait for consent
- **DHAKIRA (ذاكرة)** — user-owned memory: a namespaced tree you inspect, edit, delete, and export

## Strategy in one line | الاستراتيجية في سطر

**VM-first, VirtIO-only for years** — QEMU/Firecracker are the hardware, so drivers shrink from thousands to a dozen; bare metal comes in the final phase, funded by the VM product. The purity rule: third-party code may exist in build/test tooling, **never in the shipped image**.

**VM-first ولسنين VirtIO-only** — QEMU/Firecracker هما الهاردوير، فالـ drivers بتنزل من آلاف لدستة؛ والأجهزة الحقيقية في المرحلة الأخيرة. قاعدة النقاء: كود الغير مسموح في أدوات البناء والاختبار — **ممنوع في الصورة اللي بتتشحن**.

## Try it | جرّبه

One command builds a bootable disk image and runs it. Nothing but Rust and
QEMU is needed — the GPT and the FAT32 filesystem are built by the repo's own
tooling, so there is no `mkfs`, no `sudo`, and no Linux requirement.

بأمر واحد تبني صورة قرص قابلة للإقلاع وتشغّلها. مش محتاج غير Rust وQEMU — جدول
الأقسام ونظام الملفات بيتبنوا بأدوات المشروع نفسه.

```bash
cargo xtask boot --gui
```

Or build the image and run it however you like:

```bash
cargo xtask disk
```

That writes `target/i3mlos.img` — a GPT disk with an EFI system partition the
firmware boots and a second partition the kernel keeps its journal in. Boot it
with any UEFI-capable VM:

```bash
qemu-system-x86_64 -machine q35 -cpu max -m 256M -serial stdio \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=none,id=i3mlos,format=raw,file=target/i3mlos.img \
  -device virtio-blk-pci,drive=i3mlos -device virtio-rng-pci
```

Boot it twice: the second boot recovers and verifies the journal the first one
wrote. That is the point.

## Status | الحالة

🟢 **Six milestones in, and every claim below is asserted by CI on every
commit** — a QEMU boot test on two CPU models, then a rebuild of the
distributable image and two more boots of it.

| | What works |
|---|---|
| **M0** | Boots as a UEFI application, leaves the firmware (`ExitBootServices`), own GDT/TSS/IDT, frame allocator and heap, boot screen |
| **M1** | A heartbeat (APIC), and **agents as the schedulable unit** — goal, lineage, capabilities, budget — through the AMAN gate, recorded in SIJIL, with consent for the irreversible and budgets that suspend rather than overspend |
| **M2** | Own page tables and **ring 3**: untrusted code cannot reach kernel memory, and the CPU is what refuses it |
| **M3** | An agent can be a **WebAssembly module**, bounded by fuel, its **imports resolved against capabilities before it runs** |
| **M4** | **Real hardware**: PCI, the virtio 1.0 transport, entropy and a disk |
| **M5** | **The journal outlives the machine** — SHA-256 and a hash chain, verified before it is trusted |
| **M6** | Ships as **one bootable disk image**, GPT and FAT32 built by this repo's own tooling |

What it deliberately cannot do yet is listed in
[WHAT-IT-CANT-DO-YET.md](WHAT-IT-CANT-DO-YET.md), and kept honest.

<p align="center">
  <img src="assets/bootscreen.png" alt="i3mlOS boot screen: the three-petal mark above إعمل" width="640">
</p>

<p align="center"><em>The actual framebuffer of the NAWA kernel, drawn by its own code — no firmware, no Linux, no libraries.</em></p>

## Documents | المستندات

| File | What's inside |
|---|---|
| [MASTER_PLAN.md](MASTER_PLAN.md) | **The definitive plan (v3, from-scratch edition):** purity charter, kernel architecture, agent-native ABI, roadmap with binding gates, community & funding, risks |
| [docs/PLAN.ar.md](docs/PLAN.ar.md) | الخطة كاملة بالعربي |
| [docs/research/](docs/research/) | 9 research briefs: landscape, OS paths, agent primitives, GTM, bare-metal reality, kernel independence, Rust OSdev 2026, purity precedents, from-scratch community playbooks |
| [docs/proposals/](docs/proposals/) | Competing architecture proposals + adversarial judges' verdicts (v1 three-way + v3 kernel duel) |
| [docs/archive/](docs/archive/) | Paths not taken: v1 (layer-first) and v2 (Linux-engine) master plans, kept as reference |

## The demo we're building toward | الديمو اللي بنبني ناحيته

> An original kernel written by one Egyptian developer boots to an Arabic prompt — and the first thing it runs is not a shell but a **goal**: an agent fetches, transforms, writes; AMAN grants appear on screen; SIJIL records everything; one command rewinds it live.

> نواة أصلية كتبها مطوّر مصري واحد بتقلع على prompt عربي — وأول حاجة بتشغّلها مش shell، **هدف**: agent بيجيب ويحوّل ويكتب؛ منح أمان ظاهرة على الشاشة؛ سجل بيسجّل كل حاجة؛ وأمر واحد بيرجّع الزمن لايف.
