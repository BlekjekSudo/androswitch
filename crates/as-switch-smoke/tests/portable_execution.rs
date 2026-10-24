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

fn fixture(variant: &str) -> rd_dex::Dex {
    rd_dex::Dex::parse(
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/portable-execution-fixture")
                .join(variant)
                .join("dex/classes.dex"),
        )
        .expect("run crates/rd-vm/tests/fixtures/portable-execution/build.ps1"),
    )
    .unwrap()
}
const VARIANTS: [(&str, &str); 2] = [
    ("original", "org/androswitch/vm/execution"),
    ("renamed", "org/compat/renamed/execution"),
];

#[test]
fn native_local_returns_and_errors_survive_real_dex_callback_gc_then_release() {
    portable();
    for (variant, prefix) in VARIANTS {
        contracts::native_host_allocations_survive_callback_gc_then_release(
            fixture(variant),
            prefix,
        );
    }
}

#[test]
fn class_initialization_rejects_another_owners_partial_statics_and_preserves_recursion() {
    portable();
    for (variant, prefix) in VARIANTS {
        contracts::cross_owner_initialization_rejects_partial_reads_without_poisoning_owner(
            fixture(variant),
            prefix,
        );
    }
}

#[test]
fn unknown_initializer_owner_cannot_mutate_defaults_and_valid_owner_can_retry() {
    portable();
    for (variant, prefix) in VARIANTS {
        contracts::initialization_identity_failure_leaves_unstarted_class_retryable(
            fixture(variant),
            prefix,
        );
    }
}

#[test]
fn started_initializer_runtime_failure_preserves_code_and_is_not_retried() {
    portable();
    for (variant, prefix) in VARIANTS {
        contracts::initializer_infrastructure_failure_stays_typed_and_cleans_running_owner(
            fixture(variant),
            prefix,
        );
    }
}

#[test]
fn captured_native_scope_return_is_owner_exact_and_stops_at_guest_boundary() {
    portable();
    contracts::captured_scope_return_never_pins_into_another_owner_or_guest();
}

#[test]
fn ordinary_jvm_class_initialization_rows_match_original_and_renamed_dex() {
    portable();
    use rd_vm::{Engine, Value, VmConfig};
    for (variant, prefix) in VARIANTS {
        let oracle = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/portable-execution-fixture")
                .join(variant)
                .join("jvm-oracle.txt"),
        )
        .unwrap();
        assert_eq!(oracle.lines().count(), 4);
        for _ in 0..2 {
            let mut vm = Engine::new(vec![fixture(variant)], VmConfig::default());
            for row in oracle.lines() {
                let (index, expected) = row.split_once('\t').unwrap();
                assert_eq!(
                    vm.invoke_static(
                        &format!("L{prefix}/ExecutionFixture;"),
                        "run",
                        "(I)I",
                        &[Value::Int(index.parse().unwrap())]
                    )
                    .unwrap(),
                    Value::Int(expected.parse().unwrap()),
                    "{variant}: {row}",
                );
                vm.gc();
            }
        }
    }
}
