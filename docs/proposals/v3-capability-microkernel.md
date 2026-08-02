# i3mlOS: The Capability Microkernel — نواة (Nawah) Architecture Proposal

## 1. Vision

i3mlOS is the first operating system whose schedulable unit is an agent — goal + context + capabilities + budget — and whose security story is not a sandbox bolted on top but the physics of the machine. We build an original Rust microkernel in the seL4 tradition: a tiny trusted core (~15–25K lines) that does only IPC, scheduling, memory, and capability enforcement. Everything else — drivers, filesystem, network, AMAN policy, SIJIL journaling, DHAKIRA memory, the agents themselves — is an unprivileged userspace task holding unforgeable kernel capabilities. An agent on i3mlOS cannot touch a file, a socket, or another agent except through a capability it was explicitly granted, because *no ambient authority exists anywhere in the system to steal*. That property cannot be retrofitted onto Linux; it can only be true from commit one. This is that commit.

**The one-sentence demo:** on a live screen, an agent asks to write outside its grant, the kernel refuses at the IPC layer (not a policy check — the capability simply isn't in its table), the user approves on a system-owned surface no agent can draw, SIJIL shows the exact kernel-traced message log of everything the agent did, and one command rewinds it.

## 2. Kernel architecture

**Boot.** Own UEFI loader on rustc's `x86_64-unknown-uefi` target (no `bootloader` crate, no Limine): ~6 firmware protocols, load kernel ELF from ESP, build higher-half page tables + physical-memory offset map, NX/WP on, `ExitBootServices` with the re-fetch dance, jump with a hand-rolled boot-info struct (memory map, framebuffer, RSDP, initrd of userspace tasks). 3–6 weeks. Arch code (boot, MMU formats, interrupt entry, context switch, timers) lives behind a HAL trait from day one, Hermit/Asterinas-style, so aarch64 is a 2–4-month port later, not a rewrite. x86_64 first: QEMU/Firecracker/CI all favor it.

**Memory.** Zircon-VMO-style rather than full seL4 untyped/retype — a deliberate solo-feasibility concession. The kernel owns a frame allocator; userspace sees **memory objects** (create, map, share) only via capabilities. Kernel heap is a linked-list-then-slab allocator; all allocators are `no_std` host-testable libraries, fuzzed on the host.

**Protection model.** Every kernel object — task, thread, memory object, IPC endpoint, interrupt, virtio queue window — is reachable only through a per-task **capability table** (c-space). Capabilities are unforgeable kernel-held handles carrying a rights mask (read/write/map/send/grant) plus an **attenuation record**: a capability derived from another can only shrink. Derivation trees are kernel-tracked, so **revocation is recursive and instant**: revoke a parent and every delegated descendant dies with it, mid-task. This is AMAN's enforcement half (see §3).

**IPC.** Synchronous endpoint `call/reply` (seL4-style rendezvous, no kernel buffering) plus async notification bits, plus shared-memory rings for bulk data with notifications for doorbells. Messages carry up to a few words inline **and may transfer capabilities** — delegation is literally "send a cap in a message." Every message header (sender, endpoint, badge, cap transfers, timestamp) is optionally mirrored into a kernel trace ring buffer — SIJIL's feedstock (§3).

**Scheduling.** Threads are the kernel unit; **agents are the system unit**. Kernel: fixed-priority round-robin with seL4-MCS-inspired *scheduling-context capabilities* (a cap conferring CPU budget/period), so CPU time is itself a delegable, attenuable resource. The agent-level scheduler (deadline + token-budget aware, suspend-for-hours durability) is a userspace task — `mudir` (the steward) — that owns agent control blocks and manipulates kernel scheduling contexts. Budgets in tokens/dollars are mudir policy; budgets in CPU are kernel-enforced.

**Syscall/ABI surface — two layers, only one public.** The *kernel* syscall list is tiny and frozen early (~16 calls): `send/recv/call/reply`, `notify/wait`, cap ops (`derive/transfer/revoke/inspect`), memory-object ops, thread/task create, yield, debug-out. It is explicitly **not** the public ABI. The *public, stable, agent-native ABI* is the protocol agents speak over their single bootstrap endpoint to system tasks:

- `spawn(goal, context, caps[], budget) → agent_id` — mudir creates a child agent holding an attenuated subset of the parent's caps (trust propagation is structural: you cannot give what you don't hold).
- `delegate(agent, cap, attenuation)` / `attenuate(cap, mask) → cap'` — thin wrappers over kernel cap derivation.
- `approve(action, risk_class) → grant | denial` — traps to the system approval surface (§3).
- `journal(event)` / `journal_query(filter)` — SIJIL append and read.
- `remember(namespace, key, value)` / `recall(query)` — DHAKIRA.
- `await(condition)` — durable suspension; mudir checkpoints the agent's memory objects and parks it.

