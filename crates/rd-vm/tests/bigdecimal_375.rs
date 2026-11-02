//! Regression tests for issue #375 (`jbigdecimal.rs`):
//! 1. `upscale(0, k)` must not overflow for large `k` (zero stays zero).
//! 2. `compareTo` must never throw for valid values with distant scales.
//! 3. `round(MathContext)` must honor the context's `RoundingMode`.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_static_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const BD: &str = "Ljava/math/BigDecimal;";
const RM: &str = "Ljava/math/RoundingMode;";
const MC: &str = "Ljava/math/MathContext;";

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn new_bd(vm: &mut Engine, s: &str) -> ObjRef {
    let r = vm
        .heap
        .alloc_instance(BD.to_string(), Vec::new())
        .expect("alloc BigDecimal");
    let arg = jstr(vm, s);
    vm.invoke_instance(r, "<init>", "(Ljava/lang/String;)V", &[arg])
        .expect("BigDecimal.<init>(String)");
    r
}

fn text_of(vm: &Engine, v: &Value) -> String {
    let Value::Obj(r) = v else {
        panic!("expected String, got {v:?}");
    };
    vm.heap.as_str(*r).expect("String").to_string()
}

fn rounding_mode(vm: &mut Engine, name: &str) -> Value {
    let arg = jstr(vm, name);
    match call_static_intrinsic(
        vm,
        RM,
        "valueOf",
        "(Ljava/lang/String;)Ljava/math/RoundingMode;",
        &[arg],
    )
    .expect("RoundingMode.valueOf")
    .expect("some")
    {
        v @ Value::Obj(_) => v,
        other => panic!("expected RoundingMode, got {other:?}"),
    }
}

fn math_context(vm: &mut Engine, precision: i32, mode: Value) -> Value {
    let mc = vm
        .heap
        .alloc_instance(
            MC.to_string(),
            vec![
                ("precision".to_string(), Value::Int(precision)),
                ("roundingMode".to_string(), mode),
            ],
        )
        .expect("alloc MathContext");
    Value::Obj(mc)
}

fn compare(vm: &mut Engine, a: &str, b: &str) -> Result<i32, VmExit> {
    let ra = new_bd(vm, a);
    let rb = new_bd(vm, b);
    match vm.invoke_instance(ra, "compareTo", "(Ljava/lang/Object;)I", &[Value::Obj(rb)])? {
        Value::Int(i) => Ok(i),
        other => panic!("compareTo returned {other:?}"),
    }
}

#[test]
fn zero_with_large_scale_does_not_overflow() {
    let mut vm = engine_of(&DexBuilder::new());
    let zero = new_bd(&mut vm, "0");
    let scaled = vm
        .invoke_instance(
            zero,
            "setScale",
            "(I)Ljava/math/BigDecimal;",
            &[Value::Int(40)],
        )
        .expect("ZERO.setScale(40) must not overflow");
    let Value::Obj(scaled) = scaled else {
        panic!("expected BigDecimal");
    };
    let tiny = new_bd(&mut vm, "1E-40");
    vm.invoke_instance(
        scaled,
        "add",
        "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        &[Value::Obj(tiny)],
    )
    .expect("0 + 1E-40 must not overflow");
    assert_eq!(compare(&mut vm, "0", "1E-40").unwrap(), -1);
}

#[test]
fn compare_to_never_overflows_on_distant_scales() {
    let mut vm = engine_of(&DexBuilder::new());
    assert_eq!(compare(&mut vm, "1E+30", "1E-30").unwrap(), 1);
    assert_eq!(compare(&mut vm, "1E-30", "1E+30").unwrap(), -1);
    assert_eq!(compare(&mut vm, "-1E+30", "1E-30").unwrap(), -1);
    assert_eq!(compare(&mut vm, "-1E-30", "-1E+30").unwrap(), 1);
    assert_eq!(compare(&mut vm, "0", "-1E-40").unwrap(), 1);
}

#[test]
fn compare_to_is_numeric_not_textual() {
    let mut vm = engine_of(&DexBuilder::new());
    assert_eq!(compare(&mut vm, "1.00", "1").unwrap(), 0);
    assert_eq!(compare(&mut vm, "0.1", "0.10").unwrap(), 0);
    assert_eq!(compare(&mut vm, "2E-40", "19E-41").unwrap(), 1);
    assert_eq!(compare(&mut vm, "19E-41", "2E-40").unwrap(), -1);
    assert_eq!(compare(&mut vm, "9.99", "10").unwrap(), -1);
}

fn round_plain(vm: &mut Engine, input: ObjRef, mc: Value) -> String {
    let r = vm
        .invoke_instance(
            input,
            "round",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
            &[mc],
        )
        .expect("round(MathContext)");
    let Value::Obj(r) = r else {
        panic!("expected BigDecimal");
    };
    let plain = vm
        .invoke_instance(r, "toPlainString", "()Ljava/lang/String;", &[])
        .expect("toPlainString");
    text_of(vm, &plain)
}

#[test]
fn round_math_context_honors_rounding_mode() {
    let mut vm = engine_of(&DexBuilder::new());
    let input = new_bd(&mut vm, "2.5");
    // Half-way value 2.5 at precision 1: HALF_UP gives 3, HALF_EVEN gives 2.
    let up = rounding_mode(&mut vm, "HALF_UP");
    let half_up = math_context(&mut vm, 1, up);
    assert_eq!(round_plain(&mut vm, input, half_up), "3");
    let even = rounding_mode(&mut vm, "HALF_EVEN");
    let half_even = math_context(&mut vm, 1, even);
    assert_eq!(round_plain(&mut vm, input, half_even), "2");
}
