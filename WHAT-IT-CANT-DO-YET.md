# What i3mlOS can't do yet

Honesty page, updated every release. Today the kernel:

- **Can (M0):** boot as a UEFI application on QEMU/OVMF, leave the firmware
  behind (`ExitBootServices`), run on its own GDT/TSS/IDT with exception
  handlers that report on serial (double faults get their own IST stack),
  manage physical frames with a bitmap allocator over the UEFI memory map,
  serve `Vec`/`String`/`format!` from its own free-list heap, and draw the
  **إعمل** banner on the GOP framebuffer — with `unsafe` confined to the
  trusted core and CI proving all of it on every commit.
- **Cannot:** take timer interrupts (IF=0; the APIC is M1), own its page
  tables (still on the firmware's identity map), coalesce freed heap blocks,
  reclaim boot-services memory, schedule anything, run an agent, talk to any
  device beyond serial + framebuffer, shape Arabic live (the banner is baked
  at build time), or survive on real hardware. There is no filesystem, no
  network, no WASM, no AMAN enforcement, no SIJIL persistence, no DHAKIRA.
  Yet.

Per the claims ladder we say **"governed, journaled, undoable"** only when the
gate exists, **"isolated"** only after the yard (Ring 3) lands, and **"secure"**
never — until an external audit.

Roadmap and binding gates: [MASTER_PLAN.md](MASTER_PLAN.md) §5.
