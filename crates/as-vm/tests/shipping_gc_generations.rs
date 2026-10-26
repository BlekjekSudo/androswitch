//! Original GC heap/DEX scenarios on the actual shipping VM profile.
#[path = "../../rd-vm/tests/common/mod.rs"]
mod common;
#[path = "../../rd-vm/tests/common/gc_contracts.rs"]
mod shared;
#[path = "../../rd-vm/tests/common/gc_weak_contracts.rs"]
mod weak_shared;

#[test]
fn weak_references_are_cleared() {
    weak_shared::weak_references_are_cleared();
}

#[test]
fn cleared_weak_refs_are_enqueued() {
    weak_shared::cleared_weak_refs_are_enqueued();
}

#[test]
fn minor_remembers_old_fields_arrays_and_promotes_survivors() {
    shared::minor_remembers_old_fields_arrays_and_promotes_survivors();
}

#[test]
fn one_java_call_allocates_many_times_its_heap_budget() {
    shared::one_java_call_allocates_many_times_its_heap_budget();
}

#[test]
fn fuel_window_is_bounded_but_not_a_session_lifetime_limit() {
    shared::fuel_window_is_bounded_but_not_a_session_lifetime_limit();
}
