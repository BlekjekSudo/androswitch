//! Regression tests for Issue #272: `jbigdecimal.rs` radix validation,
//! `O(1)` `doubleValue`/`floatValue`/`toEngineeringString` on extreme scales,
//! `vm.heap`-bounded `toPlainString`, `i128::MIN`/`i32` checked arithmetic,
//! and JDK `NullPointerException` contracts.

mod common;

use common::*;
use rd_vm::engine::VmConfig;
use rd_vm::err::VmExit;
use rd_vm::intrinsics::call_static_intrinsic;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const BD: &str = "Ljava/math/BigDecimal;";
const BI: &str = "Ljava/math/BigInteger;";
const RM: &str = "Ljava/math/RoundingMode;";
const MC: &str = "Ljava/math/MathContext;";

fn alloc_obj(vm: &mut Engine, class: &str) -> ObjRef {
    vm.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc_instance")
}

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn new_bd_str(vm: &mut Engine, s: &str) -> ObjRef {
    let r = alloc_obj(vm, BD);
    let arg = jstr(vm, s);
    vm.invoke_instance(r, "<init>", "(Ljava/lang/String;)V", &[arg])
        .expect("BigDecimal.<init>(String)");
    r
}

fn new_bi_str(vm: &mut Engine, s: &str) -> ObjRef {
    let r = alloc_obj(vm, BI);
    let arg = jstr(vm, s);
    vm.invoke_instance(r, "<init>", "(Ljava/lang/String;)V", &[arg])
        .expect("BigInteger.<init>(String)");
    r
}

fn assert_thrown(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_class,
            "expected {expected_class}, got {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected Exception({expected_class}), got {other:?}"),
    }
}

#[test]
fn biginteger_radix_out_of_range_throws_nfe_without_host_panic() {
    let mut vm = engine_of(&DexBuilder::new());
    let zero_str = jstr(&mut vm, "0");

    for bad_radix in [i32::MIN, -10, -1, 0, 1, 37, 100, i32::MAX] {
        let bi = alloc_obj(&mut vm, BI);
        let res = vm.invoke_instance(
            bi,
            "<init>",
            "(Ljava/lang/String;I)V",
            &[zero_str.clone(), Value::Int(bad_radix)],
        );
        assert_thrown(res, "Ljava/lang/NumberFormatException;");
    }

    // Boundary radices 2 and 36 succeed cleanly.
    let bi2 = alloc_obj(&mut vm, BI);
    let b2_str = jstr(&mut vm, "-101010");
    vm.invoke_instance(
        bi2,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[b2_str, Value::Int(2)],
    )
    .expect("radix 2");
    assert_eq!(
        vm.invoke_instance(bi2, "intValue", "()I", &[])
            .expect("intValue"),
        Value::Int(-42)
    );

    let bi36 = alloc_obj(&mut vm, BI);
    let b36_str = jstr(&mut vm, "zz");
    vm.invoke_instance(
        bi36,
        "<init>",
        "(Ljava/lang/String;I)V",
        &[b36_str, Value::Int(36)],
    )
    .expect("radix 36");
    assert_eq!(
        vm.invoke_instance(bi36, "intValue", "()I", &[])
            .expect("intValue"),
        Value::Int(35 * 36 + 35)
    );

    // BigInteger.toString(radix) defaults out-of-range radices to base 10 per JDK spec.
    let Value::Obj(out_s) = vm
        .invoke_instance(bi36, "toString", "(I)Ljava/lang/String;", &[Value::Int(0)])
        .expect("toString(0)")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(out_s).expect("str"), "1295");
}

