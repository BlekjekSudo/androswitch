//! Regression test for issue #364 item 2 (`crates/rd-vm/src/android_json.rs`):
//! `JsonWriter` escapes a string value on the host (up to 6 bytes per char).
//! That expansion must be checked against `heap_budget` before it happens.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const STRING_WRITER: &str = "Ljava/io/StringWriter;";
const JSON_WRITER: &str = "Landroid/util/JsonWriter;";

fn instance(e: &mut Engine, class: &str) -> ObjRef {
    e.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc")
}

#[test]
fn issue_364_json_string_expansion_is_checked_against_heap_budget() {
    let mut e = Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget: 64 * 1024,
            ..VmConfig::default()
        },
    );
    let sw = instance(&mut e, STRING_WRITER);
    e.invoke_instance(sw, "<init>", "()V", &[])
        .expect("sw init");
    let w = instance(&mut e, JSON_WRITER);
    e.invoke_instance(w, "<init>", "(Ljava/io/Writer;)V", &[Value::Obj(sw)])
        .expect("JsonWriter.<init>");
    // 10 000 control characters: 60 000 bytes of escapes, past the 64 KiB budget.
    let big = e
        .heap
        .alloc_string("\u{1}".repeat(10_000))
        .expect("alloc_string");
    let res = e.invoke_instance(
        w,
        "value",
        "(Ljava/lang/String;)Landroid/util/JsonWriter;",
        &[Value::Obj(big)],
    );
    match res {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/OutOfMemoryError;", "wrong exception");
            // The message comes from the check before expansion, not from the sink.
            assert!(
                t.message
                    .as_deref()
                    .is_some_and(|m| m.starts_with("JSON escaping")),
                "OOM must come from the pre-expansion check, got {:?}",
                t.message
            );
        }
        other => panic!("escaping past the heap budget must throw OOM, got {other:?}"),
    }
}
