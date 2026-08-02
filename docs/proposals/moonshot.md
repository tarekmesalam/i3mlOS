# i3mlOS — The Visionary Proposal

## 1. Vision & tagline

**Tagline: "Don't operate your computer. Tell it what to do." — إعمل**

i3mlOS is the first operating system where the schedulable unit is not a process but an **agent**: a durable object of *goal + context + capabilities + budget*, mediated by a kernel-grade broker the way seL4 and Fuchsia's Zircon mediate everything through unforgeable capability handles ([seL4](https://sel4.systems/) remains the proof that capability-mediated, formally-disciplined kernels are real, not academic vapor). Apps are demoted to tools agents summon; memory is a user-owned filesystem, not a vendor silo; every action the machine takes while you sleep is in an append-only journal you can query and undo. The three-year end-state is a Rust, agent-native shell and runtime on an atomic Linux base — the thing the research briefs show *nobody* has shipped: AIOS has the kernel ideas but no users, Letta has the memory but no OS, Warmwind has the branding but no stack. We build the stack, honestly, in the open, starting with a wedge one person can ship in 90 days.

## 2. Architecture (text diagram)

```
┌─────────────────────────── MASHHAD (the Scene) ────────────────────────────┐
│  Agent-first Wayland shell (Rust/Smithay): conversation surface = home     │
│  screen; app windows are summoned artifacts. Contains the CONSENT PLANE:   │
│  spoof-proof approval surface (system-owned layer-shell, agents cannot     │
│  render into it — Ctrl-Alt-Del for consent).                               │
├────────────────────────────────────────────────────────────────────────────┤
│                         WAKEEL (Agent Supervisor)                          │
│  Durable scheduler over Agent Control Blocks {goal, lineage, capability    │
│  handles, budget}. Syscalls: spawn/suspend/resume/fork/kill. Checkpoint/   │
│  replay (Temporal-style) → agents survive reboots and week-long waits.     │
├──────────────┬───────────────┬───────────────┬─────────────────────────────┤
│ BAWWAB       │ DHAKIRA       │ SIJIL         │ MIZAN                       │
│ (Gatekeeper) │ (Memory VFS)  │ (Journal)     │ (Cost-groups)               │
│ MCP broker:  │ FUSE ns:      │ Append-only,  │ Hierarchical token/$        │
│ every tool   │ /memory/user, │ broker-       │ budgets, inherited on       │
│ call crosses │ /memory/agent │ written, OTel │ spawn; degrade-to-local     │
│ this trusted │ /<id>; ACLs,  │ GenAI schema; │ before overspend; burn      │
│ process;     │ user-editable,│ powers undo   │ rate shown beside CPU/RAM   │
│ capabilities │ git-versioned │ ("time        │                             │
│ attenuate on │ & exportable  │ machine")     │                             │
│ delegation   │               │               │                             │
├──────────────┴───────────────┴───────────────┴─────────────────────────────┤
│ MAWJIH (Model Router): quality-class requests (fast/private/frontier) →    │
│ llama.cpp/Ollama local (incl. Jais/Fanar) or cloud APIs; capability-tagged │
│ data-residency: /memory/health/* physically cannot reach cloud endpoints   │
├────────────────────────────────────────────────────────────────────────────┤
│ MA3MAL (Workshop): execution sandboxes — bubblewrap/Landlock for cheap     │
│ tasks, Firecracker microVMs (~125ms boot) for untrusted/generated code    │
├────────────────────────────────────────────────────────────────────────────┤
│ BASE: bootc/OSTree atomic Fedora image (Universal Blue toolchain).         │
│ Atomic rollback = system-level undo for agent mistakes. Linux underneath,  │
│ forever: drivers, browsers, apps for free.                                 │
└────────────────────────────────────────────────────────────────────────────┘
   Wire formats: MCP (tool ABI) · A2A (inter-agent) · OTel GenAI (telemetry)
```

## 3. Primitives and how, concretely

