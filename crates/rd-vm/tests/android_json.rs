//! Guest-visible `android.util.JsonReader`/`JsonWriter` behavior over
//! host `StringReader` and `StringWriter` instances.

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

const STRING_READER: &str = "Ljava/io/StringReader;";
const STRING_WRITER: &str = "Ljava/io/StringWriter;";
const JSON_READER: &str = "Landroid/util/JsonReader;";
const JSON_WRITER: &str = "Landroid/util/JsonWriter;";
const JSON_TOKEN: &str = "Landroid/util/JsonToken;";

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .unwrap()
}

fn object(value: Value) -> ObjRef {
    let Value::Obj(reference) = value else {
        panic!("expected object, got {value:?}");
    };
    reference
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

fn string_reader(engine: &mut Engine, input: &str) -> ObjRef {
    let reader = instance(engine, STRING_READER);
    let input = text(engine, input);
    engine
        .invoke_instance(reader, "<init>", "(Ljava/lang/String;)V", &[input])
        .unwrap();
    reader
}

fn json_reader(engine: &mut Engine, input: &str) -> ObjRef {
    let source = string_reader(engine, input);
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

fn token_name(engine: &Engine, value: &Value) -> String {
    let token = match value {
        Value::Obj(reference) => *reference,
        _ => panic!("expected JsonToken, got {value:?}"),
    };
    let name = engine.heap.get_field(token, "name").unwrap();
    text_of(engine, &name)
}

#[test]
fn json_reader_strict_streaming_tokens_paths_and_skip_value() {
    let mut engine = engine_of(&DexBuilder::new());
    let reader = json_reader(
        &mut engine,
        r#"{"name":"A\n\u00e9","count":42,"ratio":2.5,"ok":true,"missing":null,"items":[1,"two",{"skip":[true,false]}],"done":"yes"}"#,
    );

    let first_peek = call(
        &mut engine,
        reader,
        "peek",
        "()Landroid/util/JsonToken;",
        &[],
    );
    let second_peek = call(
        &mut engine,
        reader,
        "peek",
        "()Landroid/util/JsonToken;",
        &[],
    );
    assert_eq!(
        first_peek, second_peek,
        "JsonToken constants are singletons"
    );
    assert_eq!(token_name(&engine, &first_peek), "BEGIN_OBJECT");
    call(&mut engine, reader, "beginObject", "()V", &[]);
    assert_eq!(
        call(&mut engine, reader, "hasNext", "()Z", &[]),
        Value::Int(1)
    );

    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "name"
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextString", "()Ljava/lang/String;"),
        "A\né"
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "count"
    );
    let number = call(
        &mut engine,
        reader,
        "peek",
        "()Landroid/util/JsonToken;",
        &[],
    );
    assert_eq!(token_name(&engine, &number), "NUMBER");
    assert_eq!(
        call(&mut engine, reader, "nextInt", "()I", &[]),
        Value::Int(42)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "ratio"
    );
    assert_eq!(
        call(&mut engine, reader, "nextDouble", "()D", &[]),
        Value::Double(2.5)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "ok"
    );
    assert_eq!(
        call(&mut engine, reader, "nextBoolean", "()Z", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "missing"
    );
    call(&mut engine, reader, "nextNull", "()V", &[]);

    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "items"
    );
    call(&mut engine, reader, "beginArray", "()V", &[]);
    assert_eq!(
        call(&mut engine, reader, "nextLong", "()J", &[]),
        Value::Long(1)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextString", "()Ljava/lang/String;"),
        "two"
    );
    assert_eq!(
        call_string(&mut engine, reader, "getPath", "()Ljava/lang/String;"),
        "$.items[2]"
    );
    call(&mut engine, reader, "skipValue", "()V", &[]);
    assert_eq!(
        call(&mut engine, reader, "hasNext", "()Z", &[]),
        Value::Int(0)
    );
    call(&mut engine, reader, "endArray", "()V", &[]);

    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "done"
    );
    assert_eq!(
        call_string(&mut engine, reader, "getPath", "()Ljava/lang/String;"),
        "$.done"
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextString", "()Ljava/lang/String;"),
        "yes"
    );
    call(&mut engine, reader, "endObject", "()V", &[]);
    let end = call(
        &mut engine,
        reader,
        "peek",
        "()Landroid/util/JsonToken;",
        &[],
    );
    assert_eq!(token_name(&engine, &end), "END_DOCUMENT");
    call(&mut engine, reader, "close", "()V", &[]);
}

