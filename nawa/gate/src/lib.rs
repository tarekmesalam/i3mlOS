//! The **AmanGate** — the one door between an agent and the world, and the
//! supervisor that schedules agents through it.
//!
//! The ABI shape is frozen now even though the transport is still a function
//! call: arguments are plain data, capabilities are table indices, and **no
//! pointers cross the gate**. That is what lets the transport become a
//! `syscall` at M2 (the yard) and later a cross-address-space trap without
//! the ABI itself changing.
//!
//! The eight verbs (master plan §3):
//!   spawn · delegate · attenuate · invoke · approve · journal · remember · await

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod acb;

use alloc::vec::Vec;

use nawa_aman::{self as aman, Authority, Capability, Denied, ModelClass, Risk};
use nawa_core::apic;
use nawa_core::cell::SpinLock;
use nawa_core::vec_lite::SmallVec;
use nawa_sijil::{self as sijil, Label};

pub use acb::{Agent, AgentId, Budget, Context, Progress, State};

/// A pending human decision. Irreversible actions park here rather than
/// proceeding — the consent law made structural.
///
/// The binding is the whole point, and every field carries part of it:
/// an answer authorizes **this agent**, exercising **this capability**,
/// **once**. An approval that is not bound to what it approves is a blanket
/// yes with extra steps.
#[derive(Clone, Copy, Debug)]
pub struct ApprovalRequest {
    pub id: u64,
    pub agent: AgentId,
    /// Exactly what may be exercised if the answer is yes.
    pub capability: Capability,
    /// What the human is shown — derived by the kernel from the authority,
    /// never authored by the agent.
    pub what: Label,
    /// The agent's own words. Untrusted, displayed as such, never the basis
    /// of a decision.
    pub note: Label,
    pub answered: Option<bool>,
    /// Set once the approval has authorized a crossing. One yes, one action.
    pub consumed: bool,
}

struct Supervisor {
    agents: Vec<Agent>,
    approvals: Vec<ApprovalRequest>,
    next_agent_id: AgentId,
    next_approval_id: u64,
}

static SUPERVISOR: SpinLock<Option<Supervisor>> = SpinLock::new(None);

/// Tick count. An atomic, not supervisor state, because the timer handler
/// runs in interrupt context: if it took the supervisor lock it would
/// deadlock against any thread already holding it.
static TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Bring up AMAN's table and the supervisor. Bring-up only.
pub fn init() {
    aman::init_table();
    SUPERVISOR.with(|slot| {
        *slot = Some(Supervisor {
            agents: Vec::new(),
            approvals: Vec::new(),
            next_agent_id: 1,
            next_approval_id: 1,
        })
    });
}

/// Called from the timer interrupt. Lock-free by construction — see [`TICKS`].
pub fn on_tick() {
    TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// The verbs
// ---------------------------------------------------------------------------

/// `spawn(goal, caps, budget)` — create an agent. Kernel-side entry: the
/// root agents of the system are born here.
pub fn spawn(
    goal: &str,
    parent: AgentId,
    budget: Budget,
    deadline_microseconds: u64,
    step: fn(&mut Context) -> Progress,
) -> AgentId {
    let id = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        let id = supervisor.next_agent_id;
        supervisor.next_agent_id += 1;
        supervisor.agents.push(Agent {
            id,
            goal: Label::new(goal),
            parent,
            state: State::Runnable,
            budget,
            deadline: apic::now_microseconds() + deadline_microseconds,
            capabilities: SmallVec::new(),
            crossings: 0,
            awaiting: None,
            step,
            cursor: 0,
        });
        id
    });
    sijil::record(id, sijil::Event::Spawned, parent as u64, goal);
    id
}

