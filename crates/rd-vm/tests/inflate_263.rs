//! Regression tests for issue #263:
//! 1. Scrollable ancestors (`<ScrollView>`, `<HorizontalScrollView>`, etc.)
//!    allow content dimensions larger than the single-screen viewport
//!    (`720x1440`), up to `MAX_LAYOUT_DIM_PX` (`65_536px`), so real multi-page
//!    scrollable layouts such as `1000dp` (`2000px`) inflate cleanly.
//! 2. `ref_dimension_px` enforces the axis cap on `ResolvedValue::Integer(i)`
//!    instead of letting `i32::MAX` bypass bounds validation.

#[macro_use]
mod common;

use common::apkfix::{
    a, build_apk, build_axml, ArscFix, AxNode, AxVal, TYPE_DIMENSION, TYPE_FIRST_INT,
};
use common::*;
use rd_apk::arsc::ResolvedValue;
use rd_vm::err::VmExit;
use rd_vm::value::Value;

fn register_platform_classes(b: &mut DexBuilder) {
    b.class("Landroid/view/View;", "Ljava/lang/Object;");
    b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
    b.class("Landroid/widget/TextView;", "Landroid/view/View;");
    b.class("Landroid/widget/Button;", "Landroid/widget/TextView;");
    b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
    b.class("Landroid/widget/FrameLayout;", "Landroid/view/ViewGroup;");
    b.class(
        "Landroid/widget/ScrollView;",
        "Landroid/widget/FrameLayout;",
    );
    b.class(
        "Landroid/widget/HorizontalScrollView;",
        "Landroid/widget/FrameLayout;",
    );
    b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
    b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
}

fn op31i(op: u8, reg: u8, lit: i32) -> Vec<u16> {
    vec![
        (op as u16) | ((reg as u16) << 8),
        (lit & 0xFFFF) as u16,
        ((lit >> 16) & 0xFFFF) as u16,
    ]
}

