//! Regression tests for issue #316 (`framework.rs`, `view_attach.rs`, `vto.rs`, `view_layout.rs`).

use std::sync::mpsc;
use std::time::Duration;

use rd_vm::err::VmExit;
use rd_vm::heap::ObjRef;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

const LINEAR_LAYOUT: &str = "Landroid/widget/LinearLayout;";
const FRAME_LAYOUT: &str = "Landroid/widget/FrameLayout;";
const VIEW: &str = "Landroid/view/View;";
const TEXT_VIEW: &str = "Landroid/widget/TextView;";
const LAYOUT_PARAMS: &str = "Landroid/view/ViewGroup$LayoutParams;";

fn engine() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn new_view(e: &mut Engine, cls: &str) -> ObjRef {
    let r = e
        .heap
        .alloc_instance(cls.to_string(), Vec::new())
        .expect("alloc view");
    e.attach_host_state(cls, r).expect("attach_host_state");
    e.invoke_instance(r, "<init>", "(Landroid/content/Context;)V", &[Value::Null])
        .expect("<init>(Context)");
    r
}

fn assert_exc(res: Result<Value, VmExit>, expected_class: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_class,
            "unexpected exception {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected_class}, got {other:?}"),
    }
}

#[test]
fn issue_316_self_and_mutual_cycles_rejected_without_host_sigabrt_or_infinite_loop() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut e = engine();
        let vg = new_view(&mut e, FRAME_LAYOUT);

        // 1. Direct self-cycle: vg.addView(vg) -> IllegalArgumentException
        // (Previously accepted, causing unfuelled infinite loop in getViewTreeObserver / getRootView.)
        let self_res =
            e.invoke_instance(vg, "addView", "(Landroid/view/View;)V", &[Value::Obj(vg)]);
        let _ = e.invoke_instance(
            vg,
            "getViewTreeObserver",
            "()Landroid/view/ViewTreeObserver;",
            &[],
        );
        let _ = e.invoke_instance(vg, "getRootView", "()Landroid/view/View;", &[]);
        assert_exc(self_res, "Ljava/lang/IllegalArgumentException;");

        // 2. Mutual 2-node cycle: a.addView(b); b.addView(a) -> IllegalArgumentException
        let a = new_view(&mut e, LINEAR_LAYOUT);
        let b = new_view(&mut e, FRAME_LAYOUT);
        e.invoke_instance(a, "addView", "(Landroid/view/View;)V", &[Value::Obj(b)])
            .expect("a.addView(b)");
        assert_exc(
            e.invoke_instance(b, "addView", "(Landroid/view/View;)V", &[Value::Obj(a)]),
            "Ljava/lang/IllegalArgumentException;",
        );

        // 3. 3-node ancestor cycle: a -> b -> c, then c.addView(a) -> IllegalArgumentException
        let c = new_view(&mut e, LINEAR_LAYOUT);
        e.invoke_instance(b, "addView", "(Landroid/view/View;)V", &[Value::Obj(c)])
            .expect("b.addView(c)");
        assert_exc(
            e.invoke_instance(c, "addView", "(Landroid/view/View;)V", &[Value::Obj(a)]),
            "Ljava/lang/IllegalArgumentException;",
        );
        assert_eq!(
            e.invoke_instance(c, "getRootView", "()Landroid/view/View;", &[])
                .expect("getRootView"),
            Value::Obj(a)
        );

        let _ = tx.send(());
    });

    rx.recv_timeout(Duration::from_millis(250))
        .expect("timed out: View cycle hung vto::root_of / getRootView");
}

#[test]
fn issue_316_self_cycle_remove_all_views_rejects_cycle_without_host_sigabrt() {
    let mut e = engine();
    let vg = new_view(&mut e, FRAME_LAYOUT);
    // On main@38f330d: vg.addView(vg) succeeds and vg.removeAllViews() recurses infinitely
    // inside view_attach::dispatch_detach -> native stack overflow SIGABRT!
    let self_res = e.invoke_instance(vg, "addView", "(Landroid/view/View;)V", &[Value::Obj(vg)]);
    e.invoke_instance(vg, "removeAllViews", "()V", &[])
        .expect("removeAllViews");
    assert_exc(self_res, "Ljava/lang/IllegalArgumentException;");
}

