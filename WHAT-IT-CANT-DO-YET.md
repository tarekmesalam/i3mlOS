# What i3mlOS can't do yet

Honesty page, updated every release. Today the kernel:

- **Can (M0):** boot as a UEFI application on QEMU/OVMF, leave the firmware
  behind (`ExitBootServices`), run on its own GDT/TSS/IDT with exception
  handlers that report on serial (double faults get their own IST stack),
  manage physical frames with a bitmap allocator over the UEFI memory map,
  serve `Vec`/`String`/`format!` from its own free-list heap, and draw its
  boot screen — the three-petal mark above the **إعمل** banner.
- **Can (M1):** keep time and take interrupts (x2APIC, TSC-deadline where the
  CPU offers it, APIC periodic mode everywhere else, calibrated against the
  PIT), and — the point of the whole project — **schedule agents rather than
  processes**: an Agent Control Block carries goal, lineage, capabilities and
  budget; AMAN mediates every action; SIJIL records every crossing;
  irreversible actions park for human consent; delegation can only narrow
  authority; and an exhausted budget suspends an agent instead of
  overspending. CI asserts all ten of these behaviors on every commit.
- **Can (M2 — the yard):** run on **its own page tables**, and run untrusted
  code in **ring 3** with exactly one way back in: `syscall` into the gate.
  A yard resident cannot read kernel memory — the CPU refuses at the page
  walk, not our code — and every crossing it makes is journaled by the
  kernel, in memory it cannot address. This is where "capabilities cannot be
  forged" and "the journal cannot be falsified" stop being properties of the
  type system and become properties of the hardware.
- **Can (M3):** run an agent that is a **WebAssembly module** — decoded,
  bounded by fuel, memory-checked, and bound to capabilities through its
  import list *before* it executes. A module that imports a verb it was not
  granted is refused at load, so an unauthorized call is unreachable rather
  than merely denied.
- **Can (M4):** find its own hardware (PCI configuration space), bring up
  **virtio** devices over the modern 1.0 transport, and drive two of them:
  **virtio-rng** for entropy it cannot invent, and **virtio-blk** — it writes
  a sector, reads it back, and flushes it. Device registers are mapped on
  demand, which they must be: a virtio BAR under OVMF sits at 768 GiB.
- **Can (M5):** **keep its journal.** Every boot opens the hash-chained log
  on disk, verifies every link before trusting a word of it, appends what
  happened this time, and commits. A record edited or truncated on disk
  stops verifying and is refused rather than believed. SHA-256 is ours,
  checked against the published vectors.
- **Can (M6):** ship as **one bootable disk image**. `cargo xtask disk`
  writes a GPT disk — an EFI system partition holding the kernel, and a
  partition the kernel finds *by its type GUID* rather than by an assumed
  offset. The image builder (CRC-32, GPT, FAT32) is part of this repo, so the
  build needs no `mkfs`, no `sudo`, and no Linux. CI builds the image, boots
  it twice, and publishes it as an artifact.
- **Can (M7 — the machine thinks):** reach an **AI model as a device behind
  the gate**. An agent must hold a `Model` capability of the right class; the
  tokens it spends are charged by the kernel from what the model reported,
  not from what the agent claimed; every call is journaled; and the
  **private** class has no route off the machine at all — residency enforced
  by not routing rather than by policy. The host relay is where anything
  vendor-shaped lives, so the kernel never holds a key.
- **Can (M8):** **show a person what it did**, on the framebuffer: every
  agent and its state, what each spent, that a model answered and what the
  kernel charged for it, how much of the journal is on disk — and, framed
  and last, the consent request it is waiting on. An 8x16 bitmap font
  rendered at build time; a real glyph pipeline is a later milestone.
- **Cannot:** accept an answer *from* the screen (there is no keyboard driver
  yet, so the consent card is shown, not clicked), render Arabic text beyond
  the pre-shaped banner, run a model locally (the private class answers honestly that
  there is none yet), stream a reply, hold a conversation across turns
  (each call stands alone), boot on real hardware (this is a VM image: virtio devices, no
  bare-metal drivers), survive an attacker who owns the disk and rewrites the whole
  chain (that needs a signature, and it is a later milestone — the chain
  detects corruption and tampering-in-place, not a full forgery), store
  anything but the journal (there is no filesystem yet), take device
  interrupts (drivers poll), drive a network card or a console device,
  run WASM modules in the yard yet (the interpreter is still
  kernel-side; moving it into ring 3 is the next step), checkpoint a running
  module (the state is data by construction, but nothing serializes it yet),
  handle floats, tables, globals, or `br_table` (outside the subset, refused
  by the decoder), isolate yard residents *from each other* (one shared user
  address space; per-agent address spaces are a Phase 3 milestone costed at
  4–6 months), run anything but hand-assembled residents (the WASM
  interpreter is next), checkpoint or resume an agent across a reboot (agents are
  in-kernel step functions; WASM instances in Phase 2 make execution state
  data by construction), coalesce freed heap blocks, reclaim boot-services
  memory, talk to any device beyond serial + framebuffer, reach a model or a
  network, persist SIJIL (the ring is in RAM and drops its oldest entries),
  store anything in DHAKIRA, shape Arabic live (the banner is baked at build
  time), or run on real hardware.

Per the claims ladder we may now say **"governed, journaled, undoable"** and
— since M2 — **"isolated"**, meaning untrusted code is isolated from the
kernel by the CPU. We do not say **"secure"**, and will not until an external
audit; nor do we claim residents are isolated from one another.

Roadmap and binding gates: [MASTER_PLAN.md](MASTER_PLAN.md) §5.
