# i3mlOS — The Pragmatist's Proposal

## 1. Vision & Tagline

**Tagline:** *"i3mel. Tell your computer what to do — and watch it do it, safely."*

i3mlOS is not (yet) a bootable OS — it is an **agent runtime + shell** that installs on the machine you already own and becomes your de-facto operating layer: a supervised daemon that runs durable agents, a hotkey-summoned command surface, a capability broker that mediates every tool call, a user-owned memory file, and a "time machine" journal that lets you see and undo everything your computer did. It rides the ecosystem that already won (MCP as syscall ABI, frontier + local models, Linux underneath) instead of fighting it. Once the runtime has users and a proven trust model, it graduates into a bootable atomic Linux image where the same runtime is PID-1-adjacent. The OS is earned, not declared — exactly the sequencing OpenClaw (140k+ stars in three months, still shipping [monthly stable releases](https://github.com/openclaw/openclaw/releases/tag/v2026.6.8) as of August 2026) proved the market rewards, and OpenAI Atlas's shutdown proved the alternative punishes.

## 2. Architecture (diagram-in-text)

```
┌──────────────────────────────── USER SURFACES ────────────────────────────────┐
│  i3ml Shell (Tauri 2 app: hotkey overlay + Mission Deck timeline + Sijil UI)  │
│  i3ml CLI (`i3ml do "..."`) · Telegram/WhatsApp bridge (OpenClaw-style)       │
└───────────────▲───────────────────────────────────────────────▲───────────────┘
                │ gRPC/unix socket                              │ approval traps
┌───────────────┴───────────────  i3mld (DAEMON, Rust) ─────────┴───────────────┐
│  Agent Supervisor ── Agent Control Blocks (goal, lineage, caps, budget)       │
│    spawn/suspend/resume/kill/fork · durable checkpoints (SQLite WAL)          │
│  AMAN Capability Broker ── ALL MCP traffic passes through here                │
│    capability grants · attenuation on sub-agent spawn · revocation · gates    │
│  SIJIL Journal ── append-only log of every tool call/grant/spend + undo hooks │
│  DHAKIRA Memory Service ── /memory VFS namespace, ACL'd, one portable file    │
│  Model Router ── quality classes (fast|private|frontier), budget-aware        │
└──────▲──────────────▲──────────────────▲───────────────────▲─────────────────┘
       │              │                  │                   │
  MCP servers    Sandbox layer      Model backends      A2A endpoint
  (files, shell, (bubblewrap/      (Anthropic, OpenAI, (delegate to/from
  browser via    Landlock · macOS   Ollama/llama.cpp,   external agents)
  Playwright,    Seatbelt · Win     Jais/Fanar via
  10k+ registry) AppContainer;      OpenRouter)
                 microVM later)
─────────────────── PHASE 2: bootc/Universal Blue atomic image ─────────────────
        i3mlOS distro: Fedora Atomic base, i3mld as system service,
        agent workspaces on separate Wayland sessions, ostree rollback
```

## 3. Agent-Native Primitives & Concrete How

**Language/stack:** `i3mld` in **Rust** (tokio; single static binary, low idle footprint — it runs 24/7). Shell UI in **Tauri 2** (stable, [v2.10 as of March 2026](https://en.wikipedia.org/wiki/Tauri_(software_framework)), ~5–20MB apps, first-class [sidecar/daemon patterns](https://v2.tauri.app/develop/sidecar/)) with a TypeScript/Svelte frontend. All state in **SQLite** (+ `sqlite-vec` for memory embeddings) — one file per concern, trivially backed up, no server dependency.

**Agent lifecycle (durable execution):** Each agent = an Agent Control Block row: goal, parent lineage, capability handles, token budget, checkpoint blob. Every step (LLM call, tool result) is checkpointed; `i3mld` restart resumes mid-task. This is Temporal-style replay implemented small — no Temporal cluster, just SQLite WAL + idempotent step IDs. Agents suspend on human-approval waits, webhooks, or cron (agents can schedule themselves).

**Capability broker (AMAN — أمان):** Steal the AIOS insight but productize it: MCP today is a library the agent links, so a hijacked agent calls tools directly. In i3mlOS **agents never hold MCP connections**. They emit tool-call intents to AMAN, which checks the agent's capability set (`fs:read:~/Projects/**`, `email:draft-only`, `spend:$2`), attenuates on sub-agent spawn (child ⊆ parent, cgroup-style), logs to Sijil, and traps risk-classed calls (send/pay/delete) to an approval surface **rendered only by the Shell process** — agents cannot draw it, so they cannot spoof it. Wire format is plain MCP (official Rust SDK), so all 10,000+ registry servers work unmodified.

**Sandboxing (pragmatic ladder):** Month 1–3: OS-native — **bubblewrap + Landlock** (Linux), **Seatbelt/`sandbox-exec` profiles** (macOS), **AppContainer** (Windows) — for shell/code execution, plus Playwright-driven browser in a separate profile. Month 6+: **Firecracker/cloud-hypervisor microVMs** on Linux and the distro (the E2B/Daytona-standard stack) for untrusted generated code. Honest rule shipped in docs: *cross-platform sandboxing is best-effort; the distro is where isolation gets real.*

**Memory (DHAKIRA — ذاكرة):** Letta's tiered model as a *system service*, not a per-app silo: a VFS-like namespace (`/memory/user/preferences`, `/memory/projects/<x>`, `/memory/agents/<id>/episodic`) with per-path ACLs enforced by AMAN. Critically: **it's one SQLite file the user owns** — inspectable in the Shell ("what does my computer know about me?"), editable, deletable, exportable, syncable via any file sync. Nobody ships user-owned portable memory; we make it the flagship feature.

**Journal & undo (SIJIL — سجل):** Append-only, broker-recorded (never agent-self-reported), OTel GenAI-schema-compatible. Reversible ops (file writes → copy-on-write shadow via reflinks/APFS clones; drafts vs sends) register undo handlers: the Shell shows a scrubable timeline — *"what did my computer do while I slept?"* with per-action ⟲.

**Model routing:** Agents request quality classes, never models: `fast` (local Ollama/llama.cpp — Qwen/Llama-class), `private` (local-only, enforced: `/memory/health/*`-tagged data physically cannot leave the machine), `frontier` (Claude/GPT via native APIs), `arabic` (Jais 2 / Fanar via local or OpenRouter). Budget-aware degradation: near budget exhaustion → downgrade to local, then suspend — never silently overspend. Router is in-daemon (LiteLLM-inspired, Rust), not a proxy dependency.

## 4. MVP (~3 months, 1–2 people)

**In scope:** `i3mld` daemon (supervisor, checkpoints, AMAN with 6–8 capability types, Sijil, basic Dhakira) · Tauri Shell (global hotkey → command bar; timeline; approval cards; memory inspector) · bundled MCP servers: filesystem, shell (sandboxed), browser (Playwright), calendar/email (draft-only default) · model router with Anthropic + Ollama + Jais · `curl | sh` installer for macOS + Linux (Windows Q2) · Arabic/RTL UI from day one.

**Out of scope (say no):** distro, microVMs, A2A, multi-agent teams, plugin marketplace, mobile, voice.

**The single killer demo (one 3-minute video):** Hotkey. Type — in Arabic or English — *"i3mel: go through my Downloads and Desktop, file everything into my project folders, unsubscribe me from the newsletters in my inbox from this month, and draft replies to the three emails waiting on me."* Split screen: left, the agent working; right, Sijil filling in real time — every file move, every capability check, an approval card popping for the unsubscribe clicks. Then the money shot: user drags the timeline back and **un-does one file move with a single click**, and opens the memory inspector to see "user files invoices under ~/Finance" was learned — then deletes that memory. No other product on earth can show act + audit + undo + owned-memory in one loop. That's the wow, and it's honest.

## 5. 12-Month Roadmap

- **Q1 (Sep–Nov 2026):** MVP above. Ship week-6 alpha to 50 Claude Code/OpenClaw power users. Weekly devlog, Discord, the demo video. Launch on HN + X + Arabic dev communities simultaneously.
- **Q2 (Dec–Feb 2027):** Windows port; scheduled/recurring agents; MCP registry one-click install with AMAN-generated least-privilege grants; memory sync (file-based, E2EE); community **skill/policy packs** (Obsidian-plugin analog). Target: 5k installs.
- **Q3 (Mar–May 2027):** **i3mlOS Distro v0.1** — bootc image on Fedora Atomic (a Containerfile, per Universal Blue playbook): i3mld as system service, Firecracker sandboxing, ostree rollback aligned with Sijil undo. Ship as VM image (qcow2/UTM) first — "a disposable, snapshottable computer for your agents" — ISO second. First revenue: paid support pilots (Gulf enterprise/education, air-gapped image + Jais/Fanar local).
- **Q4 (Jun–Aug 2027):** A2A endpoint (delegate to external agents under attenuated caps); fleet/policy management (open-core paid tier: SSO, audit export, signed images); Arabic voice intent on-device; 20k installs, 3 paying org pilots, 2–4 person team funded by pilots/Gulf accelerator.

## 6. Globally Remarkable + Arabic-First

Remarkable because it fills the exact gaps every researcher lists as empty: **broker-mediated MCP** (nobody kernel-mediates tool calls), **spoof-proof approval surface**, **user-owned portable memory**, **system-wide undo for agent actions**. These are trust features frontier labs structurally underinvest in (they optimize capability, and they won't ship a cross-vendor memory file that reduces lock-in). Arabic-first is not a translation checkbox: the *name is the interface* (إعمل — the imperative "do!"), RTL-native shell, dialect-aware intent, Jais/Fanar routing, and data-sovereignty defaults — landing into GCC sovereign-AI budgets (88% CEO gen-AI adoption per PwC) where everyone sells models and clouds but **nobody sells an Arabic agent computing environment**. Zero competition in that lane; an Egyptian founder owns the narrative credibly.

## 7. Top 3 Risks & Mitigations

1. **Platform absorption** — Anthropic/Microsoft ship the same layer natively (Cowork, Windows agent workspaces). *Mitigation:* be the **cross-vendor, user-sovereign** layer they can't be: your memory file, your journal, any model, local-capable; and hold the distro exit ramp they'll never take. Ship the trust layer, not the agent smarts.
2. **Prompt injection through brokered tools** — the industry's open wound; one viral disaster kills a trust-branded product. *Mitigation:* AMAN's design assumes compromise: least-privilege caps, draft-only defaults for outbound actions, risk-class traps on irreversibles, taint-tracking of web-derived content triggering stricter gates, and public red-team bounties from beta.
3. **Solo-founder burnout / scope creep** (the Open Interpreter arc). *Mitigation:* the roadmap above says "no" to 80% of the vision for a year; community carries skill packs and MCP integrations (Home Assistant playbook); revenue from Q3 pilots funds hires before ambition outruns hands.

## 8. Honest Weaknesses of My Approach

- **It's not an OS on day one, and skeptics will say so.** "Warmwind backlash" risk is real; mitigated only by honest framing ("agent runtime today, distro in Q3") — which dulls the marketing.
- **Riding host OSes means inheriting their limits:** best-effort sandboxing on macOS/Windows, no true capability enforcement below my broker, Apple/Microsoft can constrain daemons or ship competing primitives at any WWDC/Build.
- **The daemon+overlay category is crowding** (Raycast AI, Claude Desktop, OpenClaw); my differentiators are trust features, which historically sell worse than capability features — I'm betting 2026's agent-fear moment changes that, and that bet could simply be early.
- **Deferred differentiation:** the truly OS-shaped work (agent-native compositor, microVM-per-agent) is 12+ months out, giving a funded team time to leapfrog if the idea proves out; my only defenses are speed, community, and the Arabic lane.

Sources: [OpenClaw releases](https://github.com/openclaw/openclaw/releases/tag/v2026.6.8) · [OpenClaw status 2026](https://petronellatech.com/blog/openclaw-ai-agent-guide-2026) · [Tauri v2 sidecar](https://v2.tauri.app/develop/sidecar/) · [Tauri framework](https://en.wikipedia.org/wiki/Tauri_(software_framework)) · plus research-brief citations (AIOS, MCP/Linux Foundation, Universal Blue/bootc, Firecracker/E2B, Letta, A2A, OTel GenAI, Jais/Fanar).
