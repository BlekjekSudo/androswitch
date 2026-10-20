//! Mark-sweep collector (`rd_vm::gc`): reachability through statics,
//! instance fields and arrays; garbage tombstoned and its bytes returned.
use rd_vm::heap::{ElemKind, HeapObj};
use rd_vm::{Engine, Value, VmConfig};

fn class_of(e: &Engine, r: u32) -> String {
    match e.heap.get(r).unwrap() {
        HeapObj::Instance { class, .. } => class.clone(),
        HeapObj::Str(_) => "String".into(),
        HeapObj::Array { .. } => "Array".into(),
    }
}

#[test]
fn collects_unreachable_and_keeps_reachable() {
    let mut e = Engine::new(vec![], VmConfig::default());
    // live: static -> a -> (field) b -> (array) s
    let s = e.heap.alloc_string("kept".repeat(100)).unwrap();
    let arr = e
        .heap
        .alloc_array(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .unwrap();
    if let HeapObj::Array { elems, .. } = e.heap.get_mut(arr).unwrap() {
        *elems = vec![Value::Obj(s)];
    }
    let b = e
        .heap
        .alloc_instance("LB;".into(), vec![("arr".into(), Value::Obj(arr))])
        .unwrap();
    let a = e
        .heap
        .alloc_instance("LA;".into(), vec![("b".into(), Value::Obj(b))])
        .unwrap();
    e.statics
        .insert(("LRoot;".into(), "a".into()), Value::Obj(a));
    // garbage: a cycle and a big string nobody references
    let g1 = e
        .heap
        .alloc_instance("LG;".into(), vec![("next".into(), Value::Null)])
        .unwrap();
    let g2 = e
        .heap
        .alloc_instance("LG;".into(), vec![("next".into(), Value::Obj(g1))])
        .unwrap();
    e.heap.put_field(g1, "next", Value::Obj(g2)).unwrap();
    let big = e.heap.alloc_string("x".repeat(100_000)).unwrap();

    let before = e.heap.used();
    let st = e.gc();
    assert!(st.last_freed_objects >= 3, "{st:?}");
    assert!(
        e.heap.used() + 200_000 <= before,
        "big string bytes returned: {st:?}"
    );
    for r in [g1, g2, big] {
        assert_eq!(class_of(&e, r), rd_vm::gc::FREED_CLASS);
    }
    assert_eq!(class_of(&e, a), "LA;");
    assert_eq!(class_of(&e, b), "LB;");
    assert_eq!(class_of(&e, arr), "Array");
    assert_eq!(e.heap.as_str(s).unwrap(), "kept".repeat(100));

    // freed slots are recycled before the heap grows
    let len = e.heap.len();
    let reused = e.heap.alloc_string("new".into()).unwrap();
    assert!([g1, g2, big].contains(&reused), "slot recycled");
    assert_eq!(e.heap.len(), len);
    assert_eq!(e.heap.as_str(reused).unwrap(), "new");
    e.statics
        .insert(("LRoot;".into(), "n".into()), Value::Obj(reused));

    // second cycle frees nothing new; stats accumulate
    let st2 = e.gc();
    assert_eq!(st2.last_freed_objects, 0);
    assert_eq!(st2.collections, 2);
    assert_eq!(st2.total_freed_objects, st.total_freed_objects);

    // dropping the root makes the whole chain garbage
    e.statics.clear();
    let st3 = e.gc();
    assert!(st3.last_freed_objects >= 4, "{st3:?}");
    assert_eq!(class_of(&e, a), rd_vm::gc::FREED_CLASS);
}

#[test]
fn maybe_gc_waits_for_threshold() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let _ = e.heap.alloc_string("small".into()).unwrap();
    assert!(e.maybe_gc().is_none(), "below the +16 MB threshold");
    for _ in 0..20 {
        let _ = e.heap.alloc_string("y".repeat(1 << 20)).unwrap();
    }
    let st = e.maybe_gc().expect("collects past the threshold");
    assert!(st.last_freed_bytes >= 20 << 20, "{st:?}");
}