/// Grant an agent authority. Kernel-only (root authority enters here);
/// agents move authority to each other with [`delegate`], which can only
/// narrow it.
pub fn grant(agent: AgentId, authority: Authority) -> Option<Capability> {
    let capability = aman::mint(agent, authority);
    let stored = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        supervisor
            .agents
            .iter_mut()
            .find(|candidate| candidate.id == agent)
            .map(|found| found.capabilities.push(capability))
            .unwrap_or(false)
    });
    stored.then_some(capability)
}

/// `delegate(goal, subset, sub_budget)` — spawn a child that holds a strict
/// subset of the parent's authority and a carved slice of its budget.
/// Order matters and is the fix for a real defect: **validate first, commit
/// second.** Carving the budget and spawning the child before checking the
/// attenuation left a funded, runnable orphan behind whenever the check
/// failed — and the parent permanently poorer.
pub fn delegate(
    parent: AgentId,
    goal: &str,
    capability: Capability,
    narrower: Authority,
    micro_dollars: u64,
    wall_microseconds: u64,
    step: fn(&mut Context) -> Progress,
) -> Result<AgentId, Denied> {
    // 1. Would this widen authority? Ask before anything is created.
    aman::can_delegate(parent, capability, &narrower)?;

    // 2. Can the parent actually fund it?
    let carved = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        supervisor
            .agents
            .iter_mut()
            .find(|candidate| candidate.id == parent)
            .and_then(|found| found.budget.carve(micro_dollars, wall_microseconds))
    });
    let Some(child_budget) = carved else {
        sijil::record(parent, sijil::Event::Denied, 0, "denied:budget");
        return Err(Denied::BudgetExhausted);
    };

    // 3. Now commit. Neither step below can fail.
    let child = spawn(goal, parent, child_budget, wall_microseconds.max(1_000), step);
    let derived = aman::delegate(parent, child, capability, narrower)?;
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        if let Some(found) = supervisor.agents.iter_mut().find(|candidate| candidate.id == child) {
            found.capabilities.push(derived);
        }
    });
    Ok(child)
}

/// `attenuate(cap, constraint)` — derive a weaker capability.
pub fn attenuate(
    agent: AgentId,
    capability: Capability,
    narrower: Authority,
) -> Result<Capability, Denied> {
    let derived = aman::attenuate(agent, capability, narrower)?;
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        if let Some(found) = supervisor.agents.iter_mut().find(|candidate| candidate.id == agent) {
            found.capabilities.push(derived);
        }
    });
    Ok(derived)
}

/// `invoke(cap, args)` — the only way to touch anything.
///
/// The cost is **derived from the authority by the kernel**, never supplied
/// by the caller: an agent that could price its own actions could spend a
/// budget without appearing to. Consent is checked against this exact
/// capability, and a yes is consumed when it is used.
pub fn invoke(agent: AgentId, capability: Capability) -> Result<Authority, Denied> {
    // Is there a live, matching, unused approval for exactly this?
    let approved = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_ref().expect("gate::init");
        let Some(found) = supervisor.agents.iter().find(|candidate| candidate.id == agent) else {
            return false;
        };
        let Some(request) = found.awaiting else {
            return false;
        };
        supervisor.approvals.iter().any(|approval| {
            approval.id == request
                && approval.agent == agent
                && approval.capability == capability
                && approval.answered == Some(true)
                && !approval.consumed
        })
    });

    let authority = aman::authorize(agent, capability, approved)?;
    let cost = price_of(&authority);

    let charged = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        let Some(found) = supervisor.agents.iter_mut().find(|candidate| candidate.id == agent)
        else {
            return false;
        };
        found.crossings += 1;
        if found.budget.remaining_micro_dollars() < cost {
            found.state = State::Suspended;
            return false;
        }
        found.budget.spent_micro_dollars += cost;
        // Burn the approval: one human yes authorizes one irreversible act.
        if authority.risk() == Risk::Irreversible {
            if let Some(request) = found.awaiting.take() {
                if let Some(approval) =
                    supervisor.approvals.iter_mut().find(|approval| approval.id == request)
                {
                    approval.consumed = true;
                }
            }
        }
        true
    });
    if !charged {
        sijil::record(agent, sijil::Event::BudgetExhausted, cost, "suspended");
        return Err(Denied::BudgetExhausted);
    }
    // Journaled only once the crossing has actually been paid for and
    // committed — a refused invocation must not read as a completed one.
    sijil::record(agent, sijil::Event::Invoked, capability.index() as u64, aman::describe(&authority));
    Ok(authority)
}

