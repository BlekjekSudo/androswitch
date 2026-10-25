//! Regression tests for Issue #301:
//! 1. Host slice & signed `i32` arithmetic overflow panics in `Matrix.mapPoints`,
//!    `Bitmap.createBitmap` / `Bitmap.getPixels`, `Canvas.drawText` /
//!    `Paint.{measureText,getTextBounds}`, and `RenderNode` position/offset helpers.
//! 2. Exponential host OOM outside `VmConfig::heap_budget` via `Path.addPath` /
//!    `Path.op` contour doubling and unbounded `Canvas.save` stacks.
//! 3. Exponential `2^N` host hang when traversing `RenderNode` DAGs in
//!    `frame_op_count`, `rasterize_frame`, `frame_summary`, and `screen_model`.

mod common;

use common::*;
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef};

fn new_obj(vm: &mut Engine, class: &str, sig: &str, args: &[Value]) -> ObjRef {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .expect("alloc_instance");
    vm.invoke_instance(r, "<init>", sig, args)
        .expect("<init> ok");
    r
}

fn float_arr(vm: &mut Engine, vals: &[f32]) -> ObjRef {
    let a = vm
        .heap
        .alloc_array_zeroed(ElemKind::Float, vals.len(), "[F")
        .expect("alloc [F");
    let slots = vm.heap.array_elems_mut(a).expect("[F elems");
    for (dst, &v) in slots.iter_mut().zip(vals) {
        *dst = Value::Float(v);
    }
    a
}

fn char_arr(vm: &mut Engine, s: &str) -> ObjRef {
    let units: Vec<u16> = s.encode_utf16().collect();
    let a = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, units.len(), "[C")
        .expect("alloc [C");
    let slots = vm.heap.array_elems_mut(a).expect("[C elems");
    for (dst, u) in slots.iter_mut().zip(units) {
        *dst = Value::Int(i32::from(u));
    }
    a
}

fn int_arr(vm: &mut Engine, vals: &[i32]) -> ObjRef {
    let a = vm
        .heap
        .alloc_array_zeroed(ElemKind::Int, vals.len(), "[I")
        .expect("alloc [I");
    let slots = vm.heap.array_elems_mut(a).expect("[I elems");
    for (dst, &v) in slots.iter_mut().zip(vals) {
        *dst = Value::Int(v);
    }
    a
}

fn assert_exc(res: Result<Value, VmExit>, expected_desc: &str) {
    match res {
        Err(VmExit::Exception(ex)) => {
            assert_eq!(ex.class, expected_desc, "unexpected exception descriptor");
        }
        other => panic!("expected exception {expected_desc}, got {other:?}"),
    }
}

