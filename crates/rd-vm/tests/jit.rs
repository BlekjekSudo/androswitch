#![cfg(feature = "jit")]
//! Cranelift/interpreter differential tests; no unsafe outside rd-jni.
mod common;
use common::*;
use rd_vm::{Engine, Value};
fn engines(op: u8) -> (Engine, Engine) {
    let mut b = DexBuilder::new();
    let c = b.class("LCaso;", "Ljava/lang/Object;");
    let mut code = op23x(op, 0, 1, 2);
    code.extend(op11x(0x0f, 0));
    b.direct(
        c,
        "run",
        "I",
        vec!["I", "I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(3, 2, 0, code)),
    );
    let i = engine_of(&b);
    let mut j = engine_of(&b);
    j.enable_jit().unwrap();
    (i, j)
}
#[test]
fn native_integer_leaves_match_interpreter_at_edges_and_random_inputs() {
    for op in [0x90, 0x91, 0x92, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a] {
        let (mut i, mut j) = engines(op);
        let mut seed = 0x9282ab34u32;
        for n in 0..1200 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = seed as i32;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let y = match n % 8 {
                0 => -1,
                1 => i32::MIN,
                2 => i32::MAX,
                3 => 0,
                4 => 32,
                5 => -32,
                _ => seed as i32,
            };
            let args = [Value::Int(x), Value::Int(y)];
            let expected = i.invoke_static("LCaso;", "run", "(II)I", &args).unwrap();
            assert_eq!(
                j.invoke_static("LCaso;", "run", "(II)I", &args).unwrap(),
                expected,
                "op={op:x} x={x} y={y}"
            );
        }
        assert_eq!(j.jit_stats().compiled, 1);
        assert!(
            j.jit_stats().native_calls > 1100,
            "op={op:x} {:?}",
            j.jit_stats()
        );
        assert_eq!(
            i.fuel_used(),
            j.fuel_used(),
            "native fuel must match interpreted leaf"
        );
    }
}
#[test]
fn throwing_division_falls_back_and_fuel_is_still_enforced() {
    let (_, mut j) = engines(0x93);
    for _ in 0..40 {
        assert_eq!(
            j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(6), Value::Int(2)])
                .unwrap(),
            Value::Int(3)
        );
    }
    assert_eq!(j.jit_stats().compiled, 0);
    assert_eq!(j.jit_stats().unsupported, 1);
    assert!(matches!(
        j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(6), Value::Int(0)]),
        Err(rd_vm::err::VmExit::Exception(_))
    ));
    let (_, mut j) = engines(0x90);
    for _ in 0..40 {
        j.invoke_static("LCaso;", "run", "(II)I", &[Value::Int(1), Value::Int(2)])
            .unwrap();
    }
    assert!(j.jit_stats().native_calls > 0);
    j.config.fuel = j.fuel_used();
    assert!(
        matches!(j.invoke_static("LCaso;","run","(II)I",&[Value::Int(1),Value::Int(2)]),Err(rd_vm::err::VmExit::Error(ref e)) if e.code=="VM_FUEL")
    );
}
#[test]
fn literal_reverse_subtract_and_narrowing_match() {
    for opcode in [0x7b, 0x7c, 0x8d, 0x8e, 0x8f, 0xd1, 0xd9] {
        let mut b = DexBuilder::new();
        let c = b.class("LCaso;", "Ljava/lang/Object;");
        let mut code = if opcode == 0xd9 {
            op22b(opcode, 0, 1, -7)
        } else if opcode == 0xd1 {
            op22s(opcode, 0, 1, -7)
        } else {
            op12x(opcode, 0, 1)
        };
        code.extend(op11x(0x0f, 0));
        b.direct(
            c,
            "run",
            "I",
            vec!["I"],
            ACC_PUBLIC | ACC_STATIC,
            Some(b.code(2, 1, 0, code)),
        );
        let mut i = engine_of(&b);
        let mut j = engine_of(&b);
        j.enable_jit().unwrap();
        for n in 0i32..300 {
            let x = n.wrapping_mul(0x12345);
            let args = [Value::Int(x)];
            assert_eq!(
                j.invoke_static("LCaso;", "run", "(I)I", &args).unwrap(),
                i.invoke_static("LCaso;", "run", "(I)I", &args).unwrap()
            );
        }
        assert_eq!(
            j.jit_stats().compiled,
            1,
            "op={opcode:x} {:?}",
            j.jit_stats()
        );
    }
}
