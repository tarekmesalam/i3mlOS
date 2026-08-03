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
- **Cannot:** own its page tables (still on the firmware's identity map),
  isolate anything in hardware (no user mode yet — that is the yard, M2; until
  then the trust claims are enforced by the type system and the gate, not by
  the CPU), checkpoint or resume an agent across a reboot (agents are
  in-kernel step functions; WASM instances in Phase 2 make execution state
  data by construction), coalesce freed heap blocks, reclaim boot-services
  memory, talk to any device beyond serial + framebuffer, reach a model or a
  network, persist SIJIL (the ring is in RAM and drops its oldest entries),
  store anything in DHAKIRA, shape Arabic live (the banner is baked at build
  time), or run on real hardware.

Per the claims ladder we say **"governed, journaled, undoable"** now that the
gate exists, **"isolated"** only after the yard (Ring 3) lands, and
**"secure"** never — until an external audit.

Roadmap and binding gates: [MASTER_PLAN.md](MASTER_PLAN.md) §5.