#[test]
fn extreme_exponent_double_float_and_engineering_are_o1_and_plain_string_respects_heap_budget() {
    let mut vm = Engine::new(
        vec![],
        VmConfig {
            heap_budget: 256 * 1024,
            ..VmConfig::default()
        },
    );

    // `new BigDecimal("1e2147483648")` produces scale == i32::MIN (-2147483648).
    let huge = new_bd_str(&mut vm, "1e2147483648");
    assert_eq!(
        vm.invoke_instance(huge, "scale", "()I", &[])
            .expect("scale"),
        Value::Int(i32::MIN)
    );

    // doubleValue() and floatValue() must run in O(1) without allocating gigabytes.
    let dv = vm
        .invoke_instance(huge, "doubleValue", "()D", &[])
        .expect("doubleValue");
    assert_eq!(dv, Value::Double(f64::INFINITY));

    let fv = vm
        .invoke_instance(huge, "floatValue", "()F", &[])
        .expect("floatValue");
    assert_eq!(fv, Value::Float(f32::INFINITY));

    // Negative huge exponent -> -Infinity; tiny exponent `valueOf(1, i32::MAX)` -> +0.0 in O(1).
    let neg_huge = new_bd_str(&mut vm, "-1e2147483648");
    assert_eq!(
        vm.invoke_instance(neg_huge, "doubleValue", "()D", &[])
            .expect("neg doubleValue"),
        Value::Double(f64::NEG_INFINITY)
    );

    let tiny = call_static_intrinsic(
        &mut vm,
        BD,
        "valueOf",
        "(JI)Ljava/math/BigDecimal;",
        &[Value::Long(1), Value::Int(i32::MAX)],
    )
    .expect("valueOf(1, MAX)")
    .expect("some");
    let Value::Obj(tiny_ref) = tiny else {
        panic!("expected BigDecimal");
    };
    assert_eq!(
        vm.invoke_instance(tiny_ref, "doubleValue", "()D", &[])
            .expect("tiny doubleValue"),
        Value::Double(0.0)
    );
    assert_eq!(
        vm.invoke_instance(tiny_ref, "floatValue", "()F", &[])
            .expect("tiny floatValue"),
        Value::Float(0.0)
    );

    // toEngineeringString() formats in O(1) with multiple-of-3 exponent.
    let Value::Obj(eng_ref) = vm
        .invoke_instance(huge, "toEngineeringString", "()Ljava/lang/String;", &[])
        .expect("toEngineeringString")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(eng_ref).expect("str"), "100E+2147483646");

    // toPlainString() on `1e2147483648` AND on `1e200000` (> 256 KiB heap budget)
    // must throw catchable OutOfMemoryError BEFORE allocating on the host heap.
    let res_min = vm.invoke_instance(huge, "toPlainString", "()Ljava/lang/String;", &[]);
    assert_thrown(res_min, "Ljava/lang/OutOfMemoryError;");

    let over_budget = new_bd_str(&mut vm, "1e200000");
    let res_over = vm.invoke_instance(over_budget, "toPlainString", "()Ljava/lang/String;", &[]);
    assert_thrown(res_over, "Ljava/lang/OutOfMemoryError;");

    // Ordinary plain string within budget still works identically.
    let small = new_bd_str(&mut vm, "1.25E+3");
    let Value::Obj(plain_ok) = vm
        .invoke_instance(small, "toPlainString", "()Ljava/lang/String;", &[])
        .expect("small toPlainString")
    else {
        panic!("expected String");
    };
    assert_eq!(vm.heap.as_str(plain_ok).expect("str"), "1250");
}

