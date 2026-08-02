# devlog 002 — the machine is ours | الجهاز بقى بتاعنا

*Draft — M0 complete.*

M0 landed in one session. The serial log tells the story better than prose:

```
nawa: exit_boot_services ok — the machine is ours
nawa: gdt+tss+idt loaded
hello from the i3ml kernel
mem: 206 MiB usable, 163 MiB managed by the frame allocator
int3: breakpoint handled at 0xde082b2
heap: ok — squares [1, 4, 9, 16, 25, 36, 49, 64, 81, 100]
fb: 1280x800 banner drawn — i3mel
```

What that means, line by line:

- **ExitBootServices** — the memory-map/key/exit dance, with the retry loop
  the spec demands. From that line on, every instruction that executes is
  ours.
- **Our GDT + TSS + IDT** — handlers for divide error, breakpoint, invalid
  opcode, GP fault, page fault (CR2 included), and a double-fault handler on
  its own IST stack so we can still report when the main stack is the
  casualty.
- **`int3` resumed** — the CPU took our IDT seriously and came back. First
  exception in i3mlOS history.
- **A bitmap frame allocator** over the largest conventional region of the
  UEFI map, and a **free-list heap** on a 16-byte quantum registered as the
  global allocator — `Vec`, `String`, `format!` in the kernel with zero
  third-party code underneath.
- **The إعمل banner** on the GOP framebuffer: shaped once at build time from
  Geeza Pro Bold via CoreText (font output is Tier-3 *content* under the
  purity charter), packed to 1-bpp, blitted by our own code. The live
  TTF → bidi → shaper pipeline that replaces this bake is the Phase 2/3
  flagship track.

An adversarial review pass (three specialist reviewers + verification) ran
before the merge and confirmed six real defects — a misaligned memory-map
buffer that was UB-by-luck, an inline-asm `lateout` contract violation in the
CS reload, an unvalidated `free_frames`, a heap that could refuse over-aligned
requests while memory sat free, leaked tail slivers, and unchecked
framebuffer bounds math. All fixed before this commit. The review is part of
the process now, not an event.

## Regenerating the banner

```bash
# host/banner/banner.swift renders shaped Arabic via CoreText to banner.pgm:
swift banner.swift
# then host/banner/pack.py packs it to 1-bpp and emits
# nawa/kernel/src/banner.rs (threshold 128, MSB-first bit packing).
```

Next (M1): APIC timers, the kernel async executor, Agent Control Blocks, the
eight AMAN verbs as in-kernel calls, the in-RAM SIJIL ring, and a
budget/deadline scheduler — the first agent scheduled by the first
agent-native kernel.
