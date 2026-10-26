//! Generational barriers and collection within a single long DEX invocation.
mod common;
#[path = "common/gc_contracts.rs"]
mod shared;
use rd_vm::{Engine, Value, VmConfig};

#[test]
fn minor_remembers_old_fields_arrays_and_promotes_survivors() {
    shared::minor_remembers_old_fields_arrays_and_promotes_survivors();
}

#[test]
fn old_host_list_writes_do_not_need_a_heap_write_barrier() {
    let mut e = Engine::new(vec![], VmConfig::default());
    let list = e
        .heap
        .alloc_instance("Ljava/util/ArrayList;".into(), vec![])
        .unwrap();
    e.invoke_instance(list, "<init>", "()V", &[]).unwrap();
    e.statics
        .insert(("LRoot;".into(), "list".into()), Value::Obj(list));
    e.gc();
    let child = e.heap.alloc_string("host child".into()).unwrap();
    e.invoke_instance(list, "add", "(Ljava/lang/Object;)Z", &[Value::Obj(child)])
        .unwrap();
    e.gc_minor();
    assert_eq!(e.heap.as_str(child).unwrap(), "host child");
}

#[test]
fn one_java_call_allocates_many_times_its_heap_budget() {
    shared::one_java_call_allocates_many_times_its_heap_budget();
}

#[test]
fn fuel_window_is_bounded_but_not_a_session_lifetime_limit() {
    shared::fuel_window_is_bounded_but_not_a_session_lifetime_limit();
}
