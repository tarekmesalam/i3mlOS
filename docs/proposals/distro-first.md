# i3mlOS — The OS Purist Proposal

## 1. Vision & Tagline

**Tagline:** *"i3ml. Don't operate your computer — tell it."* (from the Arabic imperative إعمل, "do")

i3mlOS is a real, bootable Linux operating system in which the agent — not the app — is the unit of computation. You install an ISO or launch a cloud image; you log into a conversation, not a desktop; the init system supervises agents the way systemd supervises daemons; every tool call flows through a kernel-adjacent MCP broker enforcing capability-based permissions; and the screen is generative — windows exist only when an agent summons one. Nobody has shipped this (AIOS stayed a paper, Warmwind is cloud RPA, SUSE/Ubuntu bolt agents onto normal desktops), and the pieces to build it honestly — bootc, systemd, Smithay, MCP, Firecracker — are all mature open source in 2026.

## 2. Architecture (text diagram)

```
┌─────────────────────────────────────────────────────────────┐
│ SURFACE                                                     │
│  mir3āt (مرآة "mirror") — Smithay/Rust Wayland compositor   │
│   ├─ Majlis: conversational primary surface (login = agent) │
│   ├─ GenUI engine: agent-generated ephemeral panels         │
│   │   (declarative JSON→GTK/Slint cards, no arbitrary HTML) │
│   ├─ Sakinah surface: SPOOF-PROOF approval overlay drawn    │
│   │   by compositor itself — no agent can render over it    │
│   └─ App wells: XWayland/Wayland windows agents summon      │
├─────────────────────────────────────────────────────────────┤
│ AGENT RUNTIME ("userspace kernel")                          │
│  i3mld — Rust supervisor, systemd-integrated (PID-1 adj.)   │
│   ├─ Agent units: *.agent files ≈ systemd units             │
│   │   (goal, model policy, caps, budget, lineage)           │
│   ├─ Durable scheduler: checkpoint/resume via sqlite WAL    │
│   ├─ simsār (سمسار "broker"): the MCP broker — ALL tool     │
│   │   calls traverse it; enforces caps, logs, meters        │
│   ├─ Capability store: unforgeable handles, attenuation     │
│   │   on sub-agent spawn, mid-task revocation               │
│   ├─ Sijil (سجل): append-only audit journal (OTel GenAI    │
│   │   schema) + snapshot-linked undo                        │
│   └─ Model router: local llama.cpp ⇄ cloud via LiteLLM      │
├─────────────────────────────────────────────────────────────┤
│ MEMORY                                                      │
│  Dhākirah: FUSE-mounted memory VFS (/memory/user/...,       │
│  /memory/agent/<id>/...) — Markdown+SQLite, user-owned,     │
│  git-versioned (Letta-style tiers), ACL'd per capability    │
├─────────────────────────────────────────────────────────────┤
│ ISOLATION                                                   │
│  bubblewrap+Landlock (trusted tools) · Firecracker microVMs │
│  (generated code / untrusted MCP servers) · per-agent       │
│  network namespaces + egress allowlists                     │
├─────────────────────────────────────────────────────────────┤
│ BASE OS                                                     │
│  Fedora bootc image (Containerfile-defined, OCI-delivered)  │
│  · immutable /usr · ostree rollback · btrfs snapshots       │
│  · systemd · greetd → Majlis as login "shell"               │
└─────────────────────────────────────────────────────────────┘
```

## 3. Primitives & How (concrete)

