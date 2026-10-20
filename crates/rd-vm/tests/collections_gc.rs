//! Iterator removal and weak-key map regression contracts.
use rd_vm::err::VmExit;
use rd_vm::{Engine, Value, VmConfig};

fn obj(e: &mut Engine, class: &str) -> u32 {
    e.heap.alloc_instance(class.into(), vec![]).unwrap()
}
fn call(e: &mut Engine, r: u32, n: &str, sig: &str, args: &[Value]) -> Value {
    e.invoke_instance(r, n, sig, args).unwrap()
}
fn reference(v: Value) -> u32 {
    if let Value::Obj(r) = v {
        r
    } else {
        panic!("{v:?}")
    }
}
fn exception(e: &mut Engine, r: u32, expected: &str) {
    match e.invoke_instance(r, "remove", "()V", &[]) {
        Err(VmExit::Exception(t)) => assert_eq!(t.class, expected),
        other => panic!("{other:?}"),
    }
}

#[test]
fn iterator_remove_updates_source_and_cursor() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let list = obj(&mut e, "Ljava/util/ArrayList;");
    call(&mut e, list, "<init>", "()V", &[]);
    for i in [1, 1, 2] {
        call(
            &mut e,
            list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Int(i)],
        );
    }
    let it = reference(call(
        &mut e,
        list,
        "iterator",
        "()Ljava/util/Iterator;",
        &[],
    ));
    exception(&mut e, it, "Ljava/lang/IllegalStateException;");
    assert_eq!(
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]),
        Value::Int(1)
    );
    call(&mut e, it, "remove", "()V", &[]);
    exception(&mut e, it, "Ljava/lang/IllegalStateException;");
    assert_eq!(call(&mut e, list, "size", "()I", &[]), Value::Int(2));
    assert_eq!(
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]),
        Value::Int(1)
    );
    call(&mut e, it, "remove", "()V", &[]);
    assert_eq!(
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]),
        Value::Int(2)
    );
    call(&mut e, it, "remove", "()V", &[]);
    assert_eq!(call(&mut e, list, "size", "()I", &[]), Value::Int(0));
    assert_eq!(call(&mut e, it, "hasNext", "()Z", &[]), Value::Int(0));
}

#[test]
fn list_iterator_previous_remove_does_not_skip() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let list = obj(&mut e, "Ljava/util/ArrayList;");
    call(&mut e, list, "<init>", "()V", &[]);
    for i in 0..3 {
        call(
            &mut e,
            list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Int(i)],
        );
    }
    let it = reference(call(
        &mut e,
        list,
        "listIterator",
        "(I)Ljava/util/ListIterator;",
        &[Value::Int(3)],
    ));
    assert_eq!(
        call(&mut e, it, "previous", "()Ljava/lang/Object;", &[]),
        Value::Int(2)
    );
    call(&mut e, it, "remove", "()V", &[]);
    assert_eq!(call(&mut e, it, "nextIndex", "()I", &[]), Value::Int(2));
    assert_eq!(
        call(&mut e, it, "previous", "()Ljava/lang/Object;", &[]),
        Value::Int(1)
    );
    call(&mut e, it, "remove", "()V", &[]);
    assert_eq!(call(&mut e, list, "size", "()I", &[]), Value::Int(1));
}

#[test]
fn map_view_iterators_remove_the_correct_entry_even_for_duplicate_values() {
    for method in ["keySet", "values", "entrySet"] {
        let mut e = Engine::new(vec![], VmConfig::default());
        let map = obj(&mut e, "Ljava/util/HashMap;");
        call(&mut e, map, "<init>", "()V", &[]);
        for i in 1..=3 {
            call(
                &mut e,
                map,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[Value::Int(i), Value::Int(9)],
            );
        }
        let sig = if method == "values" {
            "()Ljava/util/Collection;"
        } else {
            "()Ljava/util/Set;"
        };
        let view = reference(call(&mut e, map, method, sig, &[]));
        let it = reference(call(
            &mut e,
            view,
            "iterator",
            "()Ljava/util/Iterator;",
            &[],
        ));
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
        call(&mut e, it, "remove", "()V", &[]);
        assert_eq!(
            call(
                &mut e,
                map,
                "containsKey",
                "(Ljava/lang/Object;)Z",
                &[Value::Int(2)]
            ),
            Value::Int(0)
        );
        assert_eq!(call(&mut e, map, "size", "()I", &[]), Value::Int(2));
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
        call(&mut e, it, "remove", "()V", &[]);
        assert_eq!(call(&mut e, map, "size", "()I", &[]), Value::Int(1));
    }
}