#[test]
fn i128_min_and_scale_overflows_throw_arithmetic_exception_without_panic() {
    let mut vm = engine_of(&DexBuilder::new());

    let i128_min_str = "-170141183460469231731687303715884105728";
    let bi_min = new_bi_str(&mut vm, i128_min_str);
    let bi_neg_one = new_bi_str(&mut vm, "-1");
    let bd_min = new_bd_str(&mut vm, i128_min_str);
    let bd_neg_one = new_bd_str(&mut vm, "-1");

    // BigInteger(i128::MIN): negate(), abs(), divide(-1), remainder(-1), mod(-1)
    assert_thrown(
        vm.invoke_instance(bi_min, "negate", "()Ljava/math/BigInteger;", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(bi_min, "abs", "()Ljava/math/BigInteger;", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bi_min,
            "divide",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
            &[Value::Obj(bi_neg_one)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bi_min,
            "remainder",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
            &[Value::Obj(bi_neg_one)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bi_min,
            "mod",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
            &[Value::Obj(bi_neg_one)],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    // BigDecimal(i128::MIN): negate(), abs(), divide(-1), divide(-1, 0, HALF_UP), remainder(-1)
    assert_thrown(
        vm.invoke_instance(bd_min, "negate", "()Ljava/math/BigDecimal;", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(bd_min, "abs", "()Ljava/math/BigDecimal;", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bd_min,
            "divide",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
            &[Value::Obj(bd_neg_one)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bd_min,
            "divide",
            "(Ljava/math/BigDecimal;II)Ljava/math/BigDecimal;",
            &[Value::Obj(bd_neg_one), Value::Int(0), Value::Int(4)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(
            bd_min,
            "remainder",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
            &[Value::Obj(bd_neg_one)],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    // `BigDecimal.valueOf(10, Integer.MIN_VALUE).stripTrailingZeros()` overflows scale downward.
    let Value::Obj(ten_min_scale) = call_static_intrinsic(
        &mut vm,
        BD,
        "valueOf",
        "(JI)Ljava/math/BigDecimal;",
        &[Value::Long(10), Value::Int(i32::MIN)],
    )
    .expect("valueOf")
    .expect("some") else {
        panic!("expected BigDecimal");
    };
    assert_thrown(
        vm.invoke_instance(
            ten_min_scale,
            "stripTrailingZeros",
            "()Ljava/math/BigDecimal;",
            &[],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    // `s - o.1` i32 overflow in 1-arg `BigDecimal.divide`:
    // 0 with scale i32::MIN divided by 1 with scale i32::MAX -> preferred scale < i32::MIN.
    let Value::Obj(zero_min_scale) = call_static_intrinsic(
        &mut vm,
        BD,
        "valueOf",
        "(JI)Ljava/math/BigDecimal;",
        &[Value::Long(0), Value::Int(i32::MIN)],
    )
    .expect("valueOf")
    .expect("some") else {
        panic!("expected BigDecimal");
    };
    let Value::Obj(one_max_scale) = call_static_intrinsic(
        &mut vm,
        BD,
        "valueOf",
        "(JI)Ljava/math/BigDecimal;",
        &[Value::Long(1), Value::Int(i32::MAX)],
    )
    .expect("valueOf")
    .expect("some") else {
        panic!("expected BigDecimal");
    };
    assert_thrown(
        vm.invoke_instance(
            zero_min_scale,
            "divide",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
            &[Value::Obj(one_max_scale)],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    // `round(MathContext)` when `s - (digits(u) - prec)` underflows i32::MIN.
    let Value::Obj(twelve_min_scale) = call_static_intrinsic(
        &mut vm,
        BD,
        "valueOf",
        "(JI)Ljava/math/BigDecimal;",
        &[Value::Long(12), Value::Int(i32::MIN)],
    )
    .expect("valueOf")
    .expect("some") else {
        panic!("expected BigDecimal");
    };
    let mc = vm
        .heap
        .alloc_instance(
            MC.to_string(),
            vec![("precision".to_string(), Value::Int(1))],
        )
        .expect("alloc MathContext");
    assert_thrown(
        vm.invoke_instance(
            twelve_min_scale,
            "round",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
            &[Value::Obj(mc)],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    // `intValueExact` / `longValueExact` on `1e100` must throw ArithmeticException, not return 0.
    let e100 = new_bd_str(&mut vm, "1e100");
    assert_thrown(
        vm.invoke_instance(e100, "intValueExact", "()I", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_thrown(
        vm.invoke_instance(e100, "longValueExact", "()J", &[]),
        "Ljava/lang/ArithmeticException;",
    );
}

#[test]
fn null_arguments_throw_null_pointer_exception() {
    let mut vm = engine_of(&DexBuilder::new());

    // BigDecimal((String) null)
    let bd = alloc_obj(&mut vm, BD);
    assert_thrown(
        vm.invoke_instance(bd, "<init>", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );

    // BigInteger((String) null) and BigInteger((String) null, 10)
    let bi1 = alloc_obj(&mut vm, BI);
    assert_thrown(
        vm.invoke_instance(bi1, "<init>", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );
    let bi2 = alloc_obj(&mut vm, BI);
    assert_thrown(
        vm.invoke_instance(
            bi2,
            "<init>",
            "(Ljava/lang/String;I)V",
            &[Value::Null, Value::Int(10)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // RoundingMode.valueOf((String) null) -> NullPointerException
    let rm_null = call_static_intrinsic(
        &mut vm,
        RM,
        "valueOf",
        "(Ljava/lang/String;)Ljava/math/RoundingMode;",
        &[Value::Null],
    );
    match rm_null {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NullPointerException;");
        }
        other => panic!("expected NPE from RoundingMode.valueOf(null), got {other:?}"),
    }

    // setScale(2, (RoundingMode) null) and round((MathContext) null) -> NullPointerException
    let val = new_bd_str(&mut vm, "1.234");
    assert_thrown(
        vm.invoke_instance(
            val,
            "setScale",
            "(ILjava/math/RoundingMode;)Ljava/math/BigDecimal;",
            &[Value::Int(2), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_thrown(
        vm.invoke_instance(
            val,
            "round",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
}
