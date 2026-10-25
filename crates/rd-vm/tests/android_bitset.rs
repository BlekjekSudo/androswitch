//! java.util.BitSet host — semântica JDK sobre heap longs. Cascata Primftpd
//! pós-default `View.onTouchEvent`: `MaterialShapeDrawable.<init>` faz
//! `new BitSet(4)` e o host não tinha a classe (NOT_RESOLVED → EIIE no FAB).
#[macro_use]
mod common;
use common::*;
use rd_vm::{Engine, ObjRef, Value};

const BS: &str = "Ljava/util/BitSet;";

fn bitset_of(e: &mut Engine, nbits: Option<i32>) -> ObjRef {
    let b = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    match nbits {
        Some(n) => e
            .invoke_instance(b, "<init>", "(I)V", &[Value::Int(n)])
            .unwrap(),
        None => e.invoke_instance(b, "<init>", "()V", &[]).unwrap(),
    };
    b
}

fn call(e: &mut Engine, b: ObjRef, name: &str, sig: &str, args: &[Value]) -> Value {
    e.invoke_instance(b, name, sig, args).unwrap()
}

fn set_i(e: &mut Engine, b: ObjRef, i: i32) {
    call(e, b, "set", "(I)V", &[Value::Int(i)]);
}

fn get_z(e: &mut Engine, b: ObjRef, i: i32) -> bool {
    call(e, b, "get", "(I)Z", &[Value::Int(i)]) == Value::Int(1)
}

fn card(e: &mut Engine, b: ObjRef) -> i32 {
    match call(e, b, "cardinality", "()I", &[]) {
        Value::Int(n) => n,
        other => panic!("{other:?}"),
    }
}

fn int_of(v: Value) -> i32 {
    match v {
        Value::Int(n) => n,
        other => panic!("{other:?}"),
    }
}

fn bits_str(e: &mut Engine, b: ObjRef) -> String {
    match call(e, b, "toString", "()Ljava/lang/String;", &[]) {
        Value::Obj(r) => e.heap.as_str(r).unwrap().to_string(),
        other => panic!("{other:?}"),
    }
}

fn err_class(e: &mut Engine, b: ObjRef, name: &str, sig: &str, args: &[Value]) -> String {
    match e.invoke_instance(b, name, sig, args) {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("esperava exceção, veio {other:?}"),
    }
}

use rd_vm::VmExit;

#[test]
fn ctor_sizes_and_initial_state() {
    let mut e = engine_of(&DexBuilder::new());
    let b0 = bitset_of(&mut e, None);
    assert_eq!(int_of(call(&mut e, b0, "size", "()I", &[])), 0);
    assert_eq!(int_of(call(&mut e, b0, "isEmpty", "()Z", &[])), 1);
    assert_eq!(int_of(call(&mut e, b0, "length", "()I", &[])), 0);

    // JDK initWords: ((4 - 1) >> 6) + 1 = 1 word → size 64
    let b4 = bitset_of(&mut e, Some(4));
    assert_eq!(int_of(call(&mut e, b4, "size", "()I", &[])), 64);
    assert_eq!(int_of(call(&mut e, b4, "isEmpty", "()Z", &[])), 1);

    let b00 = bitset_of(&mut e, Some(0));
    assert_eq!(int_of(call(&mut e, b00, "size", "()I", &[])), 0);

    let neg = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    match e.invoke_instance(neg, "<init>", "(I)V", &[Value::Int(-1)]) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/NegativeArraySizeException;");
            assert!(t.message.unwrap_or_default().contains("nbits < 0: -1"));
        }
        other => panic!("esperava NASE, veio {other:?}"),
    }
}

