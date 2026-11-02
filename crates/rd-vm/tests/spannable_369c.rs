//! Regression tests for issue #369 item 4 (`crates/rd-vm/src/android_spannable.rs`):
//! `insert(where, tb, start, end)` and `replace(..., tb, start, end)` take only
//! `tb[start, end)`, keeping the spans that touch it; constructors reject `null`.

use rd_vm::err::VmExit;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SSB: &str = "Landroid/text/SpannableStringBuilder;";
const SS: &str = "Landroid/text/SpannableString;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn jstr(e: &mut Engine, s: &str) -> Value {
    Value::Obj(e.heap.alloc_string(s.to_string()).unwrap())
}

fn text(e: &mut Engine, sb: ObjRef) -> String {
    let out = e
        .invoke_instance(sb, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    match out {
        Value::Obj(r) => e.heap.as_str(r).unwrap().to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}

fn ssb(e: &mut Engine, s: &str) -> ObjRef {
    let sb = e.new_instance(SSB).unwrap();
    let t = jstr(e, s);
    e.invoke_instance(sb, "<init>", "(Ljava/lang/CharSequence;)V", &[t])
        .unwrap();
    sb
}

#[test]
fn issue_369_insert_subrange_takes_only_start_to_end() {
    let mut e = engine();
    let sb = ssb(&mut e, "XY");
    let tb = jstr(&mut e, "abcdef");
    e.invoke_instance(
        sb,
        "insert",
        "(ILjava/lang/CharSequence;II)Landroid/text/SpannableStringBuilder;",
        &[Value::Int(1), tb, Value::Int(2), Value::Int(4)],
    )
    .expect("insert subrange");
    assert_eq!(text(&mut e, sb), "XcdY");
}

#[test]
fn issue_369_replace_subrange_takes_only_start_to_end_and_keeps_spans() {
    let mut e = engine();
    let sb = ssb(&mut e, "0123456789");
    let tb = ssb(&mut e, "abcdef");
    let what = e
        .heap
        .alloc_instance("Ljava/lang/Object;".to_string(), Vec::new())
        .unwrap();
    // Span sobre "bc" = tb[1, 3).
    e.invoke_instance(
        tb,
        "setSpan",
        "(Ljava/lang/Object;III)V",
        &[
            Value::Obj(what),
            Value::Int(1),
            Value::Int(3),
            Value::Int(0),
        ],
    )
    .expect("setSpan");
    e.invoke_instance(
        sb,
        "replace",
        "(IILjava/lang/CharSequence;II)Landroid/text/SpannableStringBuilder;",
        &[
            Value::Int(0),
            Value::Int(3),
            Value::Obj(tb),
            Value::Int(1),
            Value::Int(3),
        ],
    )
    .expect("replace subrange");
    assert_eq!(text(&mut e, sb), "bc3456789");
    let start = e
        .invoke_instance(
            sb,
            "getSpanStart",
            "(Ljava/lang/Object;)I",
            &[Value::Obj(what)],
        )
        .expect("getSpanStart");
    let end = e
        .invoke_instance(
            sb,
            "getSpanEnd",
            "(Ljava/lang/Object;)I",
            &[Value::Obj(what)],
        )
        .expect("getSpanEnd");
    assert_eq!((start, end), (Value::Int(0), Value::Int(2)));
}

#[test]
fn issue_369_constructors_reject_null_text() {
    let mut e = engine();
    let sb = e.new_instance(SSB).unwrap();
    let res = e.invoke_instance(sb, "<init>", "(Ljava/lang/CharSequence;)V", &[Value::Null]);
    assert!(
        matches!(&res, Err(VmExit::Exception(t)) if t.class == "Ljava/lang/NullPointerException;"),
        "SpannableStringBuilder(null) must throw NPE, got {res:?}"
    );
    let ss = e.new_instance(SS).unwrap();
    let res = e.invoke_instance(ss, "<init>", "(Ljava/lang/CharSequence;)V", &[Value::Null]);
    assert!(
        matches!(&res, Err(VmExit::Exception(t)) if t.class == "Ljava/lang/NullPointerException;"),
        "SpannableString(null) must throw NPE, got {res:?}"
    );
}
