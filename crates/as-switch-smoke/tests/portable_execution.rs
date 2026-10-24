//! Isolated real no_std VM contracts. rd-vm's own std-oriented dev-dependencies
//! must not silently unify std into the VM under test or turn this into 0 tests.
use rd_vm::portable_test_support as contracts;

#[allow(clippy::assertions_on_constants)]
fn portable() {
    assert!(rd_vm::PORTABLE_CORE, "run this package/target in isolation");
}

#[test]
fn real_gc_keeps_the_native_suspended_owner_alive() {
    portable();
    contracts::real_gc_retains_suspended_owner_after_older_owner_publishes_and_unwinds();
}

#[test]
fn non_lifo_frames_keep_separate_roots_and_diagnostics() {
    portable();
    contracts::independent_frames_resume_and_unwind_in_non_lifo_order();
}

#[test]
fn same_owner_jni_reentry_has_distinct_tickets() {
    portable();
    contracts::same_owner_nested_callbacks_keep_distinct_frame_tickets();
}

#[test]
fn owner_metadata_is_bounded_and_pruned() {
    portable();
    contracts::owner_bound_is_checked_and_completed_owners_do_not_accumulate();
}

#[test]
fn heap_scope_restore_and_allocation_pins_are_owner_specific() {
    portable();
    contracts::heap_restore_and_allocation_pins_use_their_own_actual_owner();
}

#[test]
fn guest_allocations_do_not_inherit_another_owners_host_pins() {
    portable();
    contracts::guest_allocations_do_not_leak_into_another_owners_host_scope();
}
