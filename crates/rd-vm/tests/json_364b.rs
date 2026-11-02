//! Regression tests for issue #364 (`crates/rd-vm/src/android_json.rs`):
//! 3. `JsonReader` in lenient mode accepts a literal newline inside a quoted
//!    string; strict mode still rejects it.
//! 4. Boolean arguments are true for any nonzero `i32` (Dalvik `Z`), not only `1`.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const STRING_READER: &str = "Ljava/io/StringReader;";
const JSON_READER: &str = "Landroid/util/JsonReader;";
const JSON_WRITER: &str = "Landroid/util/JsonWriter;";
const STRING_WRITER: &str = "Ljava/io/StringWriter;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn instance(e: &mut Engine, class: &str) -> ObjRef {
    e.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc")
}

fn text(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn text_of(e: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => e.heap.as_str(*r).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}

fn json_reader(e: &mut Engine, input: &str) -> ObjRef {
    let src = instance(e, STRING_READER);
    let t = text(e, input);
    e.invoke_instance(src, "<init>", "(Ljava/lang/String;)V", &[t])
        .expect("StringReader.<init>");
    let r = instance(e, JSON_READER);
    e.invoke_instance(r, "<init>", "(Ljava/io/Reader;)V", &[Value::Obj(src)])
        .expect("JsonReader.<init>");
    r
}

#[test]
fn issue_364_lenient_reader_accepts_newline_inside_string() {
    let mut e = engine();
    let r = json_reader(&mut e, "\"a\nb\"");
    e.invoke_instance(r, "setLenient", "(Z)V", &[Value::Int(1)])
        .expect("setLenient");
    let s = e
        .invoke_instance(r, "nextString", "()Ljava/lang/String;", &[])
        .expect("lenient nextString must accept a literal newline");
    assert_eq!(text_of(&e, &s), "a\nb");
}

#[test]
fn issue_364_strict_reader_still_rejects_newline_inside_string() {
    let mut e = engine();
    let r = json_reader(&mut e, "\"a\nb\"");
    let res = e.invoke_instance(r, "nextString", "()Ljava/lang/String;", &[]);
    assert!(
        matches!(res, Err(VmExit::Exception(_))),
        "strict nextString must reject a literal newline, got {res:?}"
    );
}

#[test]
fn issue_364_boolean_argument_is_true_for_any_nonzero_value() {
    let mut e = engine();
    let sw = instance(&mut e, STRING_WRITER);
    e.invoke_instance(sw, "<init>", "()V", &[])
        .expect("sw init");
    let w = instance(&mut e, JSON_WRITER);
    e.invoke_instance(w, "<init>", "(Ljava/io/Writer;)V", &[Value::Obj(sw)])
        .expect("JsonWriter.<init>");
    // Dalvik `Z` registers hold any nonzero i32 for true (here -1).
    e.invoke_instance(
        w,
        "value",
        "(Z)Landroid/util/JsonWriter;",
        &[Value::Int(-1)],
    )
    .expect("value(Z)");
    let out = e
        .invoke_instance(sw, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    assert_eq!(text_of(&e, &out), "true");
}
