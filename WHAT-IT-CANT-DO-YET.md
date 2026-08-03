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
- **Cannot:** isolate yard residents *from each other* (one shared user
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