/// The kernel's price list. Crude for now — the point is that the *kernel*
/// sets it. Model classes will price by tokens once the router exists.
fn price_of(authority: &Authority) -> u64 {
    match authority {
        Authority::Fs { write: false, .. } | Authority::Memory { write: false, .. } => 10,
        Authority::Fs { write: true, .. } | Authority::Memory { write: true, .. } => 40,
        Authority::Model { class: ModelClass::Fast } => 50,
        Authority::Model { class: ModelClass::Private } => 80,
        Authority::Model { class: ModelClass::Arabic } => 150,
        Authority::Model { class: ModelClass::Frontier } => 300,
        Authority::Send { draft_only: true } => 20,
        Authority::Send { draft_only: false } => 200,
        // Spending is metered by `spend`, which checks the ceiling itself.
        Authority::Spend { .. } => 0,
    }
}

/// Exercise a `Spend` capability for a specific amount. The capability's own
/// ceiling is enforced here — an attenuated `Spend` that could be exceeded
/// would be decoration.
pub fn spend(agent: AgentId, capability: Capability, micro_dollars: u64) -> Result<(), Denied> {
    let approved = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_ref().expect("gate::init");
        let Some(found) = supervisor.agents.iter().find(|candidate| candidate.id == agent) else {
            return false;
        };
        let Some(request) = found.awaiting else {
            return false;
        };
        supervisor.approvals.iter().any(|approval| {
            approval.id == request
                && approval.agent == agent
                && approval.capability == capability
                && approval.answered == Some(true)
                && !approval.consumed
        })
    });
    let authority = aman::authorize(agent, capability, approved)?;
    let Authority::Spend { micro_dollars: ceiling } = authority else {
        return Err(Denied::NotASubset);
    };
    if micro_dollars > ceiling {
        sijil::record(agent, sijil::Event::Denied, micro_dollars, "denied:over-ceiling");
        return Err(Denied::NotASubset);
    }
    let charged = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        let Some(found) = supervisor.agents.iter_mut().find(|candidate| candidate.id == agent)
        else {
            return false;
        };
        found.crossings += 1;
        if found.budget.remaining_micro_dollars() < micro_dollars {
            found.state = State::Suspended;
            return false;
        }
        found.budget.spent_micro_dollars += micro_dollars;
        if let Some(request) = found.awaiting.take() {
            if let Some(approval) =
                supervisor.approvals.iter_mut().find(|approval| approval.id == request)
            {
                approval.consumed = true;
            }
        }
        true
    });
    if !charged {
        sijil::record(agent, sijil::Event::BudgetExhausted, micro_dollars, "suspended");
        return Err(Denied::BudgetExhausted);
    }
    sijil::record(agent, sijil::Event::Invoked, capability.index() as u64, "spend");
    Ok(())
}

/// `approve(action, risk_class)` — raise a consent request for one specific
/// capability. The agent parks; the answer arrives from the human-facing
/// surface, never from an agent.
///
/// The request id is assigned here and recorded on the agent by the kernel:
/// an agent cannot choose which pending decision it is waiting on, because
/// choosing would mean borrowing someone else's yes.
pub fn request_approval(agent: AgentId, capability: Capability, note: &str) -> Result<u64, Denied> {
    let authority = aman::authority_of(agent, capability)?;
    let what = aman::describe(&authority);
    let id = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        let id = supervisor.next_approval_id;
        supervisor.next_approval_id += 1;
        supervisor.approvals.push(ApprovalRequest {
            id,
            agent,
            capability,
            what: Label::new(what),
            note: Label::new(note),
            answered: None,
            consumed: false,
        });
        if let Some(found) = supervisor.agents.iter_mut().find(|a| a.id == agent) {
            found.awaiting = Some(id);
        }
        id
    });
    sijil::record(agent, sijil::Event::ApprovalRequested, capability.index() as u64, what);
    Ok(id)
}

