//! The interpreter. A stack machine over the decoded module.
//!
//! Three properties matter more than speed, and each one is enforced here
//! rather than hoped for:
//!
//! * **Bounded.** Every instruction costs fuel; a module that will not stop
//!   is stopped. There is no such thing as a trusted loop in a kernel.
//! * **Contained.** Memory accesses are checked against the instance's own
//!   linear memory. A tool cannot address anything else — not because we
//!   trust its indices, but because we never use them unchecked.
//! * **Traceable.** Every call into the host is a gate crossing, and the host
//!   decides what it means. The interpreter never performs an effect itself.

use alloc::vec::Vec;

use crate::leb::Reader;
use crate::module::{Function, Module, ValueType};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Trap {
    /// The module asked for something outside its memory.
    OutOfBounds,
    /// Fuel ran out: the tool did not finish in the budget it was given.
    OutOfFuel,
    /// Division by zero, `unreachable`, or a type confusion the decoder let
    /// through.
    Faulted,
    /// The stack grew past its ceiling.
    StackOverflow,
    /// Instruction outside the implemented subset.
    Unsupported,
    /// The host refused the call — a denied capability, most often.
    HostDenied,
    Malformed,
}

pub type Result<T> = core::result::Result<T, Trap>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Value {
    I32(i32),
    I64(i64),
}

impl Value {
    pub fn as_i32(self) -> Result<i32> {
        match self {
            Value::I32(value) => Ok(value),
            Value::I64(value) => Ok(value as i32),
        }
    }

    pub fn as_i64(self) -> i64 {
        match self {
            Value::I32(value) => value as i64,
            Value::I64(value) => value,
        }
    }

    fn zero(value_type: ValueType) -> Value {
        match value_type {
            ValueType::I32 => Value::I32(0),
            ValueType::I64 => Value::I64(0),
        }
    }
}

/// What the interpreter calls when a module invokes an imported function.
/// The host — in i3mlOS, the gate — decides what happens and may refuse.
pub trait Host {
    /// `import` is the index into the module's import list, so the host can
    /// map it back to the manifest entry it authorized.
    fn call(&mut self, import: usize, arguments: &[Value]) -> Result<Option<Value>>;
}

const MAX_STACK: usize = 1024;
const MAX_FRAMES: usize = 64;
const PAGE_SIZE: usize = 64 * 1024;

pub struct Instance<'a> {
    module: &'a Module<'a>,
    memory: Vec<u8>,
    pub fuel: u64,
}

/// One entry of the control stack: where a branch to this label lands.
#[derive(Clone, Copy)]
struct Label {
    /// Byte offset of the matching `end` (or of the loop header, for loops).
    target: usize,
    /// Operand stack height on entry, so a branch can unwind to it.
    height: usize,
    /// Loops branch backwards to their header; blocks branch forwards.
    is_loop: bool,
    /// Does the block produce a value the branch must preserve?
    returns_value: bool,
}

