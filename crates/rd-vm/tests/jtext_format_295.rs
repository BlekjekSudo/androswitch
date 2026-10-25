//! Integration regression tests for issue #295 (`java.text.DecimalFormat`
//! and friends in `jtext_format.rs`), exercising the guest-visible API
//! through `Engine::invoke_instance`/`invoke_static`:
//! 1. Digit setters clamp to the JDK bounds (`DOUBLE_INTEGER_DIGITS=309`,
//!    `DOUBLE_FRACTION_DIGITS=340`) instead of storing `Integer.MAX_VALUE`
//!    and making `format` hang/OOM with quadratic padding.
//! 2. `parse` round-trips negative subpatterns (`"(1.5)"`) and negative
//!    currency (`"-$9.90"`) that `format` itself produces.
//! 3. Null contracts: `parse(null)`, `new DecimalFormat(null)`,
//!    `applyPattern(null)` are NPE; `Charset.isSupported(null)` is IAE;
//!    `new ComponentName((String) null, ...)` is NPE.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const DF: &str = "Ljava/text/DecimalFormat;";
const CHARSET: &str = "Ljava/nio/charset/Charset;";
const CN: &str = "Landroid/content/ComponentName;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn s(e: &mut Engine, txt: &str) -> u32 {
    e.heap.alloc_string(txt.to_string()).unwrap()
}

/// `new DecimalFormat(pattern)` through the real intrinsic path.
fn new_df(e: &mut Engine, pattern: &str) -> u32 {
    let df = e.heap.alloc_instance(DF.to_string(), Vec::new()).unwrap();
    let p = Value::Obj(s(e, pattern));
    e.invoke_instance(df, "<init>", "(Ljava/lang/String;)V", &[p])
        .unwrap();
    df
}

fn get_int(e: &mut Engine, obj: u32, name: &str) -> i32 {
    e.invoke_instance(obj, name, "()I", &[])
        .unwrap()
        .as_int()
        .unwrap()
}

fn assert_exc_class(res: Result<Value, VmExit>, expected: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected,
            "unexpected exception {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected}, got {other:?}"),
    }
}

/// Unboxes the `Number` returned by `DecimalFormat.parse` to `f64`.
fn unbox_double(e: &mut Engine, v: Value) -> f64 {
    let Value::Obj(r) = v else {
        panic!("expected boxed Number, got {v:?}")
    };
    match e.heap.get_field(r, "value").unwrap() {
        Value::Double(d) => d,
        Value::Long(l) => l as f64,
        Value::Int(i) => i as f64,
        other => panic!("unexpected boxed value {other:?}"),
    }
}

#[test]
fn issue_295_digit_setters_clamp_to_jdk_bounds() {
    let mut e = engine();
    let df = new_df(&mut e, "#,##0.###");

    e.invoke_instance(
        df,
        "setMinimumIntegerDigits",
        "(I)V",
        &[Value::Int(i32::MAX)],
    )
    .unwrap();
    assert_eq!(get_int(&mut e, df, "getMinimumIntegerDigits"), 309);

    e.invoke_instance(
        df,
        "setMinimumFractionDigits",
        "(I)V",
        &[Value::Int(i32::MAX)],
    )
    .unwrap();
    assert_eq!(get_int(&mut e, df, "getMinimumFractionDigits"), 340);
    // The coupled maximum follows the clamped minimum, never MAX_VALUE.
    assert_eq!(get_int(&mut e, df, "getMaximumFractionDigits"), 340);

    e.invoke_instance(
        df,
        "setMaximumFractionDigits",
        "(I)V",
        &[Value::Int(i32::MAX)],
    )
    .unwrap();
    assert_eq!(get_int(&mut e, df, "getMaximumFractionDigits"), 340);

    e.invoke_instance(df, "setMinimumIntegerDigits", "(I)V", &[Value::Int(-1)])
        .unwrap();
    assert_eq!(get_int(&mut e, df, "getMinimumIntegerDigits"), 0);
}

#[test]
fn issue_295_format_at_jdk_digit_ceiling_is_exact() {
    // With the unclamped setter this format call padded toward
    // 2_147_483_647 digits with an O(N²) insert loop (hang + host OOM).
    // Clamped to 309 it is immediate and exact.
    let mut e = engine();
    let df = new_df(&mut e, "0");
    e.invoke_instance(
        df,
        "setMinimumIntegerDigits",
        "(I)V",
        &[Value::Int(i32::MAX)],
    )
    .unwrap();
    let out = e
        .invoke_instance(df, "format", "(D)Ljava/lang/String;", &[Value::Double(7.0)])
        .unwrap();
    let Value::Obj(r) = out else {
        panic!("expected String, got {out:?}")
    };
    let text = e.heap.as_str(r).unwrap().to_string();
    assert_eq!(text.len(), 309);
    assert!(text.ends_with('7'));
    assert!(text[..308].chars().all(|c| c == '0'));
}

