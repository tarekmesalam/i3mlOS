//! The model broker — reaching an AI model as an act the kernel mediates.
//!
//! This is the piece that makes the architecture more than a metaphor.
//! Everywhere else, "the AI" is a library a program links and a key the
//! program holds; whatever it sends, and whatever it costs, is between the
//! program and the vendor. Here it is a **device behind the gate**:
//!
//! * an agent must hold a `Model` capability of the right class,
//! * the tokens it spends come out of a budget the kernel enforces,
//! * every request and every answer is a line in SIJIL the agent cannot write,
//! * and a class marked private is *routed*, not trusted — the broker refuses
//!   to send it anywhere off the machine.
//!
//! The wire protocol is deliberately tiny — one text line and a body — so the
//! kernel needs no JSON and the host end can speak to whatever API it likes.

use core::fmt::Write;

use nawa_aman::{Authority, Denied, ModelClass};
use nawa_gate::{self as gate, AgentId};
use nawa_sijil as sijil;
use nawa_virtio::console::Console;

/// The most a single answer may be. Anything longer is truncated by the
/// broker rather than buffered: brokering is the kernel's job, storage is not.
pub const MAX_ANSWER: usize = 4096;

pub struct Broker {
    console: Console,
    /// Answers served without leaving the machine, because the class said so.
    pub kept_local: u32,
    pub calls: u32,
}

pub struct Answer {
    pub bytes: [u8; MAX_ANSWER],
    pub length: usize,
    pub tokens: u64,
}

impl Answer {
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
    }
}

fn class_name(class: ModelClass) -> &'static str {
    match class {
        ModelClass::Fast => "fast",
        ModelClass::Private => "private",
        ModelClass::Frontier => "frontier",
        ModelClass::Arabic => "arabic",
    }
}

impl Broker {
    pub fn open() -> Option<Broker> {
        Some(Broker { console: Console::open()?, kept_local: 0, calls: 0 })
    }

    /// Ask a model something, on behalf of an agent, using one of its
    /// capabilities. The capability decides the class; the class decides
    /// whether the question may leave the machine at all.
    pub fn ask(
        &mut self,
        agent: AgentId,
        capability: nawa_aman::Capability,
        prompt: &str,
    ) -> Result<Answer, Denied> {
        // The gate first: authority, budget, journal. If this refuses, no
        // request is formed, so there is nothing to leak.
        let authority = gate::invoke(agent, capability)?;
        let Authority::Model { class } = authority else {
            return Err(Denied::NotASubset);
        };
        self.calls += 1;

        if matches!(class, ModelClass::Private) {
            // Residency is enforced by *not routing*, not by asking nicely.
            // A private class has no path off this machine, so there is no
            // configuration mistake that can send it.
            self.kept_local += 1;
            sijil::record(agent, sijil::Event::Invoked, 0, "model:private kept local");
            return Ok(local_answer(prompt));
        }

        let mut request = [0u8; 512];
        let header = {
            let mut writer = SliceWriter { bytes: &mut request, length: 0 };
            let _ = write!(
                writer,
                "ASK {} {} {}\n",
                class_name(class),
                agent,
                prompt.len().min(2048)
            );
            writer.length
        };
        if !self.console.send(&request[..header]) {
            sijil::record(agent, sijil::Event::Denied, 0, "model:channel refused");
            return Err(Denied::NotHeld);
        }
        if !self.console.send(prompt.as_bytes()) {
            sijil::record(agent, sijil::Event::Denied, 0, "model:channel refused");
            return Err(Denied::NotHeld);
        }

        let mut buffer = [0u8; MAX_ANSWER];
        let Some(length) = self.console.receive(&mut buffer, 400_000_000) else {
            sijil::record(agent, sijil::Event::Denied, 0, "model:no answer");
            return Err(Denied::NotHeld);
        };

        // Reply format: "OK <tokens>\n<body>" or "ERR <reason>".
        let text = core::str::from_utf8(&buffer[..length]).unwrap_or("");
        let Some((head, body)) = text.split_once('\n') else {
            sijil::record(agent, sijil::Event::Denied, 0, "model:malformed reply");
            return Err(Denied::NotHeld);
        };
        let mut parts = head.split(' ');
        if parts.next() != Some("OK") {
            sijil::record(agent, sijil::Event::Denied, 0, "model:refused by host");
            return Err(Denied::NotHeld);
        }
        let tokens: u64 = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);

        let mut answer = Answer { bytes: [0; MAX_ANSWER], length: 0, tokens };
        let body = body.as_bytes();
        answer.length = body.len().min(MAX_ANSWER);
        answer.bytes[..answer.length].copy_from_slice(&body[..answer.length]);

        // What it cost is recorded as the model reported it, not as the agent
        // claims — the agent never sees this number before it is charged.
        gate::charge(agent, tokens);
        sijil::record(agent, sijil::Event::Invoked, tokens, "model:answered");
        Ok(answer)
    }
}

/// The private class, served without a network. Deliberately unhelpful: an
/// honest "no model runs on this machine yet" beats a plausible sentence a
/// user might believe. Local inference is a Phase 4 milestone.
fn local_answer(prompt: &str) -> Answer {
    let mut answer = Answer { bytes: [0; MAX_ANSWER], length: 0, tokens: 0 };
    let message = b"[private class: no local model on this machine yet, and nothing left it]";
    answer.bytes[..message.len()].copy_from_slice(message);
    answer.length = message.len();
    answer.tokens = (prompt.len() / 4) as u64;
    answer
}

/// `write!` into a fixed buffer, because the request line must not allocate:
/// a broker that needs a heap cannot report that the heap is gone.
struct SliceWriter<'a> {
    bytes: &'a mut [u8],
    length: usize,
}

impl Write for SliceWriter<'_> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let take = text.len().min(self.bytes.len() - self.length);
        self.bytes[self.length..self.length + take].copy_from_slice(&text.as_bytes()[..take]);
        self.length += take;
        Ok(())
    }
}
