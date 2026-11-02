//! Regression tests for issue #376 (`crates/rd-vm/src/jsb.rs`): a lone
//! surrogate is a valid code point for `appendCodePoint` and `codePointAt`,
//! as in the JDK. Only values above U+10FFFF are rejected.

use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SB: &str = "Ljava/lang/StringBuilder;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn append_code_point(e: &mut Engine, sb: ObjRef, cp: i32) -> Result<(), rd_vm::err::VmExit> {
    e.invoke_instance(
        sb,
        "appendCodePoint",
        "(I)Ljava/lang/StringBuilder;",
        &[Value::Int(cp)],
    )
    .map(|_| ())
}

fn append_char(e: &mut Engine, sb: ObjRef, c: i32) {
    e.invoke_instance(
        sb,
        "append",
        "(C)Ljava/lang/StringBuilder;",
        &[Value::Int(c)],
    )
    .expect("append(char)");
}

fn to_string(e: &mut Engine, sb: ObjRef) -> String {
    match e
        .invoke_instance(sb, "toString", "()Ljava/lang/String;", &[])
        .expect("toString")
    {
        Value::Obj(s) => e.heap.as_str(s).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}

fn code_point_at(e: &mut Engine, sb: ObjRef, i: i32) -> i32 {
    match e
        .invoke_instance(sb, "codePointAt", "(I)I", &[Value::Int(i)])
        .expect("codePointAt")
    {
        Value::Int(n) => n,
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn issue_376_append_code_point_accepts_lone_surrogate_like_jdk() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_code_point(&mut e, sb, 0xD83D).expect("lone surrogate is a valid code point");
    append_code_point(&mut e, sb, 0xDE00).expect("low surrogate is a valid code point");
    // Dois surrogates anexados por code point formam o par correto.
    assert_eq!(to_string(&mut e, sb), "\u{1F600}");
}

#[test]
fn issue_376_append_code_point_rejects_only_out_of_range() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    assert!(append_code_point(&mut e, sb, 0x110000).is_err());
    assert!(append_code_point(&mut e, sb, -1).is_err());
    append_code_point(&mut e, sb, 0x10FFFF).expect("max code point");
}

#[test]
fn issue_376_code_point_at_returns_trailing_lone_surrogate_value() {
    // Só um surrogate alto no fim é guardado (campo `pend`); um surrogate
    // isolado no meio cai em U+FFFD porque a storage é uma `String` Rust.
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_char(&mut e, sb, 0xD83D);
    assert_eq!(code_point_at(&mut e, sb, 0), 0xD83D);
}

#[test]
fn issue_376_code_point_at_combines_valid_pair() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_char(&mut e, sb, 0xD83D);
    append_char(&mut e, sb, 0xDE00);
    assert_eq!(code_point_at(&mut e, sb, 0), 0x1F600);
    assert_eq!(code_point_at(&mut e, sb, 1), 0xDE00);
}
