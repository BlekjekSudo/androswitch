//! Regression tests for issue #376 (`crates/rd-vm/src/jsb.rs`):
//! a surrogate pair appended one `char` at a time must survive; the first half
//! used to be replaced by U+FFFD as soon as it was stored.

use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SB: &str = "Ljava/lang/StringBuilder;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
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

fn length(e: &mut Engine, sb: ObjRef) -> i32 {
    match e.invoke_instance(sb, "length", "()I", &[]).expect("length") {
        Value::Int(n) => n,
        other => panic!("expected int, got {other:?}"),
    }
}

#[test]
fn issue_376_surrogate_pair_appended_char_by_char_is_preserved() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_char(&mut e, sb, 0xD83D); // high surrogate of U+1F600
    append_char(&mut e, sb, 0xDE00); // low surrogate
    assert_eq!(to_string(&mut e, sb), "\u{1F600}");
    assert_eq!(length(&mut e, sb), 2);
}

#[test]
fn issue_376_lone_high_surrogate_becomes_replacement_when_followed_by_other_text() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_char(&mut e, sb, 0xD83D);
    append_char(&mut e, sb, 'a' as i32);
    assert_eq!(to_string(&mut e, sb), "\u{FFFD}a");
    assert_eq!(length(&mut e, sb), 2);
}

#[test]
fn issue_376_plain_text_is_unchanged() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    append_char(&mut e, sb, 'h' as i32);
    append_char(&mut e, sb, 'i' as i32);
    assert_eq!(to_string(&mut e, sb), "hi");
    assert_eq!(length(&mut e, sb), 2);
}
