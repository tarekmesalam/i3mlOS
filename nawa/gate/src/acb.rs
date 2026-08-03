//! The Agent Control Block — what replaces the process.
//!
//! A process is code plus memory. An agent is a **goal**, its **lineage**,
//! the **authority** it may exercise, and the **budget** it may spend. The
//! scheduler keys on deadline and budget, not on timeslices, because an
//! agent's scarce resources are tokens and money as much as CPU.

use nawa_aman::Capability;
use nawa_core::vec_lite::SmallVec;
use nawa_sijil::Label;

pub type AgentId = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Ready to run when its deadline comes up.
    Runnable,
    /// Waiting for a human decision. Costs nothing while parked.
    AwaitingConsent,
    /// Out of budget: suspended, never silently overspending.
    Suspended,
    Finished,
    Killed,
}

/// Hierarchical budget. Delegation carves a child's share out of the parent's
/// remainder, so a subtree can never outspend its root — trust propagation
/// expressed in arithmetic.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub micro_dollars: u64,
    pub spent_micro_dollars: u64,
    /// Wall-clock allowance in microseconds (0 = unbounded for now).
    pub wall_microseconds: u64,
    pub used_microseconds: u64,
}

impl Budget {
    pub const fn new(micro_dollars: u64, wall_microseconds: u64) -> Budget {
        Budget { micro_dollars, spent_micro_dollars: 0, wall_microseconds, used_microseconds: 0 }
    }

    pub fn remaining_micro_dollars(&self) -> u64 {
        self.micro_dollars.saturating_sub(self.spent_micro_dollars)
    }

    pub fn exhausted(&self) -> bool {
        self.remaining_micro_dollars() == 0
            || (self.wall_microseconds != 0 && self.used_microseconds >= self.wall_microseconds)
    }

    /// Carve a child's budget out of what remains. Returns None if the parent
    /// cannot cover it — a child is never funded on credit.
    ///
    /// **Both** resources are carved. Deducting money while conjuring wall
    /// time from nothing would let a subtree outlast its root, which is the
    /// same defect as outspending it.
    pub fn carve(&mut self, micro_dollars: u64, wall_microseconds: u64) -> Option<Budget> {
        if micro_dollars > self.remaining_micro_dollars() {
            return None;
        }
        if self.wall_microseconds != 0 {
            let remaining_wall = self.wall_microseconds.saturating_sub(self.used_microseconds);
            if wall_microseconds == 0 || wall_microseconds > remaining_wall {
                return None;
            }
            self.used_microseconds += wall_microseconds;
        }
        self.spent_micro_dollars += micro_dollars;
        Some(Budget::new(micro_dollars, wall_microseconds))
    }
}

/// Up to this many capabilities per agent in M1 (no heap growth in the
/// scheduler's hot path; the table itself lives in AMAN).
pub const MAX_CAPABILITIES: usize = 8;

pub struct Agent {
    pub id: AgentId,
    pub goal: Label,
    /// Who spawned it. 0 = the kernel itself.
    pub parent: AgentId,
    pub state: State,
    pub budget: Budget,
    /// Deadline in microseconds since boot — the scheduler's sort key.
    pub deadline: u64,
    pub capabilities: SmallVec<Capability, MAX_CAPABILITIES>,
    /// How many gate crossings this agent has made (cheap liveness signal).
    pub crossings: u64,
    /// Set while parked on consent: which approval request it waits for.
    pub awaiting: Option<u64>,
    /// The work itself. M1 runs first-party step functions; Phase 2 replaces
    /// this with WASM instances, which are checkpointable by construction
    /// (master plan §3, the durability law).
    pub step: fn(&mut Context) -> Progress,
    pub cursor: u32,
}

/// What an agent's step function returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Progress {
    /// More to do; reschedule.
    Continue,
    /// Parked waiting for a human. Which request it waits on is kernel
    /// state, set by `request_approval` — never named by the agent.
    AwaitConsent,
    Done,
    Failed,
}

/// The safe view of itself an agent gets while running: its identity, its
/// authority, and a *read-only* look at its budget — nothing else.
///
/// The budget is a snapshot by design: **only the gate may charge a budget.**
/// An agent can see what it has left; it can never adjust what it has spent.
pub struct Context<'a> {
    pub id: AgentId,
    pub goal: &'a Label,
    pub capabilities: &'a SmallVec<Capability, MAX_CAPABILITIES>,
    pub budget: Budget,
    pub cursor: &'a mut u32,
}
