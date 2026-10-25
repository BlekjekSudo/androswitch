//! Regression tests for issue #274 (`android.util.JsonReader`):
//! 1. Calling `hashCode()`, `equals()`, `getClass()`, or custom subclass methods
//!    before parsing must NOT drain and discard the underlying `java.io.Reader`.
//! 2. `READER_TEXT` must be loaded once and never re-allocated or cloned on each
//!    token (`O(1)` per token instead of `O(N * T)`).
//! 3. `drain_reader` must preserve UTF-16 surrogate pairs split across `read()`
//!    chunk boundaries (`4096` chars or `1` char per `read()`), reuse a single
//!    `[C` buffer, and enforce `vm.heap` budget on unbounded streams.

#[macro_use]
mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const STRING_READER: &str = "Ljava/io/StringReader;";
const JSON_READER: &str = "Landroid/util/JsonReader;";
const ACC_CONSTRUCTOR: u32 = 0x1_0000;

fn instance(engine: &mut Engine, class: &str) -> ObjRef {
    engine
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc instance")
}

fn text(engine: &mut Engine, value: &str) -> Value {
    Value::Obj(
        engine
            .heap
            .alloc_string(value.to_string())
            .expect("alloc string"),
    )
}

fn text_of(engine: &Engine, value: &Value) -> String {
    let Value::Obj(reference) = value else {
        panic!("expected String, got {value:?}");
    };
    engine.heap.as_str(*reference).expect("as_str").to_string()
}

fn json_reader_from_str(engine: &mut Engine, input: &str) -> ObjRef {
    let source = instance(engine, STRING_READER);
    let s = text(engine, input);
    engine
        .invoke_instance(source, "<init>", "(Ljava/lang/String;)V", &[s])
        .expect("StringReader.<init>");
    let reader = instance(engine, JSON_READER);
    engine
        .invoke_instance(
            reader,
            "<init>",
            "(Ljava/io/Reader;)V",
            &[Value::Obj(source)],
        )
        .expect("JsonReader.<init>");
    reader
}