#[test]
fn issue_295_parse_round_trips_negative_subpattern_and_currency() {
    let mut e = engine();

    // Accounting-style negative subpattern.
    let paren = new_df(&mut e, "#.##;(#.##)");
    let src = Value::Obj(s(&mut e, "(1.5)"));
    let v = e
        .invoke_instance(
            paren,
            "parse",
            "(Ljava/lang/String;)Ljava/lang/Number;",
            &[src],
        )
        .unwrap();
    assert_eq!(unbox_double(&mut e, v), -1.5);
    let src = Value::Obj(s(&mut e, "1.5"));
    let v = e
        .invoke_instance(
            paren,
            "parse",
            "(Ljava/lang/String;)Ljava/lang/Number;",
            &[src],
        )
        .unwrap();
    assert_eq!(unbox_double(&mut e, v), 1.5);

    // Currency: format produces "-$9.90"; parse must accept it back.
    let money = new_df(&mut e, "$#,##0.00");
    let out = e
        .invoke_instance(
            money,
            "format",
            "(D)Ljava/lang/String;",
            &[Value::Double(-9.9)],
        )
        .unwrap();
    let Value::Obj(r) = out else {
        panic!("expected String, got {out:?}")
    };
    assert_eq!(e.heap.as_str(r).unwrap(), "-$9.90");
    let src = Value::Obj(s(&mut e, "-$9.90"));
    let v = e
        .invoke_instance(
            money,
            "parse",
            "(Ljava/lang/String;)Ljava/lang/Number;",
            &[src],
        )
        .unwrap();
    assert_eq!(unbox_double(&mut e, v), -9.9);
}

#[test]
fn issue_295_null_contracts_throw_typed() {
    let mut e = engine();
    let df = new_df(&mut e, "#,##0.###");

    // DecimalFormat.parse(null) -> NullPointerException (not ParseException)
    assert_exc_class(
        e.invoke_instance(
            df,
            "parse",
            "(Ljava/lang/String;)Ljava/lang/Number;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );

    // applyPattern(null) -> NullPointerException
    assert_exc_class(
        e.invoke_instance(df, "applyPattern", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );

    // new DecimalFormat((String) null) -> NullPointerException
    let shell = e.heap.alloc_instance(DF.to_string(), Vec::new()).unwrap();
    assert_exc_class(
        e.invoke_instance(shell, "<init>", "(Ljava/lang/String;)V", &[Value::Null]),
        "Ljava/lang/NullPointerException;",
    );

    // Charset.isSupported(null) -> IllegalArgumentException
    assert_exc_class(
        e.invoke_static(
            CHARSET,
            "isSupported",
            "(Ljava/lang/String;)Z",
            &[Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // new ComponentName((String) null, "com.foo.Bar") -> NullPointerException
    let cn = e.heap.alloc_instance(CN.to_string(), Vec::new()).unwrap();
    let cls = Value::Obj(s(&mut e, "com.foo.Bar"));
    assert_exc_class(
        e.invoke_instance(
            cn,
            "<init>",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[Value::Null, cls],
        ),
        "Ljava/lang/NullPointerException;",
    );
}

#[test]
fn issue_295_format_long_is_exact_and_object_is_gated() {
    let mut e = engine();
    let df = new_df(&mut e, "#,##0");
    let fmt_long = "(J)Ljava/lang/String;";
    let fmt_obj = "(Ljava/lang/Object;)Ljava/lang/String;";

    // format(long) exato além de 2^53 (antes: …808 via f64).
    let max = match e
        .invoke_instance(df, "format", fmt_long, &[Value::Long(i64::MAX)])
        .unwrap()
    {
        Value::Obj(r) => e.heap.as_str(r).unwrap().to_string(),
        v => panic!("format(long) inesperado: {v:?}"),
    };
    assert_eq!(max, "9,223,372,036,854,775,807");

    // format(Object) com Long box: também exato.
    let boxed = match e
        .invoke_static(
            "Ljava/lang/Long;",
            "valueOf",
            "(J)Ljava/lang/Long;",
            &[Value::Long((1i64 << 53) + 1)],
        )
        .unwrap()
    {
        Value::Obj(r) => r,
        v => panic!("Long.valueOf inesperado: {v:?}"),
    };
    let via_obj = match e
        .invoke_instance(df, "format", fmt_obj, &[Value::Obj(boxed)])
        .unwrap()
    {
        Value::Obj(r) => e.heap.as_str(r).unwrap().to_string(),
        v => panic!("format(Object=Long) inesperado: {v:?}"),
    };
    assert_eq!(via_obj, "9,007,199,254,740,993");

    // format(Object) com String "123" -> IllegalArgumentException
    // (antes formatava a String como se fosse número).
    let txt = Value::Obj(s(&mut e, "123"));
    assert_exc_class(
        e.invoke_instance(df, "format", fmt_obj, &[txt]),
        "Ljava/lang/IllegalArgumentException;",
    );
}
