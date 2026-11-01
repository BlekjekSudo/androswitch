//! Deterministic mutation smoke test for the WASM parser, validator and
//! interpreter (ADR 013 §8). The `fuzz/` target runs the same property under
//! libFuzzer; this test keeps the property in plain `cargo test`.
//!
//! Property: for any input, `Module::parse`, `Instance::new` and `invoke` return
//! `Ok` or a structured `WasmError` with a known code. None may panic.

mod support;

use rd_engine_wasm::{Instance, Module, Value};
use support::*;

const KNOWN: &[&str] = &[
    "PARSE_ERROR",
    "INVALID_FORMAT",
    "NOT_IMPLEMENTED",
    "LIMIT_EXCEEDED",
    "INSTANTIATION_FAILED",
    "NOT_FOUND",
    "INVALID_ARG",
    "TRAP",
    "FUEL_EXHAUSTED",
    "STACK_EXHAUSTED",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: deterministic, no external crate.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn seed_module() -> Vec<u8> {
    let mut b = ModuleBuilder::default();
    b.types.push(functype(&[I32], &[I32]));
    b.types.push(functype(&[I32, I32], &[I32]));
    b.memory = Some((1, Some(4)));
    b.globals.push(Global {
        ty: I32,
        mutable: true,
        init: i32_const(3),
    });
    // loop that sums with memory traffic
    let mut body = Vec::new();
    body.extend([0x02, 0x40, 0x03, 0x40]);
    body.extend(local_get(0));
    body.push(0x45);
    body.extend(br_if(1));
    body.extend(local_get(0));
    body.extend(i32_const(4));
    body.push(0x6c);
    body.extend(local_get(0));
    body.extend(mem(0x36, 2, 0));
    body.extend(local_get(0));
    body.extend(i32_const(1));
    body.push(0x6b);
    body.extend(local_set(0));
    body.extend(br(0));
    body.extend([0x0b, 0x0b]);
    body.extend(global_get(0));
    body.extend(local_get(0));
    body.push(0x6a);
    body.extend(global_set(0));
    body.extend(global_get(0));
    b.funcs.push(FuncDef {
        type_idx: 0,
        locals: vec![],
        body,
    });
    let mut two = local_get(0);
    two.extend(local_get(1));
    two.extend(call(0));
    two.push(0x6a);
    b.funcs.push(FuncDef {
        type_idx: 1,
        locals: vec![],
        body: two,
    });
    b.exports.push(("run".into(), 0, 0));
    b.exports.push(("two".into(), 0, 1));
    b.data.push((8, vec![9, 8, 7]));
    b.build()
}

/// How far an input got: 0 rejected by the parser, 1 rejected at
/// instantiation, 2 reached the interpreter.
fn exercise(bytes: &[u8]) -> u8 {
    let module = match Module::parse(bytes) {
        Ok(m) => m,
        Err(e) => {
            assert!(KNOWN.contains(&e.code), "unknown error code {}", e.code);
            return 0;
        }
    };
    let mut inst = match Instance::new(module, 50_000) {
        Ok(i) => i,
        Err(e) => {
            assert!(KNOWN.contains(&e.code), "unknown error code {}", e.code);
            return 1;
        }
    };
    for (name, args) in [
        ("run", vec![Value::I32(5)]),
        ("two", vec![Value::I32(3), Value::I32(4)]),
        ("missing", vec![]),
    ] {
        if let Err(e) = inst.invoke(name, &args, 50_000) {
            assert!(KNOWN.contains(&e.code), "unknown error code {}", e.code);
        }
    }
    2
}

#[test]
fn seed_module_is_valid_and_runs() {
    let bytes = seed_module();
    let mut inst = Instance::new(Module::parse(&bytes).unwrap(), 50_000).unwrap();
    assert!(inst.invoke("run", &[Value::I32(5)], 50_000).is_ok());
}

#[test]
fn mutated_modules_never_panic() {
    let seed = seed_module();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut reached = [0usize; 3];
    for _ in 0..20_000 {
        let mut bytes = seed.clone();
        for _ in 0..(1 + rng.below(6)) {
            match rng.below(5) {
                0 => {
                    let i = rng.below(bytes.len());
                    bytes[i] = rng.next() as u8;
                }
                1 => {
                    let i = rng.below(bytes.len());
                    bytes.truncate(i);
                    if bytes.is_empty() {
                        bytes = seed.clone();
                    }
                }
                2 => {
                    let i = rng.below(bytes.len() + 1);
                    bytes.insert(i, rng.next() as u8);
                }
                3 => {
                    let i = rng.below(bytes.len());
                    bytes[i] ^= 1 << rng.below(8);
                }
                _ => {
                    let i = rng.below(bytes.len());
                    let j = rng.below(bytes.len());
                    bytes.swap(i, j);
                }
            }
        }
        reached[exercise(&bytes) as usize] += 1;
    }
    // Most byte mutations corrupt a section size or the header, so only a few
    // percent reach execution. That is still hundreds of runs, enough to prove
    // the interpreter is exercised rather than skipped.
    assert!(
        reached[2] >= 250,
        "only {} inputs reached the interpreter: {reached:?}",
        reached[2]
    );
}

#[test]
fn random_inputs_never_panic() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..5_000 {
        let len = rng.below(96);
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend((0..len).map(|_| rng.next() as u8));
        exercise(&bytes);
    }
}
