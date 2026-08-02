# What i3mlOS can't do yet

Honesty page, updated every release. Today the kernel:

- **Can:** boot as a UEFI application on QEMU/OVMF, print to serial and the
  firmware console, exit cleanly under test — with `unsafe` confined to the
  trusted core and CI proving the boot on every commit.
- **Cannot:** manage memory it allocated itself, take interrupts, schedule
  anything, run an agent, talk to any device beyond serial, or survive on
  real hardware. There is no filesystem, no network, no WASM, no AMAN
  enforcement, no SIJIL persistence, no DHAKIRA. Yet.

Per the claims ladder we say **"governed, journaled, undoable"** only when the
gate exists, **"isolated"** only after the yard (Ring 3) lands, and **"secure"**
never — until an external audit.

Roadmap and binding gates: [MASTER_PLAN.md](MASTER_PLAN.md) §5.
