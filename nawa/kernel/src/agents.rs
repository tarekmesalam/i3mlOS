//! The first agents i3mlOS ever schedules.
//!
//! They are deliberately trivial as *work* — the point is what surrounds the
//! work: every action crosses the gate, every crossing lands in SIJIL, the
//! irreversible one parks for consent instead of proceeding, and a child
//! runs under strictly less authority than its parent.
//!
//! These are first-party in-kernel step functions. Phase 2 replaces them
//! with WASM instances, whose execution state is data by construction and
//! therefore checkpointable (master plan §3, the durability law).

use nawa_aman::{Authority, Denied, ModelClass};
use nawa_gate::{self as gate, Context, Progress};

/// The filing agent: reads, then asks to send. Its steps are a small state
/// machine so its progress is inspectable between gate crossings — the same
/// shape a WASM checkpoint will take.
pub fn file_invoices(context: &mut Context) -> Progress {
    let read = context.capabilities.get(0);
    let send = context.capabilities.get(1);

    match *context.cursor {
        // Step 0: read the inbox. Benign, no consent needed.
        0 => {
            *context.cursor += 1;
            match read.map(|cap| gate::invoke(context.id, cap, 120)) {
                Some(Ok(_)) => Progress::Continue,
                _ => Progress::Failed,
            }
        }
        // Step 1: file them (still reversible — staged, undoable).
        1 => {
            *context.cursor += 1;
            match read.map(|cap| gate::invoke(context.id, cap, 80)) {
                Some(Ok(_)) => Progress::Continue,
                _ => Progress::Failed,
            }
        }
        // Step 2: sending is irreversible. The gate refuses it without a
        // human answer, so the agent parks rather than pushing through.
        2 => {
            *context.cursor += 1;
            match send.map(|cap| gate::invoke(context.id, cap, 0)) {
                Some(Err(Denied::NeedsApproval)) => {
                    let request = gate::request_approval(context.id, "send 3 reminder emails");
                    Progress::AwaitConsent(request)
                }
                Some(Ok(_)) => Progress::Continue,
                _ => Progress::Failed,
            }
        }
        // Step 3: reached only after a human said yes.
        _ => {
            match send.map(|cap| gate::invoke(context.id, cap, 200)) {
                Some(Ok(_)) => Progress::Done,
                _ => Progress::Failed,
            }
        }
    }
}

/// A child agent: summarizes what the parent found. It holds only the
/// narrowed capability it was delegated — it cannot read outside the prefix
/// it was given, and it cannot send at all.
pub fn summarize(context: &mut Context) -> Progress {
    let Some(capability) = context.capabilities.get(0) else {
        return Progress::Failed;
    };
    match gate::invoke(context.id, capability, 40) {
        Ok(_) => Progress::Done,
        Err(_) => Progress::Failed,
    }
}

/// A greedy agent, to prove budgets bind: it never stops asking. It always
/// reports `Continue` — including after being refused — so that the only
/// thing that can stop it is the kernel. It must never be allowed to
/// overspend, and it must never be able to talk its way out of suspension.
pub fn spendthrift(context: &mut Context) -> Progress {
    let Some(capability) = context.capabilities.get(0) else {
        return Progress::Failed;
    };
    match gate::invoke(context.id, capability, 300) {
        Ok(_) | Err(Denied::BudgetExhausted) => Progress::Continue,
        Err(_) => Progress::Failed,
    }
}

/// Authorities the demo hands out. Root authority is minted by the kernel;
/// agents can only ever narrow it from here.
pub fn root_read() -> Authority {
    Authority::Fs { prefix: nawa_sijil::Label::new("/inbox"), write: false }
}

pub fn root_send() -> Authority {
    Authority::Send { draft_only: false }
}

pub fn narrowed_read() -> Authority {
    Authority::Fs { prefix: nawa_sijil::Label::new("/inbox/invoices"), write: false }
}

pub fn model_arabic() -> Authority {
    Authority::Model { class: ModelClass::Arabic }
}