/// The human answers. In M1 this is called by the kernel's own console
/// stand-in; from Phase 2 it is the only thing the consent surface can do.
///
/// A rejection is final: the agent is killed rather than left to ask again in
/// a loop, and its pending request is cleared so nothing can inherit the
/// decision.
pub fn answer_approval(request: u64, approved: bool) {
    let outcome = SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        let Some(found) = supervisor.approvals.iter_mut().find(|a| a.id == request) else {
            return None;
        };
        if found.answered.is_some() {
            return None; // already decided; answers are not revisited
        }
        found.answered = Some(approved);
        let agent = found.agent;
        let capability = found.capability;
        if let Some(waiting) = supervisor.agents.iter_mut().find(|a| a.id == agent) {
            // Only the agent actually parked on THIS request is affected.
            if waiting.awaiting == Some(request) {
                if approved {
                    waiting.state = State::Runnable;
                    waiting.deadline = apic::now_microseconds();
                } else {
                    waiting.awaiting = None;
                    waiting.state = State::Killed;
                }
            }
        }
        Some((agent, capability))
    });
    let Some((agent, capability)) = outcome else {
        // An answer to nothing is not journaled as a decision — it never
        // authorized anything.
        return;
    };
    sijil::record(
        agent,
        if approved { sijil::Event::Approved } else { sijil::Event::Rejected },
        capability.index() as u64,
        if approved { "human approved" } else { "human rejected" },
    );
}

/// `journal(query)` — an agent may read its own history (and, later, its
/// descendants'). It can never write to it.
pub fn journal_for(agent: AgentId, visit: impl FnMut(&sijil::Entry)) {
    sijil::for_agent(agent, visit);
}

/// `remember(ns, op)` — DHAKIRA access. M1 journals the intent and enforces
/// the capability; the store itself lands with our filesystem in Phase 2.
pub fn remember(
    agent: AgentId,
    capability: Capability,
    namespace: &str,
) -> Result<Authority, Denied> {
    let authority = invoke(agent, capability)?;
    sijil::record(agent, sijil::Event::Remembered, 0, namespace);
    Ok(authority)
}

// ---------------------------------------------------------------------------
// The supervisor loop
// ---------------------------------------------------------------------------