No POSIX, no fork/exec/open. Tools see MCP-shaped typed calls (§4); the kernel never learns what a "file" is.

## 3. AMAN / SIJIL / DHAKIRA as kernel constructs

**AMAN is split in two, and that split is the architecture.** *Enforcement* is the kernel capability table — unforgeable, attenuate-only, recursively revocable. *Policy* is `aman-d`, a userspace task holding the root resource caps, which decides what to mint: it maps intent ("read my project folder") to a derived cap (an endpoint badge on `fs-d` scoped to a subtree), attaches risk classes, and consults the approval surface for irreversible actions. The approval surface is a system-owned task holding the *only* capability to a reserved framebuffer region and input queue — agents architecturally cannot draw or spoof the consent dialog. Compromise `aman-d` and you still can't forge a capability; compromise an agent and you hold only its shrunken cap set.

**SIJIL** is fed by the kernel IPC trace ring, not by agent self-reporting — agents *cannot lie to the journal* because the journal records the messages the kernel actually moved. `sijil-d` holds the sole trace-buffer capability, enriches kernel records with semantic events from system tasks (grant issued, file written, tokens spent), and appends to a log-structured, hash-chained store on virtio-blk. Undo is two-class: state undo via copy-on-write snapshots of DHAKIRA/filesystem memory objects; action undo via compensation records that side-effectful tools must register to receive a "send"-class capability at all.

**DHAKIRA** is `dhakira-d`: a namespace (`/memory/user/...`, `/memory/agent/<id>/...`) where each subtree is reachable only via a derived capability — memory sharing between agents is cap delegation, ACLs fall out of the protection model, and residency policy ("health data never routes to cloud") is enforced by *not granting* the model-broker cap for tagged subtrees. User inspect/edit/delete is just the user's shell holding the root DHAKIRA cap.

## 4. Tools and apps with no Linux userland

**Tool format: own WASM interpreter** (non-JIT, 3–6 months, spec test suite for correctness). A tool is a `.wasm` module whose imports are exactly its MCP-shaped typed calls; the runtime maps each import to an IPC endpoint capability. Least privilege becomes mechanical: a tool's import list *is* its permission manifest. 10–50× slower than JIT is irrelevant for IO-bound agent tools.

**Network, staged.** Phase A (months 9–15): **virtio-vsock only** — 2–3 weeks of driver work, no IP stack. A host-side relay (dev tooling, not shipped OS code) forwards model-API and HTTP requests. This is what makes the microkernel demo *fast*: agents reach Claude/GPT APIs within the first year. Phase B: own virtio-net driver (2–4 weeks) + own TCP/IP stack (3–6 months, differential-tested against smoltcp and Linux *in CI only*). Phase C: `net-d` speaks HTTP/1.1 ourselves; TLS 1.3 state machine ours, **crypto primitives vendored (rustls/ring) inside the isolated `net-d` sidecar task** — the purity exception never links into kernel, AMAN, or agents, and the capability model quarantines it: `net-d` holds only its virtio queue and endpoint caps. Model access is a `model-d` broker: agents request qualities ("fast/private" vs "frontier"), never raw sockets — token spend metered here into cost-budgets. Local inference (llama.cpp-class, own port) is year-4+; the broker interface doesn't change.

## 5. Arabic-first rendering

`shasha-d` (display server) drives virtio-gpu 2D (2–4 weeks: a framebuffer, honestly). Own TTF/OTF parser (2–4 months, safe Rust, fuzzed — fonts are hostile input). Console pipeline: **UAX #9 bidi** (2–3 months, validated against `BidiTest.txt`) + **terminal-grade Arabic shaping** — joining classes straight from `ArabicShaping.txt`, contextual forms on a monospace grid (1–2 months, mlterm precedent). The first boot message is إعمل, shaped correctly, RTL, by our own code — no OS has ever done that from scratch. Naskh-grade GSUB/GPOS shaping (rustybuzz-scale, validated against the HarfBuzz corpus) is the year-3+ flagship, scoped Arabic+Latin only. Fonts (Amiri/Noto Naskh) are content, like model weights.