**Base distro:** Fedora bootc. The whole OS is a Containerfile atop `quay.io/fedora/fedora-bootc`; `bootc-image-builder` emits ISO, qcow2, AMI, and OVA from one definition ([Fedora Magazine](https://fedoramagazine.org/building-your-own-atomic-bootc-desktop/), [bootc-image-builder](https://github.com/osbuild/bootc-image-builder)). Immutable /usr + ostree rollback means an agent that breaks the system is one reboot from undone. Universal Blue proves a tiny team can maintain this.

**Agents as init units:** i3mld (Rust, tokio) registers each `*.agent* file as a transient systemd scope — so agents get cgroups (CPU/RAM) for free, and we add *cost-groups*: hierarchical token budgets inherited on spawn, enforced by the broker; near-exhaustion degrades routing to the local model, exhaustion suspends rather than overspends. Durable execution: every agent step (LLM call, tool result) is checkpointed to SQLite; `i3ml suspend/resume/fork <agent>` are real commands.

**Login shell is an agent:** greetd launches mir3āt, which boots the user's root agent. `/bin/i3ml` is also a POSIX-terminal client to the same daemon, so SSH into an i3mlOS cloud image drops you into the agent, not bash (bash is one capability away).

**Compositor:** mir3āt in Rust on Smithay — the niri path (solo author, daily-drivable within a year; niri shipped 26.04 in April, [Phoronix](https://www.phoronix.com/news/Niri-26.04-Released)). We fork niri's plumbing rather than start from zero. Generative UI is deliberately constrained: agents emit declarative card JSON rendered by trusted native widgets (Slint) — never agent-authored HTML/JS, which would be an injection cannon.

**MCP as syscall layer, brokered:** agents never link MCP clients directly. simsār is the only process holding MCP server connections (10k+ public servers work day one). Every call is checked against the caller's capability set, journaled to Sijil, cost-metered, and — if tagged irreversible (send/pay/delete, from tool annotations + our risk classifier) — trapped to the Sakinah overlay, which only the compositor can draw. This is Flatpak-portals-for-agents, and it is the moat.

**Capabilities:** seL4/Zircon-inspired but pragmatic: signed tokens in i3mld (`fs:read:~/Projects/**`, `mcp:gmail:draft-only`, `net:egress:api.anthropic.com`), attenuated automatically on delegation, revocable live. Enforcement is layered: broker checks (all tools) + Landlock/bwrap (filesystem/net) + Firecracker (~125 ms boot) for anything an LLM wrote.

**Memory:** Dhākirah mounts at `/memory` via FUSE — plain Markdown + SQLite FTS underneath, git-versioned. The user's memory is a *file they own and can take to another machine* — no vendor silos it today.

**Model routing:** router requests qualities, not models: `private` pins to local llama.cpp (Qwen3-class 8–30B; Jais 2 / Fanar for Arabic), `frontier` goes through LiteLLM to Claude/GPT/Gemini. Capability-tagged data residency: anything under `/memory/health/*` physically cannot be routed to cloud endpoints — the broker strips it or refuses.

## 4. MVP — ~3 months, 1–2 people

Ruthless scope: **ship the bootable VM image, not the compositor.**

- Fedora bootc image (qcow2 + ISO + UTM for Apple Silicon): boots to fullscreen Majlis (initially a Rust/egui kiosk app on cage — a 200-line Wayland kiosk — not mir3āt yet).
- i3mld v0: agent units, spawn/suspend/resume, SQLite checkpoints, cost-groups v0 (hard token caps).
- simsār v0: brokered MCP for filesystem, shell (bwrap'd), browser (Playwright), + any user-added MCP server; capability grants per agent; Sijil journal with a "what did it do?" timeline view; btrfs snapshot before/after every session → one-command undo.
- Approval overlay for irreversible-class calls.
- Local model (Ollama-compatible) + one cloud provider via LiteLLM.
- English + Arabic RTL in Majlis from day one.

**The killer demo (one 3-minute video):** *"Boot a fresh i3mlOS VM. Say: 'قارن أسعار الاستضافة واعمل لي تقرير' / 'Research these 3 hosting providers and draft a comparison doc.' Watch the agent work in a generated panel — then type `i3ml undo` and watch the entire session's filesystem effects roll back atomically, with the full Sijil audit log on screen."* Safe-by-construction + undo is the demo OpenClaw users are scared they need; no product shows it today.

## 5. 12-Month Roadmap

- **Q1 (Sep–Nov 2026):** MVP above. Public repo (Apache-2.0, DCO), weekly devlog, Discord, the "running agents safely on Linux" wiki. Target: 1,000 VM boots.
- **Q2 (Dec–Feb 2027):** Firecracker tier for generated code; memory VFS v1 (portable memory file); multi-agent spawn with capability attenuation; cloud images (Hetzner/DO/AWS) — SSH-into-your-agent; installer polish for bare-metal enthusiasts.
- **Q3 (Mar–May 2027):** mir3āt v0 replaces the kiosk (fork niri plumbing): app wells, generative cards, compositor-drawn Sakinah. Agent/policy package format (community skills). Arabic voice intent (Whisper large-v3 + dialect finetune), Jais/Fanar routing as default `private` tier for Arabic locales.
- **Q4 (Jun–Aug 2027):** Hardening: signed images, policy packs (audit-export, SSO) as first paid tier; two Gulf pilot deployments (university lab / gov innovation office) as services revenue; A2A endpoint so i3mlOS boxes appear as agents to external orchestrators.

## 6. Globally Remarkable + Arabic-First

Remarkable because it's *real*: everyone else ships an app on someone's OS; we ship an ISO where "kill an agent" is `systemctl`, undo is ostree/btrfs, and the approval dialog is unspoofable because the compositor owns it. It directly fills research-identified gaps #1, #2, #4 (agent-native DE, OS-level agent security, auditability/undo). Arabic-first is structural, not a translation: the name and the interaction verb are Arabic; RTL is native in a compositor we control (impossible to retrofit well); local Jais 2/Fanar routing + data-sovereignty-by-capability speaks directly to Gulf sovereign-AI buyers who fund models and clouds but have no agent *environment* to run them in. An Egyptian founder can own "the Arab world's operating system" narrative with literally zero competitors.

## 7. Top 3 Risks & Mitigations

1. **Solo-dev scope death (COSMIC took 5–10 engineers, 4 years).** Mitigation: the compositor is Q3, not Q1; MVP rides cage + egui; fork niri, don't rewrite; the runtime (i3mld/simsār) works on plain Ubuntu too, so value ships even if mir3āt slips.
2. **Frontier labs absorb the category (Atlas died against Chrome; OpenAI folds everything into ChatGPT).** Mitigation: labs won't ship a Linux distro — our moat is the layer *below* their agents: broker, capabilities, undo, sovereignty. i3mlOS runs Claude/GPT as guests; we're the airport, not an airline.
3. **Prompt injection through MCP tools/browsing wrecks trust.** Mitigation: this is the product, not a patch — untrusted content is capability-tainted, taint can't cross into irreversible tools without Sakinah approval, browsing runs in Firecracker with egress allowlists, and every incident is reconstructable from Sijil. Publish red-team results openly.

## 8. Honest Weaknesses

- **Boot friction is brutal.** Asking users to boot a VM/ISO is 100x the friction of `curl | sh`; OpenClaw got 140k stars because install took 60 seconds. The daemon-on-Ubuntu fallback dilutes purity but may be where actual users live — I'm betting the "real OS" wow factor overcomes friction, and that bet may simply be wrong.
- **The compositor is romance, not requirement.** 80% of stated value (broker, capabilities, undo, memory) needs no custom Wayland shell; mir3āt risks being years of art project. I keep it because it's the differentiation and the demo — but a disciplined CFO would cut it.
- **Hardware/daily-driver reality.** Nobody will daily-drive this for years; it's honestly an *agent appliance* (VM/cloud/spare box), and calling it an OS invites Warmwind-style backlash unless we stay transparent that it's Linux underneath.
- **Two-person burn vs. revenue timeline:** first real revenue (Gulf pilots, policy packs) is 12+ months out; the plan silently assumes runway or consulting income that the architecture cannot provide.

Sources: [Fedora bootc desktop](https://fedoramagazine.org/building-your-own-atomic-bootc-desktop/) · [bootc-image-builder](https://github.com/osbuild/bootc-image-builder) · [BlueBuild](https://blue-build.org/) · [Niri 26.04](https://www.phoronix.com/news/Niri-26.04-Released) · [niri repo](https://github.com/niri-wm/niri)
