# i3mlOS — Master Plan v3: The From-Scratch Edition

**Status: definitive. Supersedes v1 (layer, archived) and v2 (Linux-engine, archived). Binding decision 2026-08-03: an original Rust kernel — zero Linux code, zero BSD code, no existing OS underneath.** Kernel codename **NAWA** (نواة). User-facing subsystems stay exactly three Arabic names: **AMAN** (capability broker), **SIJIL** (journal + undo), **DHAKIRA** (owned memory). Everything else keeps boring English names.

---

## 1. Vision & Positioning

i3mlOS (**إعمل** — "do!") is the first operating system whose schedulable unit is not a process but an **agent**: goal + context + capabilities + budget. You do not operate the computer; you delegate to it. Every action an agent takes crosses AMAN, lands in SIJIL, is undoable if reversible and waits for consent if not, and the machine's memory is a file the user owns.

**Why from-scratch is itself the moat.** The shipped image contains zero third-party code: no transitive-dependency CVEs, no xz-style backdoor surface, no crates.io supply chain — a TCB auditable end-to-end by one person. That property matters uniquely here: the component enforcing agent governance cannot itself be trojaned via a dependency. It is also the cultural engine (SerenityOS proved purity recruits) and the story no lab or distro can copy. The honest caveat, printed everywhere: CVE-free is not bug-free, and our own crypto bug has no advisory feed — so every parser is fuzzed, every crypto path conformance-tested, and the word "secure" is banned until an external audit.

**Scope amputation that makes it feasible:** VM-first, **VirtIO-only** for years. QEMU/Firecracker are the hardware; drivers reduce to a dozen OASIS-specified devices. Redox's eleven years of bare-metal driver pain is the base rate we opt out of; Hermit and Asterinas are the existence proofs that a tiny team, VM-only, pure-Rust OS ships.

**Honest cost, accepted:** 18–30 months to a self-designed kernel running real agent workloads; years to dailyable; a decade to bare metal. The market signal (E2B: $21M Series A, Firecracker sandboxes at Fortune-100 scale) says agent-execution isolation is where this decade's money is — and nobody ships a non-Linux, capability-native guest. Arabic-first is the second uncontested lane: the GCC funds models and clouds; nobody ships an Arabic agent OS.

**The claims ladder is law:** say "governed, journaled, undoable" until isolation is hardware; say "isolated" only after the yard (§3); say "secure" only after audit.

---

## 2. The Purity Charter

**Standing rule: third-party code may appear in build/test/CI tooling (differential tests, fuzz corpora, interop suites) but never in the shipped image.**

| Tier | Contents |
|---|---|
| **1 — Write now, from scratch** | UEFI-app kernel + boot path, frame/heap allocators, scheduler + executor, all VirtIO drivers (PCI + MMIO transports), filesystem + SIJIL on-disk format, TCP/IP stack, HTTP/1.1, TLS 1.3 state machine, TTF/OTF parser, UAX #9 bidi, Arabic shaper (terminal-grade first), 2D compositor, WASM interpreter (non-JIT), QOI/PNG decoders, pvclock/kvmclock time reader |
| **2 — Vendor-then-replace (the sole exception, public)** | Crypto primitives + X.509 path validation (rustls/ring/webpki), confined to the **net-broker task in the yard** — never linked into NAWA, AMAN, or any agent. Replace triggers, both required: (a) own constant-time primitives pass Wycheproof + BoGo; (b) a funded external cryptographic audit. **Declared permanent-until-funded, publicly** — we do not pretend the trigger fires on a hobby budget. fiat-crypto-generated code is an acceptable intermediate, flagged as from-scratch-adjacent |
| **3 — Accepted forever (boundary and data, not code)** | rustc/LLVM toolchain, machine firmware (UEFI), QEMU/Firecracker as dev/deploy hosts, AI model weights/APIs, Unicode data files (UCD, `ArabicShaping.txt`, `BidiTest.txt`) + conformance suites, OpenType/IETF/OASIS specs, Mozilla CA store as versioned data with an update mechanism, fonts (Amiri/Noto Naskh — content, like weights) |

**Explicit TLS decision.** TLS is mostly X.509 and time, not RFC 8446. Named line items with owners and phases: certificate chain validation, name constraints, CA-store update path, and a guest time source (own pvclock reader + vsock time sync at boot). The TLS 1.3 *state machine* is ours (Tier 1); cipher operations and X.509 stay Tier 2 in the yard, where "quarantined" is architecturally true, not a module boundary wearing an isolation costume.

