//! Regression tests for issue #376 item 2 (`crates/rd-vm/src/jsb.rs`):
//! `replace`, `indexOf`, `lastIndexOf` and `compareTo` throw
//! `NullPointerException` on a null argument. `append(null)` keeps "null".

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SB: &str = "Ljava/lang/StringBuilder;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn builder_with(e: &mut Engine, text: &str) -> ObjRef {
    let sb = e.new_instance(SB).unwrap();
    let s = Value::Obj(e.heap.alloc_string(text.to_string()).unwrap());
    e.invoke_instance(
        sb,
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        &[s],
    )
    .unwrap();
    sb
}

fn exception_class(res: Result<Value, VmExit>) -> String {
    match res {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("expected an exception, got {other:?}"),
    }
}

#[test]
fn issue_376_replace_with_null_throws_npe() {
    let mut e = engine();
    let sb = builder_with(&mut e, "abc");
    let res = e.invoke_instance(
        sb,
        "replace",
        "(IILjava/lang/String;)Ljava/lang/StringBuilder;",
        &[Value::Int(0), Value::Int(1), Value::Null],
    );
    assert_eq!(exception_class(res), "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_376_index_of_null_throws_npe() {
    let mut e = engine();
    let sb = builder_with(&mut e, "abc");
    let res = e.invoke_instance(sb, "indexOf", "(Ljava/lang/String;)I", &[Value::Null]);
    assert_eq!(exception_class(res), "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_376_compare_to_null_throws_npe() {
    let mut e = engine();
    let sb = builder_with(&mut e, "abc");
    let res = e.invoke_instance(
        sb,
        "compareTo",
        "(Ljava/lang/StringBuilder;)I",
        &[Value::Null],
    );
    assert_eq!(exception_class(res), "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_376_append_null_still_appends_the_text_null() {
    let mut e = engine();
    let sb = e.new_instance(SB).unwrap();
    e.invoke_instance(
        sb,
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        &[Value::Null],
    )
    .unwrap();
    let out = e
        .invoke_instance(sb, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    let Value::Obj(r) = out else {
        panic!("expected String");
    };
    assert_eq!(e.heap.as_str(r).unwrap(), "null");
}
