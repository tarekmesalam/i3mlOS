# i3mlOS — The Demo-First Monokernel ("NAWA")

## 1. Vision

i3mlOS inverts the OS stack: the schedulable unit is not a process but an **agent** — goal + context + capabilities + budget — and the kernel's core service is not virtualizing hardware but *governing agency*. NAWA (نواة, "the seed/kernel") is a single-purpose, from-scratch Rust monokernel in the TempleOS/Hermit lineage: one address space, no POSIX, no user/kernel split at first, where AMAN capability checks are typed function calls in the trusted core, SIJIL is the kernel's own write path, and agents are the kernel's tasks. Everything is optimized for the shortest path to a legendary, filmable moment — then grows real protection boundaries along seams designed in from commit one.

**The one-sentence demo:** *An original kernel written by one Egyptian developer boots to an Arabic prompt in under one second, and the first thing it runs is not a shell but a goal — an agent that fetches, transforms, and writes real data, every action capability-checked and journaled, then undone live on camera.*

## 2. Kernel architecture

**Boot (weeks, not months).** The kernel *is* a UEFI application: rustc's `x86_64-unknown-uefi` target emits the PE binary directly — no separate loader, no assembly stub, no Limine. On entry: read GOP framebuffer, grab RSDP, `AllocatePages`, do the `ExitBootServices` memory-map dance, build 4-level page tables (higher-half kernel at `0xffff_8000_...`, full physical offset map, NX/WP on), load GDT/IDT, enable x2APIC + TSC-deadline timer. A separate purity-clean loader (per the research brief's 3–6 week shape) is deferred until a kernel-as-ELF split earns its keep. Single core until M5; SMP is post-demo.