impl<'a> Instance<'a> {
    pub fn new(module: &'a Module<'a>, fuel: u64) -> Result<Instance<'a>> {
        let pages = module.memory_pages.map(|(minimum, _)| minimum).unwrap_or(0) as usize;
        let mut memory = Vec::new();
        memory.resize(pages * PAGE_SIZE, 0);
        for (offset, bytes) in &module.data {
            let start = *offset as usize;
            let end = start.checked_add(bytes.len()).ok_or(Trap::OutOfBounds)?;
            if end > memory.len() {
                return Err(Trap::OutOfBounds);
            }
            memory[start..end].copy_from_slice(bytes);
        }
        Ok(Instance { module, memory, fuel })
    }

    pub fn memory(&self) -> &[u8] {
        &self.memory
    }

    /// Read a NUL-free byte range out of the instance's memory — how a tool
    /// hands text to the host without the host ever trusting a pointer.
    pub fn read(&self, offset: u32, length: u32) -> Result<&[u8]> {
        let start = offset as usize;
        let end = start.checked_add(length as usize).ok_or(Trap::OutOfBounds)?;
        self.memory.get(start..end).ok_or(Trap::OutOfBounds)
    }

    pub fn call_export(
        &mut self,
        name: &str,
        arguments: &[Value],
        host: &mut dyn Host,
    ) -> Result<Option<Value>> {
        let index = self.module.export(name).ok_or(Trap::Malformed)?;
        self.call_function(index, arguments, host, 0)
    }

    fn call_function(
        &mut self,
        index: u32,
        arguments: &[Value],
        host: &mut dyn Host,
        depth: usize,
    ) -> Result<Option<Value>> {
        if depth >= MAX_FRAMES {
            return Err(Trap::StackOverflow);
        }
        let import_count = self.module.imports.len();
        if (index as usize) < import_count {
            return host.call(index as usize, arguments);
        }
        let function = self
            .module
            .functions
            .get(index as usize - import_count)
            .ok_or(Trap::Malformed)?;
        let signature =
            self.module.types.get(function.type_index as usize).ok_or(Trap::Malformed)?;
        if arguments.len() != signature.params.len() {
            return Err(Trap::Faulted);
        }

        let mut locals: Vec<Value> = Vec::with_capacity(arguments.len() + function.locals.len());
        locals.extend_from_slice(arguments);
        for value_type in &function.locals {
            locals.push(Value::zero(*value_type));
        }
        let returns_value = !signature.results.is_empty();
        self.execute(function, &mut locals, host, depth, returns_value)
    }

    fn execute(
        &mut self,
        function: &Function,
        locals: &mut Vec<Value>,
        host: &mut dyn Host,
        depth: usize,
        returns_value: bool,
    ) -> Result<Option<Value>> {
        let body = function.body.clone();
        let mut reader = Reader::new(&self.module.bytes[..body.end]);
        reader.seek(body.start).map_err(|_| Trap::Malformed)?;

        let mut stack: Vec<Value> = Vec::new();
        let mut labels: Vec<Label> = Vec::new();

        loop {
            if self.fuel == 0 {
                return Err(Trap::OutOfFuel);
            }
            self.fuel -= 1;
            if stack.len() > MAX_STACK || labels.len() > MAX_STACK {
                return Err(Trap::StackOverflow);
            }
            if reader.position() >= body.end {
                // Ran off the end without an explicit `end`: malformed.
                return Err(Trap::Malformed);
            }
            let opcode = reader.byte().map_err(|_| Trap::Malformed)?;

            match opcode {
                0x00 => return Err(Trap::Faulted), // unreachable
                0x01 => {}                          // nop

                // --- structured control flow ---
                0x02 | 0x03 | 0x04 => {
                    let block_type = reader.byte().map_err(|_| Trap::Malformed)?;
                    let returns = match block_type {
                        0x40 => false,
                        0x7f | 0x7e => true,
                        _ => return Err(Trap::Unsupported),
                    };
                    let is_loop = opcode == 0x03;
                    let target = if is_loop {
                        reader.position()
                    } else {
                        find_end(self.module.bytes, reader.position(), body.end)?
                    };
                    if opcode == 0x04 {
                        // `if`: consume the condition now.
                        let condition = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                        if condition == 0 {
                            // Jump past the else marker, or past the end.
                            let (else_at, end_at) =
                                find_else(self.module.bytes, reader.position(), body.end)?;
                            match else_at {
                                Some(position) => {
                                    reader.seek(position).map_err(|_| Trap::Malformed)?;
                                }
                                None => {
                                    reader.seek(end_at).map_err(|_| Trap::Malformed)?;
                                    continue;
                                }
                            }
                        }
                    }
                    labels.push(Label {
                        target,
                        height: stack.len(),
                        is_loop,
                        returns_value: returns,
                    });
                }
                0x05 => {
                    // `else` reached by falling out of the taken branch: the
                    // block is finished, so skip the alternative AND close the
                    // label — leaving it open unbalances the control stack and
                    // the function never finds its own `end`.
                    let end_at = find_end(self.module.bytes, reader.position(), body.end)?;
                    reader.seek(end_at).map_err(|_| Trap::Malformed)?;
                    labels.pop().ok_or(Trap::Malformed)?;
                }
                0x0b => {
                    // `end`
                    if labels.pop().is_none() {
                        // Function end.
                        return Ok(if returns_value { stack.pop() } else { None });
                    }
                }
                0x0c | 0x0d => {
                    let depth_index = reader.u32().map_err(|_| Trap::Malformed)? as usize;
                    let take = if opcode == 0x0d {
                        stack.pop().ok_or(Trap::Faulted)?.as_i32()? != 0
                    } else {
                        true
                    };
                    if take {
                        if depth_index >= labels.len() {
                            // Branch out of the function body.
                            return Ok(if returns_value { stack.pop() } else { None });
                        }
                        let label = labels[labels.len() - 1 - depth_index];
                        let carried =
                            if label.returns_value && !label.is_loop { stack.pop() } else { None };
                        stack.truncate(label.height);
                        if let Some(value) = carried {
                            stack.push(value);
                        }
                        labels.truncate(labels.len() - depth_index - if label.is_loop { 0 } else { 1 });
                        reader.seek(label.target).map_err(|_| Trap::Malformed)?;
                    }
                }
                0x0f => return Ok(if returns_value { stack.pop() } else { None }),
                0x10 => {
                    let index = reader.u32().map_err(|_| Trap::Malformed)?;
                    let signature =
                        self.module.function_type(index).ok_or(Trap::Malformed)?.clone();
                    let count = signature.params.len();
                    if stack.len() < count {
                        return Err(Trap::Faulted);
                    }
                    let arguments: Vec<Value> = stack.split_off(stack.len() - count);
                    if let Some(result) =
                        self.call_function(index, &arguments, host, depth + 1)?
                    {
                        stack.push(result);
                    }
                }

                0x1a => {
                    stack.pop().ok_or(Trap::Faulted)?;
                }
                0x1b => {
                    let condition = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let alternative = stack.pop().ok_or(Trap::Faulted)?;
                    let consequent = stack.pop().ok_or(Trap::Faulted)?;
                    stack.push(if condition != 0 { consequent } else { alternative });
                }

                // --- locals ---
                0x20 => {
                    let index = reader.usize().map_err(|_| Trap::Malformed)?;
                    stack.push(*locals.get(index).ok_or(Trap::Faulted)?);
                }
                0x21 | 0x22 => {
                    let index = reader.usize().map_err(|_| Trap::Malformed)?;
                    let value = if opcode == 0x21 {
                        stack.pop().ok_or(Trap::Faulted)?
                    } else {
                        *stack.last().ok_or(Trap::Faulted)?
                    };
                    *locals.get_mut(index).ok_or(Trap::Faulted)? = value;
                }

                // --- memory ---
                0x28 | 0x2d | 0x2f => {
                    let _align = reader.u32().map_err(|_| Trap::Malformed)?;
                    let offset = reader.u32().map_err(|_| Trap::Malformed)?;
                    let base = stack.pop().ok_or(Trap::Faulted)?.as_i32()? as u32;
                    let address = base.checked_add(offset).ok_or(Trap::OutOfBounds)? as usize;
                    let value = match opcode {
                        0x28 => {
                            let bytes = self
                                .memory
                                .get(address..address + 4)
                                .ok_or(Trap::OutOfBounds)?;
                            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                        }
                        0x2d => *self.memory.get(address).ok_or(Trap::OutOfBounds)? as i32,
                        _ => {
                            let bytes = self
                                .memory
                                .get(address..address + 2)
                                .ok_or(Trap::OutOfBounds)?;
                            u16::from_le_bytes([bytes[0], bytes[1]]) as i32
                        }
                    };
                    stack.push(Value::I32(value));
                }
                0x36 | 0x3a => {
                    let _align = reader.u32().map_err(|_| Trap::Malformed)?;
                    let offset = reader.u32().map_err(|_| Trap::Malformed)?;
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let base = stack.pop().ok_or(Trap::Faulted)?.as_i32()? as u32;
                    let address = base.checked_add(offset).ok_or(Trap::OutOfBounds)? as usize;
                    if opcode == 0x36 {
                        let slot =
                            self.memory.get_mut(address..address + 4).ok_or(Trap::OutOfBounds)?;
                        slot.copy_from_slice(&value.to_le_bytes());
                    } else {
                        *self.memory.get_mut(address).ok_or(Trap::OutOfBounds)? = value as u8;
                    }
                }
                0x3f => {
                    reader.byte().map_err(|_| Trap::Malformed)?; // reserved
                    stack.push(Value::I32((self.memory.len() / PAGE_SIZE) as i32));
                }

                // --- constants ---
                0x41 => {
                    let value = reader.signed(32).map_err(|_| Trap::Malformed)?;
                    stack.push(Value::I32(value as i32));
                }
                0x42 => {
                    let value = reader.signed(64).map_err(|_| Trap::Malformed)?;
                    stack.push(Value::I64(value));
                }

                // --- i32 comparison ---
                0x45 => {
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    stack.push(Value::I32((value == 0) as i32));
                }
                0x46..=0x4f => {
                    let right = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let left = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let result = match opcode {
                        0x46 => left == right,
                        0x47 => left != right,
                        0x48 => left < right,
                        0x49 => (left as u32) < (right as u32),
                        0x4a => left > right,
                        0x4b => (left as u32) > (right as u32),
                        0x4c => left <= right,
                        0x4d => (left as u32) <= (right as u32),
                        0x4e => left >= right,
                        _ => (left as u32) >= (right as u32),
                    };
                    stack.push(Value::I32(result as i32));
                }

                // --- i64 comparison ---
                0x50 => {
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    stack.push(Value::I32((value == 0) as i32));
                }
                0x51..=0x5a => {
                    let right = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    let left = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    let result = match opcode {
                        0x51 => left == right,
                        0x52 => left != right,
                        0x53 => left < right,
                        0x54 => (left as u64) < (right as u64),
                        0x55 => left > right,
                        0x56 => (left as u64) > (right as u64),
                        0x57 => left <= right,
                        0x58 => (left as u64) <= (right as u64),
                        0x59 => left >= right,
                        _ => (left as u64) >= (right as u64),
                    };
                    stack.push(Value::I32(result as i32));
                }

                // --- i32 arithmetic ---
                0x6a..=0x76 => {
                    let right = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let left = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    let result = match opcode {
                        0x6a => left.wrapping_add(right),
                        0x6b => left.wrapping_sub(right),
                        0x6c => left.wrapping_mul(right),
                        0x6d => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            left.wrapping_div(right)
                        }
                        0x6e => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            ((left as u32) / (right as u32)) as i32
                        }
                        0x6f => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            left.wrapping_rem(right)
                        }
                        0x70 => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            ((left as u32) % (right as u32)) as i32
                        }
                        0x71 => left & right,
                        0x72 => left | right,
                        0x73 => left ^ right,
                        0x74 => left.wrapping_shl(right as u32),
                        0x75 => left.wrapping_shr(right as u32),
                        _ => ((left as u32).wrapping_shr(right as u32)) as i32,
                    };
                    stack.push(Value::I32(result));
                }

                // --- i64 arithmetic ---
                0x7c..=0x88 => {
                    let right = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    let left = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    let result = match opcode {
                        0x7c => left.wrapping_add(right),
                        0x7d => left.wrapping_sub(right),
                        0x7e => left.wrapping_mul(right),
                        0x7f => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            left.wrapping_div(right)
                        }
                        0x80 => {
                            if right == 0 {
                                return Err(Trap::Faulted);
                            }
                            ((left as u64) / (right as u64)) as i64
                        }
                        0x83 => left & right,
                        0x84 => left | right,
                        0x85 => left ^ right,
                        0x86 => left.wrapping_shl(right as u32),
                        0x87 => left.wrapping_shr(right as u32),
                        0x88 => ((left as u64).wrapping_shr(right as u32)) as i64,
                        _ => return Err(Trap::Unsupported),
                    };
                    stack.push(Value::I64(result));
                }

                // --- conversions ---
                0xa7 => {
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i64();
                    stack.push(Value::I32(value as i32));
                }
                0xac => {
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    stack.push(Value::I64(value as i64));
                }
                0xad => {
                    let value = stack.pop().ok_or(Trap::Faulted)?.as_i32()?;
                    stack.push(Value::I64(value as u32 as i64));
                }

                _ => return Err(Trap::Unsupported),
            }
        }
    }
}

