# devlog 003 — the first agent | أول agent

*Draft — M1 complete.*

An operating system whose schedulable unit is an agent has to prove that
claim in its scheduler, not its README. Here is the serial log from a QEMU
boot, unedited:

```
nawa: apic timer armed — the kernel has a heartbeat
clock: 1003 MHz TSC, 2 ticks in the first moments
aman: delegated to agent 2 under a narrower prefix
aman: refused to widen fs:read into model:arabic — correct
sched: 1 agents reached a terminal state
consent: agent 1 parked awaiting approval — "send 3 reminder emails"
sched: 1 more agents finished after consent
agent 1: "file invoices, draft reminders" parent=0 state=Finished crossings=3 spent=600/1000 µ$ caps=2
agent 2: "summarize the invoices"        parent=1 state=Finished crossings=1 spent=40/200 µ$ caps=1
agent 3: "spend everything"              parent=0 state=Suspended crossings=2 spent=300/500 µ$ caps=1
  ^ suspended by its budget — never overspent
```

Four laws, each now enforced by code that runs on every commit:

**Agents are the schedulable unit.** The Agent Control Block replaces the
process: a goal, its lineage, the authority it may exercise, the budget it may
spend. The scheduler keys on deadline and budget, because an agent's scarce
resources are tokens and money as much as CPU.

**Authority only narrows.** Agent 1 delegated `/inbox` to agent 2 as
`/inbox/invoices`. When it then asked to turn a filesystem capability into a
model capability, AMAN refused — attenuation is a subset test, not a
convention.

**The irreversible waits for a human.** Agent 1's send parked at the gate. Not
because the agent was polite: `aman::check` refuses irreversible authority
without a recorded approval, so the only way forward was consent.

**Budgets bind.** Agent 3 was written to never stop asking — it reports
"continue" even after being refused. It stopped anyway, because a suspension
imposed by the gate outranks anything an agent says about itself.

Underneath: SIJIL recorded all 17 events, written by the gate rather than
self-reported by agents, and the machine now has a heartbeat — x2APIC with
TSC-deadline where the CPU offers it, APIC periodic mode everywhere else,
both calibrated against the 8254 in a single pass.

Two real bugs surfaced during the build, both worth naming:

- **A deadlock by design.** The supervisor ran an agent's step *while holding
  the supervisor lock* — and agent code calls back into the gate, which takes
  the same lock. The fix reshaped the API for the better: the agent now gets a
  read-only budget snapshot, because **only the gate may charge a budget.**
- **A lock in interrupt context.** The timer handler took that same lock at
  1 kHz. It is now atomic-only. A spinlock shared between task and interrupt
  context is a hang waiting for a schedule.

Next (M2, the gate to Phase 2): the **yard** — ring 3, one shared user address
space, `syscall` into the gate, and the WASM interpreter as its first
resident. That is when "capabilities cannot be forged" and "the journal cannot
be falsified" stop being properties of our type system and become properties
of the hardware.