#[test]
fn set_get_and_invariants() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, Some(4));
    set_i(&mut e, b, 2);
    assert!(get_z(&mut e, b, 2));
    assert!(!get_z(&mut e, b, 1));
    // bit além da capacidade alocada é false (JDK), sem estourar
    assert!(!get_z(&mut e, b, 999));
    assert_eq!(card(&mut e, b), 1);
    assert_eq!(int_of(call(&mut e, b, "length", "()I", &[])), 3);
    assert_eq!(int_of(call(&mut e, b, "isEmpty", "()Z", &[])), 0);
    // set(2) não muda a capacidade (1 word já existe)
    assert_eq!(int_of(call(&mut e, b, "size", "()I", &[])), 64);
    assert_eq!(bits_str(&mut e, b), "{2}");
}

#[test]
fn next_set_and_clear_bit() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, Some(4));
    set_i(&mut e, b, 2);
    set_i(&mut e, b, 5);
    assert_eq!(
        int_of(call(&mut e, b, "nextSetBit", "(I)I", &[Value::Int(0)])),
        2
    );
    assert_eq!(
        int_of(call(&mut e, b, "nextSetBit", "(I)I", &[Value::Int(3)])),
        5
    );
    assert_eq!(
        int_of(call(&mut e, b, "nextSetBit", "(I)I", &[Value::Int(6)])),
        -1
    );
    assert_eq!(
        int_of(call(&mut e, b, "nextClearBit", "(I)I", &[Value::Int(0)])),
        0
    );
    assert_eq!(
        int_of(call(&mut e, b, "nextClearBit", "(I)I", &[Value::Int(3)])),
        3
    );
    // além da capacidade tudo é clear → primeiro índice além do array
    assert_eq!(
        int_of(call(&mut e, b, "nextClearBit", "(I)I", &[Value::Int(6)])),
        6
    );
}

#[test]
fn negative_indices_throw_iobe() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, Some(4));
    for (name, sig, msg) in [
        ("set", "(I)V", "bitIndex < 0: -1"),
        ("get", "(I)Z", "bitIndex < 0: -3"),
        ("clear", "(I)V", "bitIndex < 0: -2"),
        ("flip", "(I)V", "bitIndex < 0: -4"),
    ] {
        let idx = match name {
            "set" => -1,
            "get" => -3,
            "clear" => -2,
            _ => -4,
        };
        let cls = err_class(&mut e, b, name, sig, &[Value::Int(idx)]);
        assert_eq!(cls, "Ljava/lang/IndexOutOfBoundsException;", "{name}");
        let _ = msg;
    }
    let cls = err_class(&mut e, b, "nextSetBit", "(I)I", &[Value::Int(-1)]);
    assert_eq!(cls, "Ljava/lang/IndexOutOfBoundsException;");
}

#[test]
fn ranges_set_and_clear() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, None);
    call(&mut e, b, "set", "(II)V", &[Value::Int(2), Value::Int(5)]);
    assert_eq!(card(&mut e, b), 3);
    assert_eq!(bits_str(&mut e, b), "{2, 3, 4}");
    call(&mut e, b, "clear", "(II)V", &[Value::Int(3), Value::Int(5)]);
    assert_eq!(card(&mut e, b), 1);
    assert_eq!(bits_str(&mut e, b), "{2}");
    // set(from, to, false) limpa a faixa
    call(
        &mut e,
        b,
        "set",
        "(IIZ)V",
        &[Value::Int(1), Value::Int(3), Value::Int(0)],
    );
    assert_eq!(bits_str(&mut e, b), "{}");
    // toIndex < fromIndex → IOOBE
    let cls = err_class(&mut e, b, "set", "(II)V", &[Value::Int(5), Value::Int(2)]);
    assert_eq!(cls, "Ljava/lang/IndexOutOfBoundsException;");
}

#[test]
fn flip_and_clear_keep_capacity() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, Some(4));
    call(&mut e, b, "flip", "(I)V", &[Value::Int(7)]);
    assert!(get_z(&mut e, b, 7));
    call(&mut e, b, "flip", "(I)V", &[Value::Int(7)]);
    assert!(!get_z(&mut e, b, 7));
    set_i(&mut e, b, 1);
    call(&mut e, b, "clear", "()V", &[]);
    assert_eq!(int_of(call(&mut e, b, "isEmpty", "()Z", &[])), 1);
    // JDK: clear() não encolhe o array de words → size preservado
    assert_eq!(int_of(call(&mut e, b, "size", "()I", &[])), 64);
}