#[test]
fn matrix_map_points_overflow_throws_aioobe_301() {
    let mut vm = engine_of(&DexBuilder::new());
    let matrix = new_obj(&mut vm, "Landroid/graphics/Matrix;", "()V", &[]);
    let empty_dst = float_arr(&mut vm, &[]);
    let empty_src = float_arr(&mut vm, &[]);

    // Exact #301 1a PoC: count << 1 wraps to -0x6000_0000 in 32-bit signed math,
    // making si + (count << 1) == 0 and bypassing naive signed stop checks.
    let res = vm.invoke_instance(
        matrix,
        "mapPoints",
        "([FI[FII)V",
        &[
            Value::Obj(empty_dst),
            Value::Int(0x6000_0000),
            Value::Obj(empty_src),
            Value::Int(0x6000_0000),
            Value::Int(0x5000_0000),
        ],
    );
    assert_exc(res, "Ljava/lang/ArrayIndexOutOfBoundsException;");

    // Extreme counts and negative indices must also throw AIOOBE cleanly.
    let four_dst = float_arr(&mut vm, &[0.0; 4]);
    let four_src = float_arr(&mut vm, &[1.0, 2.0, 3.0, 4.0]);
    for (di, si, count) in [
        (0, 0, i32::MAX),
        (1, 0, 2),
        (0, 1, 2),
        (-1, 0, 1),
        (0, -1, 1),
        (0, 0, -1),
    ] {
        let r = vm.invoke_instance(
            matrix,
            "mapPoints",
            "([FI[FII)V",
            &[
                Value::Obj(four_dst),
                Value::Int(di),
                Value::Obj(four_src),
                Value::Int(si),
                Value::Int(count),
            ],
        );
        assert_exc(r, "Ljava/lang/ArrayIndexOutOfBoundsException;");
    }

    // Valid in-bounds transform still maps points accurately.
    vm.invoke_instance(
        matrix,
        "setTranslate",
        "(FF)V",
        &[Value::Float(10.0), Value::Float(20.0)],
    )
    .expect("setTranslate");
    vm.invoke_instance(
        matrix,
        "mapPoints",
        "([FI[FII)V",
        &[
            Value::Obj(four_dst),
            Value::Int(0),
            Value::Obj(four_src),
            Value::Int(0),
            Value::Int(2),
        ],
    )
    .expect("valid mapPoints");
    let elems = vm.heap.as_array_elems(four_dst).expect("dst elems").0;
    assert_eq!(
        elems,
        &[
            Value::Float(11.0),
            Value::Float(22.0),
            Value::Float(13.0),
            Value::Float(24.0),
        ]
    );
}