fn engine_with_layout(apk_bytes: Vec<u8>, activity: &str, layout: u32) -> rd_vm::Engine {
    let mut b = DexBuilder::new();
    register_platform_classes(&mut b);
    let p_iv = b.proto_idx("V", vec!["I".to_string()]);
    let set_content_i = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
    let main = b.class(activity, "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(4, 2, 2, {
            let mut u = op31i(0x14, 1, layout as i32);
            u.extend(op35c(0x6E, 2, set_content_i, [2, 1, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
    let mut e = engine_of(&b);
    e.set_resources(rd_apk::Apk::from_bytes(apk_bytes).expect("synthetic APK parses"));
    e
}

/// Patch a sentinel `@dimen` entry emitted by `ArscFix` (`TYPE_DIMENSION`) into
/// an exact `TYPE_FIRST_INT` (`ResolvedValue::Integer(int_val)`) entry.
fn patch_dimen_sentinel_to_int(arsc_bytes: &mut [u8], sentinel_dp: i32, int_val: i32) {
    let raw = ((sentinel_dp as u32) << 8) | 1;
    let mut needle = [0u8; 8];
    needle[0..2].copy_from_slice(&8u16.to_le_bytes());
    needle[2] = 0;
    needle[3] = TYPE_DIMENSION;
    needle[4..8].copy_from_slice(&raw.to_le_bytes());

    let mut replacement = [0u8; 8];
    replacement[0..2].copy_from_slice(&8u16.to_le_bytes());
    replacement[2] = 0;
    replacement[3] = TYPE_FIRST_INT;
    replacement[4..8].copy_from_slice(&(int_val as u32).to_le_bytes());

    let pos = arsc_bytes
        .windows(8)
        .position(|w| w == needle)
        .expect("sentinel Res_value found in synthetic ARSC");
    arsc_bytes[pos..pos + 8].copy_from_slice(&replacement);
}

#[test]
fn scrollview_allows_1000dp_tall_children_and_layout_params() {
    let mut arsc = ArscFix::new();
    let l_main = arsc.add_layout("main");
    let id_inner = arsc.add_id("inner");
    let d_tall = arsc.add_dimen_dp("tall_scroll_section", 1000); // 1000dp -> 2000px

    let root = AxNode::new(
        "ScrollView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "LinearLayout",
            vec![
                a(true, "id", AxVal::Ref(id_inner)),
                a(true, "orientation", AxVal::Int(1)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(
                    true,
                    "layout_height",
                    AxVal::Dim {
                        raw: (1000 << 8) | 1,
                    },
                ), // 1000dp = 2000px
            ],
            vec![
                AxNode::new(
                    "TextView",
                    vec![
                        a(true, "text", AxVal::Str("top".into())),
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(
                            true,
                            "layout_height",
                            AxVal::Dim {
                                raw: (1000 << 8) | 1,
                            },
                        ), // 2000px
                    ],
                    vec![],
                ),
                AxNode::new(
                    "TextView",
                    vec![
                        a(true, "text", AxVal::Str("bottom".into())),
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Ref(d_tall)), // @dimen -> 2000px
                    ],
                    vec![],
                ),
            ],
        )],
    );

    let apk_bytes = build_apk(
        "com.test.scroll263",
        "LScrollMain;",
        arsc.build(),
        vec![("res/layout/main.xml", build_axml(&root))],
    );
    let mut e = engine_with_layout(apk_bytes, "LScrollMain;", l_main);
    e.launch_app("LScrollMain;", "com.test.scroll263", Vec::new())
        .expect("ScrollView with 1000dp (2000px) child must inflate cleanly");

    let tree = e.view_tree().expect("view tree");
    assert_eq!(tree.children.len(), 1);
    let inner = &tree.children[0];
    assert_eq!(inner.children[0].bounds.3, 2000);
    assert_eq!(inner.children[1].bounds.3, 2000);
    assert_eq!(inner.bounds.3, 4000);

    // Also verify LayoutParams.height on the 1000dp LinearLayout inside ScrollView is 2000px.
    let act = e.fw.current_activity.expect("activity");
    let found = e
        .invoke_instance(
            act,
            "findViewById",
            "(I)Landroid/view/View;",
            &[Value::Int(id_inner as i32)],
        )
        .expect("findViewById");
    let Value::Obj(inner_ref) = found else {
        panic!("expected inner LinearLayout ref, got {found:?}");
    };
    let lp = e
        .invoke_instance(
            inner_ref,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .expect("getLayoutParams");
    let Value::Obj(lp_ref) = lp else {
        panic!("expected LayoutParams ref, got {lp:?}");
    };
    assert_eq!(e.heap.get_field(lp_ref, "height"), Ok(Value::Int(2000)));
}

#[test]
fn horizontal_scrollview_allows_1000dp_wide_child() {
    let mut arsc = ArscFix::new();
    let l_main = arsc.add_layout("main");
    let id_row = arsc.add_id("wide_row");
    let d_wide = arsc.add_dimen_dp("wide_block", 900); // 900dp -> 1800px (> 720px WINDOW_W)

    let root = AxNode::new(
        "HorizontalScrollView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "LinearLayout",
            vec![
                a(true, "id", AxVal::Ref(id_row)),
                a(true, "orientation", AxVal::Int(0)),
                a(
                    true,
                    "layout_width",
                    AxVal::Dim {
                        raw: (1000 << 8) | 1,
                    },
                ), // 1000dp = 2000px
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            vec![AxNode::new(
                "TextView",
                vec![
                    a(true, "text", AxVal::Str("wide".into())),
                    a(true, "layout_width", AxVal::Ref(d_wide)), // 1800px
                    a(true, "layout_height", AxVal::Int(-2)),
                ],
                vec![],
            )],
        )],
    );

    let apk_bytes = build_apk(
        "com.test.hscroll263",
        "LHScrollMain;",
        arsc.build(),
        vec![("res/layout/main.xml", build_axml(&root))],
    );
    let mut e = engine_with_layout(apk_bytes, "LHScrollMain;", l_main);
    e.launch_app("LHScrollMain;", "com.test.hscroll263", Vec::new())
        .expect("HorizontalScrollView with 1000dp (2000px) child must inflate cleanly");

    let act = e.fw.current_activity.expect("activity");
    let Value::Obj(row_ref) = e
        .invoke_instance(
            act,
            "findViewById",
            "(I)Landroid/view/View;",
            &[Value::Int(id_row as i32)],
        )
        .expect("findViewById")
    else {
        panic!("expected wide_row ref");
    };
    let Value::Obj(lp_ref) = e
        .invoke_instance(
            row_ref,
            "getLayoutParams",
            "()Landroid/view/ViewGroup$LayoutParams;",
            &[],
        )
        .expect("getLayoutParams")
    else {
        panic!("expected LayoutParams ref");
    };
    assert_eq!(e.heap.get_field(lp_ref, "width"), Ok(Value::Int(2000)));
}

#[test]
fn integer_resource_i32_max_rejected_with_typed_inflate_error() {
    const SENTINEL_MAX_DP: i32 = 0x13579;
    const SENTINEL_OK_DP: i32 = 0x2468A;

    // 1. Verify a hostile ResolvedValue::Integer(i32::MAX) is rejected even inside ScrollView,
    //    for leaf layout_height, container LayoutParams, and layout_margin.
    for case in ["leaf_height", "container_height", "margin"] {
        let mut arsc = ArscFix::new();
        let l_main = arsc.add_layout("main");
        let r_max = arsc.add_dimen_dp("int_max", SENTINEL_MAX_DP);
        let mut arsc_bytes = arsc.build();
        patch_dimen_sentinel_to_int(&mut arsc_bytes, SENTINEL_MAX_DP, i32::MAX);

        let child = match case {
            "leaf_height" => AxNode::new(
                "TextView",
                vec![
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Ref(r_max)),
                ],
                vec![],
            ),
            "container_height" => AxNode::new(
                "LinearLayout",
                vec![
                    a(true, "orientation", AxVal::Int(1)),
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Ref(r_max)),
                ],
                vec![AxNode::new(
                    "TextView",
                    vec![
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Int(-2)),
                    ],
                    vec![],
                )],
            ),
            _ => AxNode::new(
                "TextView",
                vec![
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Int(-2)),
                    a(true, "layout_marginTop", AxVal::Ref(r_max)),
                ],
                vec![],
            ),
        };

        let root = AxNode::new(
            "ScrollView",
            vec![
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            vec![child],
        );

        let apk_bytes = build_apk(
            "com.test.intmax263",
            "LIntMaxMain;",
            arsc_bytes,
            vec![("res/layout/main.xml", build_axml(&root))],
        );
        let mut e = engine_with_layout(apk_bytes, "LIntMaxMain;", l_main);
        assert_eq!(
            e.resolve_resource_value(r_max).expect("resource resolves"),
            ResolvedValue::Integer(i32::MAX),
            "fixture must exercise ResolvedValue::Integer(i32::MAX)"
        );

        let err = e
            .launch_app("LIntMaxMain;", "com.test.intmax263", Vec::new())
            .expect_err("ResolvedValue::Integer(i32::MAX) must fail inflation");
        match err {
            VmExit::Error(rd) => {
                assert_eq!(
                    rd.code, "INFLATE",
                    "case={case}: expected INFLATE, got {rd:?}"
                );
                assert!(
                    rd.cause.contains("2147483647") && rd.cause.contains("viewport"),
                    "case={case}: cause must mention offending px and cap: {}",
                    rd.cause
                );
            }
            other => panic!("case={case}: expected typed VmExit::Error(INFLATE), got {other:?}"),
        }
    }

    // 2. Verify a valid positive integer resource within cap (300px) still works cleanly.
    let mut arsc_ok = ArscFix::new();
    let l_ok = arsc_ok.add_layout("main");
    let r_ok = arsc_ok.add_dimen_dp("int_ok", SENTINEL_OK_DP);
    let mut arsc_ok_bytes = arsc_ok.build();
    patch_dimen_sentinel_to_int(&mut arsc_ok_bytes, SENTINEL_OK_DP, 300);

    let root_ok = AxNode::new(
        "LinearLayout",
        vec![
            a(true, "orientation", AxVal::Int(1)),
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Ref(r_ok)),
            ],
            vec![],
        )],
    );
    let apk_ok = build_apk(
        "com.test.intok263",
        "LIntOkMain;",
        arsc_ok_bytes,
        vec![("res/layout/main.xml", build_axml(&root_ok))],
    );
    let mut e_ok = engine_with_layout(apk_ok, "LIntOkMain;", l_ok);
    e_ok.launch_app("LIntOkMain;", "com.test.intok263", Vec::new())
        .expect("ResolvedValue::Integer(300) within cap must inflate cleanly");
    let tree_ok = e_ok.view_tree().expect("view tree");
    assert_eq!(tree_ok.children[0].bounds.3, 300);
}