## 6. Milestone ladder (strong solo dev, full-time; months are cumulative)

| Milestone | When | Contents |
|---|---|---|
| M0 Ramp | mo 0–3 | phil-opp end-to-end, OSTEP+xv6, Hermit/Asterinas source reading |
| M1 **First boot** | mo 4–6 | Own UEFI loader, higher-half kernel, serial + framebuffer hello (Arabic), interrupts, frame allocator, heap. HN writeup #1 |
| M2 Usermode | mo 7–11 | Syscall entry, user page tables, cap tables, sync IPC, ELF load, round-robin sched, kernel trace ring |
| M3 **First agent syscall** | mo 11–13 | mudir + aman-d + sijil-d minimal; an agent task calls `spawn`/`attenuate`/`journal`; vsock driver; host relay → agent calls a frontier model **from inside i3mlOS**. The goal-as-shell screencast |
| M4 **First real task** | mo 15–20 | virtio-blk + own log-structured FS, DHAKIRA v0, WASM runtime v0, approval surface; agent does fetch→transform→write→notify over vsock; SIJIL undo on camera. SMP lands here |
| M5 **First outside user** | mo 21–30 | Firecracker image (MMIO transport), virtio-net + own TCP/IP, net-d with vendored-crypto TLS, docs-as-product, `i3ml` host CLI; self-hosters run an agent box. NLnet applied at M3 |

Honest total: 2–2.5 years to an outside user, matching Redox/Hermit/Asterinas base rates *minus* the driver hell we amputated via virtio-only.

## 7. Purity charter position

**Write (Tier 1):** loader, kernel, all virtio drivers (PCI+MMIO), FS+SIJIL store, TCP/IP, HTTP, TLS state machine, TTF parser, bidi+shaper, 2D compositor, WASM interpreter, QOI/PNG decoders. **Vendor-then-replace (Tier 2, the sole exception):** crypto primitives (rustls/ring) confined to the `net-d` sidecar; replace when own constant-time primitives pass Wycheproof+BoGo *and* an external audit is funded; fiat-crypto-generated code is an acceptable intermediate, flagged explicitly. **Accept forever (Tier 3):** rustc/LLVM, UEFI, QEMU/Firecracker as hosts, model weights/APIs, Unicode data files and conformance suites, CA store as data, fonts. Standing rule: third-party code in CI/test tooling freely; never in the shipped image.

## 8. Three biggest risks, honestly

1. **The desert between M1 and M3 (~9 months of invisible plumbing).** Microkernels back-load gratification: cap tables and IPC produce no screenshots. Mitigation: monthly public writeups treating each subsystem as an artifact (SerenityOS cadence), and the vsock-early decision, which pulls "agent talks to a frontier model" into month ~12 instead of month 24. If motivation collapses, it collapses here.
2. **IPC-tax and complexity risk.** Every file read crosses 2–4 address spaces; a naive sync-IPC design can be 10× slower than a monolith, and seL4-grade IPC performance took experts years. Mitigation: shared-memory rings for bulk paths, and acceptance that v1 performance only needs to beat "agent latency is dominated by model inference anyway" — mostly true, but a real ceiling for local inference later. Also honest: I claim "formal-verification-adjacent," not verified — verification of even this kernel is beyond solo scope.
3. **The policy layer is the actual unsolved problem, and the kernel doesn't solve it.** Unforgeable caps enforce whatever aman-d mints; mapping natural-language intent to least-privilege grants is a research problem a perfect kernel cannot answer, and a too-annoying approval surface makes users grant `root-equivalent` caps, reproducing ambient authority socially. Mitigation: ship opinionated risk-class defaults, make SIJIL replay the feedback loop for tightening grants — and admit this layer will be rewritten three times.

Weaknesses conceded: bus factor of one (Theseus's warning); the Zircon-style memory-object simplification trades away seL4's strongest resource-isolation story; and until Phase B networking, demos lean on a host-side relay a skeptic can call a crutch — the roadmap's job is to make that crutch visibly temporary.