/// Run agents until none is runnable. Earliest deadline first — and an agent
/// that is out of budget is suspended, never quietly overspent.
///
/// **The step runs with no kernel lock held.** Agent code calls back into the
/// gate (that is the entire point), so holding the supervisor lock across it
/// would deadlock the moment an agent invoked anything.
///
/// Returns how many agents reached a terminal state.
pub fn run_until_idle(max_steps: u64) -> u64 {
    let mut completed = 0;
    for _ in 0..max_steps {
        let Some(id) = next_runnable() else { break };

        // Snapshot what the agent may see, then release the lock.
        let snapshot = SUPERVISOR.with(|supervisor| {
            let supervisor = supervisor.as_ref().expect("gate::init");
            supervisor
                .agents
                .iter()
                .find(|candidate| candidate.id == id)
                .map(|found| (found.step, found.goal, found.capabilities, found.budget, found.cursor))
        });
        let Some((step, goal, capabilities, budget, mut cursor)) = snapshot else { continue };

        let started = apic::now_microseconds();
        let mut context = Context { id, goal: &goal, capabilities: &capabilities, budget, cursor: &mut cursor };
        let progress = step(&mut context);
        let elapsed = apic::now_microseconds().saturating_sub(started);

        let mut suspended_by_budget = false;
        let suspended = SUPERVISOR.with(|supervisor| {
            let supervisor = supervisor.as_mut().expect("gate::init");
            let Some(found) = supervisor.agents.iter_mut().find(|candidate| candidate.id == id)
            else {
                return false;
            };
            // Only the cursor comes back from the agent: the budget it saw
            // was a snapshot, and charges were applied by the gate itself.
            found.cursor = cursor;
            found.budget.used_microseconds += elapsed;

            // A suspension imposed by the gate outranks whatever the agent
            // reports. An agent cannot declare itself finished — or keep
            // running — out from under an exhausted budget.
            if found.state == State::Suspended {
                return true;
            }
            match progress {
                Progress::Continue => {
                    if found.budget.exhausted() {
                        found.state = State::Suspended;
                        suspended_by_budget = true;
                        return true;
                    }
                    // Round-robin among ready agents by pushing the deadline
                    // just past the others'.
                    found.deadline = apic::now_microseconds() + 1;
                }
                Progress::AwaitConsent => {
                    // `awaiting` was set by request_approval; the agent only
                    // reports that it is parked, never on what.
                    found.state = State::AwaitingConsent;
                }
                Progress::Done => found.state = State::Finished,
                Progress::Failed => found.state = State::Killed,
            }
            false
        });

        if suspended {
            // A terminal state is never silent: an agent stopped by its
            // budget leaves the same trail as one that finished.
            if suspended_by_budget {
                sijil::record(id, sijil::Event::BudgetExhausted, 0, "suspended at step end");
            }
            continue;
        }
        match progress {
            Progress::Done => {
                sijil::record(id, sijil::Event::Finished, 0, "ok");
                completed += 1;
            }
            Progress::Failed => {
                sijil::record(id, sijil::Event::Finished, 1, "failed");
                completed += 1;
            }
            _ => {}
        }
    }
    completed
}

fn next_runnable() -> Option<AgentId> {
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_mut().expect("gate::init");
        supervisor
            .agents
            .iter()
            .filter(|agent| agent.state == State::Runnable && !agent.budget.exhausted())
            .min_by_key(|agent| agent.deadline)
            .map(|agent| agent.id)
    })
}

/// Snapshot for reporting — the shell's `i3ml ps`, in kernel form.
#[derive(Clone, Copy)]
pub struct AgentSummary {
    pub id: AgentId,
    pub parent: AgentId,
    pub goal: Label,
    pub state: State,
    pub crossings: u64,
    pub spent_micro_dollars: u64,
    pub budget_micro_dollars: u64,
    pub capabilities: usize,
}

pub fn summaries() -> Vec<AgentSummary> {
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_ref().expect("gate::init");
        supervisor
            .agents
            .iter()
            .map(|agent| AgentSummary {
                id: agent.id,
                parent: agent.parent,
                goal: agent.goal,
                state: agent.state,
                crossings: agent.crossings,
                spent_micro_dollars: agent.budget.spent_micro_dollars,
                budget_micro_dollars: agent.budget.micro_dollars,
                capabilities: agent.capabilities.len(),
            })
            .collect()
    })
}

pub fn pending_approvals() -> Vec<ApprovalRequest> {
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_ref().expect("gate::init");
        supervisor.approvals.iter().filter(|a| a.answered.is_none()).copied().collect()
    })
}

/// Look up one of an agent's capabilities by slot — for the kernel's own
/// tests and, later, the shell's inspector.
pub fn capability_of(agent: AgentId, slot: usize) -> Option<Capability> {
    SUPERVISOR.with(|supervisor| {
        let supervisor = supervisor.as_ref().expect("gate::init");
        supervisor
            .agents
            .iter()
            .find(|candidate| candidate.id == agent)
            .and_then(|found| found.capabilities.get(slot))
    })
}

pub fn ticks_observed() -> u64 {
    TICKS.load(core::sync::atomic::Ordering::Relaxed)
}
