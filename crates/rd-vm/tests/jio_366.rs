//! Regression tests for issue #366 (`crates/rd-vm/src/jio_chars.rs`), JDK/AOSP
//! contracts: `ByteArrayInputStream(buf, off, len)` clamps `len` to `buf`;
//! `ByteArrayOutputStream(-1)` throws `IllegalArgumentException`;
//! `StringWriter.close()` has no effect; `PrintWriter` writes after `close()`
//! set `checkError()` instead of throwing.

use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const SW: &str = "Ljava/io/StringWriter;";
const PW: &str = "Ljava/io/PrintWriter;";
const BAOS: &str = "Ljava/io/ByteArrayOutputStream;";
const BAIS: &str = "Ljava/io/ByteArrayInputStream;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn alloc_obj(vm: &mut Engine, class: &str) -> ObjRef {
    vm.heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc_instance")
}

fn alloc_jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).expect("alloc_string"))
}

fn alloc_bytes(vm: &mut Engine, data: &[u8]) -> ObjRef {
    let arr = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, data.len(), "[B")
        .expect("alloc [B");
    let elems = vm.heap.array_elems_mut(arr).expect("elems");
    for (slot, b) in elems.iter_mut().zip(data) {
        *slot = Value::Int(i32::from(*b as i8));
    }
    arr
}

#[test]
fn issue_366_bais_clamps_length_to_buffer() {
    let mut e = engine();
    let buf = alloc_bytes(&mut e, &[1, 2, 3, 4]);
    let s = alloc_obj(&mut e, BAIS);
    let res = e.invoke_instance(
        s,
        "<init>",
        "([BII)V",
        &[Value::Obj(buf), Value::Int(2), Value::Int(10)],
    );
    assert!(
        res.is_ok(),
        "ByteArrayInputStream(buf, 2, 10) must clamp, got {res:?}"
    );
    let first = e.invoke_instance(s, "read", "()I", &[]).expect("read");
    assert_eq!(first, Value::Int(3));
}

#[test]
fn issue_366_baos_negative_size_is_illegal_argument() {
    let mut e = engine();
    let s = alloc_obj(&mut e, BAOS);
    match e.invoke_instance(s, "<init>", "(I)V", &[Value::Int(-1)]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/IllegalArgumentException;"),
        other => panic!("expected IllegalArgumentException, got {other:?}"),
    }
}

#[test]
fn issue_366_stringwriter_close_has_no_effect() {
    let mut e = engine();
    let w = alloc_obj(&mut e, SW);
    e.invoke_instance(w, "<init>", "()V", &[]).expect("init");
    e.invoke_instance(w, "close", "()V", &[]).expect("close");
    let s = alloc_jstr(&mut e, "x");
    e.invoke_instance(w, "write", "(Ljava/lang/String;)V", &[s])
        .expect("write after close must not throw for StringWriter");
    let out = e
        .invoke_instance(w, "toString", "()Ljava/lang/String;", &[])
        .expect("toString");
    let Value::Obj(r) = out else {
        panic!("expected String")
    };
    assert_eq!(e.heap.as_str(r).unwrap(), "x");
}

#[test]
fn issue_366_printwriter_after_close_sets_check_error_instead_of_throwing() {
    let mut e = engine();
    let sink = alloc_obj(&mut e, SW);
    e.invoke_instance(sink, "<init>", "()V", &[])
        .expect("sink init");
    let pw = alloc_obj(&mut e, PW);
    e.invoke_instance(pw, "<init>", "(Ljava/io/Writer;)V", &[Value::Obj(sink)])
        .expect("pw init");
    e.invoke_instance(pw, "close", "()V", &[]).expect("close");
    let s = alloc_jstr(&mut e, "y");
    let res = e.invoke_instance(pw, "print", "(Ljava/lang/String;)V", &[s]);
    assert!(
        res.is_ok(),
        "PrintWriter.print after close must not throw, got {res:?}"
    );
    let err = e
        .invoke_instance(pw, "checkError", "()Z", &[])
        .expect("checkError");
    assert_eq!(err, Value::Int(1));
}
