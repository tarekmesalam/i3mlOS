//! The attacks an adversarial review actually ran against this interpreter,
//! kept as tests so they cannot come back.
//!
//! Each one was a way to kill the kernel from a module's bytes: a hang the
//! fuel meter did not notice, a heap exhaustion the ceilings did not stop, a
//! shift the arithmetic did not survive. They are cheap to run and expensive
//! to relearn.

use std::time::Instant;

fn uleb(mut value: u64) -> Vec<u8> {
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

fn section(id: u8, payload: Vec<u8>) -> Vec<u8> {
    let mut out = vec![id];
    out.extend(uleb(payload.len() as u64));
    out.extend(payload);
    out
}

fn header() -> Vec<u8> {
    b"\0asm\x01\0\0\0".to_vec()
}

struct Deny;
impl i3ml_wasm::Host for Deny {
    fn call(
        &mut self,
        _import: usize,
        _arguments: &[i3ml_wasm::Value],
    ) -> Result<Option<i3ml_wasm::Value>, i3ml_wasm::Trap> {
        Err(i3ml_wasm::Trap::HostDenied)
    }
}

/// A `block` inside a `loop` used to buy a full re-scan of the function body
/// for one unit of fuel. Measured at 13 seconds of frozen kernel for a 256 KiB
/// module that never left its budget — on a machine where nothing preempts
/// the interpreter.
#[test]
fn a_scan_amplification_module_cannot_outrun_its_fuel() {
    const FILLER: usize = 60_000;
    let mut code = vec![0x03, 0x40, 0x02, 0x40, 0x0c, 0x01];
    code.extend(std::iter::repeat(0x01).take(FILLER)); // nop padding
    code.extend([0x0b, 0x0b]);

    let mut body = uleb(0); // no locals
    body.extend(code);
    body.push(0x0b);
    let mut sized = uleb(body.len() as u64);
    sized.extend(body);

    let mut bytes = header();
    bytes.extend(section(1, {
        let mut payload = uleb(1);
        payload.extend([0x60, 0x00, 0x00]);
        payload
    }));
    bytes.extend(section(3, {
        let mut payload = uleb(1);
        payload.extend(uleb(0));
        payload
    }));
    bytes.extend(section(7, {
        let mut payload = uleb(1);
        payload.extend(uleb(4));
        payload.extend(b"spin");
        payload.push(0x00);
        payload.extend(uleb(0));
        payload
    }));
    bytes.extend(section(10, {
        let mut payload = uleb(1);
        payload.extend(sized);
        payload
    }));

    let module = i3ml_wasm::Module::decode(&bytes).expect("a legal module, just hostile");
    let mut instance = i3ml_wasm::Instance::new(&module, 100_000).unwrap();

    let started = Instant::now();
    let outcome = instance.call_export("spin", &[], &mut Deny);
    let elapsed = started.elapsed();

    assert_eq!(outcome, Err(i3ml_wasm::Trap::OutOfFuel));
    assert!(
        elapsed.as_millis() < 200,
        "fuel must bound wall-clock work, not just instruction count (took {elapsed:?})"
    );
}

/// Every ceiling counted one section's entries, and the decoder accepted any
/// section any number of times — so repetition multiplied every limit. A
/// ~280 KiB module could exhaust a 4 MiB kernel heap, and an allocation
/// failure in a kernel is a halt.
#[test]
fn repeating_a_section_cannot_multiply_its_ceiling() {
    let one_type = [0x60u8, 0x00, 0x00];
    let mut type_section_payload = uleb(64);
    for _ in 0..64 {
        type_section_payload.extend(one_type);
    }

    let mut bytes = header();
    for _ in 0..50 {
        bytes.extend(section(1, type_section_payload.clone()));
    }

    let module = i3ml_wasm::Module::decode(&bytes);
    assert!(module.is_err(), "a second type section must be refused, not accumulated");
}

/// Sections must also arrive in the order the format mandates; out-of-order
/// sections are how a decoder ends up matching bodies to the wrong types.
#[test]
fn sections_out_of_order_are_refused() {
    let mut bytes = header();
    bytes.extend(section(7, uleb(0))); // exports before types
    bytes.extend(section(1, uleb(0)));
    assert!(i3ml_wasm::Module::decode(&bytes).is_err());
}

/// Twelve attacker-chosen bytes used to reach `<< 70` on an i64 — an overflow
/// panic in a checked build, and a kernel halt either way.
#[test]
fn an_overlong_constant_traps_instead_of_panicking() {
    let mut code = vec![0x42];
    code.extend([0x80u8; 11]); // i64.const with eleven continuation bytes
    code.push(0x00);

    let mut body = uleb(0);
    body.extend(code);
    body.push(0x0b);
    let mut sized = uleb(body.len() as u64);
    sized.extend(body);

    let mut bytes = header();
    bytes.extend(section(1, {
        let mut payload = uleb(1);
        payload.extend([0x60, 0x00, 0x00]);
        payload
    }));
    bytes.extend(section(3, {
        let mut payload = uleb(1);
        payload.extend(uleb(0));
        payload
    }));
    bytes.extend(section(7, {
        let mut payload = uleb(1);
        payload.extend(uleb(4));
        payload.extend(b"trap");
        payload.push(0x00);
        payload.extend(uleb(0));
        payload
    }));
    bytes.extend(section(10, {
        let mut payload = uleb(1);
        payload.extend(sized);
        payload
    }));

    let module = i3ml_wasm::Module::decode(&bytes).expect("decodes; the constant is in the body");
    let mut instance = i3ml_wasm::Instance::new(&module, 10_000).unwrap();
    // Malformed, not a panic. That distinction is the whole test.
    assert_eq!(instance.call_export("trap", &[], &mut Deny), Err(i3ml_wasm::Trap::Malformed));
}

/// Module bytes are the one input whose size an attacker picks freely.
#[test]
fn an_oversized_module_is_refused_before_it_is_parsed() {
    let bytes = vec![0u8; i3ml_wasm::module::MAX_MODULE_BYTES + 1];
    assert_eq!(i3ml_wasm::Module::decode(&bytes).err(), Some(i3ml_wasm::module::Error::TooLarge));
}