#[test]
fn growth_beyond_initial_capacity() {
    let mut e = engine_of(&DexBuilder::new());
    let b = bitset_of(&mut e, None);
    set_i(&mut e, b, 200);
    assert!(get_z(&mut e, b, 200));
    assert_eq!(card(&mut e, b), 1);
    assert_eq!(int_of(call(&mut e, b, "length", "()I", &[])), 201);
    // 200 exige 4 words (JDK realoca na mesma proporção) → size 256
    assert_eq!(int_of(call(&mut e, b, "size", "()I", &[])), 256);
}

#[test]
fn equals_hashcode_jdk_algorithm() {
    let mut e = engine_of(&DexBuilder::new());
    let a = bitset_of(&mut e, Some(4));
    let c = bitset_of(&mut e, Some(4));
    set_i(&mut e, a, 2);
    set_i(&mut e, c, 2);
    assert_eq!(
        call(
            &mut e,
            a,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(c)]
        ),
        Value::Int(1)
    );
    // JDK: h=1234; h ^= words[0]*(0+1)=4 → 0x4D2 ^ 0x004 = 0x4D6 = 1238
    assert_eq!(int_of(call(&mut e, a, "hashCode", "()I", &[])), 1238);
    assert_eq!(int_of(call(&mut e, c, "hashCode", "()I", &[])), 1238);
    set_i(&mut e, c, 9);
    assert_eq!(
        call(
            &mut e,
            a,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(c)]
        ),
        Value::Int(0)
    );
    // trailing zeros equivalentes: BitSet() com bit 200 ≠ BitSet() vazio,
    // mas clear de tudo volta a ser igual ao vazio
    let d = bitset_of(&mut e, None);
    assert_eq!(
        call(
            &mut e,
            a,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(d)]
        ),
        Value::Int(0)
    );
    call(&mut e, c, "clear", "(I)V", &[Value::Int(9)]);
    call(&mut e, c, "clear", "(I)V", &[Value::Int(2)]);
    assert_eq!(
        call(
            &mut e,
            c,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(d)]
        ),
        Value::Int(1)
    );
}

#[test]
fn long_array_ctor_and_to_long_array_round_trip() {
    let mut e = engine_of(&DexBuilder::new());
    let arr = match crate_arrays_long(&mut e, &[4, 1]) {
        Value::Obj(r) => Value::Obj(r),
        other => panic!("{other:?}"),
    };
    let b = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    e.invoke_instance(b, "<init>", "([J)V", &[arr]).unwrap();
    assert!(get_z(&mut e, b, 2));
    assert!(get_z(&mut e, b, 64));
    assert_eq!(card(&mut e, b), 2);
    assert_eq!(bits_str(&mut e, b), "{2, 64}");
    // round-trip toLongArray
    match call(&mut e, b, "toLongArray", "()[J", &[]) {
        Value::Obj(r) => {
            let (elems, _) = e.heap.as_array_elems(r).unwrap();
            assert_eq!(elems, &[Value::Long(4), Value::Long(1)]);
        }
        other => panic!("{other:?}"),
    }
    // null → NPE (JDK Objects.requireNonNull)
    let b2 = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    match e.invoke_instance(b2, "<init>", "([J)V", &[Value::Null]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("{other:?}"),
    }
}

/// Cria um array `[J` no heap (API pública do heap, a mesma rota do
/// new-array do interpretador).
fn crate_arrays_long(e: &mut Engine, words: &[i64]) -> Value {
    let arr = e
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Long, words.len(), "[J")
        .unwrap();
    {
        let slot = e.heap.array_elems_mut(arr).unwrap();
        for (i, w) in words.iter().enumerate() {
            slot[i] = Value::Long(*w);
        }
    }
    Value::Obj(arr)
}
