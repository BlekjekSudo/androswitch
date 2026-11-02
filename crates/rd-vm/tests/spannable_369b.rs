//! Regression tests for issue #369 item 3 (`crates/rd-vm/src/android_spannable.rs`):
//! `getChars` throws `NullPointerException` for a null `dest` and
//! `IndexOutOfBoundsException` for a negative `destoff` or when the copy does
//! not fit, instead of clamping or silently dropping characters.

use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SSB: &str = "Landroid/text/SpannableStringBuilder;";
const GET_CHARS: &str = "(II[CI)V";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn ssb(e: &mut Engine, text: &str) -> ObjRef {
    let sb = e.new_instance(SSB).unwrap();
    let t = Value::Obj(e.heap.alloc_string(text.to_string()).unwrap());
    e.invoke_instance(sb, "<init>", "(Ljava/lang/CharSequence;)V", &[t])
        .unwrap();
    sb
}

fn chars(e: &mut Engine, len: usize) -> ObjRef {
    e.heap
        .alloc_array_zeroed(ElemKind::Int, len, "[C")
        .expect("alloc [C")
}

fn exception_class(res: Result<Value, VmExit>) -> String {
    match res {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("expected an exception, got {other:?}"),
    }
}

#[test]
fn issue_369_get_chars_copies_into_destination() {
    let mut e = engine();
    let sb = ssb(&mut e, "hello");
    let dst = chars(&mut e, 5);
    e.invoke_instance(
        sb,
        "getChars",
        GET_CHARS,
        &[Value::Int(1), Value::Int(4), Value::Obj(dst), Value::Int(1)],
    )
    .expect("valid getChars");
    let (elems, _) = e.heap.as_array_elems(dst).unwrap();
    assert_eq!(elems[1], Value::Int('e' as i32));
    assert_eq!(elems[3], Value::Int('l' as i32));
    assert_eq!(elems[4], Value::Int(0));
}

#[test]
fn issue_369_get_chars_null_dest_is_npe() {
    let mut e = engine();
    let sb = ssb(&mut e, "hello");
    let cls = exception_class(e.invoke_instance(
        sb,
        "getChars",
        GET_CHARS,
        &[Value::Int(0), Value::Int(2), Value::Null, Value::Int(0)],
    ));
    assert_eq!(cls, "Ljava/lang/NullPointerException;");
}

#[test]
fn issue_369_get_chars_negative_destoff_is_ioobe() {
    let mut e = engine();
    let sb = ssb(&mut e, "hello");
    let dst = chars(&mut e, 5);
    let cls = exception_class(e.invoke_instance(
        sb,
        "getChars",
        GET_CHARS,
        &[
            Value::Int(0),
            Value::Int(2),
            Value::Obj(dst),
            Value::Int(-1),
        ],
    ));
    assert_eq!(cls, "Ljava/lang/IndexOutOfBoundsException;");
}

#[test]
fn issue_369_get_chars_overflowing_destination_is_ioobe_not_truncated() {
    let mut e = engine();
    let sb = ssb(&mut e, "hello");
    let dst = chars(&mut e, 3);
    let cls = exception_class(e.invoke_instance(
        sb,
        "getChars",
        GET_CHARS,
        &[Value::Int(0), Value::Int(4), Value::Obj(dst), Value::Int(1)],
    ));
    assert_eq!(cls, "Ljava/lang/IndexOutOfBoundsException;");
    // Nothing was written before the exception.
    let (elems, _) = e.heap.as_array_elems(dst).unwrap();
    assert!(elems.iter().all(|v| *v == Value::Int(0)));
}