**Memory.** Bitmap frame allocator → linked-list heap → slab. One address space shared by kernel and agents. Isolation at this stage is **language-based** (Theseus-style): all `unsafe` confined to a `nawa-core` crate (Asterinas' framekernel discipline — one small audited unsafe core; AMAN/SIJIL/DHAKIRA logic entirely in safe Rust above it). Per-agent heaps are distinct allocator arenas from day one — this is a seam, see §"Growing boundaries."

**Scheduling.** Agents are async Rust tasks over a kernel executor; the scheduler is a priority queue keyed by **deadline and budget**, not timeslice. An Agent Control Block (ACB) holds: goal string, lineage (parent agent), capability table, budget counters (tokens/bytes/wall-clock), and journal cursor. Preemption arrives with the APIC timer in M1 (async yield points make cooperative scheduling honest even before that). Durable suspension is native: an ACB parks on a waker (virtio interrupt, approval, timer) and its state is checkpointable to SIJIL.

**Protection model, phase 0.** None in hardware; total in types. A capability is an unforgeable Rust value (`Cap<FsWrite>`, index into the ACB's table) that only `nawa-core` can mint. Agents cannot forge one because agents at this stage are either (a) kernel-compiled Rust tasks or (b) WASM tools (M3) interpreted in a sandbox with no raw memory access. This is honest about what it is: a *correctness* boundary, not yet a *security* boundary.

**IPC.** Typed message channels between ACBs (bounded queues, ownership-transfer semantics — no shared mutable state). `delegate` spawns a child agent whose capability table is an **attenuated subset** of the parent's and whose budget is carved from the parent's (cost-groups by construction).

**Syscall/ABI surface — agent-native, never POSIX.** One narrow trait, `AmanGate`, is the *only* door between an agent and the world. Eight verbs:

| Verb | Semantics |
|---|---|
| `spawn(goal, caps, budget)` | Create agent; lineage + grant recorded in SIJIL |
| `delegate(goal, subset, sub_budget)` | Spawn child with attenuated caps/budget |
| `attenuate(cap, constraint)` | Derive a weaker capability (path prefix, rate, expiry) |
| `invoke(cap, args)` | Use a capability: file I/O, net, model inference — *everything* |
| `approve(action) → await` | Trap to the system-owned approval surface; agent parks |
| `journal(query)` | Read own/descendant SIJIL entries; request `undo(entry)` |
| `remember(ns, op)` | DHAKIRA read/write through capability-scoped namespaces |
| `emit/await(chan)` | Typed IPC |

In phase 0 these are function calls. The binding rule that prevents a future rewrite: **arguments are plain-old-data, capabilities are table indices, no pointers cross the gate.** That makes the gate serializable, hence trappable, later.

## 3. AMAN / SIJIL / DHAKIRA as kernel constructs

- **AMAN** *is* the `AmanGate` implementation: the capability table per ACB, mint/attenuate/revoke logic, and the policy hook that decides `invoke` → allow / deny / `approve`. There is no path around it because drivers are only reachable through `invoke` — AMAN is the kernel's syscall dispatcher, not a service beside it.
- **SIJIL** is the kernel's flight recorder: an append-only, checksummed record stream (own on-disk format over virtio-blk) written *by the gate itself* — agents cannot self-report or omit. Every entry: agent id, lineage, capability used, args digest, result, budget delta. Undo is two-class: reversible effects (file writes go through a copy-on-write staging layer in the from-scratch filesystem; undo = drop the staged generation) vs irreversible ones (network sends, model calls) which are journal-flagged and gated behind `approve`. SIJIL doubles as the checkpoint store for durable agents.
- **DHAKIRA** is a namespace tree (`/memory/user/…`, `/memory/agent/<id>/…`) on the same filesystem, reachable only via `remember` with namespace-scoped caps — memory sharing between agents is a capability grant, inspectable and revocable by the user, journaled like everything else.

## 4. Tools and the outside world (no Linux userland)

**Tool format: own WASM interpreter** (M3; 3–6 months, non-JIT, spec test suite for correctness — wasm3/LibWasm precedents). Tools are `.wasm` blobs whose imports are exactly the eight verbs; 10–50× slower than JIT is irrelevant for IO-bound agent tools. WASM is also the third-party contribution surface: outsiders write tools without touching the kernel.

**Reaching AI models — staged honestly:**
1. **M2 (the demo): virtio-vsock model channel.** Own vsock driver (2–3 weeks), speaking a tiny framed protocol to a host-side relay that forwards to the Anthropic API. The relay is *dev-host tooling* in the same category as QEMU — but I flag plainly: it's a crutch, and the "no Linux inside" claim must be worded as "inside the guest" until step 3.
2. **M3–M4: own virtio-net + own TCP/IP** (the 3–6 month bill, differential-tested against smoltcp in CI only), with **TLS terminated in a net-broker sidecar component** inside the image that vendors rustls/ring per the purity charter's single exception — never linked into `nawa-core` or AMAN.
3. **Own TLS 1.3** replaces the vendored crypto when Wycheproof + BoGo pass and an external audit is funded. Local inference (GGUF-style, CPU) is a year-3+ flag, not a promise.

## 5. Arabic-first rendering

Console path: GOP framebuffer at boot, virtio-gpu 2D scanout after (2–4 weeks). Own TTF/OTF parser (2–4 months, safe Rust, fuzzed) rendering Amiri/Noto Naskh (fonts = content, Tier 3). Text pipeline: **UAX #9 bidi** validated against `BidiTest.txt` (2–3 months) + **terminal-grade Arabic shaping** from `ArabicShaping.txt` joining classes on a monospace grid (1–2 months, mlterm precedent). The boot banner, the agent prompt, and SIJIL's log viewer are RTL-native from the first screenshot — Arabic is the identity of the demo, not a locale. Naskh-grade GSUB/GPOS shaping is the flagship long-term flex (rustybuzz's test corpus as the bar), scoped Arabic+Latin only.

## 6. Milestone ladder (strong solo dev, full-time; ramp included)

| Milestone | When | Contents |
|---|---|---|
| **M0 — First boot** | weeks 1–8 | phil-opp end-to-end (2–4 wks) folded into own UEFI-app kernel: framebuffer prints **إعمل**, serial logging, QEMU CI with `isa-debug-exit`. HN post #1. |
| **M1 — First agent syscall** | months 2–4 | Heap, IDT/APIC/timers, async executor, ACBs, in-RAM SIJIL. A kernel agent `spawn`s, `attenuate`s, `journal`s on screen. |
| **M2 — First real task (THE demo)** | months 5–7 | virtio-blk + own FS + persistent SIJIL with undo; virtio-vsock model channel; virtio-console. Agent fetches data via model/relay, transforms, writes, notifies — then live undo. |
| **M3 — Tools + Arabic + net** | months 7–12 | WASM interpreter, TTF parser + bidi + shaped Arabic console, virtio-net + minimal own TCP/IP. |
| **M4 — First outside user** | months 12–18 | User-mode boundary (below), rustls net-broker sidecar, qcow2/Firecracker "agent box" image, readable docs, `i3ml` host CLI. Dozens of self-hoster deployments. |
| **M5 — Hardening** | months 18–30 | SMP, aarch64 port (HAL trait from commit one; 2–4 months), driver isolation, snapshot/restore time-travel demos. |

**Growing boundaries without a rewrite (the M4 mechanism):** because `AmanGate` is POD-only, phase 1 moves agent bodies to ring 3 with `syscall`/`sysret` trapping into the *same* gate (4–8 weeks per the brief); per-agent allocator arenas become per-agent page tables; WASM tools were sandboxed already; virtio drivers, always behind queue traits, migrate to user tasks last. The ABI never changes — only the transport under it.

## 7. Purity charter position

Adopt the research charter verbatim. **Write now:** kernel, executor, virtio (blk/vsock/console/net/gpu-2D/rng), FS + SIJIL format, TCP/IP, TTF parser, bidi + Arabic shaper, WASM interpreter, QOI/PNG. **Vendor-then-replace (sole exception, named triggers):** rustls/ring inside the net-broker sidecar only; replaced on Wycheproof+BoGo pass plus funded audit. **Accept forever:** rustc/LLVM, UEFI, QEMU/Firecracker as hosts, Unicode data files and conformance suites, CA store as data, fonts, model weights/APIs. **Flagged as an interim impurity:** the M2 host-side vsock relay (host tooling, zero code in the image; retired at M3–M4). Zero third-party crates in the shipped image; third-party freely in CI/fuzzing/differential tests.

## 8. Three biggest risks, honestly

1. **The monokernel's security claims outrun its reality.** Until M4, "AMAN mediates everything" is enforced by the Rust type system, not hardware — one `unsafe` bug or compiler soundness hole voids it. If I market the M2 demo as *security*, I'm lying. Mitigation: say "governed, journaled, undoable" pre-M4, "isolated" only after; keep `nawa-core` tiny and fuzzed. Residual risk: demo pressure erodes exactly this discipline.
2. **The seam discipline fails under momentum.** The whole no-rewrite promise rests on POD-only gate calls and trait-fronted drivers. Solo, chasing monthly devlog beats, the temptation to pass a `&mut` through the gate "just this once" is constant — and each violation is invisible until the M4 retrofit, where the brief's estimate doubles. Mitigation: a CI lint denying non-POD types in `AmanGate` signatures from commit one.
3. **Spectacle-shaped scope + bus factor of one.** Demo-first selects for what films well (Arabic boot screen, live undo) over what compounds (TCP/IP maturity, TLB correctness); TempleOS and post-Kling SerenityOS mark both ends of this failure. And the vsock relay means the flagship demo quietly depends on a Linux host process — a critic will notice. Mitigation: the M3 own-netstack milestone is non-negotiable before any "no Linux" framing; monthly Redox-style written updates whether or not there's a spectacle; NLnet application in year 1 so survival isn't coupled to virality.

Weakness admitted: this plan reaches "outside users" ~6 months faster than a microkernel plan but pays for it with an 18-month window where the protection story is typed, not trapped. I claim the demo compounds into funding and contributors fast enough to buy that window back. That is a bet, not a fact.
