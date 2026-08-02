> **ARCHIVED — path not taken.** On 2026-08-03 the founder decided i3mlOS will be built **entirely from scratch (no Linux code)**. This Linux-engine plan is kept as the documented fallback/reference.

# i3mlOS — Master Plan v2.0 (OS-first edition, August 2026)

**Supersedes v1.0.** The founder's binding decision: i3mlOS is a **standalone bootable operating system** that installs on the machine *instead of* Windows/macOS/desktop Linux. There is no host-OS app, no `curl | sh` runtime, no macOS/Windows product — ever. The first public artifact **boots**. v1's phase-1 "runtime on your laptop" is dead; everything proven in v1's architecture (i3mld, AMAN, SIJIL, DHAKIRA, router, cost-groups, residency tags) survives — it now lives *inside* the OS image.

---

## 1. Vision & Positioning — unapologetically an OS

**i3mlOS** (from the Arabic imperative **إعمل — i3mel, "do!"**; spoken alias **Amel**) is an operating system in which the schedulable unit is not an app but an **agent**: goal + context + capabilities + budget. You boot it, you log into a conversation, and you delegate: every tool call crosses the AMAN broker, every action lands in the SIJIL flight recorder, everything reversible is undoable in one click, everything irreversible waits for your approval, and the machine's memory (DHAKIRA) is a file you own.

**Legitimacy model: Android, not Ubuntu-with-a-skin.** Android, ChromeOS, and SteamOS are real operating systems whose engine is a Linux kernel nobody ever sees. We claim exactly that legitimacy: Linux kernel, systemd, Mesa, PipeWire, NetworkManager as invisible plumbing; the **identity layers — init policy, session model, shell, app/agent model, permission system — are 100% ours**. i3mlOS never shows a desktop-Linux experience: no GNOME, no bash login, no package manager in the user's face. AMAN/SIJIL/DHAKIRA sit where Android's system services sit — above the kernel, below the UI.

**Honesty doctrine, adapted.** We call it an OS from day one because it *is* one — it boots on metal, owns the disk, and mediates everything. What we publish instead of hedging the word is **`WHAT-IT-CANT-DO-YET.md`**: no Photoshop/AutoCAD/anticheat gaming, no Android apps, NVIDIA best-effort, dual-boot arrives v1.1, daily-drivable in year one only for developers and AI enthusiasts who live in a browser + terminal + agents. Plus an Asahi-style public **Certified / Compatible / Unsupported** hardware matrix — honesty as policy, the anti-Warmwind clause made structural.

**Positioning.** The airport, not an airline: Claude, GPT, Jais land here as guests. Flatpak-portals-for-agents at OS depth. Against frontier labs: they will ship agent features *on* Windows/macOS forever; they will never ship you a sovereign, auditable, local-capable operating system — that structural refusal is the moat.

**Arabic-first, structural.** The name is the interaction verb; RTL is native in a shell we own from the first commit (impossible to retrofit); an `arabic` model class routes to **Jais 2 (70B)** / **Fanar** / **ALLaM** locally or in-region; capability-tagged data residency maps directly onto Gulf sovereign-AI procurement. The GCC funds models and clouds; **nobody ships an Arabic agent operating system**. And because the product is now literally an installable appliance image, the Gulf air-gapped "agent box" is no longer a distant channel — it is a core Phase-1-adjacent revenue motion (§7).

---

## 2. Strategy in One Paragraph

We ship one OS three times, each phase demoting Linux one step. **Phase 1 (now):** a Fedora-**bootc** image — a Containerfile emitting qcow2/ISO/AMI via bootc-image-builder, the Universal Blue/Bluefin proof that 1–2 people can run an image-based OS — that boots into a **cage + egui kiosk** agent shell with the full v1 runtime (i3mld, AMAN, SIJIL, DHAKIRA) inside; distribution rides a **VM-image → live-USB → install** funnel plus cloud images, and retention rides recurring agents on an always-on box you reach from any browser (the Home Assistant model — the browser is a *client*, not a host product). **Phase 2:** the same image daily-drives **certified reference hardware** (Framework 13 AMD, ThinkPad T14, one AMD mini-PC) with a safe installer, bundled Chromium, and Flathub — while Gulf air-gapped pilots pay for it as an appliance. **Phase 3:** our own Smithay compositor makes the consent surface literally unspoofable, and fleet/enterprise features monetize. **Phase 4:** because our only public ABI was always AMAN/MCP/SIJIL/DHAKIRA — never POSIX, never systemd — Linux is demoted from *the kernel* to *a hardened engine* to *an unprivileged driver VM behind virtio* to, on our own SKUs, **gone**, replaced by a Linux-ABI-compatible Rust kernel (Asterinas-class). Gates bind; dates don't.

