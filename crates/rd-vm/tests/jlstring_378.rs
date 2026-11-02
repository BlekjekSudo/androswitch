//! Regression tests for issue #378 (`crates/rd-vm/src/jlstring.rs`):
//! 1. `String.replace` with a non-empty target budgets the result before building it.
//! 2. `getBytes((String) null)` throws `NullPointerException`; an unknown charset
//!    name throws `UnsupportedEncodingException` (a missing argument keeps UTF-8).
//! 3. `String.format("%h", arg)` formats `arg.hashCode()`, not a heap slot id.

use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const STRING: &str = "Ljava/lang/String;";

fn engine(heap_budget: usize) -> Engine {
    Engine::new(
        Vec::new(),
        VmConfig {
            heap_budget,
            ..VmConfig::default()
        },
    )
}

fn str_val(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.to_string()).unwrap())
}

fn obj(v: Value) -> u32 {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

fn exception_class(res: Result<Value, VmExit>) -> String {
    match res {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("expected an exception, got {other:?}"),
    }
}

#[test]
fn issue_378_getbytes_null_charset_name_throws_npe() {
    let mut e = engine(64 * 1024 * 1024);
    let text = obj(str_val(&mut e, "abc"));
    let res = e.invoke_instance(text, "getBytes", "(Ljava/lang/String;)[B", &[Value::Null]);
    assert_eq!(exception_class(res), "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_378_getbytes_unknown_charset_name_throws_unsupported_encoding() {
    let mut e = engine(64 * 1024 * 1024);
    let text = obj(str_val(&mut e, "abc"));
    let name = str_val(&mut e, "NO_SUCH_CHARSET");
    let res = e.invoke_instance(text, "getBytes", "(Ljava/lang/String;)[B", &[name]);
    assert_eq!(
        exception_class(res),
        "Ljava/io/UnsupportedEncodingException;"
    );
}

#[test]
fn issue_378_getbytes_known_charset_and_default_still_work() {
    let mut e = engine(64 * 1024 * 1024);
    let text = obj(str_val(&mut e, "abc"));
    let name = str_val(&mut e, "utf-8");
    let arr = obj(e
        .invoke_instance(text, "getBytes", "(Ljava/lang/String;)[B", &[name])
        .expect("utf-8 is supported"));
    assert_eq!(e.heap.as_array_elems(arr).unwrap().0.len(), 3);
    let arr2 = obj(e
        .invoke_instance(text, "getBytes", "()[B", &[])
        .expect("default charset"));
    assert_eq!(e.heap.as_array_elems(arr2).unwrap().0.len(), 3);
}

#[test]
fn issue_378_format_percent_h_uses_hash_code() {
    let mut e = engine(64 * 1024 * 1024);
    let fmt = str_val(&mut e, "%h");
    let arg = str_val(&mut e, "abc");
    let args = e
        .heap
        .alloc_array(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .unwrap();
    *e.heap.array_elems_mut(args).unwrap() = vec![arg];
    let out = e
        .invoke_static(
            STRING,
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[fmt, Value::Obj(args)],
        )
        .unwrap();
    // "abc".hashCode() == 96354 == 0x17862
    assert_eq!(e.heap.as_str(obj(out)).unwrap(), "17862");
}

#[test]
fn issue_378_replace_with_non_empty_target_is_budgeted_before_building() {
    // 2 MiB heap: 100k matches of a 100k-char replacement is ~10 GB of result.
    let mut e = engine(2 * 1024 * 1024);
    let text = obj(str_val(&mut e, &"a".repeat(100_000)));
    let target = str_val(&mut e, "a");
    let repl = str_val(&mut e, &"b".repeat(100_000));
    let res = e.invoke_instance(
        text,
        "replace",
        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;",
        &[target, repl],
    );
    assert_eq!(exception_class(res), "Ljava/lang/OutOfMemoryError;");
}
