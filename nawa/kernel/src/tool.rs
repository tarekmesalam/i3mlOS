//! Running an agent that is a **WebAssembly module** rather than a kernel
//! function — the change that makes an agent something you can write, ship,
//! and one day checkpoint.
//!
//! The binding rule, and the reason this file is short: **a module's imports
//! are its manifest, and the host resolves them against capabilities the
//! kernel already granted — before the module runs.** A tool asks for
//! `("i3ml", "invoke")`; it does not get to say which capability that is.
//! Anything it imports that we did not authorize is refused at load time, so
//! an unauthorized call cannot even be reached.

use i3ml_wasm::{exec, manifest, Host, Instance, Module, Trap, Value};
use nawa_aman::Capability;
use nawa_gate::{self as gate, AgentId};
use nawa_sijil as sijil;

/// The verbs a tool may import. Each maps to a gate verb; nothing else is
/// resolvable, so the manifest is closed by construction.
const VERBS: [&str; 3] = ["invoke", "journal", "approve"];

/// Binds a running module to one agent's authority.
pub struct GateHost {
    agent: AgentId,
    /// Capability bound to each import index, decided before execution.
    bindings: [Option<Capability>; VERBS.len()],
    /// Which verb each import index refers to.
    verbs: [Option<usize>; 8],
    pub crossings: u32,
    pub refusals: u32,
    /// Notes written this run. Bounded because the journal is a fixed ring:
    /// a tool that could write without limit could push every other entry
    /// out of it — erasing history is as good as forging it.
    notes: u32,
}

/// How many notes one run may add to the journal.
const MAX_NOTES: u32 = 8;

impl Host for GateHost {
    fn call(&mut self, import: usize, arguments: &[Value]) -> exec::Result<Option<Value>> {
        self.crossings += 1;
        let Some(verb) = self.verbs.get(import).copied().flatten() else {
            self.refusals += 1;
            return Err(Trap::HostDenied);
        };
        match VERBS[verb] {
            "invoke" => {
                let Some(capability) = self.bindings[verb] else {
                    self.refusals += 1;
                    return Err(Trap::HostDenied);
                };
                match gate::invoke(self.agent, capability) {
                    Ok(_) => Ok(Some(Value::I32(0))),
                    Err(_) => {
                        self.refusals += 1;
                        Ok(Some(Value::I32(-1)))
                    }
                }
            }
            "journal" => {
                if self.notes >= MAX_NOTES {
                    self.refusals += 1;
                    return Ok(Some(Value::I32(-1)));
                }
                self.notes += 1;
                let detail = arguments.first().map(|value| value.as_i64() as u64).unwrap_or(0);
                // `Noted`, not `Invoked`: a tool may say things, but it may
                // not make the record claim the kernel did something.
                sijil::record(self.agent, sijil::Event::Noted, detail, "wasm:note");
                Ok(Some(Value::I32(0)))
            }
            "approve" => {
                let Some(capability) = self.bindings[verb] else {
                    self.refusals += 1;
                    return Err(Trap::HostDenied);
                };
                match gate::request_approval(self.agent, capability, "requested by a wasm tool") {
                    Ok(id) => Ok(Some(Value::I32(id as i32))),
                    Err(_) => {
                        self.refusals += 1;
                        Ok(Some(Value::I32(-1)))
                    }
                }
            }
            _ => {
                self.refusals += 1;
                Err(Trap::HostDenied)
            }
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    /// The module imports something outside the gate's vocabulary, or
    /// something this agent was not granted. (Bytes that are not a module at
    /// all fail earlier, in the decoder.)
    UnauthorizedImport,
}

/// Load a tool for an agent, resolving its manifest against capabilities the
/// kernel grants it. `grants` pairs a verb name with the capability that verb
/// may exercise; an import with no matching grant fails the load.
pub fn bind<'a>(
    module: &'a Module<'a>,
    agent: AgentId,
    grants: &[(&str, Capability)],
) -> Result<GateHost, LoadError> {
    let mut host = GateHost {
        agent,
        bindings: [None; VERBS.len()],
        verbs: [None; 8],
        crossings: 0,
        refusals: 0,
        notes: 0,
    };

    for requirement in manifest(module) {
        if requirement.module != "i3ml" || requirement.index >= host.verbs.len() {
            return Err(LoadError::UnauthorizedImport);
        }
        let Some(verb) = VERBS.iter().position(|verb| *verb == requirement.name) else {
            return Err(LoadError::UnauthorizedImport);
        };
        // `journal` needs no capability: writing to the record is not an
        // authority an agent can misuse — the kernel writes it either way.
        if VERBS[verb] != "journal" {
            let Some((_, capability)) =
                grants.iter().find(|(name, _)| *name == requirement.name)
            else {
                return Err(LoadError::UnauthorizedImport);
            };
            host.bindings[verb] = Some(*capability);
        }
        host.verbs[requirement.index] = Some(verb);
    }
    Ok(host)
}

/// Run a bound tool's exported entry point under a fuel ceiling.
pub fn run<'a>(
    module: &'a Module<'a>,
    host: &mut GateHost,
    entry: &str,
    fuel: u64,
) -> exec::Result<Option<Value>> {
    let mut instance = Instance::new(module, fuel)?;
    instance.call_export(entry, &[], host)
}
