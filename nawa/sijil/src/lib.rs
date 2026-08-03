//! SIJIL (سجل) — the system journal.
//!
//! The guarantee, stated precisely because it will be marketed precisely:
//! **every gate crossing and authority transfer the kernel actually
//! performed is recorded.** Entries are written by the gate itself, never by
//! an agent, so an agent cannot omit, delay, or rewrite its own history.
//!
//! M1 is the in-RAM ring: fixed capacity, oldest entries drop, sequence
//! numbers never repeat so a reader can always tell that it lost history.
//! The hash-chained on-disk store arrives with virtio-blk in Phase 2.

#![no_std]
#![deny(unsafe_code)]

use nawa_core::apic;
use nawa_core::cell::SpinLock;

/// What happened. Kept as a small enum rather than free text so the journal
/// is queryable by the shell without parsing prose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// An agent was created. `detail` = parent id (0 = born of the kernel).
    Spawned,
    /// Authority was delegated to a child. `detail` = child id.
    Delegated,
    /// A capability was narrowed. `detail` = derived capability index.
    Attenuated,
    /// Root authority was minted for an agent — the only way authority enters
    /// the system. `detail` = capability index.
    Granted,
    /// A capability was exercised. `detail` = capability index.
    Invoked,
    /// A capability check said no. `detail` = capability index (or 0).
    Denied,
    /// An irreversible action parked for human consent. `detail` = request id.
    ApprovalRequested,
    /// A human answered. `detail` = request id.
    Approved,
    Rejected,
    /// Memory (DHAKIRA) was read or written. `detail` = namespace id.
    Remembered,
    /// The agent finished. `detail` = 0 ok, 1 failed, 2 killed.
    Finished,
    /// Budget exhausted; the agent was suspended rather than overspending.
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// Monotonic, never reused — a gap means entries were dropped.
    pub sequence: u64,
    /// Microseconds since boot (APIC/TSC clock).
    pub at: u64,
    pub agent: u32,
    pub event: Event,
    /// Event-specific; see `Event` variants.
    pub detail: u64,
    /// Short human-facing tag (goal, capability name). Truncated, never
    /// allocated — the journal must work when the heap is exhausted.
    pub label: Label,
}

/// A fixed-size, copyable string. The journal never allocates.
#[derive(Clone, Copy)]
pub struct Label {
    bytes: [u8; Self::CAPACITY],
    length: u8,
}

impl Label {
    pub const CAPACITY: usize = 40;

    pub fn new(text: &str) -> Label {
        let mut bytes = [0u8; Self::CAPACITY];
        let mut length = 0;
        // Copy whole UTF-8 characters only, so a truncated label is still
        // valid text (Arabic labels are multi-byte).
        for ch in text.chars() {
            let width = ch.len_utf8();
            if length + width > Self::CAPACITY {
                break;
            }
            ch.encode_utf8(&mut bytes[length..]);
            length += width;
        }
        Label { bytes, length: length as u8 }
    }

    /// Build a label that must represent the text **exactly**, or not at all.
    /// Journal labels may be trimmed for display; anything a security
    /// decision reads (a path, a namespace) must use this instead, because a
    /// shortened scope is a broader scope.
    pub fn exact(text: &str) -> Option<Label> {
        if text.len() > Self::CAPACITY {
            return None;
        }
        Some(Label::new(text))
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length as usize]).unwrap_or("")
    }
}

impl core::fmt::Debug for Label {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Equality is over the text, not the padding bytes — two labels built from
/// the same string are the same label.
impl PartialEq for Label {
    fn eq(&self, other: &Label) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Label {}

const CAPACITY: usize = 512;

struct Ring {
    entries: [Option<Entry>; CAPACITY],
    next_sequence: u64,
    written: usize,
}

static RING: SpinLock<Ring> = SpinLock::new(Ring {
    entries: [None; CAPACITY],
    next_sequence: 1,
    written: 0,
});

/// Append an entry. Only the gate (and the supervisor it serves) may call
/// this — it is not exported to agents in any form.
pub fn record(agent: u32, event: Event, detail: u64, label: &str) -> u64 {
    let entry = Entry {
        sequence: 0,
        at: apic::now_microseconds(),
        agent,
        event,
        detail,
        label: Label::new(label),
    };
    RING.with(|ring| {
        let sequence = ring.next_sequence;
        ring.next_sequence += 1;
        let slot = ((sequence - 1) as usize) % CAPACITY;
        ring.entries[slot] = Some(Entry { sequence, ..entry });
        ring.written += 1;
        sequence
    })
}

/// Total entries ever written (including any the ring has since dropped).
pub fn written() -> usize {
    RING.with(|ring| ring.written)
}

/// Visit retained entries oldest-first. The journal is read under the lock,
/// so `visit` must not itself journal.
pub fn for_each(mut visit: impl FnMut(&Entry)) {
    RING.with(|ring| {
        let total = ring.next_sequence - 1;
        let first = total.saturating_sub(CAPACITY as u64);
        for sequence in (first + 1)..=total {
            let slot = ((sequence - 1) as usize) % CAPACITY;
            if let Some(entry) = &ring.entries[slot] {
                if entry.sequence == sequence {
                    visit(entry);
                }
            }
        }
    });
}

/// Entries belonging to one agent, oldest-first.
pub fn for_agent(agent: u32, mut visit: impl FnMut(&Entry)) {
    for_each(|entry| {
        if entry.agent == agent {
            visit(entry);
        }
    });
}