#[test]
fn issue_316_null_non_view_duplicate_parent_and_diamond_dag_rejected() {
    let mut e = engine();
    let vg1 = new_view(&mut e, LINEAR_LAYOUT);
    let vg2 = new_view(&mut e, FRAME_LAYOUT);
    let child = new_view(&mut e, TEXT_VIEW);

    // 1. addView(null) and addViewInLayout(null, ...) -> IllegalArgumentException
    assert_exc(
        e.invoke_instance(vg1, "addView", "(Landroid/view/View;)V", &[Value::Null]),
        "Ljava/lang/IllegalArgumentException;",
    );
    assert_exc(
        e.invoke_instance(
            vg1,
            "addViewInLayout",
            "(Landroid/view/View;ILandroid/view/ViewGroup$LayoutParams;)Z",
            &[Value::Null, Value::Int(-1), Value::Null],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // 2. Non-View ObjRef -> IllegalArgumentException
    let str_ref = e.heap.alloc_string("not a view".into()).expect("str");
    assert_exc(
        e.invoke_instance(
            vg1,
            "addView",
            "(Landroid/view/View;)V",
            &[Value::Obj(str_ref)],
        ),
        "Ljava/lang/IllegalArgumentException;",
    );

    // 3. Adding a child that already has a live parent -> IllegalStateException
    // (Prevents both cross-parent corruption and 2^N diamond DAG amplification!)
    e.invoke_instance(
        vg1,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    )
    .expect("first addView");
    assert_exc(
        e.invoke_instance(
            vg1,
            "addView",
            "(Landroid/view/View;)V",
            &[Value::Obj(child)],
        ),
        "Ljava/lang/IllegalStateException;",
    );
    assert_exc(
        e.invoke_instance(
            vg2,
            "addView",
            "(Landroid/view/View;)V",
            &[Value::Obj(child)],
        ),
        "Ljava/lang/IllegalStateException;",
    );

    // 4. After removeView(child), re-parenting under vg2 succeeds cleanly
    e.invoke_instance(
        vg1,
        "removeView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    )
    .expect("removeView");
    e.invoke_instance(
        vg2,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    )
    .expect("re-parent under vg2");
    assert_eq!(
        e.invoke_instance(child, "getParent", "()Landroid/view/ViewParent;", &[])
            .expect("getParent"),
        Value::Obj(vg2)
    );
}

#[test]
fn issue_316_view_layout_saturates_extreme_coordinates_and_wraps_generate_view_id() {
    let mut e = engine();
    let parent = new_view(&mut e, FRAME_LAYOUT);
    let child = new_view(&mut e, VIEW);
    e.invoke_instance(
        parent,
        "addView",
        "(Landroid/view/View;)V",
        &[Value::Obj(child)],
    )
    .expect("addView");

    // 1. Extreme layout bounds (-1, -1, MAX, MAX) must saturate width/height instead of panicking
    e.invoke_instance(
        parent,
        "layout",
        "(IIII)V",
        &[
            Value::Int(i32::MAX - 4),
            Value::Int(i32::MAX - 4),
            Value::Int(i32::MAX),
            Value::Int(i32::MAX),
        ],
    )
    .expect("parent layout near MAX");

    e.invoke_instance(
        child,
        "layout",
        "(IIII)V",
        &[
            Value::Int(-1),
            Value::Int(-1),
            Value::Int(i32::MAX),
            Value::Int(i32::MAX),
        ],
    )
    .expect("child layout (-1, -1, MAX, MAX)");
    assert_eq!(
        e.invoke_instance(child, "getWidth", "()I", &[])
            .expect("getWidth"),
        Value::Int(i32::MAX)
    );
    assert_eq!(
        e.invoke_instance(child, "getHeight", "()I", &[])
            .expect("getHeight"),
        Value::Int(i32::MAX)
    );

    // 2. offsetLeftAndRight / offsetTopAndBottom / setRight / getRight / getBottom saturate
    e.invoke_instance(child, "offsetLeftAndRight", "(I)V", &[Value::Int(i32::MAX)])
        .expect("offsetLeftAndRight(MAX)");
    e.invoke_instance(child, "offsetTopAndBottom", "(I)V", &[Value::Int(i32::MAX)])
        .expect("offsetTopAndBottom(MAX)");
    assert_eq!(
        e.invoke_instance(child, "getRight", "()I", &[])
            .expect("getRight"),
        Value::Int(i32::MAX)
    );
    assert_eq!(
        e.invoke_instance(child, "getBottom", "()I", &[])
            .expect("getBottom"),
        Value::Int(i32::MAX)
    );
    e.invoke_instance(child, "setRight", "(I)V", &[Value::Int(i32::MAX)])
        .expect("setRight(MAX)");

    // 3. View.generateViewId() stays in 1..=0x00FF_FFFF
    let id1 = e
        .invoke_static(VIEW, "generateViewId", "()I", &[])
        .expect("generateViewId 1")
        .as_int()
        .expect("int");
    let id2 = e
        .invoke_static(VIEW, "generateViewId", "()I", &[])
        .expect("generateViewId 2")
        .as_int()
        .expect("int");
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
}

#[test]
fn issue_316_add_view_and_add_view_in_layout_honor_index_and_width_height_overload() {
    let mut e = engine();
    let vg = new_view(&mut e, LINEAR_LAYOUT);
    let v0 = new_view(&mut e, VIEW);
    let v1 = new_view(&mut e, VIEW);
    let v2 = new_view(&mut e, VIEW);
    let v_head = new_view(&mut e, VIEW);

    e.invoke_instance(vg, "addView", "(Landroid/view/View;)V", &[Value::Obj(v0)])
        .expect("add v0");
    e.invoke_instance(vg, "addView", "(Landroid/view/View;)V", &[Value::Obj(v2)])
        .expect("add v2");
    // Insert v1 at index 1 and v_head at index 0 -> [v_head, v0, v1, v2]
    e.invoke_instance(
        vg,
        "addView",
        "(Landroid/view/View;I)V",
        &[Value::Obj(v1), Value::Int(1)],
    )
    .expect("add v1 at 1");
    e.invoke_instance(
        vg,
        "addView",
        "(Landroid/view/View;I)V",
        &[Value::Obj(v_head), Value::Int(0)],
    )
    .expect("add v_head at 0");

    for (idx, expected) in [v_head, v0, v1, v2].into_iter().enumerate() {
        assert_eq!(
            e.invoke_instance(
                vg,
                "getChildAt",
                "(I)Landroid/view/View;",
                &[Value::Int(idx as i32)],
            )
            .expect("getChildAt"),
            Value::Obj(expected),
            "mismatch at index {idx}"
        );
    }

    // Out-of-bounds index (< -1 or > childCount) -> IndexOutOfBoundsException
    let v_oob = new_view(&mut e, VIEW);
    assert_exc(
        e.invoke_instance(
            vg,
            "addView",
            "(Landroid/view/View;I)V",
            &[Value::Obj(v_oob), Value::Int(-2)],
        ),
        "Ljava/lang/IndexOutOfBoundsException;",
    );
    assert_exc(
        e.invoke_instance(
            vg,
            "addView",
            "(Landroid/view/View;I)V",
            &[Value::Obj(v_oob), Value::Int(5)],
        ),
        "Ljava/lang/IndexOutOfBoundsException;",
    );

    // addView(View, width=-2 WRAP_CONTENT, height=-1 MATCH_PARENT) must treat args[1..=2] as dimensions, NOT index!
    let v_wh = new_view(&mut e, VIEW);
    e.invoke_instance(
        vg,
        "addView",
        "(Landroid/view/View;II)V",
        &[Value::Obj(v_wh), Value::Int(-2), Value::Int(-1)],
    )
    .expect("addView(View, width, height)");
    assert_eq!(
        e.invoke_instance(vg, "getChildAt", "(I)Landroid/view/View;", &[Value::Int(4)],)
            .expect("getChildAt(4)"),
        Value::Obj(v_wh)
    );

    // addViewInLayout(View, index=2, LayoutParams) inserts at index 2
    let v_in_layout = new_view(&mut e, VIEW);
    let lp = e
        .heap
        .alloc_instance(LAYOUT_PARAMS.to_string(), Vec::new())
        .expect("alloc lp");
    e.invoke_instance(lp, "<init>", "(II)V", &[Value::Int(50), Value::Int(30)])
        .expect("lp.<init>");
    assert_eq!(
        e.invoke_instance(
            vg,
            "addViewInLayout",
            "(Landroid/view/View;ILandroid/view/ViewGroup$LayoutParams;)Z",
            &[Value::Obj(v_in_layout), Value::Int(2), Value::Obj(lp)],
        )
        .expect("addViewInLayout"),
        Value::Int(1)
    );
    assert_eq!(
        e.invoke_instance(vg, "getChildAt", "(I)Landroid/view/View;", &[Value::Int(2)],)
            .expect("getChildAt(2)"),
        Value::Obj(v_in_layout)
    );
}
