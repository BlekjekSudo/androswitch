//! Regression tests for issue #379 (`crates/rd-vm/src/arrays.rs`):
//! `Arrays.copyOf` / `copyOfRange` dropped one dimension of a nested array
//! (`[[I` came back as `[I`). The copy must keep the full array class.

use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig};

const ARRAYS: &str = "Ljava/util/Arrays;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn obj(v: Value) -> ObjRef {
    match v {
        Value::Obj(r) => r,
        other => panic!("expected Obj, got {other:?}"),
    }
}

/// `int[][]` with two inner `int[]`s of length 1 and 2.
fn int_matrix(e: &mut Engine) -> ObjRef {
    let outer = e.heap.alloc_array_exact(ElemKind::Obj, 2, "[[I").unwrap();
    let row0 = e.heap.alloc_array_exact(ElemKind::Int, 1, "[I").unwrap();
    let row1 = e.heap.alloc_array_exact(ElemKind::Int, 2, "[I").unwrap();
    *e.heap.array_elems_mut(outer).unwrap() = vec![Value::Obj(row0), Value::Obj(row1)];
    outer
}

#[test]
fn issue_379_copy_of_nested_int_array_keeps_both_dimensions() {
    let mut e = engine();
    let m = int_matrix(&mut e);
    let out = obj(e
        .invoke_static(
            ARRAYS,
            "copyOf",
            "([Ljava/lang/Object;I)[Ljava/lang/Object;",
            &[Value::Obj(m), Value::Int(3)],
        )
        .unwrap());
    assert_eq!(e.heap.class_of(out), Ok("[[I"));
    assert_eq!(e.heap.as_array_elems(out).unwrap().0.len(), 3);
}

#[test]
fn issue_379_copy_of_range_nested_array_keeps_both_dimensions() {
    let mut e = engine();
    let m = int_matrix(&mut e);
    let out = obj(e
        .invoke_static(
            ARRAYS,
            "copyOfRange",
            "([Ljava/lang/Object;II)[Ljava/lang/Object;",
            &[Value::Obj(m), Value::Int(0), Value::Int(1)],
        )
        .unwrap());
    assert_eq!(e.heap.class_of(out), Ok("[[I"));
    assert_eq!(e.heap.as_array_elems(out).unwrap().0.len(), 1);
}

#[test]
fn issue_379_copy_of_flat_int_array_and_string_array_keep_their_class() {
    let mut e = engine();
    let ints = e.heap.alloc_array_exact(ElemKind::Int, 2, "[I").unwrap();
    let out = obj(e
        .invoke_static(
            ARRAYS,
            "copyOf",
            "([II)[I",
            &[Value::Obj(ints), Value::Int(4)],
        )
        .unwrap());
    assert_eq!(e.heap.class_of(out), Ok("[I"));

    let strings = e
        .heap
        .alloc_array_exact(ElemKind::Obj, 1, "[Ljava/lang/String;")
        .unwrap();
    let out2 = obj(e
        .invoke_static(
            ARRAYS,
            "copyOf",
            "([Ljava/lang/Object;I)[Ljava/lang/Object;",
            &[Value::Obj(strings), Value::Int(2)],
        )
        .unwrap());
    assert_eq!(e.heap.class_of(out2), Ok("[Ljava/lang/String;"));
}