/// Scan forward for the `end` that closes the block starting at `position`.
/// Instruction immediates must be skipped, not merely stepped over, or a
/// constant that happens to encode 0x0b would look like a block terminator.
fn find_end(bytes: &[u8], position: usize, limit: usize) -> Result<usize> {
    let (_, end) = scan(bytes, position, limit, false)?;
    Ok(end)
}

/// Like [`find_end`], but also reports the position just past a top-level
/// `else`, if there is one.
fn find_else(bytes: &[u8], position: usize, limit: usize) -> Result<(Option<usize>, usize)> {
    scan(bytes, position, limit, true)
}

fn scan(
    bytes: &[u8],
    position: usize,
    limit: usize,
    want_else: bool,
) -> Result<(Option<usize>, usize)> {
    let mut reader = Reader::new(&bytes[..limit]);
    reader.seek(position).map_err(|_| Trap::Malformed)?;
    let mut depth = 0usize;
    let mut else_at = None;

    while reader.position() < limit {
        let opcode = reader.byte().map_err(|_| Trap::Malformed)?;
        match opcode {
            0x02 | 0x03 | 0x04 => {
                reader.byte().map_err(|_| Trap::Malformed)?; // block type
                depth += 1;
            }
            0x05 if depth == 0 && want_else && else_at.is_none() => {
                else_at = Some(reader.position());
            }
            0x0b => {
                if depth == 0 {
                    return Ok((else_at, reader.position()));
                }
                depth -= 1;
            }
            // Immediates that must be consumed so their bytes are never
            // mistaken for opcodes.
            0x0c | 0x0d | 0x10 | 0x20 | 0x21 | 0x22 | 0x23 | 0x24 => {
                reader.u32().map_err(|_| Trap::Malformed)?;
            }
            0x28..=0x3e => {
                reader.u32().map_err(|_| Trap::Malformed)?;
                reader.u32().map_err(|_| Trap::Malformed)?;
            }
            0x3f | 0x40 => {
                reader.byte().map_err(|_| Trap::Malformed)?;
            }
            0x41 => {
                reader.signed(32).map_err(|_| Trap::Malformed)?;
            }
            0x42 => {
                reader.signed(64).map_err(|_| Trap::Malformed)?;
            }
            0x0e => return Err(Trap::Unsupported), // br_table
            _ => {}
        }
    }
    Err(Trap::Malformed)
}
