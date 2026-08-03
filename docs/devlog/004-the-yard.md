# devlog 004 — the yard | الساحة

*Draft — M2 complete.*

Two sentences in this project's README were, until today, promises about
Rust's type system:

> Capabilities cannot be forged. The journal cannot be falsified.

They are now promises about the CPU. Here is the serial log:

```
nawa: own page tables live
nawa: syscall gate armed — the yard can be entered
paging: 4 GiB identity-mapped by our own tables
yard: resident loaded at 0x100000000000, dropping to ring 3
yard: resident faulted reaching 0x100000 from 0x100000000017 (error 0x5)
yard: killed resident — page fault at 0x100000000017 (0x100000)
yard: back in the kernel — 1 gate crossing(s) from ring 3, last verb 5
yard: 1 ring-3 crossing(s) in the journal — isolation is hardware now
```

What happened, line by line. The kernel built **its own 4-level page
tables** — identity-mapping physical memory supervisor-only, plus a separate
PML4 slot at 16 TiB whose every level carries the user bit. It armed
`syscall`/`sysret`. Then it copied 26 hand-assembled bytes into a
user-mapped page and `iretq`'d into ring 3.

The resident did two things. It crossed the gate (`verb 5`, journaled by the
kernel in memory the resident cannot address), and then it read `0x100000` —
an ordinary kernel address. The CPU refused: page present, user mode, no
access. Not because our code checked; because the page tables say so.

Three bugs, all worth naming, all from the same family:

**The UEFI target's C ABI is Microsoft x64, not SysV.** This bit twice. First
the gate handler read its "verb" from `rcx` — which on entry holds the user's
return address — so the first crossing arrived as `verb=17592186044432`. Then,
worse, `rdi`/`rsi` are **callee-saved** on Windows x64, and the ring 3 entry
zeroed them (correctly — a resident must inherit nothing) without saving them
first. The caller's live pointers came back as zero and the kernel jumped into
unmapped space. That one presented as a heisenbug: adding a `println` moved
the register allocation and made it disappear. A kernel is not allowed to have
those, so the transition was rebuilt to save the full Windows non-volatile set
and to run on a dedicated stack with an explicitly captured continuation —
nothing about the caller's frame is assumed any more.

**TSS.RSP0 was zero.** When an exception arrives from ring 3 the CPU switches
to the privilege-0 stack in the TSS. Ours was never set, so the very first
fault in the yard pushed its frame onto address 0 and became a double fault.

**`sysret` dictates the GDT layout.** It computes SS from `STAR + 8` and CS
from `STAR + 16`, so the user *data* descriptor must precede the user *code*
descriptor. Ours were the other way round and `iretq` refused with #GP.

The honest boundary, printed in
[WHAT-IT-CANT-DO-YET.md](../../WHAT-IT-CANT-DO-YET.md): the yard isolates
untrusted code from the kernel, not tools from each other. Per-agent address
spaces are a Phase 3 milestone costed at 4–6 months there, and this design
does not need a retrofit to get them — untrusted code is separately-loaded
user code from this milestone onward.

Next: the WASM interpreter moves in as the yard's first real resident, and
agents stop being kernel step functions.
