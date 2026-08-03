//! A WebAssembly interpreter, written from scratch for i3mlOS.
//!
//! Why an interpreter and not a JIT: a tool is IO-bound — it waits on models,
//! files, and humans — so 10–50× slower execution costs nothing that matters,
//! while a JIT would mean generating executable memory inside the kernel's
//! trust boundary. The purity charter puts the interpreter in Tier 1; this is
//! the module that makes agents *portable and checkpointable* rather than
//! kernel functions.
//!
//! **The import list is the permission manifest.** A module can only affect
//! the world by calling something the host provided, so what a tool may do is
//! readable from its bytes before it runs. [`manifest`] returns exactly that.
//!
//! Host-testable first, kernel-resident second: this crate is `no_std` but
//! its tests run on the development machine (`cargo test -p i3ml-wasm`).

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

extern crate alloc;

pub mod exec;
pub mod leb;
pub mod module;

pub use exec::{Host, Instance, Trap, Value};
pub use module::{Module, ValueType};

use alloc::vec::Vec;

/// One entry of a module's permission manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement<'a> {
    /// Import index, as passed to [`Host::call`].
    pub index: usize,
    pub module: &'a str,
    pub name: &'a str,
}

/// What this module is asking to be allowed to do, in the order the host will
/// see the calls. Reading it is how a capability is bound to a tool *before*
/// the tool runs — never after, and never on the tool's say-so.
pub fn manifest<'a>(module: &'a Module<'a>) -> Vec<Requirement<'a>> {
    module
        .imports
        .iter()
        .enumerate()
        .map(|(index, import)| Requirement { index, module: import.module, name: import.name })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// A tiny WebAssembly assembler, so tests state intent rather than bytes.
    mod build {
        use alloc::vec::Vec;

        pub fn unsigned(mut value: u64) -> Vec<u8> {
            let mut out = Vec::new();
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                out.push(byte);
                if value == 0 {
                    return out;
                }
            }
        }

        pub fn signed(mut value: i64) -> Vec<u8> {
            let mut out = Vec::new();
            loop {
                let byte = (value & 0x7f) as u8;
                value >>= 7;
                let sign_bit = byte & 0x40 != 0;
                if (value == 0 && !sign_bit) || (value == -1 && sign_bit) {
                    out.push(byte);
                    return out;
                }
                out.push(byte | 0x80);
            }
        }

        pub fn section(id: u8, payload: Vec<u8>) -> Vec<u8> {
            let mut out = vec![id];
            out.extend(unsigned(payload.len() as u64));
            out.extend(payload);
            out
        }

        pub fn vector(items: Vec<Vec<u8>>) -> Vec<u8> {
            let mut out = unsigned(items.len() as u64);
            for item in items {
                out.extend(item);
            }
            out
        }

        pub fn name(text: &str) -> Vec<u8> {
            let mut out = unsigned(text.len() as u64);
            out.extend(text.as_bytes());
            out
        }
    }

    use build::*;

    struct Module {
        types: Vec<Vec<u8>>,
        imports: Vec<Vec<u8>>,
        functions: Vec<u32>,
        bodies: Vec<Vec<u8>>,
        exports: Vec<Vec<u8>>,
        memory: Option<u32>,
        data: Vec<(u32, Vec<u8>)>,
    }

    impl Module {
        fn new() -> Module {
            Module {
                types: Vec::new(),
                imports: Vec::new(),
                functions: Vec::new(),
                bodies: Vec::new(),
                exports: Vec::new(),
                memory: None,
                data: Vec::new(),
            }
        }

        /// 0x7f = i32, 0x7e = i64
        fn add_type(&mut self, params: &[u8], results: &[u8]) -> u32 {
            let mut out = vec![0x60];
            out.extend(unsigned(params.len() as u64));
            out.extend_from_slice(params);
            out.extend(unsigned(results.len() as u64));
            out.extend_from_slice(results);
            self.types.push(out);
            self.types.len() as u32 - 1
        }

        fn add_import(&mut self, module: &str, field: &str, type_index: u32) {
            let mut out = name(module);
            out.extend(name(field));
            out.push(0x00);
            out.extend(unsigned(type_index as u64));
            self.imports.push(out);
        }

        fn add_function(&mut self, type_index: u32, locals: &[(u32, u8)], code: Vec<u8>) -> u32 {
            self.functions.push(type_index);
            let mut body = unsigned(locals.len() as u64);
            for (count, value_type) in locals {
                body.extend(unsigned(*count as u64));
                body.push(*value_type);
            }
            body.extend(code);
            body.push(0x0b); // end
            let mut sized = unsigned(body.len() as u64);
            sized.extend(body);
            self.bodies.push(sized);
            self.imports.len() as u32 + self.functions.len() as u32 - 1
        }

        fn export(&mut self, field: &str, function_index: u32) {
            let mut out = name(field);
            out.push(0x00);
            out.extend(unsigned(function_index as u64));
            self.exports.push(out);
        }

        fn finish(self) -> Vec<u8> {
            let mut out = b"\0asm\x01\0\0\0".to_vec();
            if !self.types.is_empty() {
                out.extend(section(1, vector(self.types)));
            }
            if !self.imports.is_empty() {
                out.extend(section(2, vector(self.imports)));
            }
            if !self.functions.is_empty() {
                let entries: Vec<Vec<u8>> =
                    self.functions.iter().map(|index| unsigned(*index as u64)).collect();
                out.extend(section(3, vector(entries)));
            }
            if let Some(pages) = self.memory {
                let mut payload = unsigned(1);
                payload.push(0x00);
                payload.extend(unsigned(pages as u64));
                out.extend(section(5, payload));
            }
            if !self.exports.is_empty() {
                out.extend(section(7, vector(self.exports)));
            }
            if !self.bodies.is_empty() {
                out.extend(section(10, vector(self.bodies)));
            }
            if !self.data.is_empty() {
                let entries: Vec<Vec<u8>> = self
                    .data
                    .iter()
                    .map(|(offset, bytes)| {
                        let mut entry = unsigned(0);
                        entry.push(0x41);
                        entry.extend(signed(*offset as i64));
                        entry.push(0x0b);
                        entry.extend(unsigned(bytes.len() as u64));
                        entry.extend(bytes.iter().copied());
                        entry
                    })
                    .collect();
                out.extend(section(11, vector(entries)));
            }
            out
        }
    }

    struct Recorder {
        calls: Vec<(usize, Vec<Value>)>,
        answer: Option<Value>,
        deny: bool,
    }

    impl Host for Recorder {
        fn call(&mut self, import: usize, arguments: &[Value]) -> exec::Result<Option<Value>> {
            self.calls.push((import, arguments.to_vec()));
            if self.deny {
                return Err(Trap::HostDenied);
            }
            Ok(self.answer)
        }
    }

    fn recorder() -> Recorder {
        Recorder { calls: Vec::new(), answer: None, deny: false }
    }

    fn run(bytes: &[u8], name: &str, arguments: &[Value], host: &mut dyn Host) -> exec::Result<Option<Value>> {
        let module = super::Module::decode(bytes).expect("decodes");
        let mut instance = Instance::new(&module, 100_000)?;
        instance.call_export(name, arguments, host)
    }

    #[test]
    fn arithmetic_and_locals() {
        let mut builder = Module::new();
        let signature = builder.add_type(&[0x7f, 0x7f], &[0x7f]);
        // (a + b) * 2
        let mut code = vec![0x20, 0x00, 0x20, 0x01, 0x6a];
        code.extend([0x41, 0x02, 0x6c]);
        let index = builder.add_function(signature, &[], code);
        builder.export("compute", index);
        let bytes = builder.finish();

        let result = run(&bytes, "compute", &[Value::I32(20), Value::I32(1)], &mut recorder());
        assert_eq!(result, Ok(Some(Value::I32(42))));
    }

    #[test]
    fn loops_terminate_and_count() {
        let mut builder = Module::new();
        let signature = builder.add_type(&[0x7f], &[0x7f]);
        // sum = 0; while (n > 0) { sum += n; n -= 1 } return sum
        //
        // The block/loop pair is the standard idiom: `br 0` repeats the loop,
        // `br 1` leaves the block — which is what "break" means. (Without the
        // block, depth 1 is the function body, and the break becomes a
        // return.)
        let code = vec![
            0x41, 0x00, 0x21, 0x01, // local 1 = 0 (sum)
            0x02, 0x40, // block
            0x03, 0x40, // loop
            0x20, 0x00, 0x45, 0x0d, 0x01, // if n == 0, br 1 (leave the block)
            0x20, 0x01, 0x20, 0x00, 0x6a, 0x21, 0x01, // sum += n
            0x20, 0x00, 0x41, 0x01, 0x6b, 0x21, 0x00, // n -= 1
            0x0c, 0x00, // br 0 (repeat)
            0x0b, // end loop
            0x0b, // end block
            0x20, 0x01, // sum
        ];
        let index = builder.add_function(signature, &[(1, 0x7f)], code);
        builder.export("sum", index);
        let bytes = builder.finish();

        assert_eq!(run(&bytes, "sum", &[Value::I32(10)], &mut recorder()), Ok(Some(Value::I32(55))));
    }

    #[test]
    fn an_endless_loop_runs_out_of_fuel_rather_than_hanging() {
        let mut builder = Module::new();
        let signature = builder.add_type(&[], &[]);
        let code = vec![0x03, 0x40, 0x0c, 0x00, 0x0b];
        let index = builder.add_function(signature, &[], code);
        builder.export("spin", index);
        let bytes = builder.finish();

        let module = super::Module::decode(&bytes).unwrap();
        let mut instance = Instance::new(&module, 500).unwrap();
        assert_eq!(instance.call_export("spin", &[], &mut recorder()), Err(Trap::OutOfFuel));
    }

    #[test]
    fn memory_beyond_the_instance_traps() {
        let mut builder = Module::new();
        builder.memory = Some(1);
        let signature = builder.add_type(&[0x7f], &[0x7f]);
        // load i32 at the given address
        let code = vec![0x20, 0x00, 0x28, 0x02, 0x00];
        let index = builder.add_function(signature, &[], code);
        builder.export("load", index);
        let bytes = builder.finish();

        assert_eq!(run(&bytes, "load", &[Value::I32(0)], &mut recorder()), Ok(Some(Value::I32(0))));
        // One page is 65536 bytes; reading at the very end must fail.
        assert_eq!(
            run(&bytes, "load", &[Value::I32(65_534)], &mut recorder()),
            Err(Trap::OutOfBounds)
        );
        assert_eq!(
            run(&bytes, "load", &[Value::I32(-1)], &mut recorder()),
            Err(Trap::OutOfBounds)
        );
    }

    #[test]
    fn imports_are_the_manifest_and_calls_reach_the_host() {
        let mut builder = Module::new();
        let host_signature = builder.add_type(&[0x7f], &[0x7f]);
        builder.add_import("i3ml", "invoke", host_signature);
        builder.add_import("i3ml", "journal", host_signature);
        let own = builder.add_type(&[], &[0x7f]);
        // invoke(7)
        let code = vec![0x41, 0x07, 0x10, 0x00];
        let index = builder.add_function(own, &[], code);
        builder.export("run", index);
        let bytes = builder.finish();

        let module = super::Module::decode(&bytes).unwrap();
        let requirements = manifest(&module);
        assert_eq!(requirements.len(), 2);
        assert_eq!((requirements[0].module, requirements[0].name), ("i3ml", "invoke"));
        assert_eq!((requirements[1].module, requirements[1].name), ("i3ml", "journal"));

        let mut host = Recorder { calls: Vec::new(), answer: Some(Value::I32(1)), deny: false };
        let mut instance = Instance::new(&module, 10_000).unwrap();
        assert_eq!(instance.call_export("run", &[], &mut host), Ok(Some(Value::I32(1))));
        assert_eq!(host.calls, vec![(0, vec![Value::I32(7)])]);
    }

    #[test]
    fn a_denied_capability_stops_the_tool() {
        let mut builder = Module::new();
        let host_signature = builder.add_type(&[0x7f], &[0x7f]);
        builder.add_import("i3ml", "invoke", host_signature);
        let own = builder.add_type(&[], &[0x7f]);
        let code = vec![0x41, 0x07, 0x10, 0x00];
        let index = builder.add_function(own, &[], code);
        builder.export("run", index);
        let bytes = builder.finish();

        let mut host = Recorder { calls: Vec::new(), answer: None, deny: true };
        assert_eq!(run(&bytes, "run", &[], &mut host), Err(Trap::HostDenied));
    }

    #[test]
    fn branches_and_conditionals() {
        let mut builder = Module::new();
        let signature = builder.add_type(&[0x7f], &[0x7f]);
        // if (n) { 10 } else { 20 }
        let code = vec![0x20, 0x00, 0x04, 0x7f, 0x41, 0x0a, 0x05, 0x41, 0x14, 0x0b];
        let index = builder.add_function(signature, &[], code);
        builder.export("pick", index);
        let bytes = builder.finish();

        assert_eq!(run(&bytes, "pick", &[Value::I32(1)], &mut recorder()), Ok(Some(Value::I32(10))));
        assert_eq!(run(&bytes, "pick", &[Value::I32(0)], &mut recorder()), Ok(Some(Value::I32(20))));
    }

    #[test]
    fn division_by_zero_traps_rather_than_faulting_the_kernel() {
        let mut builder = Module::new();
        let signature = builder.add_type(&[0x7f, 0x7f], &[0x7f]);
        let code = vec![0x20, 0x00, 0x20, 0x01, 0x6e];
        let index = builder.add_function(signature, &[], code);
        builder.export("divide", index);
        let bytes = builder.finish();

        assert_eq!(
            run(&bytes, "divide", &[Value::I32(1), Value::I32(0)], &mut recorder()),
            Err(Trap::Faulted)
        );
    }

    #[test]
    fn rubbish_is_refused_not_executed() {
        assert_eq!(super::Module::decode(b"not wasm at all").err(), Some(module::Error::NotWasm));
        assert_eq!(
            super::Module::decode(b"\0asm\x09\0\0\0").err(),
            Some(module::Error::UnsupportedVersion)
        );
        assert_eq!(super::Module::decode(b"\0asm").err(), Some(module::Error::Truncated));
    }

    #[test]
    fn data_segments_land_in_memory() {
        let mut builder = Module::new();
        builder.memory = Some(1);
        builder.data.push((16, b"i3mel".to_vec()));
        let signature = builder.add_type(&[0x7f], &[0x7f]);
        let code = vec![0x20, 0x00, 0x2d, 0x00, 0x00]; // i32.load8_u
        let index = builder.add_function(signature, &[], code);
        builder.export("byte", index);
        let bytes = builder.finish();

        assert_eq!(
            run(&bytes, "byte", &[Value::I32(16)], &mut recorder()),
            Ok(Some(Value::I32(b'i' as i32)))
        );
    }
}