#[test]
fn hash_code_equals_and_subclass_methods_before_parse_preserve_stream() {
    let mut b = DexBuilder::new();
    let p_reader_v = b.proto_idx("V", vec!["Ljava/io/Reader;".to_string()]);
    let super_init = b.method_idx(JSON_READER, p_reader_v, "<init>");

    let sub = b.class("LMyJsonReader;", JSON_READER);
    b.direct(
        sub,
        "<init>",
        "V",
        vec!["Ljava/io/Reader;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 2, {
            let mut u = op35c(0x70, 2, super_init, [0, 1, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.r#virtual(
        sub,
        "customTag",
        "I",
        vec![],
        ACC_PUBLIC,
        Some(b.code(2, 1, 0, {
            let mut u = op11n(0x12, 0, 7);
            u.extend(op11x(0x0F, 0));
            u
        })),
    );

    let mut engine = engine_of(&b);
    let source = instance(&mut engine, STRING_READER);
    let payload = text(&mut engine, r#"{"status":"intact","code":200}"#);
    engine
        .invoke_instance(source, "<init>", "(Ljava/lang/String;)V", &[payload])
        .expect("StringReader.<init>");

    let reader = instance(&mut engine, "LMyJsonReader;");
    engine
        .invoke_instance(
            reader,
            "<init>",
            "(Ljava/io/Reader;)V",
            &[Value::Obj(source)],
        )
        .expect("MyJsonReader.<init>");

    let hc = engine
        .invoke_instance(reader, "hashCode", "()I", &[])
        .expect("hashCode");
    assert!(matches!(hc, Value::Int(_)));

    let eq = engine
        .invoke_instance(
            reader,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(reader)],
        )
        .expect("equals");
    assert_eq!(eq, Value::Int(1));

    let tag = engine
        .invoke_instance(reader, "customTag", "()I", &[])
        .expect("customTag");
    assert_eq!(tag, Value::Int(7));

    assert_eq!(
        engine.heap.get_field(reader, "rd_json_text"),
        Ok(Value::Null),
        "non-parsing methods must not drain the underlying Reader"
    );

    engine
        .invoke_instance(reader, "beginObject", "()V", &[])
        .expect("beginObject must succeed after hashCode/equals/customTag");
    let k1 = engine
        .invoke_instance(reader, "nextName", "()Ljava/lang/String;", &[])
        .expect("nextName 1");
    assert_eq!(text_of(&engine, &k1), "status");
    let v1 = engine
        .invoke_instance(reader, "nextString", "()Ljava/lang/String;", &[])
        .expect("nextString 1");
    assert_eq!(text_of(&engine, &v1), "intact");
    let k2 = engine
        .invoke_instance(reader, "nextName", "()Ljava/lang/String;", &[])
        .expect("nextName 2");
    assert_eq!(text_of(&engine, &k2), "code");
    let v2 = engine
        .invoke_instance(reader, "nextInt", "()I", &[])
        .expect("nextInt 2");
    assert_eq!(v2, Value::Int(200));
    engine
        .invoke_instance(reader, "endObject", "()V", &[])
        .expect("endObject");
}

#[test]
fn reader_text_allocated_once_and_large_document_streams_under_tight_heap() {
    let mut json = String::with_capacity(170_000);
    json.push('[');
    for i in 0..4_000 {
        if i > 0 {
            json.push(',');
        }
        json.push_str("                                  ");
        json.push_str(&i.to_string());
    }
    json.push(']');
    assert!(json.len() > 140_000);

    let dex = rd_dex::Dex::parse(DexBuilder::new().finish()).expect("dex");
    let cfg = VmConfig {
        heap_budget: 4 * 1024 * 1024, // 4 MiB tight heap budget
        ..VmConfig::default()
    };
    let mut engine = Engine::new(vec![dex], cfg);
    let reader = json_reader_from_str(&mut engine, &json);

    engine
        .invoke_instance(reader, "beginArray", "()V", &[])
        .expect("beginArray");
    let first_text_field = engine
        .heap
        .get_field(reader, "rd_json_text")
        .expect("rd_json_text loaded");
    assert!(matches!(first_text_field, Value::Obj(_)));

    let mut sum: i64 = 0;
    for expected in 0..4_000 {
        let has_next = engine
            .invoke_instance(reader, "hasNext", "()Z", &[])
            .expect("hasNext");
        assert_eq!(has_next, Value::Int(1));
        let item = engine
            .invoke_instance(reader, "nextInt", "()I", &[])
            .expect("nextInt");
        assert_eq!(item, Value::Int(expected));
        sum += i64::from(expected);

        if expected % 500 == 0 || expected == 3_999 {
            assert_eq!(
                engine.heap.get_field(reader, "rd_json_text"),
                Ok(first_text_field.clone()),
                "READER_TEXT ObjRef must never be reallocated across tokens"
            );
        }
    }
    assert_eq!(sum, 3_999 * 4_000 / 2);
    assert_eq!(
        engine
            .invoke_instance(reader, "hasNext", "()Z", &[])
            .expect("hasNext end"),
        Value::Int(0)
    );
    engine
        .invoke_instance(reader, "endArray", "()V", &[])
        .expect("endArray");
}

#[test]
fn utf16_surrogate_pairs_across_4096_boundary_and_1_char_reads_are_preserved() {
    let mut expected_val = String::new();
    expected_val.push_str(&"a".repeat(4089));
    expected_val.push('🦀');
    expected_val.push_str(&"b".repeat(4094));
    expected_val.push('🚀');
    expected_val.push_str("tail");

    let doc = format!(r#"{{"v":"{expected_val}"}}"#);
    let utf16_units: Vec<u16> = doc.encode_utf16().collect();
    assert_eq!(utf16_units[4095], 0xD83E, "high surrogate at 4095");
    assert_eq!(utf16_units[4096], 0xDD80, "low surrogate at 4096");
    assert_eq!(utf16_units[8191], 0xD83D, "high surrogate at 8191");
    assert_eq!(utf16_units[8192], 0xDE80, "low surrogate at 8192");

    let mut engine = engine_of(&DexBuilder::new());
    let reader = json_reader_from_str(&mut engine, &doc);
    engine
        .invoke_instance(reader, "beginObject", "()V", &[])
        .expect("beginObject");
    let name_val = engine
        .invoke_instance(reader, "nextName", "()Ljava/lang/String;", &[])
        .expect("nextName");
    assert_eq!(text_of(&engine, &name_val), "v");
    let str_val = engine
        .invoke_instance(reader, "nextString", "()Ljava/lang/String;", &[])
        .expect("nextString");
    let actual_val = text_of(&engine, &str_val);
    assert_eq!(actual_val, expected_val);
    assert!(!actual_val.contains('\u{FFFD}'));
    engine
        .invoke_instance(reader, "endObject", "()V", &[])
        .expect("endObject");

    // Custom DEX Reader subclass (extending java.io.Reader directly) returning
    // 1 UTF-16 code unit per read([C, off, len) via String.charAt(idx) and
    // asserting drain_reader reuses a single [C scratch array across all calls.
    let mut b = DexBuilder::new();
    let p_v = b.proto_idx("V", vec![]);
    let p_len = b.proto_idx("I", vec![]);
    let p_char_at = b.proto_idx("C", vec!["I".to_string()]);
    let reader_init = b.method_idx("Ljava/io/Reader;", p_v, "<init>");
    let str_len = b.method_idx("Ljava/lang/String;", p_len, "length");
    let str_char_at = b.method_idx("Ljava/lang/String;", p_char_at, "charAt");

    let one_char_reader = b.class("LOneUnitReader;", "Ljava/io/Reader;");
    b.instance_field(one_char_reader, "src", "Ljava/lang/String;");
    b.instance_field(one_char_reader, "idx", "I");
    b.instance_field(one_char_reader, "firstBuf", "[C");
    b.instance_field(one_char_reader, "sameBufCount", "I");
    b.instance_field(one_char_reader, "calls", "I");
    let f_src = b.field_idx("LOneUnitReader;", "Ljava/lang/String;", "src");
    let f_idx = b.field_idx("LOneUnitReader;", "I", "idx");
    let f_first_buf = b.field_idx("LOneUnitReader;", "[C", "firstBuf");
    let f_same_buf = b.field_idx("LOneUnitReader;", "I", "sameBufCount");
    let f_calls = b.field_idx("LOneUnitReader;", "I", "calls");

    b.direct(
        one_char_reader,
        "<init>",
        "V",
        vec!["Ljava/lang/String;"],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(2, 2, 1, {
            let mut u = op35c(0x70, 1, reader_init, [0, 0, 0, 0, 0]);
            u.extend(op22c(0x5B, 1, 0, f_src));
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.r#virtual(
        one_char_reader,
        "read",
        "I",
        vec!["[C", "I", "I"],
        ACC_PUBLIC,
        Some(b.code(7, 4, 2, {
            // regs=7, ins=4 -> this=v3, buf=v4, off=v5, len=v6; locals v0, v1, v2
            let mut u = Vec::new();
            // calls++
            u.extend(op22c(0x52, 0, 3, f_calls));
            u.extend(op22b(0xD8, 0, 0, 1));
            u.extend(op22c(0x59, 0, 3, f_calls));
            // if (firstBuf == null) firstBuf = buf
            u.extend(op22c(0x54, 0, 3, f_first_buf));
            u.extend(op21s(0x39, 0, 4)); // if-nez v0, +4
            u.extend(op22c(0x5B, 4, 3, f_first_buf));
            // if (firstBuf == buf) sameBufCount++
            u.extend(op22c(0x54, 0, 3, f_first_buf));
            u.extend(op22t(0x33, 0, 4, 8)); // if-ne v0, v4, +8
            u.extend(op22c(0x52, 1, 3, f_same_buf));
            u.extend(op22b(0xD8, 1, 1, 1));
            u.extend(op22c(0x59, 1, 3, f_same_buf));
            // v0 = this.src; v1 = v0.length(); v2 = this.idx; if (v2 >= v1) return -1
            u.extend(op22c(0x54, 0, 3, f_src));
            u.extend(op35c(0x6E, 1, str_len, [0, 0, 0, 0, 0]));
            u.extend(op11x(0x0A, 1));
            u.extend(op22c(0x52, 2, 3, f_idx));
            u.extend(op22t(0x34, 2, 1, 4)); // if-lt v2, v1, +4 (2 + 1 + 1 words)
            u.extend(op11n(0x12, 0, -1)); // const/4 v0, -1 (1 word)
            u.extend(op11x(0x0F, 0)); // return v0 (1 word)
                                      // v1 = v0.charAt(v2); buf[off] = v1; this.idx = v2 + 1; return 1
            u.extend(op35c(0x6E, 2, str_char_at, [0, 2, 0, 0, 0]));
            u.extend(op11x(0x0A, 1));
            u.extend(op23x(0x50, 1, 4, 5)); // aput-char v1, v4, v5
            u.extend(op22b(0xD8, 2, 2, 1));
            u.extend(op22c(0x59, 2, 3, f_idx));
            u.extend(op11n(0x12, 0, 1));
            u.extend(op11x(0x0F, 0));
            u
        })),
    );
    b.r#virtual(
        one_char_reader,
        "close",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0E))),
    );

    let mut e2 = engine_of(&b);
    let mini_json = r#"["😀🦀🧪"]"#;
    let units_len = mini_json.encode_utf16().count();
    let mini_str = text(&mut e2, mini_json);
    let one_reader = instance(&mut e2, "LOneUnitReader;");
    e2.invoke_instance(one_reader, "<init>", "(Ljava/lang/String;)V", &[mini_str])
        .expect("OneUnitReader.<init>");

    let jr = instance(&mut e2, JSON_READER);
    e2.invoke_instance(
        jr,
        "<init>",
        "(Ljava/io/Reader;)V",
        &[Value::Obj(one_reader)],
    )
    .expect("JsonReader.<init>");

    e2.invoke_instance(jr, "beginArray", "()V", &[])
        .expect("beginArray");
    let emoji_val = e2
        .invoke_instance(jr, "nextString", "()Ljava/lang/String;", &[])
        .expect("nextString");
    let parsed_emoji = text_of(&e2, &emoji_val);
    assert_eq!(parsed_emoji, "😀🦀🧪");
    e2.invoke_instance(jr, "endArray", "()V", &[])
        .expect("endArray");

    let total_calls = e2.heap.get_field(one_reader, "calls").expect("calls");
    let same_buf = e2
        .heap
        .get_field(one_reader, "sameBufCount")
        .expect("sameBufCount");
    assert_eq!(
        total_calls,
        Value::Int((units_len + 1) as i32),
        "1 unit per call + 1 EOF call"
    );
    assert_eq!(
        same_buf, total_calls,
        "drain_reader must reuse the exact same [C buffer across all read() calls"
    );
    assert_eq!(
        e2.heap.get_field(jr, "rd_json_buf"),
        Ok(Value::Null),
        "temporary scratch buffer reference must be cleared after draining"
    );
}

#[test]
fn unbounded_reader_hits_capturable_oom_within_heap_budget() {
    let mut b = DexBuilder::new();
    let p_v = b.proto_idx("V", vec![]);
    let reader_init = b.method_idx("Ljava/io/Reader;", p_v, "<init>");
    let inf = b.class("LInfiniteJsonReader;", "Ljava/io/Reader;");
    b.direct(
        inf,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC | ACC_CONSTRUCTOR,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, reader_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.r#virtual(
        inf,
        "read",
        "I",
        vec!["[C", "I", "I"],
        ACC_PUBLIC,
        Some(b.code(4, 4, 0, op11x(0x0F, 3))),
    );
    b.r#virtual(
        inf,
        "close",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 0, op10x(0x0E))),
    );

    let dex = rd_dex::Dex::parse(b.finish()).expect("dex");
    let cfg = VmConfig {
        heap_budget: 512 * 1024, // 512 KiB
        ..VmConfig::default()
    };
    let mut engine = Engine::new(vec![dex], cfg);
    let inf_reader = instance(&mut engine, "LInfiniteJsonReader;");
    engine
        .invoke_instance(inf_reader, "<init>", "()V", &[])
        .expect("InfiniteJsonReader.<init>");
    let jr = instance(&mut engine, JSON_READER);
    engine
        .invoke_instance(
            jr,
            "<init>",
            "(Ljava/io/Reader;)V",
            &[Value::Obj(inf_reader)],
        )
        .expect("JsonReader.<init>");

    let err = engine
        .invoke_instance(jr, "peek", "()Landroid/util/JsonToken;", &[])
        .expect_err("infinite Reader must abort with OutOfMemoryError");
    match err {
        VmExit::Exception(t) => assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;"),
        other => panic!("expected OutOfMemoryError, got {other:?}"),
    }
    assert_eq!(engine.heap.get_field(jr, "rd_json_buf"), Ok(Value::Null));
}
