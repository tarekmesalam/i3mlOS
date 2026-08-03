//! AMAN (أمان) — the capability broker.
//!
//! The architecture is the enforcement/policy split (master plan §3):
//!
//! * **Enforcement** (this module) is kernel state: capability tables that
//!   agents address by index and can never fabricate, narrow-only
//!   derivation, lineage-tracked revocation. Small, boring, and permanent.
//! * **Policy** (intent → minted grant, approval routing) is expected to be
//!   rewritten repeatedly and is deliberately kept out of the trusted core.
//!
//! Two laws hold from this first version, because retrofitting them is
//! exactly what fails elsewhere:
//!   1. Capabilities are bound **before** inference — never parsed out of a
//!      model's output. `Capability` values only ever come from the table.
//!   2. Delegation can only **attenuate**: a child's authority is a subset of
//!      its parent's, recursively, and so is its budget.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use nawa_core::cell::SpinLock;
use nawa_sijil as sijil;

/// What a capability lets an agent touch. Paths/limits live in the token, so
/// an agent holding `Fs { write: false, .. }` cannot widen it by asking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Authority {
    /// Read (and optionally write) under a path prefix, e.g. `/projects`.
    Fs { prefix: Prefix, write: bool },
    /// Reach the model broker at a given quality class. Never a raw socket.
    Model { class: ModelClass },
    /// Read (and optionally write) a DHAKIRA namespace.
    Memory { namespace: Namespace, write: bool },
    /// Send something outward — mail, message, payment. Always irreversible,
    /// therefore always consent-gated: `draft` alone can be exercised freely.
    Send { draft_only: bool },
    /// Spend, in micro-dollars. Carved from the parent on delegation.
    Spend { micro_dollars: u64 },
}

/// A path prefix, fixed-size so capabilities never allocate.
pub type Prefix = sijil::Label;
/// A DHAKIRA namespace name.
pub type Namespace = sijil::Label;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelClass {
    Fast,
    Private,
    Frontier,
    /// Arabic-first class — routes to Jais/Fanar-class models.
    Arabic,
}

/// How bad is it if this goes wrong? Risk decides consent, not the caller.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Risk {
    /// Reversible and cheap: reads.
    Benign,
    /// Reversible with effort: writes we can stage and roll back.
    Reversible,
    /// Cannot be undone by us: sends, payments, deletions.
    Irreversible,
}

impl Authority {
    pub fn risk(&self) -> Risk {
        match self {
            Authority::Fs { write: false, .. } | Authority::Memory { write: false, .. } => {
                Risk::Benign
            }
            Authority::Model { .. } => Risk::Benign,
            Authority::Fs { write: true, .. } | Authority::Memory { write: true, .. } => {
                Risk::Reversible
            }
            Authority::Send { draft_only: true } => Risk::Reversible,
            Authority::Send { draft_only: false } => Risk::Irreversible,
            Authority::Spend { .. } => Risk::Irreversible,
        }
    }

    /// Is `self` at least as narrow as `parent`? The whole delegation law.
    pub fn is_subset_of(&self, parent: &Authority) -> bool {
        match (self, parent) {
            (
                Authority::Fs { prefix: child, write: child_write },
                Authority::Fs { prefix: parent_prefix, write: parent_write },
            ) => {
                child.as_str().starts_with(parent_prefix.as_str())
                    && (!child_write || *parent_write)
            }
            (Authority::Model { class: a }, Authority::Model { class: b }) => a == b,
            (
                Authority::Memory { namespace: child, write: child_write },
                Authority::Memory { namespace: parent_ns, write: parent_write },
            ) => {
                child.as_str().starts_with(parent_ns.as_str()) && (!child_write || *parent_write)
            }
            (Authority::Send { draft_only: child }, Authority::Send { draft_only: parent }) => {
                *child || !*parent
            }
            (Authority::Spend { micro_dollars: child }, Authority::Spend { micro_dollars: parent }) => {
                child <= parent
            }
            _ => false,
        }
    }
}

/// A handle into the kernel's capability table. An agent can hold, pass, and
/// attenuate one; it cannot construct or forge one — the field is private and
/// there is no public constructor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capability(usize);

impl Capability {
    pub fn index(&self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy)]
struct Slot {
    authority: Authority,
    /// Which agent may exercise it.
    holder: u32,
    /// Where it came from, for lineage revocation (None = minted by kernel).
    parent: Option<usize>,
    revoked: bool,
}

struct Table {
    slots: Vec<Slot>,
}

static TABLE: SpinLock<Option<Table>> = SpinLock::new(None);

/// Bring up the capability table. Called once by the gate during bring-up.
pub fn init_table() {
    TABLE.with(|slot| *slot = Some(Table { slots: Vec::new() }));
}