- **Language:** Rust throughout the i3ml layer (supervisor, broker, shell, journal). Not ideology: capability handles must be unforgeable and the broker is the trusted computing base — memory safety there is load-bearing. Python/TS allowed only *inside* sandboxes.
- **Agents as first-class objects:** Wakeel keeps ACBs in SQLite + content-addressed checkpoint store; steps are event-sourced so any agent can be replayed, forked, or resumed. This is AIOS's validated scheduler design ([COLM 2025](https://arxiv.org/abs/2403.16971)) rebuilt as a product daemon, not a research artifact.
- **Capabilities:** Zircon-style handles implemented in the broker: `read:~/Projects/**`, `email:draft-only`, each attenuable when agent A delegates to B (B gets a subset, and a slice of A's Mizan budget — cgroup-style inheritance). Revocable mid-task.
- **MCP as syscall layer, brokered:** agents never link MCP clients directly. Bawwab is the only process holding tool credentials; every call is capability-checked, journaled, cost-metered, and — for irreversible risk classes (send/pay/delete) — trapped to the Consent Plane. MCP is the POSIX of this era (97M monthly SDK downloads, Linux Foundation-governed); we are its Flatpak-portals, not its competitor.
- **Memory:** Dhakira exposes a FUSE filesystem with Letta-style tiers; plain files + embeddings sidecar, git-versioned. The radical part is political, not technical: **the user owns the memory file** and can take it to any model vendor. Gap #3 in the landscape brief; nobody ships it.
- **Sandboxing:** graded — Landlock/bwrap for read-only work, Firecracker microVMs for anything that writes or executes generated code (the 2026 consensus stack of E2B/Daytona/Modal).
- **Model routing:** Mawjih speaks quality classes, backed by llama.cpp for local (Qwen/Llama and Arabic-first Jais/Fanar) and any cloud API; LiteLLM-style config, plus the kernel-enforced residency rule from capability tags.
- **Base distro:** bootc image on Fedora Atomic via Universal Blue tooling — a custom OS image is essentially a Containerfile, and OSTree rollback is the perfect substrate for a machine agents are allowed to modify.
- **Shell:** Smithay compositor. [niri](https://smithay.github.io/index.html) proves one author can ship a daily-drivable Smithay compositor in about a year; we fork that effort profile, not COSMIC's 5-person/4-year one, by keeping the shell deliberately spartan: conversation pane + summoned windows + Consent Plane + journal viewer.

## 4. MVP — ~3 months, 1–2 people

Ship **`i3mld` + `i3ml` CLI/panel on existing Linux/macOS** (Path 1): the broker (Bawwab) with capability grants and journal (Sijil), Wakeel with spawn/suspend/resume and checkpointed durability, Dhakira as a directory-based memory VFS, Mawjih routing to one local + one cloud model, Firecracker/bwrap sandboxes on Linux. No compositor yet. Distribution: curl-install script + qcow2/UTM VM image.

**The killer demo (one 3-minute video):** *"I told my computer to find every invoice in my email, file them, and draft the late-payment reminders — then I closed the laptop."* Reopen: the agent had suspended on the Consent Plane; the screen shows three drafted emails queued behind one approval prompt, a journal of every tool call it made overnight, the exact token spend, and a one-click **undo** that reverts the filing. Approve → sends. That single loop — durable overnight agency + spoof-proof consent + audit + undo — is the demo no lab product (Claude Code, ChatGPT agent) can replicate, because their trust layer is app-level, not system-level.

## 5. 12-month roadmap

- **Q1 (Aug–Oct 2026):** MVP above; public repo (Apache-2.0, DCO from day one); weekly devlog; the definitive "running agents safely on Linux" wiki (Arch-wiki playbook).
- **Q2 (Nov 2026–Jan):** **i3mlOS image v0.1** — bootc atomic image with i3mld preinstalled, booting on real hardware and cloud (Hetzner/AWS). A2A endpoint so external agents can task the box. Capability policy packs; first 1,000 users from the Claude Code/OpenClaw crowd ("a disposable, snapshottable computer for your agents").
- **Q3 (Feb–Apr 2027):** **Mashhad alpha** — the Smithay shell with conversation-as-home and the Consent Plane as a true system layer. Arabic/RTL as a first-class locale, Jais/Fanar local presets. First Gulf pilot conversations (appliance image, air-gappable).
- **Q4 (May–Jul 2027):** Multi-agent: forking, delegation with capability attenuation, Mizan budget trees in the system monitor. Enterprise seeds: signed images, audit-log export, fleet config — the open-core line. Target: daily-drivable by enthusiasts, 5–10k installs.

## 6. Why globally remarkable + Arabic-first

It attacks the five gaps every 2026 product left open *as one coherent artifact*: agent-native shell, capability security as an OS feature, user-owned memory, journal+undo, local-model offline operation. "The OS where agents are the process model" is a story that recruits systems-Rust contributors the way Redox and COSMIC did — but unlike Redox, it's useful in month three because it's Linux underneath. The Arabic angle is structural, not cosmetic: the name *means* the product (i3mel — "do!"), the shell is RTL-native from the first commit, voice/dialect intent and Jais/Fanar routing are defaults, and data-sovereignty capability tags map directly onto Gulf sovereign-AI procurement — a lane with literally zero competitors and enormous state funding, owned naturally by an Egyptian founder. Global alias ("Amal OS"-style) secured for pronounceability.

## 7. Top 3 risks & mitigations

1. **Labs ship the trust layer** (Microsoft is bolting agent workspaces onto Windows). *Mitigation:* be the open, local-first, user-owned counter-position — Home Assistant vs. Nest. Their incentive is lock-in; our moat is the journal and memory the *user* keeps. Ship the broker as a spec + reference so it can outlive us.
2. **Solo scope death** — this plan spans daemon, distro, and compositor. *Mitigation:* strict gates — the compositor doesn't start until the daemon has 1,000 weekly actives; each layer is independently useful and independently shippable; steal (AIOS architecture, Universal Blue tooling, niri patterns) rather than derive.
3. **Prompt injection breaks the trust story** — one viral jailbreak of the Consent Plane kills credibility. *Mitigation:* the broker treats all model output as untrusted (capabilities bound *before* inference, not parsed from it); irreversible actions always trap to the system surface; publish red-team results and pay bounties from day one.

## 8. Honest weaknesses

- The "kernel-level" framing is aspirational: for years this is daemons and a compositor on Linux, and critics will say so — the Warmwind backlash pattern. Our defense is only honesty in messaging.
- The killer demo depends on frontier-model reliability for long-horizon tasks; if overnight agents mostly fail, the OS showcases failures beautifully.
- Rust-everything slows the wedge versus a TypeScript hack; OpenClaw's 140k stars came from shipping fast and messy.
- Revenue is late (enterprise images, Gulf services) and the MENA channel is partnerships-heavy — a distraction risk for a solo builder before segment-1 traction exists.
- A compositor's hidden 80% (a11y, portals, notifications) may consume Year 2; Mashhad could slip a full year without harming the daemon, but the "it's an OS" narrative would slip with it.

Sources: [seL4](https://sel4.systems/) · [AIOS paper](https://arxiv.org/abs/2403.16971) · [Smithay](https://smithay.github.io/index.html) · [niri guide 2026](https://petronellatech.com/blog/niri-scrollable-tiling-wayland-compositor-guide-2026/) · plus research-brief citations (MCP/Linux Foundation, Universal Blue, Firecracker, Letta, A2A, OTel GenAI).