#[test]
fn json_reader_lenient_mode_and_typed_errors() {
    let mut engine = engine_of(&DexBuilder::new());
    let reader = json_reader(
        &mut engine,
        "/*lead*/ {unquoted:'value', list:[1, two, NaN], empty:[]}",
    );
    call(&mut engine, reader, "setLenient", "(Z)V", &[Value::Int(1)]);
    assert_eq!(
        call(&mut engine, reader, "isLenient", "()Z", &[]),
        Value::Int(1)
    );
    call(&mut engine, reader, "beginObject", "()V", &[]);
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "unquoted"
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextString", "()Ljava/lang/String;"),
        "value"
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "list"
    );
    call(&mut engine, reader, "beginArray", "()V", &[]);
    assert_eq!(
        call(&mut engine, reader, "nextInt", "()I", &[]),
        Value::Int(1)
    );
    assert_eq!(
        call_string(&mut engine, reader, "nextString", "()Ljava/lang/String;"),
        "two"
    );
    let nan = call(&mut engine, reader, "nextDouble", "()D", &[]);
    assert!(matches!(nan, Value::Double(value) if value.is_nan()));
    call(&mut engine, reader, "endArray", "()V", &[]);
    assert_eq!(
        call_string(&mut engine, reader, "nextName", "()Ljava/lang/String;"),
        "empty"
    );
    call(&mut engine, reader, "beginArray", "()V", &[]);
    call(&mut engine, reader, "endArray", "()V", &[]);
    call(&mut engine, reader, "endObject", "()V", &[]);

    let malformed = json_reader(&mut engine, r#"{"a":}"#);
    call(&mut engine, malformed, "beginObject", "()V", &[]);
    assert_eq!(
        call_string(&mut engine, malformed, "nextName", "()Ljava/lang/String;"),
        "a"
    );
    let error = engine
        .invoke_instance(malformed, "nextString", "()Ljava/lang/String;", &[])
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("MalformedJsonException"),
        "unexpected error: {error:?}"
    );

    let wrong_token = json_reader(&mut engine, "[]");
    let error = engine
        .invoke_instance(wrong_token, "beginObject", "()V", &[])
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("IllegalStateException"),
        "unexpected error: {error:?}"
    );
}

fn string_writer(engine: &mut Engine) -> ObjRef {
    let writer = instance(engine, STRING_WRITER);
    engine
        .invoke_instance(writer, "<init>", "()V", &[])
        .unwrap();
    writer
}

fn json_writer(engine: &mut Engine, out: ObjRef) -> ObjRef {
    let writer = instance(engine, JSON_WRITER);
    engine
        .invoke_instance(writer, "<init>", "(Ljava/io/Writer;)V", &[Value::Obj(out)])
        .unwrap();
    writer
}

