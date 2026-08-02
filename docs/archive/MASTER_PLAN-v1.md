# i3mlOS — Master Plan v1.0 (August 2026)

## 1. Vision & Positioning

**Vision.** i3mlOS (from the Arabic imperative **إعمل — i3mel, "do!"**) is a computing environment where the schedulable unit is not an app but an **agent**: goal + context + capabilities + budget. You don't operate the computer; you delegate to it — and it acts inside a trust fabric *you* own: every tool call brokered, every action journaled, everything reversible undoable, everything irreversible gated behind your approval, and a memory file that belongs to you, not a vendor.

**Honesty doctrine (anti-Warmwind clause).** The OS is **earned, not declared**. Publicly: Phase 1 ships the **i3ml runtime** on the machine you already own; the artifact called **i3mlOS** — a bootable image — ships only when it's real (Phase 2). Day one we publish a "What i3mlOS is and isn't (yet)" page with the criteria for when we'll use the word OS. This converts the Warmwind-backlash risk into a credibility asset.

**Tagline options** (pick one per market, test all three):
1. *"Don't operate your computer. Tell it."* — إعمل
2. *"The computer that works while you sleep — and shows its work."*
3. *"i3mel: tell your computer what to do. Safely."*

**Positioning.** We are not another agent — we're the layer *under* agents: **"Flatpak portals for agents"**; the **airport, not an airline** (Claude/GPT/Jais land here as guests); Home-Assistant-vs-Nest user sovereignty. Critically, we do **not** sell trust in the abstract (judges' consensus: trust features alone don't sell). We sell the **capability trust unlocks**: *"the only agent you can safely leave running overnight."* Fear of granting access is today's #1 blocker for the 3.2M-user OpenClaw crowd ([355k GitHub stars by April 2026](https://medium.com/data-science-collective/355k-github-stars-in-5-months-17-defense-rate-the-complete-honest-guide-to-openclaw-28d2f59598e1)); we remove the fear, so users grant more, so agents do more.