/// Long session under a small budget: ~25x the budget is allocated across
/// 200 "tool calls"; with the safe-point collector the app never hits the
/// cap and the live set survives intact; without it the arena OOMs early.
#[test]
fn long_session_stays_under_budget() {
    let run = |gc: bool| -> Result<(Engine, Vec<u32>), usize> {
        let cfg = VmConfig {
            heap_budget: 8 << 20,
            ..VmConfig::default()
        };
        let mut e = Engine::new(vec![], cfg);
        let list = e.heap.alloc_instance("LLive;".into(), Vec::new()).unwrap();
        e.statics
            .insert(("LRoot;".into(), "live".into()), Value::Obj(list));
        let mut live = Vec::new();
        for call in 0..200usize {
            for _ in 0..16 {
                if e.heap.alloc_string("g".repeat(32 << 10)).is_err() {
                    return Err(call);
                }
            }
            let keep = e
                .heap
                .alloc_string(format!("item {call}"))
                .map_err(|_| call)?;
            e.heap
                .put_field(list, &format!("f{call}"), Value::Obj(keep))
                .map_err(|_| call)?;
            live.push(keep);
            if gc {
                let _ = e.maybe_gc();
            }
        }
        Ok((e, live))
    };
    let failed_at = run(false).err().expect("arena without GC must hit the cap");
    assert!(failed_at < 20, "no-GC OOM at call {failed_at}");
    let (e, live) = run(true).expect("GC keeps the session under budget");
    assert!(e.gc_stats().collections >= 10, "{:?}", e.gc_stats());
    assert!(e.heap.peak() <= 8 << 20);
    for (i, r) in live.iter().enumerate() {
        assert_eq!(e.heap.as_str(*r).unwrap(), format!("item {i}"));
    }
    // recycled slots: the slot vector stays far below total allocations
    assert!(e.heap.len() < 2_000, "slots {}", e.heap.len());
}

/// Host-backed objects: the host half (`fw.objects`, e.g. an ArrayList) is
/// traced only while its heap object is reachable, and purged with it.
#[test]
fn host_object_table_is_ephemeron() {
    use rd_vm::framework::HostObj;
    let mut e = Engine::new(vec![], VmConfig::default());
    let elem = e.heap.alloc_string("in list".into()).unwrap();
    let list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".into(), Vec::new())
        .unwrap();
    e.fw.objects.insert(
        list,
        HostObj::List {
            elems: vec![Value::Obj(elem)],
        },
    );
    let dead_elem = e.heap.alloc_string("in dead list".into()).unwrap();
    let dead = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".into(), Vec::new())
        .unwrap();
    e.fw.objects.insert(
        dead,
        HostObj::List {
            elems: vec![Value::Obj(dead_elem)],
        },
    );
    e.statics
        .insert(("LRoot;".into(), "l".into()), Value::Obj(list));
    e.gc();
    assert_eq!(
        e.heap.as_str(elem).unwrap(),
        "in list",
        "element of a live host list kept"
    );
    assert!(e.fw.objects.contains_key(&list));
    assert!(
        !e.fw.objects.contains_key(&dead),
        "host half of a dead list purged"
    );
    assert_eq!(class_of(&e, dead_elem), rd_vm::gc::FREED_CLASS);
}

/// WeakReference clears an otherwise-unreachable referent; SoftReference and
/// strongly reachable referents survive.
/// (Root key names avoid digits: the conservative root scan would read "w1"
/// as ObjRef 1 and legitimately keep that object alive.)
#[test]
fn weak_references_are_cleared() {
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

/// A cleared WeakReference registered with a ReferenceQueue is enqueued once
/// (intrusive list in heap fields that `ReferenceQueue.poll` pops).
#[test]
fn cleared_weak_refs_are_enqueued() {
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
