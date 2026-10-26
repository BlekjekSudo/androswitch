//! Unchanged original RustyDroid weak-reference/queue heap scenarios.
use rd_vm::heap::HeapObj;
use rd_vm::{Engine, Value, VmConfig};

fn class_of(e: &Engine, r: u32) -> String {
    match e.heap.get(r).unwrap() {
        HeapObj::Instance { class, .. } => class.clone(),
        HeapObj::Str(_) => "String".into(),
        HeapObj::Array { .. } => "Array".into(),
    }
}

pub fn weak_references_are_cleared() {
    let mut e = Engine::new(vec![], VmConfig::default());
    // realistic ids: small refs collide with small constants in a fresh
    // engine's host state, which the conservative root scan keeps alive
    for _ in 0..20_000 {
        let _ = e.heap.alloc_string(String::new()).unwrap();
    }
    let gone = e.heap.alloc_string("weak only".into()).unwrap();
    let soft_t = e.heap.alloc_string("soft".into()).unwrap();
    let strong = e.heap.alloc_string("strong too".into()).unwrap();
    let w1 = e
        .heap
        .alloc_instance(
            "Ljava/lang/ref/WeakReference;".into(),
            vec![("referent".into(), Value::Obj(gone))],
        )
        .unwrap();
    let w2 = e
        .heap
        .alloc_instance(
            "Ljava/lang/ref/WeakReference;".into(),
            vec![("referent".into(), Value::Obj(strong))],
        )
        .unwrap();
    let s1 = e
        .heap
        .alloc_instance(
            "Ljava/lang/ref/SoftReference;".into(),
            vec![("referent".into(), Value::Obj(soft_t))],
        )
        .unwrap();
    for (k, r) in [("wa", w1), ("wb", w2), ("sa", s1), ("st", strong)] {
        e.statics.insert(("LRoot;".into(), k.into()), Value::Obj(r));
    }
    let st = e.gc();
    assert_eq!(st.last_cleared_weak, 1, "{st:?}");
    assert_eq!(e.heap.get_field(w1, "referent").unwrap(), Value::Null);
    assert_eq!(class_of(&e, gone), rd_vm::gc::FREED_CLASS);
    assert_eq!(
        e.heap.get_field(w2, "referent").unwrap(),
        Value::Obj(strong)
    );
    assert_eq!(
        e.heap.get_field(s1, "referent").unwrap(),
        Value::Obj(soft_t)
    );
    assert_eq!(e.heap.as_str(soft_t).unwrap(), "soft");
}

pub fn cleared_weak_refs_are_enqueued() {
    let mut e = Engine::new(vec![], VmConfig::default());
    for _ in 0..20_000 {
        let _ = e.heap.alloc_string(String::new()).unwrap();
    }
    let gone = e.heap.alloc_string("cached".into()).unwrap();
    let q = e
        .heap
        .alloc_instance("Ljava/lang/ref/ReferenceQueue;".into(), vec![])
        .unwrap();
    let w = e
        .heap
        .alloc_instance(
            "Ljava/lang/ref/WeakReference;".into(),
            vec![
                ("referent".into(), Value::Obj(gone)),
                ("queue".into(), Value::Obj(q)),
            ],
        )
        .unwrap();
    e.statics
        .insert(("LRoot;".into(), "wq".into()), Value::Obj(w));
    let st = e.gc();
    assert_eq!(st.last_cleared_weak, 1, "{st:?}");
    assert_eq!(e.heap.get_field(q, "rd$head").unwrap(), Value::Obj(w));
    assert_eq!(e.heap.get_field(w, "rd$enq").unwrap(), Value::Int(1));
    assert_eq!(e.heap.get_field(w, "rd$next").unwrap(), Value::Null);
    // the queue stays alive through the reference; a new cycle does not
    // enqueue again
    let st = e.gc();
    assert_eq!(st.last_cleared_weak, 0);
    assert_eq!(e.heap.get_field(q, "rd$head").unwrap(), Value::Obj(w));
    assert_eq!(e.heap.get_field(w, "rd$next").unwrap(), Value::Null);
}