#[test]
fn bitmap_overflow_and_get_pixels_bounds_301() {
    let mut vm = engine_of(&DexBuilder::new());
    let cfg_name = Value::Obj(vm.heap.alloc_string("ARGB_8888".into()).expect("str"));
    let argb_8888 = vm
        .invoke_static(
            "Landroid/graphics/Bitmap$Config;",
            "valueOf",
            "(Ljava/lang/String;)Landroid/graphics/Bitmap$Config;",
            &[cfg_name],
        )
        .expect("Config.ARGB_8888");

    // Extreme dimensions that would overflow w*4 or w*h*4 must throw IAE.
    for (w, h) in [(i32::MAX, i32::MAX), (24_000, 24_000), (0, 10), (10, -1)] {
        let res = vm.invoke_static(
            "Landroid/graphics/Bitmap;",
            "createBitmap",
            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            &[Value::Int(w), Value::Int(h), argb_8888.clone()],
        );
        assert_exc(res, "Ljava/lang/IllegalArgumentException;");
    }

    let Value::Obj(bm) = vm
        .invoke_static(
            "Landroid/graphics/Bitmap;",
            "createBitmap",
            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            &[Value::Int(4), Value::Int(4), argb_8888],
        )
        .expect("createBitmap 4x4")
    else {
        panic!("expected Bitmap");
    };
    assert_eq!(
        vm.invoke_instance(bm, "getByteCount", "()I", &[])
            .expect("getByteCount"),
        Value::Int(64)
    );

    // Null array -> NPE.
    let null_res = vm.invoke_instance(
        bm,
        "getPixels",
        "([IIIIIII)V",
        &[
            Value::Null,
            Value::Int(0),
            Value::Int(4),
            Value::Int(0),
            Value::Int(0),
            Value::Int(2),
            Value::Int(2),
        ],
    );
    assert_exc(null_res, "Ljava/lang/NullPointerException;");

    let buf = int_arr(&mut vm, &[99; 10]);

    // Invalid bitmap coordinates or |stride| < width -> IAE.
    for (offset, stride, x, y, w, h) in [
        (0, 1, 0, 0, 2, 2),
        (0, 4, -1, 0, 2, 2),
        (0, 4, 3, 0, 2, 2),
        (0, 4, 0, 3, 2, 2),
    ] {
        let r = vm.invoke_instance(
            bm,
            "getPixels",
            "([IIIIIII)V",
            &[
                Value::Obj(buf),
                Value::Int(offset),
                Value::Int(stride),
                Value::Int(x),
                Value::Int(y),
                Value::Int(w),
                Value::Int(h),
            ],
        );
        assert_exc(r, "Ljava/lang/IllegalArgumentException;");
    }

    // Invalid destination array offset / stride overflow -> AIOOBE.
    for (offset, stride, w, h) in [(-1, 4, 2, 2), (7, 4, 2, 2), (2, -4, 2, 2)] {
        let r = vm.invoke_instance(
            bm,
            "getPixels",
            "([IIIIIII)V",
            &[
                Value::Obj(buf),
                Value::Int(offset),
                Value::Int(stride),
                Value::Int(0),
                Value::Int(0),
                Value::Int(w),
                Value::Int(h),
            ],
        );
        assert_exc(r, "Ljava/lang/ArrayIndexOutOfBoundsException;");
    }

    // Valid sub-slice zeroes only rows [2..4] and [6..8], preserving sentinels.
    vm.invoke_instance(
        bm,
        "getPixels",
        "([IIIIIII)V",
        &[
            Value::Obj(buf),
            Value::Int(2),
            Value::Int(4),
            Value::Int(0),
            Value::Int(0),
            Value::Int(2),
            Value::Int(2),
        ],
    )
    .expect("valid getPixels");
    let out: Vec<i32> = vm
        .heap
        .as_array_elems(buf)
        .expect("buf")
        .0
        .iter()
        .map(|v| match v {
            Value::Int(i) => *i,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(out, vec![99, 99, 0, 0, 99, 99, 0, 0, 99, 99]);
}

#[test]
fn canvas_and_paint_char_array_bounds_overflow_301() {
    let mut vm = engine_of(&DexBuilder::new());
    let canvas = new_obj(&mut vm, "Landroid/graphics/Canvas;", "()V", &[]);
    let paint = new_obj(&mut vm, "Landroid/graphics/Paint;", "()V", &[]);
    let rect = new_obj(&mut vm, "Landroid/graphics/Rect;", "()V", &[]);
    let chars = char_arr(&mut vm, "hello");
    let s = Value::Obj(vm.heap.alloc_string("hello".into()).expect("str"));

    // index = 1, count = Integer.MAX_VALUE previously panicked in debug on i32 `1 + MAX`.
    let r1 = vm.invoke_instance(
        canvas,
        "drawText",
        "([CIIFFLandroid/graphics/Paint;)V",
        &[
            Value::Obj(chars),
            Value::Int(1),
            Value::Int(i32::MAX),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Obj(paint),
        ],
    );
    assert_exc(r1, "Ljava/lang/IndexOutOfBoundsException;");

    let r2 = vm.invoke_instance(
        paint,
        "measureText",
        "([CII)F",
        &[Value::Obj(chars), Value::Int(1), Value::Int(i32::MAX)],
    );
    assert_exc(r2, "Ljava/lang/IndexOutOfBoundsException;");

    let r3 = vm.invoke_instance(
        paint,
        "getTextBounds",
        "([CIILandroid/graphics/Rect;)V",
        &[
            Value::Obj(chars),
            Value::Int(1),
            Value::Int(i32::MAX),
            Value::Obj(rect),
        ],
    );
    assert_exc(r3, "Ljava/lang/IndexOutOfBoundsException;");

    // String start > end or end > len also throws IndexOutOfBoundsException.
    let r4 = vm.invoke_instance(
        canvas,
        "drawText",
        "(Ljava/lang/String;IIFFLandroid/graphics/Paint;)V",
        &[
            s,
            Value::Int(4),
            Value::Int(2),
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Obj(paint),
        ],
    );
    assert_exc(r4, "Ljava/lang/IndexOutOfBoundsException;");
}

#[test]
fn render_node_saturating_bounds_arithmetic_301() {
    let mut vm = engine_of(&DexBuilder::new());
    let name = Value::Obj(vm.heap.alloc_string("rn".into()).expect("str"));
    let rn = new_obj(
        &mut vm,
        "Landroid/graphics/RenderNode;",
        "(Ljava/lang/String;)V",
        &[name],
    );

    vm.invoke_instance(
        rn,
        "setPosition",
        "(IIII)Z",
        &[
            Value::Int(-1_800_000_000),
            Value::Int(-1_800_000_000),
            Value::Int(1_800_000_000),
            Value::Int(1_800_000_000),
        ],
    )
    .expect("setPosition");

    assert_eq!(
        vm.invoke_instance(rn, "getWidth", "()I", &[])
            .expect("getWidth"),
        Value::Int(i32::MAX)
    );
    assert_eq!(
        vm.invoke_instance(rn, "getHeight", "()I", &[])
            .expect("getHeight"),
        Value::Int(i32::MAX)
    );
    vm.invoke_instance(rn, "offsetLeftAndRight", "(I)Z", &[Value::Int(i32::MAX)])
        .expect("offsetLeftAndRight");
    vm.invoke_instance(rn, "offsetTopAndBottom", "(I)Z", &[Value::Int(i32::MIN)])
        .expect("offsetTopAndBottom");
    let rec = vm
        .invoke_instance(
            rn,
            "beginRecording",
            "()Landroid/graphics/RecordingCanvas;",
            &[],
        )
        .expect("beginRecording saturates safely");
    assert!(matches!(rec, Value::Obj(_)));
}

#[test]
fn path_and_canvas_unbounded_growth_capped_301() {
    let mut vm = engine_of(&DexBuilder::new());
    let path = new_obj(&mut vm, "Landroid/graphics/Path;", "()V", &[]);
    vm.invoke_instance(
        path,
        "lineTo",
        "(FF)V",
        &[Value::Float(1.0), Value::Float(1.0)],
    )
    .expect("lineTo");

    // Stage 1: 17 self-doublings reach 2 * 2^17 = 262_144 points (> 65_536 cap),
    // failing deterministically on unpatched code without OOM-killing the test host.
    let mut threw_oom = false;
    for _ in 0..17 {
        match vm.invoke_instance(
            path,
            "addPath",
            "(Landroid/graphics/Path;)V",
            &[Value::Obj(path)],
        ) {
            Ok(_) => {}
            Err(VmExit::Exception(ex)) => {
                assert_eq!(ex.class, "Ljava/lang/OutOfMemoryError;");
                threw_oom = true;
                break;
            }
            Err(other) => panic!("unexpected exit {other:?}"),
        }
    }
    assert!(
        threw_oom,
        "Path.addPath(self) must throw OutOfMemoryError once point budget is exceeded"
    );

    // Stage 2: Full 30-iteration #301 class-2 PoC also terminates immediately with OOM.
    let path30 = new_obj(&mut vm, "Landroid/graphics/Path;", "()V", &[]);
    vm.invoke_instance(
        path30,
        "lineTo",
        "(FF)V",
        &[Value::Float(2.0), Value::Float(3.0)],
    )
    .expect("lineTo");
    let mut oom30 = false;
    for _ in 0..30 {
        if let Err(VmExit::Exception(ex)) = vm.invoke_instance(
            path30,
            "addPath",
            "(Landroid/graphics/Path;)V",
            &[Value::Obj(path30)],
        ) {
            assert_eq!(ex.class, "Ljava/lang/OutOfMemoryError;");
            oom30 = true;
            break;
        }
    }
    assert!(oom30);

    // Canvas.save() stack growth is also bounded.
    let canvas = new_obj(&mut vm, "Landroid/graphics/Canvas;", "()V", &[]);
    let mut save_oom = false;
    for _ in 0..1_100 {
        match vm.invoke_instance(canvas, "save", "()I", &[]) {
            Ok(_) => {}
            Err(VmExit::Exception(ex)) => {
                assert_eq!(ex.class, "Ljava/lang/OutOfMemoryError;");
                save_oom = true;
                break;
            }
            Err(other) => panic!("unexpected exit {other:?}"),
        }
    }
    assert!(save_oom, "Canvas.save() loop must be bounded");
}

fn build_render_node_dag(vm: &mut Engine, levels: usize) -> ObjRef {
    let paint = new_obj(vm, "Landroid/graphics/Paint;", "()V", &[]);
    vm.invoke_instance(
        paint,
        "setColor",
        "(I)V",
        &[Value::Int(0xFFFF_0000u32 as i32)],
    )
    .expect("setColor");

    let mut nodes = Vec::with_capacity(levels);
    for i in 0..levels {
        let label = Value::Obj(vm.heap.alloc_string(format!("n{i}")).expect("str"));
        let rn = new_obj(
            vm,
            "Landroid/graphics/RenderNode;",
            "(Ljava/lang/String;)V",
            &[label],
        );
        vm.invoke_instance(rn, "setClipToBounds", "(Z)Z", &[Value::Int(0)])
            .expect("setClipToBounds(false)");
        vm.invoke_instance(
            rn,
            "setPosition",
            "(IIII)Z",
            &[Value::Int(0), Value::Int(0), Value::Int(64), Value::Int(64)],
        )
        .expect("setPosition");
        nodes.push(rn);
    }

    // Leaf node draws a visible rect; every parent node draws its child twice (2^levels DAG).
    let leaf = nodes[levels - 1];
    let Value::Obj(leaf_canvas) = vm
        .invoke_instance(
            leaf,
            "beginRecording",
            "(II)Landroid/graphics/RecordingCanvas;",
            &[Value::Int(64), Value::Int(64)],
        )
        .expect("leaf beginRecording")
    else {
        panic!("expected RecordingCanvas");
    };
    vm.invoke_instance(
        leaf_canvas,
        "drawRect",
        "(FFFFLandroid/graphics/Paint;)V",
        &[
            Value::Float(0.0),
            Value::Float(0.0),
            Value::Float(12.0),
            Value::Float(12.0),
            Value::Obj(paint),
        ],
    )
    .expect("drawRect");
    vm.invoke_instance(leaf, "endRecording", "()V", &[])
        .expect("leaf endRecording");

    for i in (0..levels - 1).rev() {
        let parent = nodes[i];
        let child = nodes[i + 1];
        let Value::Obj(rec) = vm
            .invoke_instance(
                parent,
                "beginRecording",
                "(II)Landroid/graphics/RecordingCanvas;",
                &[Value::Int(64), Value::Int(64)],
            )
            .expect("beginRecording")
        else {
            panic!("expected RecordingCanvas");
        };
        for _ in 0..2 {
            vm.invoke_instance(
                rec,
                "drawRenderNode",
                "(Landroid/graphics/RenderNode;)V",
                &[Value::Obj(child)],
            )
            .expect("drawRenderNode");
        }
        vm.invoke_instance(parent, "endRecording", "()V", &[])
            .expect("endRecording");
    }
    nodes[0]
}

#[test]
fn render_node_dag_traversal_budget_bounds_hang_301() {
    let mut b = DexBuilder::new();
    let p_v = b.proto_idx("V", vec![]);
    let p_v_view = b.proto_idx("V", vec!["Landroid/view/View;".into()]);
    let p_v_ctx = b.proto_idx("V", vec!["Landroid/content/Context;".into()]);
    let p_v_rn = b.proto_idx("V", vec!["Landroid/graphics/RenderNode;".into()]);

    let act_init = b.method_idx("Landroid/app/Activity;", p_v, "<init>");
    let set_cv = b.method_idx("Landroid/app/Activity;", p_v_view, "setContentView");
    let view_init = b.method_idx("Landroid/view/View;", p_v_ctx, "<init>");
    let draw_rn = b.method_idx("Landroid/graphics/Canvas;", p_v_rn, "drawRenderNode");
    let dag_view_init = b.method_idx("LDagView;", p_v_ctx, "<init>");
    let s_root = b.field_idx("LDagView;", "Landroid/graphics/RenderNode;", "sRoot");

    let dv = b.class("LDagView;", "Landroid/view/View;");
    b.static_field(dv, "sRoot", "Landroid/graphics/RenderNode;", SVal::Int(0));
    b.direct(
        dv,
        "<init>",
        "V",
        vec!["Landroid/content/Context;"],
        ACC_PUBLIC,
        Some(b.code(2, 2, 2, {
            let mut u = op35c(0x70, 2, view_init, [0, 1, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.direct(
        dv,
        "setRoot",
        "V",
        vec!["Landroid/graphics/RenderNode;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(b.code(1, 1, 0, {
            let mut u = op21c(0x69, 0, s_root); // sput-object p0, s_root
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.r#virtual(
        dv,
        "onDraw",
        "V",
        vec!["Landroid/graphics/Canvas;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, {
            // v0 = LDagView.sRoot; if (v0 != null) p1.drawRenderNode(v0); return;
            let mut u = op21c(0x62, 0, s_root);
            u.extend([0x38u16, 5u16]); // if-eqz v0, +5 -> return-void
            u.extend(op35c(0x6E, 2, draw_rn, [2, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let dv_ty = b.type_idx("LDagView;");
    let main = b.class("LDagMain;", "Landroid/app/Activity;");
    b.direct(
        main,
        "<init>",
        "V",
        vec![],
        ACC_PUBLIC,
        Some(b.code(1, 1, 1, {
            let mut u = op35c(0x70, 1, act_init, [0, 0, 0, 0, 0]);
            u.extend(op10x(0x0E));
            u
        })),
    );
    b.r#virtual(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(3, 2, 2, {
            let mut u = op21c(0x22, 0, dv_ty);
            u.extend(op35c(0x70, 2, dag_view_init, [0, 1, 0, 0, 0]));
            u.extend(op35c(0x6E, 2, set_cv, [1, 0, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );

    let mut vm = engine_of(&b);
    let p0 = Value::Null;
    vm.invoke_static(
        "LDagView;",
        "setRoot",
        "(Landroid/graphics/RenderNode;)V",
        &[p0],
    )
    .expect("init sRoot null");
    vm.launch_app("LDagMain;", "com.test.dag", Vec::new())
        .expect("launch_app");

    // Stage 1: 17-level binary RenderNode DAG (theoretical 2^18 - 1 = 262,143 visits).
    // Unpatched code visits all 262,143 ops and fails the budget assertion quickly;
    // patched code caps visits cleanly at 32,768.
    let root17 = build_render_node_dag(&mut vm, 17);
    vm.invoke_static(
        "LDagView;",
        "setRoot",
        "(Landroid/graphics/RenderNode;)V",
        &[Value::Obj(root17)],
    )
    .expect("setRoot 17");
    let ops17 = vm.draw_frame().expect("draw_frame 17-level DAG");
    assert!(
        ops17 <= 32_768,
        "frame_op_count must enforce visited-op budget, got {ops17}"
    );
    assert_eq!(ops17, 32_768);

    // Stage 2: 40-level binary RenderNode DAG (~2.2 trillion theoretical visits, #301 class-3 PoC).
    // All four host walkers (`draw_frame`/`frame_op_count`, `rasterize_frame`,
    // `frame_summary`, and `screen_model`) must terminate cleanly within budget.
    let root40 = build_render_node_dag(&mut vm, 40);
    vm.invoke_static(
        "LDagView;",
        "setRoot",
        "(Landroid/graphics/RenderNode;)V",
        &[Value::Obj(root40)],
    )
    .expect("setRoot 40");
    let ops40 = vm.draw_frame().expect("draw_frame 40-level DAG");
    assert_eq!(ops40, 32_768);

    let mut px = vec![0u8; 64 * 64 * 3];
    vm.rasterize_frame(&mut px, 64, 64);
    assert_eq!(&px[0..3], &[0xFF, 0x00, 0x00], "first leaf painted red");

    let summary = vm.frame_summary();
    assert!(!summary.is_empty());
    assert!(summary.lines().count() <= 16_384);

    let model = vm.screen_model();
    assert!(!model.boxes.is_empty());
}