---

## 3. System Architecture — the OS image, bootloader to shell

```
┌─ SHELL (ours, RTL-first ar/en) ──────────────────────────────┐
│ Phase 1: cage (Wayland kiosk) + i3ml-shell (Rust/egui):      │
│  conversation surface · SIJIL timeline · approval cards ·    │
│  DHAKIRA inspector · burn-rate meter · fullscreen app wells  │
│  (Chromium, Flatpaks) · Phase 3: own Smithay compositor      │
├─ AGENT RUNTIME (the v1 core, unchanged, now in-image) ───────┤
│ i3mld supervisor: Agent Control Blocks, event-sourced        │
│  checkpoints (SQLite WAL), cron/recurring, agents run as     │
│  transient systemd scopes → cgroups + cost-groups            │
│ AMAN broker: capability tokens bound BEFORE inference,       │
│  MCP client pool, attenuation on spawn, taint tracking,      │
│  irreversibles trap to approval, residency enforcement       │
│ SIJIL: append-only broker-written journal (OTel GenAI) +     │
│  undo registry (reflink shadows + per-session btrfs snaps)   │
│ DHAKIRA: user-owned SQLite+Markdown memory, ACL'd namespace  │
│ Router: fast|private|frontier|arabic; llama.cpp local; cloud │
│  via LiteLLM; degrade-to-local → suspend, never overspend    │
├─ APP/COMPAT LAYER ───────────────────────────────────────────┤
│ Chromium built-in (agents drive it via CDP; users browse) ·  │
│ Flathub enabled OOTB (VS Code, LibreOffice, Zoom, Signal) ·  │
│ sandbox ladder: bwrap+Landlock → Firecracker for gen. code   │
├─ PLUMBING (Linux, invisible, wrapped) ───────────────────────┤
│ systemd · PipeWire · NetworkManager · Mesa · Wayland proto · │
│ greetd → autologin straight into the kiosk (no DE, no bash)  │
├─ BASE IMAGE & BOOT ──────────────────────────────────────────┤
│ Fedora bootc OCI image from GitHub CI · immutable /usr ·     │
│ A/B ostree updates + greenboot auto-rollback · btrfs ·       │
│ systemd-boot + UKI + measured boot · LUKS2 + TPM2 auto-      │
│ unlock (systemd-cryptenroll) · Fedora's Microsoft-signed     │
│ shim (MOK for our modules) · linux-firmware (full) + fwupd/  │
│ LVFS · factory reset = redeploy image, wipe /var ·           │
│ recovery USB image (SteamOS-style un-brick)                  │
└──────────────────────────────────────────────────────────────┘
```

