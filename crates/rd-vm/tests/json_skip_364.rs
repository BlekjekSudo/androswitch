//! Regression test for issue #364: `JsonReader.skipValue()` positioned on an
//! object property NAME skips only that name (AOSP behavior), so the property
//! value remains available to the caller.

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const STRING_READER: &str = "Ljava/io/StringReader;";
const JSON_READER: &str = "Landroid/util/JsonReader;";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}

fn text(engine: &mut Engine, value: &str) -> Value {
    Value::Obj(engine.heap.alloc_string(value.to_string()).unwrap())
}

fn text_of(engine: &Engine, value: &Value) -> String {
    let Value::Obj(reference) = value else {
        panic!("expected String, got {value:?}");
    };
    engine.heap.as_str(*reference).unwrap().to_string()
}

fn json_reader(engine: &mut Engine, input: &str) -> ObjRef {
    let source = instance(engine, STRING_READER);
    let input = text(engine, input);
    engine
        .invoke_instance(source, "<init>", "(Ljava/lang/String;)V", &[input])
        .unwrap();
    let reader = instance(engine, JSON_READER);
    engine
        .invoke_instance(
            reader,
            "<init>",
            "(Ljava/io/Reader;)V",
            &[Value::Obj(source)],
        )
        .unwrap();
    reader
}

fn call(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
    engine.invoke_instance(recv, name, sig, args).unwrap()
}

fn call_string(engine: &mut Engine, recv: ObjRef, name: &str, sig: &str) -> String {
    let value = call(engine, recv, name, sig, &[]);
    text_of(engine, &value)
}

fn peek_token(engine: &mut Engine, recv: ObjRef) -> String {
    let token = call(engine, recv, "peek", "()Landroid/util/JsonToken;", &[]);
    let Value::Obj(token) = token else {
        panic!("expected JsonToken, got {token:?}");
    };
    let name = engine.heap.get_field(token, "name").unwrap();
    text_of(engine, &name)
}

#[test]
fn skip_value_on_property_name_leaves_value_for_the_caller() {
    let mut engine = engine_of(&DexBuilder::new());
    let reader = json_reader(&mut engine, r#"{"a": 1, "b": 2}"#);
    call(&mut engine, reader, "beginObject", "()V", &[]);
    assert_eq!(peek_token(&mut engine, reader), "NAME");
    // Positioned on the NAME "a": skipValue consumes only the name.
    call(&mut engine, reader, "skipValue", "()V", &[]);
    assert_eq!(peek_token(&mut engine, reader), "NUMBER");
    assert_eq!(
        call(&mut engine, reader, "nextInt", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "b"
    );
    assert_eq!(
        call(&mut engine, reader, "nextInt", "()I", &[]),
        Value::Int(2)
    );
    call(&mut engine, reader, "endObject", "()V", &[]);
}

#[test]
fn skip_value_on_nested_property_skips_whole_container() {
    let mut engine = engine_of(&DexBuilder::new());
    let reader = json_reader(&mut engine, r#"{"a": {"x": [1, 2]}, "b": 2}"#);
    call(&mut engine, reader, "beginObject", "()V", &[]);
    call(&mut engine, reader, "skipValue", "()V", &[]);
    call(&mut engine, reader, "skipValue", "()V", &[]);
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "b"
    );
    assert_eq!(
        call(&mut engine, reader, "nextInt", "()I", &[]),
        Value::Int(2)
    );
    call(&mut engine, reader, "endObject", "()V", &[]);
}