**Explicit Arabic decision.** Terminal-grade shaping (joining classes from `ArabicShaping.txt`, contextual forms on a monospace grid) is the shipping bar — shaped RTL **output** plus simple input echo first. **Bidi editing semantics** (cursor, selection, line-wrap on a mixed-direction grid) is a named, deferred milestone in Phase 3 — it is where Arabic terminals die, and we schedule it instead of discovering it. **Naskh-grade GSUB/GPOS** validated against the HarfBuzz corpus is a **multi-year flagship track (years 3–6, Arabic+Latin only)** — rustybuzz took years; we do not pretend otherwise. The M0 boot banner uses build-time-baked glyph bitmaps from Amiri (Tier 3 data), replaced by the live pipeline in Phase 2 — flagged in the devlog, not hidden.

---

## 3. Kernel Architecture

Per the judge's hybrid: **the monokernel's calendar on the microkernel's skeleton** — a demo-first monokernel that pours **one privilege boundary at month ~4**, with the daemon zoo staying in-kernel for years.

**Boot.** The kernel *is* a UEFI application: rustc's `x86_64-unknown-uefi` target emits the PE directly — no `bootloader` crate, no Limine, no assembly stub. On entry: GOP framebuffer, RSDP, `AllocatePages`, the `ExitBootServices` re-fetch dance, 4-level page tables (higher-half at `0xffff_8000_…`, full physical offset map, NX/WP), GDT/IDT, x2APIC + TSC-deadline. All arch code (boot, MMU formats, trap entry, context switch, timers — ~10–15% of the kernel) lives behind a HAL trait from commit one, so the aarch64 port is a 2–4-month job later, not a rewrite. x86_64 first: best tutorials, best-debugged QEMU machine, Firecracker's primary target, KVM on free CI runners.

**Memory.** Bitmap frame allocator → linked-list → slab heap. Zircon-VMO-style **memory objects**, not seL4 untyped/retype — the correct solo amputation. Framekernel discipline (Asterinas): `unsafe` is legal only in `nawa-core`; AMAN/SIJIL/DHAKIRA logic is safe Rust above it; CI rejects `unsafe` elsewhere.