fn writer_call(
    engine: &mut Engine,
    writer: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Value {
    call(engine, writer, name, sig, args)
}

#[test]
fn json_writer_pretty_prints_html_safe_output_and_skips_null_names() {
    let mut engine = engine_of(&DexBuilder::new());
    let out = string_writer(&mut engine);
    let writer = json_writer(&mut engine, out);
    let indent = text(&mut engine, "  ");
    writer_call(
        &mut engine,
        writer,
        "setIndent",
        "(Ljava/lang/String;)V",
        &[indent],
    );
    writer_call(&mut engine, writer, "setHtmlSafe", "(Z)V", &[Value::Int(1)]);
    writer_call(
        &mut engine,
        writer,
        "setSerializeNulls",
        "(Z)V",
        &[Value::Int(0)],
    );
    assert_eq!(
        writer_call(&mut engine, writer, "getSerializeNulls", "()Z", &[],),
        Value::Int(0)
    );

    assert_eq!(
        writer_call(
            &mut engine,
            writer,
            "beginObject",
            "()Landroid/util/JsonWriter;",
            &[],
        ),
        Value::Obj(writer)
    );
    let name = text(&mut engine, "html");
    writer_call(
        &mut engine,
        writer,
        "name",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[name],
    );
    let value = text(&mut engine, "<a&b='c'>");
    writer_call(
        &mut engine,
        writer,
        "value",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[value],
    );
    let skipped = text(&mut engine, "skip");
    writer_call(
        &mut engine,
        writer,
        "name",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[skipped],
    );
    writer_call(
        &mut engine,
        writer,
        "nullValue",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    let nums = text(&mut engine, "nums");
    writer_call(
        &mut engine,
        writer,
        "name",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[nums],
    );
    writer_call(
        &mut engine,
        writer,
        "beginArray",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    writer_call(
        &mut engine,
        writer,
        "value",
        "(J)Landroid/util/JsonWriter;",
        &[Value::Long(7)],
    );
    writer_call(
        &mut engine,
        writer,
        "value",
        "(D)Landroid/util/JsonWriter;",
        &[Value::Double(2.5)],
    );
    writer_call(
        &mut engine,
        writer,
        "value",
        "(Z)Landroid/util/JsonWriter;",
        &[Value::Int(1)],
    );
    writer_call(
        &mut engine,
        writer,
        "endArray",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    let empty = text(&mut engine, "empty");
    writer_call(
        &mut engine,
        writer,
        "name",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[empty],
    );
    writer_call(
        &mut engine,
        writer,
        "beginObject",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    writer_call(
        &mut engine,
        writer,
        "endObject",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    writer_call(
        &mut engine,
        writer,
        "endObject",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    writer_call(&mut engine, writer, "flush", "()V", &[]);

    let rendered = call_string(&mut engine, out, "toString", "()Ljava/lang/String;");
    assert_eq!(
        rendered,
        "{\n  \"html\": \"\\u003ca\\u0026b\\u003d\\u0027c\\u0027\\u003e\",\n  \"nums\": [\n    7,\n    2.5,\n    true\n  ],\n  \"empty\": {}\n}"
    );
    writer_call(&mut engine, writer, "close", "()V", &[]);
}

#[test]
fn json_writer_boxed_values_and_json_token_enum_statics() {
    let mut engine = engine_of(&DexBuilder::new());
    let out = string_writer(&mut engine);
    let writer = json_writer(&mut engine, out);
    writer_call(
        &mut engine,
        writer,
        "beginArray",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    let integer = engine
        .invoke_static(
            "Ljava/lang/Integer;",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(123)],
        )
        .unwrap();
    writer_call(
        &mut engine,
        writer,
        "value",
        "(Ljava/lang/Number;)Landroid/util/JsonWriter;",
        &[integer],
    );
    let boolean = engine
        .invoke_static(
            "Ljava/lang/Boolean;",
            "valueOf",
            "(Z)Ljava/lang/Boolean;",
            &[Value::Int(1)],
        )
        .unwrap();
    writer_call(
        &mut engine,
        writer,
        "value",
        "(Ljava/lang/Boolean;)Landroid/util/JsonWriter;",
        &[boolean],
    );
    writer_call(
        &mut engine,
        writer,
        "value",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[Value::Null],
    );
    writer_call(
        &mut engine,
        writer,
        "endArray",
        "()Landroid/util/JsonWriter;",
        &[],
    );
    writer_call(&mut engine, writer, "close", "()V", &[]);
    assert_eq!(
        call_string(&mut engine, out, "toString", "()Ljava/lang/String;"),
        "[123,true,null]"
    );

    let values = engine
        .invoke_static(JSON_TOKEN, "values", "()[Landroid/util/JsonToken;", &[])
        .unwrap();
    let values_ref = object(values);
    let elems = engine.heap.as_array_elems(values_ref).unwrap().0;
    assert_eq!(elems.len(), 10);
    assert_eq!(token_name(&engine, &elems[0]), "BEGIN_ARRAY");
    assert_eq!(token_name(&engine, &elems[9]), "END_DOCUMENT");

    let wanted = text(&mut engine, "STRING");
    let string_token = engine
        .invoke_static(
            JSON_TOKEN,
            "valueOf",
            "(Ljava/lang/String;)Landroid/util/JsonToken;",
            &[wanted],
        )
        .unwrap();
    let string_token_ref = object(string_token.clone());
    assert_eq!(token_name(&engine, &string_token), "STRING");
    assert_eq!(
        engine
            .invoke_instance(string_token_ref, "ordinal", "()I", &[])
            .unwrap(),
        Value::Int(5)
    );
}
