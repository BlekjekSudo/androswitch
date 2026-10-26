//! Original RustyDroid heap/generation/DEX scenarios, shared unchanged by
//! the reference suite and the shipping-profile test target.
use super::common::*;
use rd_vm::heap::ElemKind;
use rd_vm::{Engine, Value, VmConfig};

pub fn minor_remembers_old_fields_arrays_and_promotes_survivors() {
    let mut e = Engine::new(vec![], VmConfig::default());
    for _ in 0..64 {
        e.heap.alloc_string("padding".into()).unwrap();
    }
    let owner = e.heap.alloc_instance("LOwner;".into(), vec![]).unwrap();
    let arr = e
        .heap
        .alloc_array(ElemKind::Obj, 1, "Ljava/lang/Object;")
        .unwrap();
    *e.heap.array_elems_mut(arr).unwrap() = vec![Value::Null];
    e.statics
        .insert(("LRoot;".into(), "owner".into()), Value::Obj(owner));
    e.statics
        .insert(("LRoot;".into(), "array".into()), Value::Obj(arr));
    e.gc();
    let child = e.heap.alloc_string("field child".into()).unwrap();
    let array_child = e.heap.alloc_string("array child".into()).unwrap();
    let garbage = e.heap.alloc_string("garbage".into()).unwrap();
    e.heap.put_field(owner, "child", Value::Obj(child)).unwrap();
    e.heap.array_elems_mut(arr).unwrap()[0] = Value::Obj(array_child);
    let st = e.gc_minor();
    assert_eq!(st.last_kind, "minor");
    assert_eq!(e.heap.as_str(child).unwrap(), "field child");
    assert_eq!(e.heap.as_str(array_child).unwrap(), "array child");
    assert_eq!(e.heap.class_of(garbage).unwrap(), rd_vm::gc::FREED_CLASS);
    // No new writes: promoted old graph survives without tracing it again.
    let st = e.gc_minor();
    assert_eq!(st.last_traced_objects, 0);
    e.statics.clear();
    assert_eq!(
        e.gc_minor().last_freed_objects,
        0,
        "old garbage waits for major"
    );
    let st = e.gc();
    assert!(st.last_freed_objects >= 4, "{st:?}");
}

pub fn one_java_call_allocates_many_times_its_heap_budget() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    let array_type = b.type_idx("[B");
    // v0=0; v1=1000; v2=32767; do { v3=new byte[v2]; v0++; } while(v0<v1)
    // ~1 GiB allocated inside one call under a 4 MiB budget.
    let mut units = op11n(0x12, 0, 0);
    units.extend(op21s(0x13, 1, 1000));
    units.extend(op21s(0x13, 2, 32767));
    let start = units.len();
    units.extend(op22c(0x23, 3, 2, array_type));
    units.extend(op22b(0xD8, 0, 0, 1));
    let branch = units.len();
    units.extend(op22t(0x34, 0, 1, start as i16 - branch as i16));
    units.extend(op11x(0x0F, 0));
    b.direct(
        cls,
        "run",
        "I",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(4, 0, 0, units)),
    );
    let dex = rd_dex::Dex::parse(b.finish()).unwrap();
    let cfg = VmConfig {
        heap_budget: 4 << 20,
        fuel: 100_000,
        ..VmConfig::default()
    };
    let mut e = Engine::new(vec![dex], cfg);
    let sentinel = e
        .heap
        .alloc_string("Rust caller local survives".into())
        .unwrap();
    assert_eq!(
        e.invoke_static("LCaso;", "run", "()I", &[]).unwrap(),
        Value::Int(1000)
    );
    assert!(e.gc_stats().collections > 100, "{:?}", e.gc_stats());
    assert_eq!(
        e.heap.as_str(sentinel).unwrap(),
        "Rust caller local survives"
    );
    assert!(e.heap_peak() <= 4 << 20);
}

pub fn fuel_window_is_bounded_but_not_a_session_lifetime_limit() {
    let mut b = DexBuilder::new();
    let cls = b.class("LCaso;", "Ljava/lang/Object;");
    b.direct(
        cls,
        "run",
        "I",
        vec!["I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 1, 0, op11x(0x0f, 0))),
    );
    let mut e = engine_of(&b);
    e.config.fuel = 5;
    for _ in 0..5 {
        assert_eq!(
            e.invoke_static("LCaso;", "run", "(I)I", &[Value::Int(7)])
                .unwrap(),
            Value::Int(7)
        );
    }
    assert!(e
        .invoke_static("LCaso;", "run", "(I)I", &[Value::Int(7)])
        .is_err());
    e.reset_fuel_window();
    assert_eq!(e.fuel_remaining(), 5);
    assert_eq!(
        e.invoke_static("LCaso;", "run", "(I)I", &[Value::Int(7)])
            .unwrap(),
        Value::Int(7)
    );
    assert_eq!(e.fuel_used(), 7);
}
