//! Bounded task history is real VM state, not an MCP navigation shortcut.
#[macro_use]
mod common;
use common::*;
use rd_vm::{Engine, VmExit};

fn engine() -> Engine {
    let mut b = DexBuilder::new();
    b.class("LMain;", "Landroid/app/Activity;");
    b.class("LDetails;", "Landroid/app/Activity;");
    let broken = b.class("LBroken;", "Landroid/app/Activity;");
    let mut code = op11n(0x12, 0, 0);
    code.extend(op11x(0x27, 0)); // throw null during onCreate
    b.r#virtual(
        broken,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 0, code)),
    );
    engine_of(&b)
}
fn code(e: VmExit) -> String {
    match e {
        VmExit::Error(e) => e.code,
        other => panic!("{other:?}"),
    }
}
#[test]
fn suspended_activity_survives_gc_and_back_restores_window_and_focus() {
    let mut e = engine();
    let main = e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    #[cfg(feature = "advanced-components")]
    let focus = {
        let f = e
            .heap
            .alloc_instance("Landroid/widget/EditText;".into(), vec![])
            .unwrap();
        e.attach_host_state("Landroid/widget/EditText;", f).unwrap();
        e.m9_focus(f).unwrap();
        Some(f)
    };
    #[cfg(not(feature = "advanced-components"))]
    let focus: Option<u32> = None;
    let details = e.start_activity("LDetails;", "org.demo", vec![]).unwrap();
    assert_eq!(e.fw.activity_stack.len(), 1);
    assert_eq!(e.fw.activity_stack[0].activity, main);
    assert_eq!(e.fw.activity_stack[0].focused_view, focus);
    #[cfg(feature = "advanced-components")]
    assert_eq!(e.fw.m9.focused, None);
    e.gc();
    assert_eq!(e.heap.class_of(main).unwrap(), "LMain;");
    if let Some(f) = focus {
        assert_eq!(e.heap.class_of(f).unwrap(), "Landroid/widget/EditText;");
    }
    e.finish_activity(details).unwrap();
    assert_eq!(e.fw.current_activity, Some(main));
    #[cfg(feature = "advanced-components")]
    assert_eq!(e.fw.m9.focused, focus);
    assert!(e.fw.activity_stack.is_empty());
    assert!(!e.activity_is_finished());
    e.finish_activity(main).unwrap();
    assert!(e.activity_is_finished());
}
#[test]
fn missing_and_failed_launch_restore_previous_task_without_fake_success() {
    let mut e = engine();
    let main = e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    assert_eq!(
        code(
            e.start_activity("LAbsent;", "org.demo", vec![])
                .unwrap_err()
        ),
        "MISSING_ENTRY"
    );
    assert_eq!(e.fw.current_activity, Some(main));
    assert!(e.fw.activity_stack.is_empty());
    assert!(e.start_activity("LBroken;", "org.demo", vec![]).is_err());
    assert_eq!(e.fw.current_activity, Some(main));
    assert!(e.fw.activity_stack.is_empty());
    assert!(!e.activity_is_finished());
}
#[test]
fn task_history_has_an_enforced_depth_limit() {
    let mut e = engine();
    e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    for _ in 0..64 {
        e.start_activity("LDetails;", "org.demo", vec![]).unwrap();
    }
    let current = e.fw.current_activity;
    assert_eq!(
        code(
            e.start_activity("LDetails;", "org.demo", vec![])
                .unwrap_err()
        ),
        "LIMIT_EXCEEDED"
    );
    assert_eq!(e.fw.current_activity, current);
    assert_eq!(e.fw.activity_stack.len(), 64);
}
#[test]
fn explicit_root_relaunch_replaces_history_and_finish_is_idempotent() {
    let mut e = engine();
    e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    e.start_activity("LDetails;", "org.demo", vec![]).unwrap();
    let root = e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    assert!(e.fw.activity_stack.is_empty());
    e.finish_activity(root).unwrap();
    e.finish_activity(root).unwrap();
    assert!(e.activity_is_finished());
}
#[test]
fn finishing_a_suspended_activity_does_not_finish_the_foreground() {
    let mut e = engine();
    let main = e.launch_app("LMain;", "org.demo", vec![]).unwrap();
    let current = e.start_activity("LDetails;", "org.demo", vec![]).unwrap();
    e.finish_activity(main).unwrap();
    assert_eq!(e.fw.current_activity, Some(current));
    assert!(!e.activity_is_finished());
    e.finish_activity(current).unwrap();
    assert!(e.activity_is_finished());
    assert!(e.fw.activity_stack.is_empty());
}