#[test]
fn scrollview_still_rejects_dimension_above_anti_overflow_cap() {
    let mut arsc = ArscFix::new();
    let l_main = arsc.add_layout("main");
    // 40_000dp * DENSITY 2.0 = 80_000px > MAX_LAYOUT_DIM_PX (65_536)
    let root = AxNode::new(
        "ScrollView",
        vec![
            a(true, "layout_width", AxVal::Int(-1)),
            a(true, "layout_height", AxVal::Int(-1)),
        ],
        vec![AxNode::new(
            "TextView",
            vec![
                a(true, "layout_width", AxVal::Int(-1)),
                a(
                    true,
                    "layout_height",
                    AxVal::Dim {
                        raw: (40_000 << 8) | 1,
                    },
                ),
            ],
            vec![],
        )],
    );
    let apk_bytes = build_apk(
        "com.test.scrollcap263",
        "LScrollCapMain;",
        arsc.build(),
        vec![("res/layout/main.xml", build_axml(&root))],
    );
    let mut e = engine_with_layout(apk_bytes, "LScrollCapMain;", l_main);
    let err = e
        .launch_app("LScrollCapMain;", "com.test.scrollcap263", Vec::new())
        .expect_err("dimension > 65_536px inside ScrollView must still be rejected");
    match err {
        VmExit::Error(rd) => {
            assert_eq!(rd.code, "INFLATE");
            assert!(rd.cause.contains("65536"), "cause: {}", rd.cause);
        }
        other => panic!("expected VmExit::Error(INFLATE), got {other:?}"),
    }
}
