//! Regression tests for issue #319 (`crates/rd-vm/src/numerics.rs`).

use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const CHARACTER: &str = "Ljava/lang/Character;";
const MATH: &str = "Ljava/lang/Math;";
const STRICT_MATH: &str = "Ljava/lang/StrictMath;";
const DOUBLE: &str = "Ljava/lang/Double;";
const FLOAT: &str = "Ljava/lang/Float;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn jstr(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.to_string()).expect("alloc string"))
}

fn assert_exc(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_class,
            "unexpected exception {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected_class}, got {other:?}"),
    }
}

#[test]
fn issue_319_character_to_chars_validates_code_point_null_dst_and_negative_dst_index() {
    let mut e = engine();
    let chars = e
        .heap
        .alloc_array_zeroed(ElemKind::Int, 4, "[C")
        .expect("char[4]");

    // 1. Negative dstIndex (-1, -2, i32::MIN) -> ArrayIndexOutOfBoundsException (never panic in debug or --release!)
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x41), Value::Obj(chars), Value::Int(-1)],
        ),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x1F600), Value::Obj(chars), Value::Int(-2)],
        ),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x41), Value::Obj(chars), Value::Int(i32::MIN)],
        ),
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
    );

    // 2. Invalid code points (< 0 or > 0x10FFFF) -> IllegalArgumentException (checked before dst != null)
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(-1), Value::Null, Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x11_0000), Value::Obj(chars), Value::Int(0)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // 3. dst == null with valid code point -> NullPointerException
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x41), Value::Null, Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // 4. Valid BMP (including surrogate 0xD800) and supplementary (0x1F600 = D83D DE00)
    assert_eq!(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0xD800), Value::Obj(chars), Value::Int(0)],
        )
        .expect("BMP surrogate"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_static(
            CHARACTER,
            "toChars",
            "(I[CI)I",
            &[Value::Int(0x1F600), Value::Obj(chars), Value::Int(1)],
        )
        .expect("supplementary"),
        Value::Int(2)
    );
    let slots = &e.heap.as_array_elems(chars).expect("elems").0;
    assert_eq!(slots[0], Value::Int(0xD800));
    assert_eq!(slots[1], Value::Int(0xD83D));
    assert_eq!(slots[2], Value::Int(0xDE00));
}