| Decision | Choice & why |
|---|---|
| Base | **Fedora bootc**: whole OS is a Containerfile; CI emits qcow2/ISO/AMI/OVA from one definition; we inherit Fedora's kernel, CVE stream, and signed shim — the distro-maintenance tax the judges feared is outsourced (Bluefin precedent) |
| Secure Boot | Inherit Fedora's shim chain (own shim-review signing is months of mess mid-2026 cert transition); MOK-enroll our modules; documented disable fallback |
| Shell v1 | **cage + egui kiosk** (~200-line Wayland kiosk, the judges' favorite de-risking idea) — the "OS feel" with zero compositor project; own compositor is Phase 3, gated |
| Browser | Chromium **in the base image** — non-negotiable: covers ~80% of daily computing and AMAN's browser tooling needs CDP anyway; Firefox one Flatpak away |
| Apps | **Flatpak is the human app model** — we invent an agent model, not an app format. Skipped: Waydroid, Wine/Proton promises |
| Updates | bootc OCI pulls, A/B, greenboot rollback — an OS that updates itself like a phone |
| Naming discipline | Exactly three Arabic-named, user-facing subsystems: AMAN, SIJIL, DHAKIRA. Internals stay boring English |

**Kernel-portability laws (day one, keep the Phase-4 swap possible):**
1. The public ABI is **AMAN capability names, MCP, SIJIL's schema, DHAKIRA's file format, and the i3ml CLI protocol** — never systemd units, cgroup paths, `/proc`, eBPF, or kernel modules. Third parties that see Linux internals foreclose the swap.
2. Every Linux-only mechanism inside i3mld lives behind Rust traits — `Isolation` (bwrap/Landlock), `Snapshot` (btrfs), `Scheduler` (systemd scopes) — with semantics (two-class undo, attenuation) defined by *us*, implementable on any kernel.
3. POSIX shell is a capability, never a promise: "bash is one capability away" and explicitly unstable.
4. **virtio becomes our internal driver ABI** from Phase 2: the VM/cloud SKU is virtio-only by construction, which is exactly the surface Rust kernels can already drive.
5. Prefer musl/static-friendly userspace where cheap (eases relibc/Asterinas targets later).

---

## 4. Agent-Native Primitives (carried from v1, adjusted for OS depth)

1. **Agent Control Block** — {goal, lineage, capabilities, budget} with `spawn/suspend/resume/fork/kill`, durable across reboots; agents are systemd scopes today (cgroups free), behind the `Scheduler` trait tomorrow.
2. **Kernel-mediated tools** — no agent holds credentials; every call crosses AMAN (MCP is the wire format; 9,600+ registry servers work unmodified).
3. **Capabilities, not ambient authority** — unforgeable, attenuated on delegation, revocable mid-task, **bound before inference, never parsed from model output**.
4. **Risk-classed consent** — irreversibles trap to the approval surface; approval *policies* persist ("auto-approve reads; queue sends for morning"). Kiosk-drawn now (hardened); compositor-drawn in Phase 3 (spoof-proof by construction).
5. **Two-class action law (honest undo)** — reversible → one click (reflink shadows + per-session btrfs snapshots + ostree rollback: three layers, all real); irreversible → waits for you (draft-only outbound defaults). Never claim universal undo.
6. **SIJIL journal** — broker-written, append-only, OTel GenAI schema; "what did my computer do while I slept?" — and with UKI **measured boot + TPM**, tamper-evidence extends below userspace.
7. **DHAKIRA memory** — one user-owned file, ACL'd namespace, inspect/edit/delete/export; portable to any machine or vendor.
8. **Cost-groups** — hierarchical token/$ budgets inherited on spawn; burn-rate beside CPU/RAM; degrade-to-local, suspend, never overspend.
9. **Data residency by capability tag** — `/memory/health/*` physically cannot reach cloud endpoints; sovereignty enforced by the broker, not policy PDFs.
10. **Trust propagation** — sub-agents run under a subset of parent capabilities *and* budget, recursively.

---

## 5. Roadmap

### Phase 0 — Weeks 1–2 (Aug 2026): a skeleton that boots
Org, repo (Apache-2.0, DCO), Discord, landing page, `WHAT-IT-CANT-DO-YET.md`. Cargo workspace + **Containerfile from day 2**; CI builds a bootc qcow2 that **boots in QEMU to the egui kiosk** and executes one brokered `fs` call end-to-end (i3mld → AMAN → SIJIL). Record the boot-to-agent GIF. AMAN spec v0.1 in `/spec`. Recruit 10 design partners from Claude Code/OpenClaw communities (OpenClaw's 355k stars and "17% defense rate" scare stories are our funnel).

### Phase 1 — First bootable release, ~4 months (Sep–Dec 2026)
**Ships (v0.1, one dev + early contributors):** qcow2 + **UTM bundle** (Apple-Silicon macOS refugees) + VirtualBox/ISO; **one-line web installer page** that flashes a live USB (Asahi's friction lesson); cloud images (Hetzner/DO/AWS) where **SSH lands in the agent, not bash**; whole-disk "appliance install" from the live USB (Calamares, LUKS2+TPM2 default) — **no dual-boot yet**. Inside: full runtime with **recurring agents in the MVP** (the retention hook), 6 capability types, SIJIL + undo, DHAKIRA + inspector, router (Anthropic + Ollama local + `arabic` via Jais 2), bundled MCP servers (fs, bwrap'd shell, CDP browser), email via user's MCP server forced draft-only; Chromium; Flathub; ar/en RTL shell.
**Printed out-list:** dual-boot, NVIDIA claims, own compositor, Waydroid/Wine, voice, mobile, marketplace, hosted anything, A2A.
**Killer demo (3-min video, ≥9/10 scripted pass rate; Arabic cut same day):** a fresh machine boots i3mlOS from USB; in Arabic, then English: *"i3mel: every night, file the invoices in my inbox into ~/Finance by vendor and draft reminders for overdue ones."* Lid closes. Morning: agent **suspended on consent** — drafts queued behind one approval card; SIJIL scrubbed on screen with exact spend; one misfiled document **un-done in one click**; a learned memory deleted from DHAKIRA; approve → send. Voiceover: *"Undone what can be undone. Approved what can't be."* Then the money shot only an OS can do: `i3ml undo --session` rolls the whole disk state back, and a reboot survives it.

### Phase 2 — Bare-metal daily-driver + revenue, months 4–16 (2027)
**Gate to enter (binding):** ≥1,000 weekly-active booted instances (VM+cloud+metal, opt-in telemetry) with ≥20% week-2 return. If unmet, we fix retention on the appliance; dates slip, gates don't.
- **Reference hardware certification**: Framework Laptop 13 AMD (partner outreach — Linux-first vendor, seeds distros), used-market ThinkPad T14 (Intel), one AMD mini-PC as the "**agent box under the desk**" (zero suspend QA). Public Asahi-style matrix; per-model QA for s2idle/lid/dock, webcam/mic (PipeWire), Bluetooth audio, printing (IPP-everywhere), Arabic/CJK/emoji fonts — the hidden daily-driver killers, QA'd explicitly.
- **Dual-boot v1.1** the safe way: suspend BitLocker, shrink NTFS from Windows, share the ESP, never touch Windows Boot Manager. `-nvidia` image variant (Universal Blue model), best-effort tier.
- Firecracker tier for generated code; A2A endpoint; skill/policy packs v1; signed images; recovery USB.
- **Portability milestones:** all i3mld Linux deps behind traits (audited in CI); **virtio-only VM SKU** declared and tested — the internal driver ABI from here on.
- **Revenue now, not month 24:** 2–3 paid **Gulf air-gapped agent-box pilots** ($30–80k services each — university lab, gov innovation office): the product is *already* an installable, auditable, residency-enforcing appliance with local Jais 2/Fanar — E2B/Daytona proved governed-agent-execution demand; we sell it as a box you own, not a SaaS. Preinstalled mini-PC ("agent box," Home Assistant Green playbook) pilots late Phase 2.

### Phase 3 — Own compositor + fleet, years 2–3 (2028–29)
**Gate:** ≥2,000 weekly-active instances + revenue covering 2 salaries + ≥2 non-founder maintainers.
- **Smithay compositor** (fork niri's plumbing — solo-viable per niri precedent) replaces cage: conversation-first shell; app windows are summoned artifacts; **compositor-owned consent plane — the "Ctrl-Alt-Del for consent," now literally spoof-proof**; constrained GenUI (declarative JSON → trusted native widgets, never agent-authored HTML).
- Fleet management open-core (SSO, RBAC, audit export, signed enterprise images); Arabic dialect voice intent; AMAN spec v1.0 to the Linux Foundation orbit where MCP/A2A live.
- **Hardened-engine stage of the endgame:** minimal kernel config, lockdown mode, module allowlist; risky drivers (Wi-Fi/USB) moved into **driver VMs** (LionsOS/seL4 pattern: unmodified Linux driver in a minimal guest, exported over virtio) on one reference box. Go/no-go: driver-VM overhead <10%.

### Phase 4 — Kernel Independence Endgame, years 3–10 (2029–2036)
The base rates are brutal — Redox: eleven years to "a few devs on real hardware by end-2026"; Fuchsia: hundreds of engineers, shipped a smart display, survived as Starnix (Linux-ABI layer) and microfuchsia (arrives *inside* pKVM VMs). Both teach the same three lessons we hard-code: never swap kernel+product+ecosystem simultaneously; enter via VMs where hardware is virtio and already solved; the Linux *ABI*, not Linux code, is the immovable object. Our unlock is **Asterinas**: a Linux-ABI-compatible Rust framekernel (~210 of ~368 syscalls, Linux-comparable benchmarks, TDX guests, deliberately targeting virtio-first environments) — a kernel our unchanged userspace can survive moving onto.

| Stage | Years | What | Go/no-go gate | Kernel team |
|---|---|---|---|---|
| 1. Invisible Linux | 0–2 | Stock Fedora bootc; portability laws enforced | AMAN spec adopted by ≥3 external tools | 0 |
| 2. Hardened engine | 2–4 | Lockdown, allowlist, traits done, virtio-only VM SKU, driver VMs piloted | Revenue funds platform team; driver-VM overhead <10% | 3–8 |
| 3. Rust ingress | 4–7 | Asterinas-class kernel as **guest** for agent sandboxes/confidential VMs; upstream the syscalls we need; native Rust virtio/NVMe/NIC drivers | Full i3mlOS userspace passes CI on it; ≥1 production workload class stable 6 months | 10–25 |
| 4. The swap | 7–10+ | Server/appliance SKUs boot the Rust kernel natively; Linux demoted to unprivileged driver VM; per-device-class elimination; certified laptop last (System76-style partnership) | Dual-track maintenance cost < single-track; hardware partner signed | 25–60 |

**Honesty clause:** "no Linux code at all" on arbitrary consumer laptops is a 15-year statement; on our own appliance and cloud SKUs it is 7–10. Every stage is independently valuable — the only reason this survives the Redox/Fuchsia base rates.

---

## 6. Repo Structure (day one)

```
i3mlos/
├── crates/            # i3mld / aman / sijil / dhakira / router /
│   #  sandbox (Isolation·Snapshot·Scheduler traits) / i3ml-cli
├── shell/             # egui kiosk app (ar/en, RTL-first); Phase 3: compositor/
├── os/                # Containerfile, image CI (qcow2/ISO/AMI/UTM),
│   #  installer branding, greenboot checks, recovery-usb/
├── servers/           # pinned MCP servers (fs, shell, browser-CDP)
├── spec/              # AMAN broker spec (CC-BY) — the standards play
├── hardware/          # certification matrix + per-model QA scripts
├── docs/              # "Running agents safely" wiki · WHAT-IT-CANT-DO-YET.md
└── LICENSE DCO SECURITY.md ROADMAP.md
```

---

## 7. Adoption Funnel & GTM for a Bootable OS

The judges' fatal objection — "nobody boots an ISO; 1,000 boots is 1,000 abandoned qcow2 files" — is answered with a **ladder, instrumented end-to-end** (anonymous, opt-in):

1. **Watch** — the 3-minute demo; the boot-to-agent GIF in the README.
2. **Meet the shell (10 min)** — qcow2 + **UTM bundle** + OVA; positioned honestly as "meet the shell" (VMs can't show hardware magic).
3. **Feel it on your hardware (30 min)** — live USB with persistence, Ventoy-friendly, "Install" button on the surface.
4. **Give it a machine** — whole-disk appliance install on a spare laptop/mini-PC first (the honest ask); dual-boot v1.1; cloud image for the DevOps crowd (SSH lands in the agent).
5. **Live with it** — certified-hardware purchase links; preinstalled agent-box mini-PC later.

**Retention without a host app:** the Home Assistant model. An i3mlOS box is *always on*; recurring agents run nightly; you check approvals and the SIJIL timeline **from any browser on your existing devices** — clients, not host products. Retention = the box did work while you weren't looking. That is measurable as *return boots/visits*, and it's the metric that gates Phase 2.

**Who daily-drives year one (say it plainly):** developers and AI enthusiasts living in browser + terminal + agents, on certified hardware or a second box. Not Photoshop users, not anticheat gamers.

**Community:** Friday devlog with a "what we said no to" line; the "Running agents safely" wiki (Arch-wiki playbook, SEO even for non-users); public red-team bounty from beta; contributor recruiting on the systems-Rust narrative — "agents are the process model," but useful in month three because Linux is underneath.

**Monetization (appliance-first, Nabu Casa ladder):** free personal forever → **services revenue in Phase 2** (Gulf air-gapped pilots — now more central since the product *is* an appliance OS: sovereign buyers receive an installable image with local Arabic models, residency-by-capability, audit export) → signed enterprise images, fleet/SSO/RBAC, policy packs → agent-box hardware → hosted only with a team ≥3. Rule: revenue before hires; MENA channel pursued via Hub71/HUMAIN/QCRI orbit *after* the global dev segment exists.

---

## 8. Top Risks — including the judges' case against distro-first

| Risk (worst first) | Answer in this plan |
|---|---|
| **Boot friction kills the flywheel** (the judges' kill shot) | Full try-ladder before any install ask; UTM/qcow2 in 10 minutes; retention via always-on recurring agents reachable from existing devices (HA model); cloud SSH-into-agent for devs; measure return-rate, not boots — and the Phase-2 gate binds on it |
| **VM appliance can't touch your real life** | Conceded and reframed: year-one product *is* the isolated agent box (the felt pain E2B/Daytona monetized); daily-driving arrives with Phase-2 certified hardware + Chromium + Flathub, scoped honestly |
| **Distro-maintenance tax eats a solo dev** | bootc/ostree + Fedora's kernel/shim/CVE inheritance + GitHub CI images (Bluefin: tiny team, proven); zero custom plumbing; kiosk not compositor; printed out-lists |
| **Compositor romance / scope death** | Cage+egui until Phase 3, which is gated on 2k WAU + revenue + maintainers; 80% of value ships without it |
| **Revenue 12+ months out, no runway** | Appliance pivot pulls services revenue into Phase 2 (pilots are installs, not SaaS builds); consulting-grade deliverables from artifacts we already ship |
| **Prompt injection / first trust disaster** | Compromise assumed: caps pre-inference, least privilege, taint-tagged web content escalates gates, draft-only outbound, irreversibles trap, Firecracker for generated code, every incident reconstructable from SIJIL; never claim spoof-proof before the compositor makes it true |
| **Platform absorption** (Windows agent workspaces, Cowork) | Be what labs structurally won't ship: a sovereign bootable OS, cross-vendor, user-owned memory/journal, Arabic lane, open AMAN spec so the layer outlives us |
| **Undo overclaim** | Two-class action law, stated in the demo voiceover; btrfs/ostree revert the box, SIJIL scopes claims about the world |
| **Secure Boot mess (2026 cert transition)** | Inherit Fedora's signed shim; MOK for our bits; documented fallback; own shim-review only at Phase 3 scale |
| **Kernel-independence is a fantasy** | It's staged, gated, VM-first, ABI-preserving (Asterinas/Starnix pattern), and every stage pays for itself; the honesty clause is printed |

---

## 9. Success Metrics & Gates (gates bold, binding)

| Phase | Metrics |
|---|---|
| **0** (wk 2) | CI-built image boots QEMU→kiosk→brokered call; GIF public; 200+ waitlist; AMAN spec v0.1 |
| **1** (mo 4–5) | v0.1 images live (qcow2/UTM/ISO/cloud); demo ≥9/10 pass; **≥1,000 weekly-active booted instances, ≥20% wk-2 return → unlocks Phase 2**; ≥30% of actives run a recurring agent; ≥1 pilot LOI; first 100 Arabic-locale instances; 10 external contributors |
| **2** (mo 16) | 3 certified + 10 compatible devices on the public matrix; dual-boot shipped; ≥100 self-reported daily-drivers; 2–3 Gulf pilots ≥$60k total; virtio-only SKU + traits audit green; **≥2,000 WAU + 2 salaries + 2 non-founder maintainers → unlocks Phase 3** |
| **3** (yr 2–3) | Compositor daily-driven by ≥500; consent plane compositor-owned; $20k+ MRR; AMAN spec in ≥3 external tools; driver-VM pilot <10% overhead; 1 MENA ministry/university deployment; **platform team funded → unlocks Stage-3 kernel ingress** |
| **4** (yr 3–10) | Stage gates per §5 table: Asterinas-class kernel passes full-userspace CI → 6-month production workload → SKU swap → per-device-class Linux elimination |

*The verb is still the vision: don't operate your computer — tell it. Now it boots.*
