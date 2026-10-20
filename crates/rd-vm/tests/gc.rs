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