**Protection — the yard (SAHA), month ~4.** Ring 3 with **one shared flat user address space** for all untrusted code, entered only via `syscall` into the gate. Capability tables and the SIJIL write path live kernel-side. The rule: **third-party = WASM = the yard.** First-party agents may remain kernel async tasks for velocity. Cost: +4–8 weeks (one trap path, one extra mapping — no ELF zoo; the WASM runtime is the yard's first resident). This converts the two claims the product rests on — *capabilities cannot be forged* and *the journal cannot be falsified* — from type-system promises into hardware facts **before the flagship demo is filmed**. Stated honestly: the yard isolates untrusted code from the kernel, not tools from each other; **per-agent address spaces within the yard are a Phase 3 milestone honestly costed at 4–6 months** — there is no "4–8 week retrofit" story in this plan, because untrusted code is separately-loaded user code from month 4 onward, so no metamorphosis is ever needed.

**Scheduling.** Agents are the unit. The Agent Control Block holds goal, lineage, capability table, budget counters (tokens/bytes/wall-clock), journal cursor. The queue is keyed by **deadline and budget**, not timeslice; APIC-timer preemption from Phase 1. **Durability law (fixing the v-mono hand-wave):** an agent is checkpointable iff its execution state is data by construction — **WASM instances and explicit state-machine agents**. Kernel-native Rust futures are ephemeral helpers and are never checkpointed. Durable agents are therefore WASM from day one; this is a design decision made now, not a surprise found later.

**IPC.** Typed bounded channels with ownership-transfer semantics. `delegate` spawns a child whose capability set is an attenuated subset and whose budget is carved from the parent — cost-groups and trust propagation by construction.

**The agent-native ABI — eight verbs, frozen shape.** One gate, `AmanGate`, is the only door. Arguments are plain-old-data, capabilities are table indices, **no pointers cross the gate** — enforced by a CI lint from commit one. That makes the gate serializable, hence trappable: the ABI never changes when the transport under it does (function call → `syscall` → later, per-address-space).

| Verb | Semantics |
|---|---|
| `spawn(goal, caps, budget)` | Create agent; lineage + grants recorded in SIJIL |
| `delegate(goal, subset, sub_budget)` | Child with attenuated caps, carved budget |
| `attenuate(cap, constraint)` | Derive weaker cap (path prefix, rate, expiry) |
| `invoke(cap, args)` | The only way to touch any device, file, or model |
| `approve(action, risk_class)` | Trap to system-owned consent surface; agent parks |
| `journal(query)` / `undo(entry)` | Read own/descendant SIJIL; request undo |
| `remember(ns, op)` | DHAKIRA access via namespace-scoped caps |
| `emit/await(chan)` | Typed IPC; `await` is durable suspension |

**AMAN** — the enforcement/policy split is the architecture. *Enforcement* = kernel capability tables: unforgeable, attenuate-only, lineage-tracked. *Policy* (intent → minted grant, risk classes, approval routing) is a rewritable kernel-side module for years, extracted as the first daemon (`aman-d`) in Phase 3, and **expected to be rewritten three times** — it is the genuinely unsolved layer, kept out of the TCB. Capabilities are bound **before inference, never parsed from model output**. Revocation v1 = lineage-tree kill (kill the subtree, reclaim at quiescence); derivation-tree *instant* revoke is deferred and named as the use-after-free tarpit it is.

**SIJIL** — written by the gate itself, kernel-side; agents cannot self-report, omit, or rewrite. Precise guarantee, marketed precisely: *every gate crossing and authority transfer the kernel actually performed is recorded* (agent, lineage, cap, args digest, result, budget delta); semantic meaning is enrichment by trusted kernel services, and bulk payloads are digested, not captured. In-RAM ring first (Phase 1), then hash-chained log-structured store on virtio-blk (Phase 2). **Undo is two-class law:** reversible effects go through copy-on-write staging generations in our filesystem (undo = drop the generation); irreversible ones (network sends, model calls) are journal-flagged and trap to `approve`, with tools registering compensation records to receive send-class caps at all. Never claim universal undo.

**DHAKIRA** — a namespace tree (`/memory/user/…`, `/memory/agent/<id>/…`) on our FS, reachable only via `remember` with namespace-scoped caps. Sharing = capability grant, journaled; residency = *not granting* the model-broker cap for tagged subtrees (`/memory/health/*` physically cannot reach cloud); the user's shell holds the root cap: inspect, edit, delete, export.

**Consent surface.** Console-rendered and kernel-owned in Phase 2; the moment a compositor exists (Phase 3), it becomes the **exclusive holder of a reserved framebuffer-region + input-queue capability** — spoof-proof by construction, adopted at birth because it cannot be retrofitted.

**VirtIO set** (both PCI and MMIO transports early; queue core ~2–4 weeks): rng (days) → blk (~1 week, first) → console (~1 week) → **vsock** (2–3 weeks; the host↔agent channel that needs no IP stack) → net (2–4 weeks; the TCP/IP stack is the real 3–6-month bill) → gpu-2D (2–4 weeks, scanout only). **GPU-3D is permanently out of scope** (Mesa-scale team-years; Firecracker has no GPU anyway).

**Tools & model access.** Tool format: our **non-JIT WASM interpreter** (3–6 months, official spec suite), yard-resident; a tool's import list *is* its permission manifest; 10–50× slower than JIT is irrelevant for IO-bound tools — and WASM is the outside-contributor surface that never touches the kernel. Model access staged: (a) **vsock → host relay** (dev-host tooling, zero code in the image, publicly flagged as the crutch it is); (b) own virtio-net + own TCP/IP, differential-tested against smoltcp and Linux **in CI only**; (c) net-broker task in the yard: our HTTP + TLS state machine, Tier-2 crypto/X.509, CA store as data, pvclock time. Agents request model *qualities* ("fast/private/frontier/arabic") from a broker cap — never raw sockets; token spend meters into budgets. Local inference is a year-4+ flag.

**Arabic path.** GOP framebuffer → virtio-gpu 2D. Own TTF/OTF parser (2–4 months, safe Rust, fuzzed — fonts are hostile input) → UAX #9 bidi against `BidiTest.txt` (2–3 months) → terminal-grade shaping (1–2 months). Shaped RTL output + input echo ship first; bidi editing is a named Phase 3 milestone; Naskh GSUB/GPOS is the year-3–6 flagship track.

---

## 4. Agent-Native Primitives (carried from v1/v2, now kernel constructs)

1. **ACB** replaces the process: goal, lineage, caps, budget; `spawn/delegate/suspend/resume/kill`; durable via WASM checkpoint.
2. **Kernel-mediated tools:** no agent holds credentials; every call is `invoke` through AMAN — the MCP-broker idea from v1, now the syscall dispatcher itself (MCP-shaped typed calls at the tool layer).
3. **Capabilities, not ambient authority:** unforgeable kernel handles, attenuated on delegation, revocable by lineage, bound pre-inference.
4. **Risk-classed consent:** irreversibles trap to a surface agents cannot draw; approval *policies* persist ("auto-approve reads, queue sends for morning").
5. **Two-class undo law:** CoW staging for the reversible; consent + compensation records for the rest.
6. **SIJIL:** gate-written, append-only, hash-chained — "what did my computer do while I slept?" is a query.
7. **DHAKIRA:** user-owned, namespaced, portable memory; sharing is a grant, not a sync service.
8. **Cost-groups:** hierarchical token/CPU/wall budgets inherited on spawn; scheduler-integrated (degrade → suspend, never overspend); burn-rate beside CPU/RAM.
9. **Residency by capability tag:** sovereignty enforced by non-grant, not policy PDFs.
10. **Trust propagation:** children run under subset caps and carved budgets, recursively — structural, not conventional.

---

## 5. Roadmap (gates bind; dates don't — all estimates carry the known 2× hobby multiplier)

**Phase 0 — Weeks 1–2: it boots in public.** Repo (Apache-2.0, DCO) + devlog live. Toolchain: rustc nightly, `x86_64-unknown-uefi`, QEMU, `xtask` runner. A UEFI-app kernel prints **"hello from the i3ml kernel"** on serial; CI boots QEMU headless (`-display none -serial stdio`, `isa-debug-exit`) and asserts the line on every commit. phil-opp's course runs in parallel as ramp.

**Phase 1 — Months 1–6: the ladder to the first agent syscall.**
- **M0 (mo 1–2):** GDT/IDT, exceptions, frame allocator, heap; framebuffer boot banner **إعمل** (baked glyphs, flagged); custom-test-framework CI. HN writeup #1.
- **M1 (mo 2–4):** APIC + timers, kernel async executor, ACBs, the eight verbs as function calls, in-RAM SIJIL, budget/deadline scheduler. On screen: an agent `spawn`s, `attenuate`s, `journal`s.
- **M2 (mo 4–6):** **The yard.** Ring 3, one shared user address space, `syscall`/`sysret` into the gate; WASM interpreter v0 loads a yard-resident tool that makes the first trapped agent syscall. CI proves the hardware fact: yard code cannot touch kernel memory.
- **Gate to Phase 2 (binding):** trap-path isolation test green in CI; first-agent-syscall screencast published; ≥6 devlogs shipped.

**Phase 2 — Months 6–18: the legendary demo, then outside users.**
- **Mo 6–9:** virtio rng/blk/console/vsock (both transports); own FS with CoW staging; persistent hash-chained SIJIL; console consent surface; vsock model relay (flagged crutch). **THE DEMO (~mo 8–10):** *an original kernel written by one Egyptian developer boots to an Arabic prompt, and the first thing it runs is not a shell but a goal — an agent fetches, transforms, writes; AMAN grants on screen; SIJIL records; one command rewinds it live.* Wording per the claims ladder: "governed, journaled, undoable." **NLnet/NGI Zero application at mo ~9.**
- **Mo 9–15:** own virtio-net + **own TCP/IP before font perfection** (the netstack retires the Linux-relay crutch; fonts don't). WASM spec-suite completion. Net-broker task in the yard: HTTP, TLS state machine, Tier-2 crypto/X.509, CA data, pvclock. Relay retired; "no Linux inside" becomes sayable.
- **Mo 15–18:** TTF parser, bidi, terminal-grade shaping (output + echo only); Firecracker (MMIO) + qcow2 **agent-box images** on Hetzner/DO; `i3ml` host CLI; docs-as-product. Honest buyer: self-hosters who won't give agents a real machine, researchers, homelabbers — dozens of deployments, credibility not revenue.
- **Gate to Phase 3 (binding):** demo filmed with ladder wording; own stack fetches an HTTPS page end-to-end and passes the CI differential suite; ≥10 outside deployments; NLnet submitted.

**Phase 3 — Years 2–4: multi-agent, shell, self-hosting, first revenue.** Per-agent address spaces inside the yard (**honest cost 4–6 months**); `aman-d` extracted as the first policy daemon; revocation v1 (lineage kill); SMP; aarch64 port (2–4 months); virtio-gpu 2D compositor + **exclusive consent-surface capability**; **bidi editing semantics milestone**; snapshot time-travel demos; **self-hosting moment:** an agent running on i3mlOS patches i3mlOS, rebuilds, reboots. Revenue, in order of realism: (1) **sandbox-wedge licensing** — a capability-mediated, journaled non-Linux *guest image* for E2B-class platforms; (2) **education** — the book/course "Build an Agent OS from Scratch," xv6/blog_os lineage; (3) **Gulf pilots** — air-gapped Arabic agent box, approached with the working artifact and the إعمل story, never a deck. Naskh flagship track starts. **Gate to Phase 4:** 100+ deployments; first revenue; ≥2 non-founder maintainers with merge rights; aarch64 CI green.

**Phase 4 — Years 4–10: the full OS.** Driver tasks behind **virtio-queue-window kernel objects** where it pays (net first); MCS-style CPU-budget caps; local inference; external security audit (unlocks the word "secure" and possibly the Tier-2 replacement); Naskh-grade shaper ≥95% on the HarfBuzz Amiri corpus; **bare-metal last:** one blessed reference device (mini-PC before laptop), NVMe + XHCI + modeset drivers written for that device only — the Redox lesson budgeted as years, entered only when the VM product funds it.

---

## 6. Learning Curriculum (ramp honestly stated)

1. **Writing an OS in Rust** (phil-opp, 2nd ed.) end-to-end, weeks 1–4 — overlapping Phase 0.
2. **OSTEP** + **xv6-riscv** source (MIT 6.1810) in parallel during months 1–3.
3. **OSDev wiki** as reference-while-building; **This Month in Rust OSDev** monthly.
4. Source order: **Hermit** (small, multi-arch, own virtio) → **Asterinas `ostd`** (the unsafe-boundary discipline to copy) → **Redox** (microkernel at scale) → **Theseus** (ideas).
5. Spec shelf: VirtIO 1.2, Intel SDM vol. 3, UAX #9, WASM core, RFC 9293/8446.

Honest ramp: 3–6 months before original work is productive; the first three months feel like drowning — that is normal and planned for. 18–30 months to agent workloads under QEMU/Firecracker is the realistic total; Phase gates already assume it.

---

## 7. Repo Structure (day one)

```
i3mlos/
├── nawa/
│   ├── core/            # the ONLY crate allowed `unsafe` (CI-enforced framekernel rule)
│   ├── hal-x86_64/      # boot, MMU, traps, timers behind the HAL trait (hal-aarch64 later)
│   ├── gate/            # AmanGate ABI: 8 verbs, POD-only types + CI lint (no pointers cross)
│   ├── aman/ sijil/ dhakira/   # safe Rust, kernel-side for years
│   └── virtio/          # queue core + pci/mmio transports + rng/blk/console/vsock/net/gpu
├── libs/                # no_std, host-testable, fuzzable: alloc, elf, fs, tcpip, ttf,
│                        #   bidi, shaper, wasm, sijil-format
├── yard/                # user-space image: wasm runtime, net-broker (Tier-2 crypto), tools/
├── host/                # dev-host tooling, never in image: i3ml CLI, vsock model relay, mkimage
├── fuzz/                # cargo-fuzz: virtqueue descriptors, ttf, elf, wasm, fs, tcpip
├── ci/                  # QEMU headless boot + expected-serial asserts; differential net tests
├── docs/                # devlog/, book/, purity-charter.md, WHAT-IT-CANT-DO-YET.md
└── xtask/               # cargo xtask build|run|test|image
```

Every parser and allocator is a host-testable `no_std` library first, kernel resident second — that is what makes solo velocity survivable.

---

## 8. Community & Funding (the SerenityOS playbook, applied)

**Standing promise** (Handmade-Hero pattern): *watch an operating system for AI agents get written from zero — every line ours.* **Cadence:** monthly written devlog held forever, decoupled from spectacle (Redox's decade of monthlies); videos/streams only when there is one (Kling's rule: build for your own joy, audience second). **Arabic-language mirror** of every devlog — an essentially uncontested funnel Tarek uniquely owns.

**Engineered demo moments:** (1) first boot + writeup; (2) **goal-as-shell** — agent scheduled, grant on screen, live undo (nobody has ever screencast this); (3) **"no Linux inside"** — real task end-to-end over our netstack, camera pans to the SLOC counter and zero vendored code (only after the relay is retired); (4) **self-hosted change**; (5) live time-travel: break it on stream, restore from the journal. Every demo ends with the issue list for the next piece; a contributor's first WASM tool PR is treated as the real product.

**Funding sequence:** OpenCollective fiscal host from month 1 (project entity, not personal Patreon — the Asahi lesson); GitHub Sponsors as a lagging indicator; **NLnet/NGI Zero at month ~9** (individuals eligible, no incorporation; memory-safe infrastructure + user-owned memory map directly onto their themes — the single most realistic grant for a solo Egyptian dev); Sovereign Tech at year 3+ (needs adoption evidence); Gulf sovereign-tech at years 2–4 with artifact in hand; sandbox-wedge licensing as first plausible revenue. Rule: audience before asks, artifact before decks, revenue before hires.

---

## 9. Top Risks (worst first) and Pre-Named Pivot Levers

| Risk | Mitigation + pivot lever |
|---|---|
| **Burnout / bus factor of one** (marcan, TempleOS, post-Kling Serenity) | Build for joy first; monthly cadence survives zero-progress months (silence is announced, never apologized for); fiscal host + co-maintainer ladder (WASM tools → yard services → libs → `nawa-core` last); the book doubles as succession documentation. **Lever:** if motivation collapses in plumbing, pull a demo milestone forward — the mono ladder permits it. |
| **Schedule truth** — every subsystem at 2× | Gates not dates; pre-named de-scopes in order: gpu-2D out of Phase 2, PNG/JPEG deferred (QOI only), SMP deferred. The netstack is never de-scoped — it retires the relay. **Lever:** if TCP/IP overruns, ship the vsock-only appliance labeled honestly ("no Linux in the guest; networking via host relay") and lean on the sandbox wedge, where a Linux *host* is already assumed. |
| **Claims outrun enforcement** (the judge's core critique of v-mono) | The yard lands *before* the flagship demo, so "cannot forge / cannot falsify" are hardware facts at filming time; claims ladder enforced in every script; gate POD-lint + `unsafe`-confinement in CI; a WASM escape lands in a capability-confined yard, not ring 0. |
| **The policy layer is unsolved** — approval fatigue socially reconstructs ambient authority on a perfect substrate | Opinionated risk-class defaults; SIJIL replay as the grant-tightening feedback loop; policy kept out of the TCB and *expected to be rewritten three times*. |
| **Working-but-userless** (Genode) | Named wedges with honest buyers (self-hosters, sandbox platforms, education, Gulf); docs-as-product. **Lever:** if the agent-OS thesis cools, NAWA is still a from-scratch capability microVM guest for the E2B market; if funding fails, the education product monetizes the artifact as-is. |

---

## 10. Success Metrics & Binding Gates

| Phase | Binding gate (advance only when true) | Health metrics |
|---|---|---|
| **0** (wk 2) | CI boots QEMU on every commit and asserts "hello from the i3ml kernel"; repo + devlog public | First writeup out |
| **1** (mo ~6) | Yard isolation proven by CI test (user code cannot touch kernel memory); first trapped agent syscall screencast | 6+ devlogs; phil-opp + xv6 completed; HN post #1 |
| **2** (mo ~18) | THE demo filmed with ladder wording; own TCP/IP passes differential suite and fetches HTTPS end-to-end (relay retired); Firecracker + qcow2 images public | ≥10 outside deployments; NLnet submitted; ≥1 external WASM-tool contributor |
| **3** (yr 2–4) | Per-agent address spaces shipped; `aman-d` extracted; self-hosting moment filmed; ≥2 non-founder maintainers | 100+ deployments; first revenue; aarch64 CI green; bidi-editing milestone; grant or sponsorship covering costs |
| **4** (yr 4–10) | External audit complete before any "secure" claim; Tier-2 crypto replaced only after Wycheproof + BoGo + audit; bare-metal boot on one blessed device | Naskh shaper ≥95% HarfBuzz-corpus pass; sandbox-wedge revenue recurring; project survives a 3-month founder absence |

*The verb is still the vision: don't operate your computer — tell it. Now every line underneath the telling is ours.*
