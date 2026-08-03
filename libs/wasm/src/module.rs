//! The module decoder: WebAssembly binary format, the subset i3mlOS runs.
//!
//! The design decision that matters here is not technical. **A module's
//! import list is its permission manifest** — the only way a tool can affect
//! anything is by importing a gate verb, so the set of things it may do is
//! readable from its bytes before a single instruction executes. Nothing else
//! about the format is load-bearing for us.

use alloc::vec::Vec;

use crate::leb::{Error as LebError, Reader};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    NotWasm,
    UnsupportedVersion,
    Malformed,
    Truncated,
    /// A feature outside the subset i3mlOS implements.
    Unsupported,
    TooLarge,
}

impl From<LebError> for Error {
    fn from(error: LebError) -> Error {
        match error {
            LebError::Truncated => Error::Truncated,
            LebError::Overlong | LebError::Malformed => Error::Malformed,
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ValueType {
    I32,
    I64,
}

impl ValueType {
    fn decode(byte: u8) -> Result<ValueType> {
        match byte {
            0x7f => Ok(ValueType::I32),
            0x7e => Ok(ValueType::I64),
            // f32/f64 are deliberately out of the subset: agents move goals
            // and identifiers, not floating point, and every instruction we
            // do not implement is one we cannot get wrong.
            0x7d | 0x7c => Err(Error::Unsupported),
            _ => Err(Error::Malformed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FuncType {
    pub params: Vec<ValueType>,
    pub results: Vec<ValueType>,
}

/// A function the module expects the host to provide. This is the manifest
/// entry: `("i3ml", "invoke")` means the tool intends to invoke capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub module: &'static str,
    pub name: &'static str,
    pub type_index: u32,
}

/// Imports as borrowed from the module bytes (the owned form above is for
/// callers that need 'static names; decoding borrows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRef<'a> {
    pub module: &'a str,
    pub name: &'a str,
    pub type_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export<'a> {
    pub name: &'a str,
    pub function_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub type_index: u32,
    /// Locals beyond the parameters, already expanded.
    pub locals: Vec<ValueType>,
    /// Byte range of the instruction stream within the module.
    pub body: core::ops::Range<usize>,
}

#[derive(Debug)]
pub struct Module<'a> {
    pub bytes: &'a [u8],
    pub types: Vec<FuncType>,
    pub imports: Vec<ImportRef<'a>>,
    pub functions: Vec<Function>,
    pub exports: Vec<Export<'a>>,
    /// Initial and maximum memory, in 64 KiB pages.
    pub memory_pages: Option<(u32, u32)>,
    /// Data segments: (offset, bytes).
    pub data: Vec<(u32, &'a [u8])>,
}

/// Ceilings. A module that needs more than these is refused rather than
/// trusted with kernel memory.
const MAX_TYPES: usize = 128;
const MAX_IMPORTS: usize = 32;
const MAX_FUNCTIONS: usize = 256;
const MAX_EXPORTS: usize = 64;
const MAX_LOCALS: usize = 256;
const MAX_MEMORY_PAGES: u32 = 16; // 1 MiB

impl<'a> Module<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Module<'a>> {
        let mut reader = Reader::new(bytes);
        if reader.slice(4)? != b"\0asm" {
            return Err(Error::NotWasm);
        }
        if reader.slice(4)? != [1, 0, 0, 0] {
            return Err(Error::UnsupportedVersion);
        }

        let mut module = Module {
            bytes,
            types: Vec::new(),
            imports: Vec::new(),
            functions: Vec::new(),
            exports: Vec::new(),
            memory_pages: None,
            data: Vec::new(),
        };
        // Function type indices, collected from the function section and
        // matched against bodies in the code section.
        let mut function_types: Vec<u32> = Vec::new();

        while !reader.at_end() {
            let id = reader.byte()?;
            let size = reader.usize()?;
            let start = reader.position();
            let end = start.checked_add(size).ok_or(Error::Truncated)?;
            if end > bytes.len() {
                return Err(Error::Truncated);
            }

            match id {
                0 => {}  // custom section: names and debug info, ignored
                1 => module.decode_types(&mut reader)?,
                2 => module.decode_imports(&mut reader)?,
                3 => {
                    let count = reader.usize()?;
                    if count > MAX_FUNCTIONS {
                        return Err(Error::TooLarge);
                    }
                    for _ in 0..count {
                        function_types.push(reader.u32()?);
                    }
                }
                5 => module.decode_memory(&mut reader)?,
                7 => module.decode_exports(&mut reader)?,
                10 => module.decode_code(&mut reader, &function_types)?,
                11 => module.decode_data(&mut reader)?,
                // Tables, globals, start, elements: not in the subset.
                4 | 6 | 8 | 9 => return Err(Error::Unsupported),
                _ => return Err(Error::Malformed),
            }
            // Sections are self-delimiting; trust the header, not the parser.
            reader.seek(end)?;
        }
        Ok(module)
    }

    fn decode_types(&mut self, reader: &mut Reader<'a>) -> Result<()> {
        let count = reader.usize()?;
        if count > MAX_TYPES {
            return Err(Error::TooLarge);
        }
        for _ in 0..count {
            if reader.byte()? != 0x60 {
                return Err(Error::Malformed);
            }
            let mut signature = FuncType::default();
            let params = reader.usize()?;
            for _ in 0..params {
                signature.params.push(ValueType::decode(reader.byte()?)?);
            }
            let results = reader.usize()?;
            if results > 1 {
                return Err(Error::Unsupported); // multi-value is post-1.0
            }
            for _ in 0..results {
                signature.results.push(ValueType::decode(reader.byte()?)?);
            }
            self.types.push(signature);
        }
        Ok(())
    }

    fn decode_imports(&mut self, reader: &mut Reader<'a>) -> Result<()> {
        let count = reader.usize()?;
        if count > MAX_IMPORTS {
            return Err(Error::TooLarge);
        }
        for _ in 0..count {
            let module_name = reader.name()?;
            let name = reader.name()?;
            match reader.byte()? {
                0x00 => {
                    let type_index = reader.u32()?;
                    self.imports.push(ImportRef { module: module_name, name, type_index });
                }
                // Importing memory, tables or globals would mean the host
                // hands over mutable state; tools get neither.
                0x01 | 0x02 | 0x03 => return Err(Error::Unsupported),
                _ => return Err(Error::Malformed),
            }
        }
        Ok(())
    }

    fn decode_memory(&mut self, reader: &mut Reader<'a>) -> Result<()> {
        let count = reader.usize()?;
        if count > 1 {
            return Err(Error::Unsupported);
        }
        for _ in 0..count {
            let (minimum, maximum) = match reader.byte()? {
                0x00 => {
                    let minimum = reader.u32()?;
                    (minimum, MAX_MEMORY_PAGES)
                }
                0x01 => (reader.u32()?, reader.u32()?),
                _ => return Err(Error::Malformed),
            };
            if minimum > MAX_MEMORY_PAGES || maximum > MAX_MEMORY_PAGES {
                return Err(Error::TooLarge);
            }
            self.memory_pages = Some((minimum, maximum));
        }
        Ok(())
    }

    fn decode_exports(&mut self, reader: &mut Reader<'a>) -> Result<()> {
        let count = reader.usize()?;
        if count > MAX_EXPORTS {
            return Err(Error::TooLarge);
        }
        for _ in 0..count {
            let name = reader.name()?;
            let kind = reader.byte()?;
            let index = reader.u32()?;
            if kind == 0x00 {
                self.exports.push(Export { name, function_index: index });
            }
            // Exported memory/tables/globals are ignored, not refused: they
            // grant the host nothing it does not already have.
        }
        Ok(())
    }

    fn decode_code(&mut self, reader: &mut Reader<'a>, function_types: &[u32]) -> Result<()> {
        let count = reader.usize()?;
        if count != function_types.len() {
            return Err(Error::Malformed);
        }
        for index in 0..count {
            let size = reader.usize()?;
            let body_start = reader.position();
            let body_end = body_start.checked_add(size).ok_or(Error::Truncated)?;
            if body_end > self.bytes.len() {
                return Err(Error::Truncated);
            }

            let local_groups = reader.usize()?;
            let mut locals = Vec::new();
            for _ in 0..local_groups {
                let repeat = reader.usize()?;
                let value_type = ValueType::decode(reader.byte()?)?;
                if locals.len() + repeat > MAX_LOCALS {
                    return Err(Error::TooLarge);
                }
                for _ in 0..repeat {
                    locals.push(value_type);
                }
            }
            self.functions.push(Function {
                type_index: function_types[index],
                locals,
                body: reader.position()..body_end,
            });
            reader.seek(body_end)?;
        }
        Ok(())
    }

    fn decode_data(&mut self, reader: &mut Reader<'a>) -> Result<()> {
        let count = reader.usize()?;
        for _ in 0..count {
            if reader.u32()? != 0 {
                return Err(Error::Unsupported); // one memory only
            }
            // Offset is a constant expression; we accept `i32.const N end`.
            if reader.byte()? != 0x41 {
                return Err(Error::Unsupported);
            }
            let offset = reader.signed(32)? as u32;
            if reader.byte()? != 0x0b {
                return Err(Error::Malformed);
            }
            let length = reader.usize()?;
            let bytes = reader.slice(length)?;
            self.data.push((offset, bytes));
        }
        Ok(())
    }

    /// The type of function `index`, whether imported or defined. Imported
    /// functions occupy the low indices, as the format requires.
    pub fn function_type(&self, index: u32) -> Option<&FuncType> {
        let index = index as usize;
        if index < self.imports.len() {
            self.types.get(self.imports[index].type_index as usize)
        } else {
            let function = self.functions.get(index - self.imports.len())?;
            self.types.get(function.type_index as usize)
        }
    }

    pub fn export(&self, name: &str) -> Option<u32> {
        self.exports.iter().find(|export| export.name == name).map(|export| export.function_index)
    }
}