**Arabic-first angle.** Structural, not cosmetic: the name *is* the interaction verb; RTL-native UI from the first commit (we control the shell — impossible to retrofit well); an `arabic` model-quality class routing to [Jais 2 (70B, Inception/Cerebras/MBZUAI, 2026)](https://mbzuai.ac.ae/news/inception-cerebras-and-mbzuai-release-jais-2-the-next-generation-of-the-worlds-leading-arabic-open-weight-llm/) and Fanar 2.0 (27B, QCRI) locally or via API; capability-tagged **data-residency** defaults that map directly onto Gulf sovereign-AI procurement. The GCC funds models and clouds; **nobody sells an Arabic agent computing environment**. Zero competitors; an Egyptian founder owns the narrative. Naming pragmatics: keep **i3mlOS** as canonical brand; secure a pronounceable spoken alias ("**Amel**", pronunciation guide "EH-mel") plus domains/handles for both spellings; always write "i3ml", never bare "i3" (i3wm SEO collision).

## 2. Strategy in One Paragraph

All three judges picked **layer-first** (26/27/27) for the same reason: it's the only plan whose first artifact meets users where they live — `curl | sh` onto the laptop they already own — and whose weaknesses (overscope, market risk) are cuttable rather than structural (distro-first's ISO friction kills the community flywheel; moonshot's daemon+distro+compositor-in-12-months is a burnout schedule). So the strategy is layer-first's **sequencing** with the losers' best organs transplanted and every named flaw fixed: Phase 1 ships the runtime (broker, journal, honest two-class undo, owned memory, recurring agents as the retention hook) on Linux/macOS; Phase 2 — **gated on 1,000 weekly-active installs, gate binds, dates don't** — wraps the same runtime into a bootable Fedora-bootc VM appliance using distro-first's cage+egui kiosk, systemd-scope agents with cost-groups, and btrfs snapshot undo; Phase 3 — gated again — builds moonshot's Smithay shell, where the approval surface finally becomes literally spoof-proof. Moonshot's principles (capabilities bound before inference, residency tags, broker-as-open-spec, traction gates) apply from day one. Each phase is independently valuable; each later phase makes an earlier promise literal instead of aspirational. The deepest shared risk — "will trust-enabled capability actually retain users?" — is exactly what this sequencing tests cheapest and soonest.

## 3. System Architecture

```
┌────────────────── SURFACES ──────────────────┐
│ i3ml Shell (Tauri 2 + Svelte, ar/en RTL):    │
│  hotkey bar · Sijil timeline · approval      │
│  cards · memory inspector · burn-rate meter  │
│ i3ml CLI: do/ps/suspend/resume/fork/kill/    │
│  undo/mem/every                              │
└──────────────▲───────────────────────────────┘
               │ gRPC over unix socket
┌──────────────┴───── i3mld (Rust daemon) ─────┐
│ Supervisor: Agent Control Blocks, durable    │
│  event-sourced checkpoints, cron scheduling  │
│ AMAN broker: caps, MCP client pool, gates,   │
│  taint tracking                              │
│ SIJIL journal: append-only + undo registry   │
│ DHAKIRA memory: one user-owned file          │
│ Router: quality classes + budgets + residency│
└──▲──────────▲──────────▲──────────▲──────────┘
 MCP servers  Sandboxes  Models     (Ph.2+) A2A
```

| Component | What it is | Concrete tech |
|---|---|---|
| `i3mld` | Always-on supervisor daemon | Rust + tokio, single static binary; all state in SQLite (WAL) |
| Supervisor | Durable agent execution | ACB rows {goal, lineage, caps, budget}; every step (LLM call, tool result) event-sourced with content-addressed checkpoints → resume after reboot, replay, `fork`; cron triggers for recurring agents |
| **AMAN** (أمان) broker | The moat: agents never hold MCP connections; all tool calls cross this trusted process | Official Rust MCP SDK client pool (10k+ registry servers work unmodified); capability tokens `fs:read:~/Projects/**`, `email:draft-only`, `net:egress:api.anthropic.com`, `spend:$5`; **bound before inference, never parsed from model output**; child ⊆ parent attenuation on spawn; live revocation; risk-classed irreversibles trap to approval; web/email-derived content is taint-tagged and escalates gates |
| **SIJIL** (سجل) journal | Flight recorder + undo | Append-only SQLite, broker-written (never agent-self-reported), OpenTelemetry GenAI schema; undo registry backed by copy-on-write shadows (Linux reflinks / APFS `clonefile`) |
| **DHAKIRA** (ذاكرة) memory | User-owned memory | One SQLite file + Markdown export; `sqlite-vec` embeddings + FTS5; namespace `/memory/user|projects|agents/<id>` with per-path ACLs enforced by AMAN; inspector UI: view/edit/delete/export |
| Router | Model access as a device | Quality classes `fast\|private\|frontier\|arabic`; backends: Anthropic + OpenAI APIs, Ollama/llama.cpp local, Jais 2/Fanar (local or API); budget-aware degradation local→suspend, never silent overspend; **residency rule:** capability-tagged paths (e.g. `/memory/health/*`) physically cannot route to cloud endpoints |
| Sandbox ladder | Honest, tiered isolation | Linux: bubblewrap + Landlock + network namespaces (real isolation). macOS: **"supervised mode"** — approvals, draft-only defaults, journal; *no isolation claim* (Seatbelt is deprecated; we won't ship security theater). Windows: WSL2 only. Phase 2: Firecracker microVMs (~125ms boot) for generated code |
| Shell | Daily surface | Tauri 2 (sidecar pattern) + Svelte; approval cards rendered **only** by the shell process — documented as *hardened, not spoof-proof* on host OSes; spoof-proofness arrives with Phase 2 kiosk / Phase 3 compositor ownership |
| Phase 2 base | The bootable OS | Fedora bootc Containerfile → [bootc-image-builder](https://github.com/osbuild/bootc-image-builder) emits qcow2/ISO/AMI from one definition; cage + egui fullscreen kiosk (a ~200-line Wayland kiosk, not a compositor); agents as transient systemd scopes (cgroups free) + cost-groups; per-session btrfs snapshots; ostree rollback |
| Wire formats | Ride the standards | MCP (tool ABI), A2A (Phase 2+ inter-agent), OTel GenAI (telemetry) |

**Naming discipline (fixes moonshot's "org chart" flaw):** exactly three Arabic names, all user-facing surfaces users actually see (AMAN = permissions UI, SIJIL = timeline, DHAKIRA = memory). Internals are boring English: supervisor, router, sandbox.

## 4. The Agent-Native Primitives (why it's an OS, not an app)

1. **Agent Control Block** — the schedulable unit is {goal, lineage, capabilities, budget}, with process-grade verbs: `spawn/suspend/resume/fork/kill`, durable across reboots and week-long waits.
2. **Kernel-mediated tool access** — no agent holds tool credentials; every call crosses AMAN (syscalls, not library calls; MCP is the wire format underneath).
3. **Capabilities, not ambient authority** — unforgeable, least-privilege, attenuated on delegation, revocable mid-task, bound *before* inference.
4. **Risk-classed consent** — irreversible ops (send/pay/delete) trap to a system-owned approval surface; approval *policies* persist ("auto-approve reads; queue sends for morning review").
5. **The two-class action law (honest undo)** — *what can be undone is undone in one click (CoW file shadows, snapshots); what cannot be undone waits for you (draft-only defaults).* Never claim universal undo.
6. **System journal** — append-only, broker-written, queryable: "what did my computer do while I slept?" — with per-action rollback where class 1 applies.
7. **Memory as a user-owned file** — ACL'd namespace shared across agents by permission; inspectable, editable, portable to any vendor. Nobody else ships this.
8. **Cost-groups** — hierarchical token/$ budgets inherited on spawn; burn rate displayed beside CPU/RAM; degrade-to-local, then suspend — never overspend.
9. **Data-residency by capability tag** — tagged data cannot reach cloud endpoints; sovereignty enforced by the broker, not by policy documents.
10. **Trust propagation** — a delegated sub-agent runs under a subset of its parent's capabilities *and* budget slice, recursively.

## 5. Roadmap

### Phase 0 — Weeks 1–2 (Aug 2026): the walking skeleton
- **Days 1–2:** GitHub org `i3mlos`, repo `i3ml` (Apache-2.0, DCO, SECURITY.md, CODE_OF_CONDUCT); domains + handles for i3ml/i3mlOS/Amel; Discord; landing page + waitlist; "What i3mlOS is and isn't (yet)" page.
- **Days 3–7:** Cargo workspace scaffold (§6); CI (clippy, test, shell build); Tauri shell scaffold with ar/en i18n and RTL switch proven.
- **Days 8–12:** End-to-end skeleton: `i3ml do "organize my Downloads"` → daemon spawns agent → one brokered `fs` MCP call through AMAN → SIJIL entry → CLI output. Record as README GIF.
- **Days 13–14:** AMAN spec v0.1 draft in `/spec` (the standards play starts now); devlog #1; recruit 10 design-partner users from Claude Code/OpenClaw communities.

### Phase 1 — MVP, ~3 months (Sep–Nov 2026; private alpha week 8, public v0.1 early Dec)
Scope is cut to survive the judges' "3x overscoped" verdict — one developer plus early contributors:

**In:** Linux (full sandboxing) + macOS (supervised mode, labeled); daemon with durable checkpoints + cron (**recurring agents ship in MVP** — the retention hook: "every morning, triage my inbox to drafts"); AMAN with 6 capability types (fs-read, fs-write, shell, net-egress, mcp-server-scoped, spend); SIJIL + file-op undo; DHAKIRA v0 + inspector; router (Anthropic + Ollama + `arabic` class via Jais 2 API); **three** bundled MCP servers: filesystem, shell (bwrap'd), browser (Playwright) — email via the user's own MCP server, forced `draft-only`; Tauri shell (hotkey bar, timeline, approvals, memory inspector, burn meter); `curl | sh` installer.

**Out (printed in README):** Windows-native, microVMs, A2A, multi-agent teams, marketplace, voice, mobile, compositor, hosted anything.

**The killer demo** (one 3-minute video; hard rule: ≥9/10 scripted dry-run pass rate before launch; tasks chosen inside current-model reliability — file ops + drafting, not open-ended web agency):
Evening, typed in Arabic then English: *"i3mel: file the invoices in my inbox into ~/Finance by vendor, and draft late-payment reminders for the overdue ones."* Laptop closes. Morning: the agent had **suspended on consent** — three drafts queued behind one approval card; SIJIL shows every tool call, capability check, and the exact token spend; the user scrubs the timeline and **un-does one misfiled document with one click**; opens DHAKIRA, sees "learned: invoices live in ~/Finance," **deletes the memory**; then approves → sends. Voiceover states the law: *"Undone what can be undone. Approved what can't be."* A 30-second Arabic cut ships the same day. This merges moonshot's overnight narrative with layer-first's inspector money-shot — minus the un-undoable unsubscribe clicks the judges flagged as brand-destroying.

### Phase 2 — The bootable OS, months 4–12 (Dec 2026–Aug 2027)
**Gate (binding):** ≥1,000 weekly-active runtime installs. If unmet, effort stays on runtime retention; dates slip, the gate doesn't.
- **i3mlOS v0.1:** Fedora bootc Containerfile; ship order = qcow2/UTM VM image first (*"a disposable, snapshottable computer for your agents"* — matches the isolated-agent-box demand E2B/Daytona proved), then Hetzner/DO/AWS cloud images, ISO last.
- Boots via greetd into a **cage + egui fullscreen kiosk** — conversation as the login shell, no compositor project.
- **SSH into the box lands in the agent, not bash** — "bash is one capability away" (the DevOps-crowd hook).
- Agents as transient systemd scopes → CPU/RAM cgroups free, cost-groups layered on; Firecracker tier for generated code; per-session **btrfs snapshot → one-command system-level undo** (complementing SIJIL's per-action undo); signed images, ostree rollback.
- A2A endpoint (external agents can task the box under attenuated caps); community skill/policy packs v1; Windows runtime via WSL2.
- **First revenue:** 2–3 paid Gulf pilots (air-gapped image, local Jais 2/Fanar, audit export) — services, not product, per the GTM research.

### Phase 3 — The moonshot, years 2–3 (2028–2029)
**Gate:** distro ≥2,000 weekly actives + revenue covering 2 salaries + ≥2 non-founder maintainers.
- **Smithay compositor** (fork niri's plumbing; solo-viable per niri precedent) — conversation-first shell; app windows are summoned artifacts.
- The consent plane becomes **compositor-owned and literally spoof-proof** — the "Ctrl-Alt-Del for consent" promise, now true by construction.
- Constrained generative UI: agents emit declarative JSON rendered by trusted Slint/native widgets — never agent-authored HTML (closes the injection cannon).
- Multi-agent teams visible in the shell (fork/delegate/attenuate as UI); fleet management open-core (SSO, RBAC, audit export); Arabic dialect voice intent (Whisper + finetune); AMAN spec v1.0 submitted to the Linux Foundation's Agentic AI Foundation (where MCP lives); optional "agent box" mini-PC (Home Assistant Green playbook) only after software earns love.

## 6. Repo Structure (day one)

```
i3ml/
├── crates/
│   ├── i3mld/           # daemon: supervisor, ACBs, checkpoints, cron
│   ├── i3ml-broker/     # AMAN: capabilities, MCP pool, gates, taint
│   ├── i3ml-journal/    # SIJIL: append-only log, OTel GenAI, undo registry
│   ├── i3ml-memory/     # DHAKIRA: sqlite+vec+fts, ACLs, export
│   ├── i3ml-router/     # quality classes, budgets, residency
│   ├── i3ml-sandbox/    # bwrap/Landlock (linux), supervised-mode (macos)
│   └── i3ml-cli/        # `i3ml` binary
├── shell/               # Tauri 2 + Svelte; i18n/{en,ar}; RTL-first CSS
├── servers/             # pinned bundled MCP servers (fs, shell, browser)
├── spec/                # AMAN broker spec (CC-BY) — the standards play
├── distro/              # Phase 2: Containerfile, image CI (README stub now)
├── docs/                # "Running agents safely on Linux" wiki source
├── install.sh
├── .github/workflows/   # clippy, test, e2e-skeleton, shell build
└── LICENSE  DCO  SECURITY.md  ROADMAP.md  WHAT-IT-ISNT.md
```

## 7. Open-Source & Go-to-Market

- **License:** Apache-2.0 + DCO (no CLA drama; Gulf-government-acceptable). Spec under CC-BY so the broker model can outlive the company.
- **Cadence:** Friday devlog (every week, no exceptions — includes a "what we said no to" line); monthly release train; public roadmap board.
- **Content moat:** the definitive **"Running agents safely on Linux" wiki** — Arch-wiki playbook; SEO value even for non-users, feeding the funnel OpenClaw's security scare stories (["17% defense rate"](https://medium.com/data-science-collective/355k-github-stars-in-5-months-17-defense-rate-the-complete-honest-guide-to-openclaw-28d2f59598e1)) created.
- **Launch sequence:** week-2 skeleton GIF → week-8 private alpha to 50 Claude Code/OpenClaw power users → v0.1 "Show HN: i3ml — a capability broker and flight recorder for AI agents" + the 3-minute video, simultaneously on X and Arabic dev communities (Egypt, Gulf) with the Arabic cut.
- **Community surfaces:** skill/policy packs (the Obsidian-plugin analog); `awesome-i3ml`; contributor recruiting on the Redox-style systems-Rust narrative — "agents are the process model" — except useful in month three because it's Linux underneath.
- **Security as marketing:** public red-team bounty from beta (small cash + hall of fame); publish the threat model, including what macOS supervised mode does *not* protect against; publish incident post-mortems from SIJIL reconstructions.
- **Monetization ladder (Nabu Casa playbook):** free personal forever → paid: signed enterprise images, fleet/SSO/RBAC/audit-export, compliance policy packs → services first (Gulf pilots, $30–80k each, fund hires before ambition outruns hands) → hosted "i3ml Cloud" only with a team ≥3. Rule: revenue before hires.
- **MENA channel:** positioning and Phase-2 revenue, not early roadmap driver — pursue Gulf accelerators/sovereign programs (Hub71, HUMAIN orbit, QCRI/Fanar partnerships) only after Segment 1 (global devs) exists.

## 8. Top Risks & Mitigations

| Risk | Mitigation |
|---|---|
| **Platform absorption** — Anthropic/Microsoft ship broker+journal natively (Cowork, Windows agent workspaces) | Be what they structurally can't: cross-vendor, local-capable, user-owned memory/journal (lock-in-reducing features labs won't ship); AMAN as open spec so the layer outlives us; Arabic lane they won't chase; distro exit ramp they'll never take |
| **Prompt injection / first trust disaster** — one viral bypass kills a trust brand | Design assumes compromise: caps bound pre-inference; least privilege; draft-only outbound defaults; taint-tagged web content escalates gates; irreversibles always trap; every incident reconstructable from SIJIL; paid red-team from beta; never claim "spoof-proof" before Phase 2 makes it true |
| **Overscope/burnout** (judges: MVP was 3x; the Open Interpreter arc) | Printed Out-lists; three named subsystems max; binding traction gates before distro (1k WAU) and compositor (2k WAU + revenue); MVP demo scoped to reliable model behaviors; services income permitted pre-product-revenue |
| **Trust features don't retain** — the one assumption all three proposals deferred | Test it cheapest/soonest: recurring agents in MVP as daily pull; instrument week-2 retention from alpha; pivot lever pre-named — if retention <20%, lead with scheduled-automation surface (the daily driver) and keep the trust fabric underneath as differentiator |
| **"It's not an OS" backlash** (Warmwind pattern) | Honesty page + artifact naming (runtime now, OS at Phase 2) + published OS criteria; let critics quote our own definitions |

## 9. Success Metrics Per Phase

| Phase | Metrics (gate metrics bold) |
|---|---|
| **0** (wk 2) | E2E skeleton demo GIF public; 200+ waitlist; devlogs #1–2; AMAN spec v0.1 published |
| **1** (mo 4) | 1,000 installs; **≥1,000 weekly actives → unlocks Phase 2**; week-2 retention ≥20% of activated; ≥30% of actives run a *recurring* agent; demo ≥9/10 scripted passes; 10 external contributors; 0 unpatched critical vulns; first 100 Arabic-locale users |
| **2** (mo 12) | 5k+ cumulative runtime installs; ≥1k VM/cloud image boots with ≥20% week-2 return (boots ≠ users — measure return); 2–3 paid pilots ≥$50k total; 3 maintainers with merge rights; 25+ community skill packs; **≥2,000 weekly actives + 2 salaries covered → unlocks Phase 3** |
| **3** (yr 2–3) | 25k+ installs; compositor daily-driven by ≥500 enthusiasts; $20k+ MRR open-core; team of 3–5; AMAN spec adopted by ≥2 external projects; 1 MENA ministry/university deployment; the sentence "i3mlOS is the Arabic-first agent OS" appears in coverage we didn't write |