#[test]
fn issue_319_code_point_before_and_at_handle_min_value_and_null_sequence() {
    let mut e = engine();
    let s = jstr(&mut e, "A\u{1F600}B");

    // idx == i32::MIN must throw StringIndexOutOfBoundsException without i32 underflow panic
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "codePointBefore",
            "(Ljava/lang/CharSequence;I)I",
            &[s.clone(), Value::Int(i32::MIN)],
        ),
        "Ljava/lang/StringIndexOutOfBoundsException;",
    );
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "codePointAt",
            "(Ljava/lang/CharSequence;I)I",
            &[s.clone(), Value::Int(i32::MIN)],
        ),
        "Ljava/lang/StringIndexOutOfBoundsException;",
    );

    // null CharSequence -> NullPointerException
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "codePointBefore",
            "(Ljava/lang/CharSequence;I)I",
            &[Value::Null, Value::Int(1)],
        ),
        "Ljava/lang/NullPointerException;",
    );
    assert_exc(
        e.invoke_static(
            CHARACTER,
            "codePointAt",
            "(Ljava/lang/CharSequence;I)I",
            &[Value::Null, Value::Int(0)],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // Surrogate pair decoding
    assert_eq!(
        e.invoke_static(
            CHARACTER,
            "codePointBefore",
            "(Ljava/lang/CharSequence;I)I",
            &[s, Value::Int(3)],
        )
        .expect("codePointBefore surrogate pair"),
        Value::Int(0x1F600)
    );
}

#[test]
fn issue_319_character_digit_rejects_out_of_range_radix() {
    let mut e = engine();
    for bad_radix in [i32::MIN, -1, 0, 1, 37, 100, i32::MAX] {
        assert_eq!(
            e.invoke_static(
                CHARACTER,
                "digit",
                "(CI)I",
                &[Value::Int('1' as i32), Value::Int(bad_radix)],
            )
            .expect("digit(char, int)"),
            Value::Int(-1),
            "radix {bad_radix} should return -1"
        );
        assert_eq!(
            e.invoke_static(
                CHARACTER,
                "digit",
                "(II)I",
                &[Value::Int('z' as i32), Value::Int(bad_radix)],
            )
            .expect("digit(int, int)"),
            Value::Int(-1),
            "radix {bad_radix} should return -1"
        );
    }
    assert_eq!(
        e.invoke_static(
            CHARACTER,
            "digit",
            "(CI)I",
            &[Value::Int('f' as i32), Value::Int(16)],
        )
        .expect("hex f"),
        Value::Int(15)
    );
}

#[test]
fn issue_319_math_ulp_handles_infinity_max_value_nan_and_zero() {
    let mut e = engine();

    for cls in [MATH, STRICT_MATH] {
        // ±Infinity -> +Infinity
        for inf in [f64::INFINITY, f64::NEG_INFINITY] {
            let v = e
                .invoke_static(cls, "ulp", "(D)D", &[Value::Double(inf)])
                .expect("ulp(±Inf)")
                .as_double()
                .expect("double");
            assert_eq!(v, f64::INFINITY, "{cls}.ulp({inf}) must be +Infinity");
        }

        // ±Double.MAX_VALUE -> 2^971 (0x7ca0_0000_0000_0000)
        let expected_max_ulp = f64::from_bits(0x7ca0_0000_0000_0000);
        for mx in [f64::MAX, -f64::MAX] {
            let v = e
                .invoke_static(cls, "ulp", "(D)D", &[Value::Double(mx)])
                .expect("ulp(±MAX_VALUE)")
                .as_double()
                .expect("double");
            assert_eq!(v.to_bits(), expected_max_ulp.to_bits());
        }

        // NaN -> NaN, ±0.0 -> Double.MIN_VALUE (0x1)
        let nan_v = e
            .invoke_static(cls, "ulp", "(D)D", &[Value::Double(f64::NAN)])
            .expect("ulp(NaN)")
            .as_double()
            .expect("double");
        assert!(nan_v.is_nan());
        let zero_v = e
            .invoke_static(cls, "ulp", "(D)D", &[Value::Double(-0.0)])
            .expect("ulp(-0.0)")
            .as_double()
            .expect("double");
        assert_eq!(zero_v.to_bits(), 1);
    }
}

#[test]
fn issue_319_parse_float_and_double_reject_duplicate_and_nonfinite_suffixes() {
    let mut e = engine();

    for bad in [
        "1.0ff",
        "1.0df",
        "1.0DD",
        "NaNf",
        "NaNd",
        "+NaNF",
        "Infinityd",
        "-Infinityf",
        "f",
        "d",
    ] {
        let s1 = jstr(&mut e, bad);
        assert_exc(
            e.invoke_static(DOUBLE, "parseDouble", "(Ljava/lang/String;)D", &[s1]),
            "Ljava/lang/NumberFormatException;",
        );
        let s2 = jstr(&mut e, bad);
        assert_exc(
            e.invoke_static(FLOAT, "parseFloat", "(Ljava/lang/String;)F", &[s2]),
            "Ljava/lang/NumberFormatException;",
        );
    }

    // Valid single trailing suffix and exact non-finite strings still parse
    let ok_f = jstr(&mut e, "  -2.5f ");
    assert_eq!(
        e.invoke_static(DOUBLE, "parseDouble", "(Ljava/lang/String;)D", &[ok_f])
            .expect("parseDouble(-2.5f)")
            .as_double()
            .expect("double"),
        -2.5
    );
    let ok_inf = jstr(&mut e, "-Infinity");
    assert_eq!(
        e.invoke_static(DOUBLE, "parseDouble", "(Ljava/lang/String;)D", &[ok_inf])
            .expect("parseDouble(-Infinity)")
            .as_double()
            .expect("double"),
        f64::NEG_INFINITY
    );
}
