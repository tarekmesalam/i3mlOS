//! The module the kernel actually ships is decoded here, on the development
//! machine, before it is ever handed to the kernel. A generated artifact that
//! only fails inside QEMU is a slow way to learn something a test can say in
//! milliseconds.

include!("../../../nawa/kernel/src/toolmod.rs");

#[test]
fn the_shipped_agent_module_decodes() {
    let module = i3ml_wasm::Module::decode(&MODULE).expect("the kernel's agent module decodes");
    let manifest = i3ml_wasm::manifest(&module);
    let names: Vec<&str> = manifest.iter().map(|requirement| requirement.name).collect();
    assert_eq!(names, vec!["invoke", "journal"]);
    assert!(module.export("run").is_some(), "exports its entry point");
}

#[test]
fn it_runs_and_reports_how_many_invocations_were_allowed() {
    struct AllowThenRefuse {
        allowed: i32,
        journaled: Vec<u64>,
    }

    impl i3ml_wasm::Host for AllowThenRefuse {
        fn call(
            &mut self,
            import: usize,
            arguments: &[i3ml_wasm::Value],
        ) -> Result<Option<i3ml_wasm::Value>, i3ml_wasm::Trap> {
            match import {
                0 => {
                    // `invoke`: allow the first N, then refuse — 0 is success.
                    if self.allowed > 0 {
                        self.allowed -= 1;
                        Ok(Some(i3ml_wasm::Value::I32(0)))
                    } else {
                        Ok(Some(i3ml_wasm::Value::I32(-1)))
                    }
                }
                1 => {
                    self.journaled.push(arguments[0].as_i64() as u64);
                    Ok(Some(i3ml_wasm::Value::I32(0)))
                }
                _ => Err(i3ml_wasm::Trap::HostDenied),
            }
        }
    }

    let module = i3ml_wasm::Module::decode(&MODULE).unwrap();

    let mut host = AllowThenRefuse { allowed: 3, journaled: Vec::new() };
    let mut instance = i3ml_wasm::Instance::new(&module, 100_000).unwrap();
    assert_eq!(
        instance.call_export("run", &[], &mut host),
        Ok(Some(i3ml_wasm::Value::I32(3))),
        "all three invocations allowed"
    );
    assert_eq!(host.journaled, vec![GREETING]);

    // A tool that is refused reports it in its own result, not just the log.
    let mut host = AllowThenRefuse { allowed: 1, journaled: Vec::new() };
    let mut instance = i3ml_wasm::Instance::new(&module, 100_000).unwrap();
    assert_eq!(
        instance.call_export("run", &[], &mut host),
        Ok(Some(i3ml_wasm::Value::I32(1))),
        "one allowed, two refused"
    );
}
