//! Regression for Issue #232, item 1: `Engine::new_instance` and `invoke_instance`
//! on host shim classes (no DEX body) must reach the same host chain as
//! `interp::do_invoke`, so `<init>` of GMS/AndroidX shims does not fail with
//! `UNRESOLVED_METHOD`.

use rd_testsupport::vm::*;
use rd_vm::Value;

#[test]
fn new_instance_task_completion_source_runs_host_init() {
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e
        .new_instance("Lcom/google/android/gms/tasks/TaskCompletionSource;")
        .expect("TaskCompletionSource <init> must resolve on the host shim");
    assert_eq!(
        e.heap.class_of(r).expect("class"),
        "Lcom/google/android/gms/tasks/TaskCompletionSource;"
    );
}

#[test]
fn new_instance_androidx_viewmodel_store_runs_host_init() {
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e
        .new_instance("Landroidx/lifecycle/ViewModelStore;")
        .expect("ViewModelStore <init> must resolve on the androidx shim");
    assert_eq!(
        e.heap.class_of(r).expect("class"),
        "Landroidx/lifecycle/ViewModelStore;"
    );
}

#[test]
fn invoke_instance_viewmodel_get_tag_on_host_instance_is_null() {
    let b = DexBuilder::new();
    let mut e = engine_of(&b);
    let r = e
        .new_instance("Landroidx/lifecycle/ViewModel;")
        .expect("ViewModel <init> on host shim");
    let got = e
        .invoke_instance(
            r,
            "getTag",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[Value::StrPlaceholder("missing".into())],
        )
        .expect("getTag via Engine::invoke_instance");
    assert_eq!(got, Value::Null);
}