#[test]
fn weak_map_keys_are_weak_but_values_and_null_keys_are_strong() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let map = obj(&mut e, "Ljava/util/WeakHashMap;");
    call(&mut e, map, "<init>", "()V", &[]);
    let dead_key = obj(&mut e, "LKey;");
    let live_key = obj(&mut e, "LKey;");
    let back_key = obj(&mut e, "LKey;");
    let value = obj(&mut e, "LValue;");
    e.heap
        .put_field(value, "key", Value::Obj(back_key))
        .unwrap();
    for (k, v) in [
        (Value::Obj(dead_key), Value::Null),
        (Value::Obj(live_key), Value::Null),
        (Value::Obj(back_key), Value::Obj(value)),
        (Value::Null, Value::Int(77)),
    ] {
        call(
            &mut e,
            map,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[k, v],
        );
    }
    e.statics
        .insert(("LRoot;".into(), "map".into()), Value::Obj(map));
    e.statics
        .insert(("LRoot;".into(), "key".into()), Value::Obj(live_key));
    let stats = e.gc();
    assert_eq!(stats.last_cleared_weak_keys, 1);
    assert_eq!(e.heap.class_of(dead_key).unwrap(), rd_vm::gc::FREED_CLASS);
    assert_eq!(call(&mut e, map, "size", "()I", &[]), Value::Int(3));
    assert_eq!(
        call(
            &mut e,
            map,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            &[Value::Null]
        ),
        Value::Int(77)
    );
    assert_ne!(e.heap.class_of(back_key).unwrap(), rd_vm::gc::FREED_CLASS);
}

#[test]
fn cow_and_unmodifiable_iterators_still_reject_removal() {
    for class in [
        "Ljava/util/concurrent/CopyOnWriteArrayList;",
        "Ljava/util/concurrent/CopyOnWriteArraySet;",
    ] {
        let mut e = Engine::new(vec![], VmConfig::default());
        let list = obj(&mut e, class);
        call(&mut e, list, "<init>", "()V", &[]);
        call(
            &mut e,
            list,
            "add",
            "(Ljava/lang/Object;)Z",
            &[Value::Int(1)],
        );
        let it = reference(call(
            &mut e,
            list,
            "iterator",
            "()Ljava/util/Iterator;",
            &[],
        ));
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
        exception(&mut e, it, "Ljava/lang/UnsupportedOperationException;");
        assert_eq!(call(&mut e, list, "size", "()I", &[]), Value::Int(1));
    }
    let mut e = Engine::new(vec![], VmConfig::default());
    let map = obj(&mut e, "Ljava/util/HashMap;");
    call(&mut e, map, "<init>", "()V", &[]);
    call(
        &mut e,
        map,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Int(1), Value::Int(2)],
    );
    let readonly = reference(
        e.invoke_static(
            "Ljava/util/Collections;",
            "unmodifiableMap",
            "(Ljava/util/Map;)Ljava/util/Map;",
            &[Value::Obj(map)],
        )
        .unwrap(),
    );
    for (method, sig) in [
        ("keySet", "()Ljava/util/Set;"),
        ("values", "()Ljava/util/Collection;"),
        ("entrySet", "()Ljava/util/Set;"),
    ] {
        let view = reference(call(&mut e, readonly, method, sig, &[]));
        let it = reference(call(
            &mut e,
            view,
            "iterator",
            "()Ljava/util/Iterator;",
            &[],
        ));
        call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
        exception(&mut e, it, "Ljava/lang/UnsupportedOperationException;");
        assert_eq!(call(&mut e, readonly, "size", "()I", &[]), Value::Int(1));
    }
}

#[test]
fn stale_map_view_remove_is_atomic() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let map = obj(&mut e, "Ljava/util/HashMap;");
    call(&mut e, map, "<init>", "()V", &[]);
    call(
        &mut e,
        map,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[Value::Int(1), Value::Int(2)],
    );
    let view = reference(call(&mut e, map, "keySet", "()Ljava/util/Set;", &[]));
    let it = reference(call(
        &mut e,
        view,
        "iterator",
        "()Ljava/util/Iterator;",
        &[],
    ));
    call(&mut e, it, "next", "()Ljava/lang/Object;", &[]);
    call(&mut e, view, "clear", "()V", &[]);
    exception(&mut e, it, "Ljava/util/ConcurrentModificationException;");
    assert_eq!(call(&mut e, map, "size", "()I", &[]), Value::Int(1));
}