/// Mint root authority. Kernel-only: this is the one place authority enters
/// the system, and it is never reachable from an agent.
pub fn mint(holder: u32, authority: Authority) -> Capability {
    TABLE.with(|table| {
        let table = table.as_mut().expect("aman::init");
        table.slots.push(Slot { authority, holder, parent: None, revoked: false });
        Capability(table.slots.len() - 1)
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Denied {
    /// The capability was never held by this agent.
    NotHeld,
    /// It was revoked (directly or by an ancestor).
    Revoked,
    /// The request asked for more than the capability grants.
    NotASubset,
    /// The action is irreversible; a human must approve it first.
    NeedsApproval,
    /// The budget is gone. We suspend rather than overspend.
    BudgetExhausted,
}

fn lookup(table: &Table, capability: Capability, agent: u32) -> Result<Slot, Denied> {
    let slot = *table.slots.get(capability.0).ok_or(Denied::NotHeld)?;
    if slot.holder != agent {
        return Err(Denied::NotHeld);
    }
    if slot.revoked {
        return Err(Denied::Revoked);
    }
    Ok(slot)
}

/// Derive a narrower capability. Returns `NotASubset` if the request would
/// widen authority in any dimension — the attenuation law, enforced.
pub fn attenuate(
    agent: u32,
    capability: Capability,
    narrower: Authority,
) -> Result<Capability, Denied> {
    let result = TABLE.with(|table| {
        let table = table.as_mut().expect("aman::init");
        let slot = lookup(table, capability, agent)?;
        if !narrower.is_subset_of(&slot.authority) {
            return Err(Denied::NotASubset);
        }
        table.slots.push(Slot {
            authority: narrower,
            holder: agent,
            parent: Some(capability.0),
            revoked: false,
        });
        Ok(Capability(table.slots.len() - 1))
    });
    match result {
        Ok(derived) => {
            sijil::record(agent, sijil::Event::Attenuated, derived.0 as u64, describe(&narrower));
            Ok(derived)
        }
        Err(denied) => {
            sijil::record(agent, sijil::Event::Denied, capability.0 as u64, describe(&narrower));
            Err(denied)
        }
    }
}

/// Hand a capability to a child agent, narrowing it on the way. This is the
/// only route by which a child gains authority.
pub fn delegate(
    parent_agent: u32,
    child_agent: u32,
    capability: Capability,
    narrower: Authority,
) -> Result<Capability, Denied> {
    let result = TABLE.with(|table| {
        let table = table.as_mut().expect("aman::init");
        let slot = lookup(table, capability, parent_agent)?;
        if !narrower.is_subset_of(&slot.authority) {
            return Err(Denied::NotASubset);
        }
        table.slots.push(Slot {
            authority: narrower,
            holder: child_agent,
            parent: Some(capability.0),
            revoked: false,
        });
        Ok(Capability(table.slots.len() - 1))
    });
    match result {
        Ok(derived) => {
            sijil::record(parent_agent, sijil::Event::Delegated, child_agent as u64, describe(&narrower));
            Ok(derived)
        }
        Err(denied) => {
            sijil::record(parent_agent, sijil::Event::Denied, capability.0 as u64, describe(&narrower));
            Err(denied)
        }
    }
}

/// Check a capability and journal the crossing. `approved` carries a prior
/// human decision; without it, irreversible authority is refused — the
/// consent law, enforced at the gate rather than by convention.
pub fn check(
    agent: u32,
    capability: Capability,
    approved: bool,
) -> Result<Authority, Denied> {
    let result = TABLE.with(|table| {
        let table = table.as_ref().expect("aman::init");
        let slot = lookup(table, capability, agent)?;
        if slot.authority.risk() == Risk::Irreversible && !approved {
            return Err(Denied::NeedsApproval);
        }
        Ok(slot.authority)
    });
    match result {
        Ok(authority) => {
            sijil::record(agent, sijil::Event::Invoked, capability.0 as u64, describe(&authority));
            Ok(authority)
        }
        Err(denied) => {
            let event = if denied == Denied::NeedsApproval {
                sijil::Event::ApprovalRequested
            } else {
                sijil::Event::Denied
            };
            sijil::record(agent, event, capability.0 as u64, denied_label(denied));
            Err(denied)
        }
    }
}

/// Revoke a capability and everything derived from it, transitively.
/// Returns how many were revoked.
pub fn revoke_tree(capability: Capability) -> usize {
    TABLE.with(|table| {
        let table = table.as_mut().expect("aman::init");
        let mut revoked = 0;
        // Slots are append-only and a child always has a higher index than
        // its parent, so one forward pass closes the whole subtree.
        for index in capability.0..table.slots.len() {
            let is_target = index == capability.0;
            let parent_revoked = table.slots[index]
                .parent
                .is_some_and(|parent| table.slots[parent].revoked);
            if (is_target || parent_revoked) && !table.slots[index].revoked {
                table.slots[index].revoked = true;
                revoked += 1;
            }
        }
        revoked
    })
}

pub fn describe(authority: &Authority) -> &'static str {
    match authority {
        Authority::Fs { write: false, .. } => "fs:read",
        Authority::Fs { write: true, .. } => "fs:write",
        Authority::Model { class: ModelClass::Fast } => "model:fast",
        Authority::Model { class: ModelClass::Private } => "model:private",
        Authority::Model { class: ModelClass::Frontier } => "model:frontier",
        Authority::Model { class: ModelClass::Arabic } => "model:arabic",
        Authority::Memory { write: false, .. } => "memory:read",
        Authority::Memory { write: true, .. } => "memory:write",
        Authority::Send { draft_only: true } => "send:draft-only",
        Authority::Send { draft_only: false } => "send",
        Authority::Spend { .. } => "spend",
    }
}

fn denied_label(denied: Denied) -> &'static str {
    match denied {
        Denied::NotHeld => "denied:not-held",
        Denied::Revoked => "denied:revoked",
        Denied::NotASubset => "denied:would-widen",
        Denied::NeedsApproval => "awaiting-consent",
        Denied::BudgetExhausted => "denied:budget",
    }
}
