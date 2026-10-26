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
    // issue #293: BitSet(long[]) é PRIVATE no JDK — invoke-direct do guest
    // recebe IllegalAccessError; a rota pública é BitSet.valueOf(long[]).
    let direct = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    match e.invoke_instance(direct, "<init>", "([J)V", std::slice::from_ref(&arr)) {
        Err(VmExit::Exception(t)) => {
            assert_eq!(t.class, "Ljava/lang/IllegalAccessError;");
            assert!(t.message.unwrap_or_default().contains("valueOf"));
        }
        other => panic!("esperava IllegalAccessError, veio {other:?}"),
    }
    let b = match e
        .invoke_static(BS, "valueOf", "([J)Ljava/util/BitSet;", &[arr])
        .unwrap()
    {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(get_z(&mut e, b, 2));
    assert!(get_z(&mut e, b, 64));
    assert_eq!(card(&mut e, b), 2);
    assert_eq!(bits_str(&mut e, b), "{2, 64}");
    // round-trip toLongArray (words [4, 1] não têm zero à direita)
    match call(&mut e, b, "toLongArray", "()[J", &[]) {
        Value::Obj(r) => {
            let (elems, _) = e.heap.as_array_elems(r).unwrap();
            assert_eq!(elems, &[Value::Long(4), Value::Long(1)]);
        }
        other => panic!("{other:?}"),
    }
    // null → NPE (JDK Objects.requireNonNull)
    match e.invoke_static(BS, "valueOf", "([J)Ljava/util/BitSet;", &[Value::Null]) {
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

// ── issue #293 — auditoria pós-merge da #225 ──────────────────────────────

fn invoke_err(e: &mut Engine, b: ObjRef, name: &str, sig: &str, args: &[Value]) -> String {
    match e.invoke_instance(b, name, sig, args) {
        Err(VmExit::Exception(t)) => t.class,
        other => panic!("esperava exceção, veio {other:?}"),
    }
}

fn crate_arrays_byte(e: &mut Engine, bytes: &[i32]) -> Value {
    let arr = e
        .heap
        .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, bytes.len(), "[B")
        .unwrap();
    {
        let slot = e.heap.array_elems_mut(arr).unwrap();
        for (i, b) in bytes.iter().enumerate() {
            slot[i] = Value::Int(*b);
        }
    }
    Value::Obj(arr)
}

fn long_of(e: &mut Engine, r: ObjRef, i: usize) -> i64 {
    let (elems, _) = e.heap.as_array_elems(r).unwrap();
    match &elems[i] {
        Value::Long(v) => *v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn next_clear_bit_never_returns_below_from_index() {
    let mut e = engine_of(&DexBuilder::new());
    // BitSet vazio (words.len() == 0): devolvia 0 em vez de 10
    let b = bitset_of(&mut e, None);
    assert_eq!(
        int_of(call(&mut e, b, "nextClearBit", "(I)I", &[Value::Int(10)])),
        10
    );
    // BitSet(64) com words zero (wordsInUse == 0): devolvia 64 em vez de 100
    let b64 = bitset_of(&mut e, Some(64));
    assert_eq!(
        int_of(call(
            &mut e,
            b64,
            "nextClearBit",
            "(I)I",
            &[Value::Int(100)]
        )),
        100
    );
    // faixa toda setada dentro de wordsInUse: JDK devolve wordsInUse * 64
    let b2 = bitset_of(&mut e, None);
    call(&mut e, b2, "set", "(II)V", &[Value::Int(0), Value::Int(64)]);
    assert_eq!(
        int_of(call(&mut e, b2, "nextClearBit", "(I)I", &[Value::Int(0)])),
        64
    );
    // meio de word: primeiro clear ≥ from
    let b3 = bitset_of(&mut e, None);
    set_i(&mut e, b3, 5);
    assert_eq!(
        int_of(call(&mut e, b3, "nextClearBit", "(I)I", &[Value::Int(6)])),
        6
    );
    assert_eq!(
        int_of(call(&mut e, b3, "nextClearBit", "(I)I", &[Value::Int(0)])),
        0
    );
    // além do uso: o próprio fromIndex, mesmo com bits setados antes
    assert_eq!(
        int_of(call(
            &mut e,
            b3,
            "nextClearBit",
            "(I)I",
            &[Value::Int(1_000)]
        )),
        1_000
    );
}

#[test]
fn to_long_array_trims_trailing_zero_words() {
    let mut e = engine_of(&DexBuilder::new());
    // new BitSet(128): words de capacidade 2, todas zero → long[0] (não [0, 0])
    let b = bitset_of(&mut e, Some(128));
    match call(&mut e, b, "toLongArray", "()[J", &[]) {
        Value::Obj(r) => {
            let (elems, _) = e.heap.as_array_elems(r).unwrap();
            assert!(elems.is_empty(), "esperava long[0], veio {:?}", elems.len());
        }
        other => panic!("{other:?}"),
    }
    // set + clear → wordsInUse volta a 0 → long[0]
    set_i(&mut e, b, 100);
    call(&mut e, b, "clear", "(I)V", &[Value::Int(100)]);
    match call(&mut e, b, "toLongArray", "()[J", &[]) {
        Value::Obj(r) => {
            let (elems, _) = e.heap.as_array_elems(r).unwrap();
            assert!(elems.is_empty());
        }
        other => panic!("{other:?}"),
    }
    // set(100) → wordsInUse 2, word 1 = 1 << 36
    set_i(&mut e, b, 100);
    match call(&mut e, b, "toLongArray", "()[J", &[]) {
        Value::Obj(r) => {
            let (elems, _) = e.heap.as_array_elems(r).unwrap();
            assert_eq!(elems.len(), 2);
            assert_eq!(long_of(&mut e, r, 0), 0);
            assert_eq!(long_of(&mut e, r, 1), 1i64 << 36);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn host_budget_fails_fast_and_ranges_operate_by_word() {
    let mut e = engine_of(&DexBuilder::new());
    // new BitSet(Integer.MAX_VALUE): 33.554.432 words (256 MiB + Vec<Value>
    // de 536 MiB) agora falha ANTES de alocar, com OutOfMemoryError capturável
    let b = e.heap.alloc_instance(BS.into(), vec![]).unwrap();
    assert_eq!(
        invoke_err(&mut e, b, "<init>", "(I)V", &[Value::Int(i32::MAX)]),
        "Ljava/lang/OutOfMemoryError;"
    );
    // set(Integer.MAX_VALUE) com o mesmo fail-fast
    let b2 = bitset_of(&mut e, None);
    assert_eq!(
        invoke_err(&mut e, b2, "set", "(I)V", &[Value::Int(i32::MAX)]),
        "Ljava/lang/OutOfMemoryError;"
    );
    // set(0, MAX) pede as mesmas 33.554.432 words → OOM, não hang
    assert_eq!(
        invoke_err(
            &mut e,
            b2,
            "set",
            "(II)V",
            &[Value::Int(0), Value::Int(i32::MAX)]
        ),
        "Ljava/lang/OutOfMemoryError;"
    );
    // clear(0, MAX) e set(0, MAX, false) em BitSet vazio: por word, nada a
    // fazer — termina imediato e segue vazio (o código antigo era um hang de
    // 2,147.483.647 iterações bit a bit)
    call(
        &mut e,
        b2,
        "clear",
        "(II)V",
        &[Value::Int(0), Value::Int(i32::MAX)],
    );
    assert_eq!(int_of(call(&mut e, b2, "isEmpty", "()Z", &[])), 1);
    call(
        &mut e,
        b2,
        "set",
        "(IIZ)V",
        &[Value::Int(0), Value::Int(i32::MAX), Value::Int(0)],
    );
    assert_eq!(int_of(call(&mut e, b2, "isEmpty", "()Z", &[])), 1);
    // faixa grande DENTRO do orçamento: 1.000.000 bits por máscara
    let b3 = bitset_of(&mut e, None);
    call(
        &mut e,
        b3,
        "set",
        "(II)V",
        &[Value::Int(0), Value::Int(1_000_000)],
    );
    assert_eq!(card(&mut e, b3), 1_000_000);
    assert_eq!(int_of(call(&mut e, b3, "length", "()I", &[])), 1_000_000);
    assert_eq!(int_of(call(&mut e, b3, "size", "()I", &[])), 1_000_000);
    // clear parcial por máscara preserva as bordas
    call(
        &mut e,
        b3,
        "clear",
        "(II)V",
        &[Value::Int(500_000), Value::Int(999_999)],
    );
    assert!(get_z(&mut e, b3, 499_999));
    assert!(!get_z(&mut e, b3, 500_000));
    assert!(get_z(&mut e, b3, 999_999));
    // prefixo [0, 500_000) + a borda 999_999
    assert_eq!(card(&mut e, b3), 500_001);
}

#[test]
fn guest_subclass_dispatches_and_equals_uses_instanceof() {
    let mut b = DexBuilder::new();
    b.class("LMyBS;", "Ljava/util/BitSet;");
    let mut e = engine_of(&b);
    let sub = e.heap.alloc_instance("LMyBS;".into(), vec![]).unwrap();
    // despacho herdado pelo receiver subclasse (gate antigo: NOT_FOUND)
    e.invoke_instance(sub, "<init>", "()V", &[]).unwrap();
    set_i(&mut e, sub, 3);
    assert!(get_z(&mut e, sub, 3));
    assert_eq!(card(&mut e, sub), 1);
    assert_eq!(bits_str(&mut e, sub), "{3}");
    assert_eq!(
        int_of(call(&mut e, sub, "nextClearBit", "(I)I", &[Value::Int(0)])),
        0
    );
    // equals entre instâncias de classes diferentes com bits iguais: JDK
    // instanceof — o gate antigo por classe textual devolvia false
    let base = bitset_of(&mut e, None);
    set_i(&mut e, base, 3);
    assert_eq!(
        call(
            &mut e,
            base,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(sub)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut e,
            sub,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(base)]
        ),
        Value::Int(1)
    );
    // bits diferentes → false
    let other = bitset_of(&mut e, None);
    set_i(&mut e, other, 4);
    assert_eq!(
        call(
            &mut e,
            sub,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(other)]
        ),
        Value::Int(0)
    );
}

#[test]
fn value_of_factories_pack_and_private_ctor_stays_closed() {
    let mut e = engine_of(&DexBuilder::new());
    // valueOf([J): cópia fiel das words
    let arr = crate_arrays_long(&mut e, &[4, 1]);
    let b = match e
        .invoke_static(BS, "valueOf", "([J)Ljava/util/BitSet;", &[arr])
        .unwrap()
    {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(get_z(&mut e, b, 2));
    assert!(get_z(&mut e, b, 64));
    assert_eq!(card(&mut e, b), 2);
    // valueOf([B): empacotamento little-endian, 8 bytes por word
    let barr = crate_arrays_byte(&mut e, &[1, 0, -1, 0, 0, 0, 0, 0, 0x80]);
    let bb = match e
        .invoke_static(BS, "valueOf", "([B)Ljava/util/BitSet;", &[barr])
        .unwrap()
    {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(get_z(&mut e, bb, 0));
    assert!(!get_z(&mut e, bb, 1));
    // byte -1 (0xff) na posição 2 → bits 16..24
    for bit in 16..24 {
        assert!(get_z(&mut e, bb, bit), "bit {bit}");
    }
    assert!(!get_z(&mut e, bb, 24));
    // byte 0x80 na posição 8 → bit 7 do word 1 → bit global 71
    assert!(!get_z(&mut e, bb, 64));
    assert!(get_z(&mut e, bb, 71));
    assert!(!get_z(&mut e, bb, 72));
    // null → NPE nas duas fábricas
    for sig in ["([J)Ljava/util/BitSet;", "([B)Ljava/util/BitSet;"] {
        match e.invoke_static(BS, "valueOf", sig, &[Value::Null]) {
            Err(VmExit::Exception(t)) => {
                assert_eq!(t.class, "Ljava/lang/NullPointerException;", "{sig}")
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn logical_ops_clone_and_get_range_jdk_semantics() {
    let mut e = engine_of(&DexBuilder::new());
    let mk = |e: &mut Engine, bits: &[i32]| {
        let b = bitset_of(e, None);
        for bit in bits {
            set_i(e, b, *bit);
        }
        b
    };
    let a = mk(&mut e, &[1, 2, 3]);
    let b = mk(&mut e, &[2, 3, 9]);
    // and: {2, 3}
    let a1 = mk(&mut e, &[1, 2, 3]);
    call(&mut e, a1, "and", "(Ljava/util/BitSet;)V", &[Value::Obj(b)]);
    assert_eq!(bits_str(&mut e, a1), "{2, 3}");
    // or: {1, 2, 3, 9}
    let a2 = mk(&mut e, &[1, 2, 3]);
    call(&mut e, a2, "or", "(Ljava/util/BitSet;)V", &[Value::Obj(b)]);
    assert_eq!(bits_str(&mut e, a2), "{1, 2, 3, 9}");
    // xor: {1, 9}
    let a3 = mk(&mut e, &[1, 2, 3]);
    call(&mut e, a3, "xor", "(Ljava/util/BitSet;)V", &[Value::Obj(b)]);
    assert_eq!(bits_str(&mut e, a3), "{1, 9}");
    // andNot(b): {1}
    let a4 = mk(&mut e, &[1, 2, 3]);
    call(
        &mut e,
        a4,
        "andNot",
        "(Ljava/util/BitSet;)V",
        &[Value::Obj(b)],
    );
    assert_eq!(bits_str(&mut e, a4), "{1}");
    // intersects: a∩b = {2, 3} → true; vazio → false; null → NPE
    assert_eq!(
        call(
            &mut e,
            a,
            "intersects",
            "(Ljava/util/BitSet;)Z",
            &[Value::Obj(b)]
        ),
        Value::Int(1)
    );
    let lone = mk(&mut e, &[70]);
    assert_eq!(
        call(
            &mut e,
            lone,
            "intersects",
            "(Ljava/util/BitSet;)Z",
            &[Value::Obj(b)]
        ),
        Value::Int(0)
    );
    match e.invoke_instance(a, "intersects", "(Ljava/util/BitSet;)Z", &[Value::Null]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, "Ljava/lang/NullPointerException;"),
        other => panic!("{other:?}"),
    }
    // or cresce com budget: 1M bits via or
    let big = mk(&mut e, &[0]);
    let wide = bitset_of(&mut e, None);
    call(
        &mut e,
        wide,
        "set",
        "(II)V",
        &[Value::Int(900_000), Value::Int(900_001)],
    );
    call(
        &mut e,
        big,
        "or",
        "(Ljava/util/BitSet;)V",
        &[Value::Obj(wide)],
    );
    assert_eq!(card(&mut e, big), 2);
    assert!(get_z(&mut e, big, 900_000));
    // clone: igual, independente, mesma capacidade
    let src = mk(&mut e, &[5]);
    let cloned = match call(&mut e, src, "clone", "()Ljava/lang/Object;", &[]) {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        call(&mut e, cloned, "hashCode", "()I", &[]),
        call(&mut e, src, "hashCode", "()I", &[])
    );
    set_i(&mut e, cloned, 6);
    assert_eq!(card(&mut e, src), 1);
    assert_eq!(card(&mut e, cloned), 2);
    // get(IIV): bits [from,to) deslocados para a base 0
    let g = mk(&mut e, &[64, 65, 130]);
    let slice = match call(
        &mut e,
        g,
        "get",
        "(II)Ljava/util/BitSet;",
        &[Value::Int(64), Value::Int(131)],
    ) {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(get_z(&mut e, slice, 0));
    assert!(get_z(&mut e, slice, 1));
    assert!(get_z(&mut e, slice, 66));
    assert_eq!(card(&mut e, slice), 3);
    // toIndex > length() é clampado ao length() (JDK)
    let g2 = mk(&mut e, &[3]);
    let slice2 = match call(
        &mut e,
        g2,
        "get",
        "(II)Ljava/util/BitSet;",
        &[Value::Int(0), Value::Int(1_000)],
    ) {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(get_z(&mut e, slice2, 3));
    assert_eq!(card(&mut e, slice2), 1);
    // sem bits na faixa → BitSet vazio
    let empty_slice = match call(
        &mut e,
        g2,
        "get",
        "(II)Ljava/util/BitSet;",
        &[Value::Int(100), Value::Int(200)],
    ) {
        Value::Obj(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(int_of(call(&mut e, empty_slice, "isEmpty", "()Z", &[])), 1);
    // flip(IIV) mesma word e entre words, idempotente via dupla aplicação
    let f = bitset_of(&mut e, None);
    call(&mut e, f, "flip", "(II)V", &[Value::Int(2), Value::Int(5)]);
    assert_eq!(bits_str(&mut e, f), "{2, 3, 4}");
    call(
        &mut e,
        f,
        "flip",
        "(II)V",
        &[Value::Int(60), Value::Int(130)],
    );
    assert!(get_z(&mut e, f, 60));
    assert!(get_z(&mut e, f, 129));
    call(
        &mut e,
        f,
        "flip",
        "(II)V",
        &[Value::Int(60), Value::Int(130)],
    );
    assert!(!get_z(&mut e, f, 60));
    assert!(!get_z(&mut e, f, 129));
    assert!(get_z(&mut e, f, 2));
}
