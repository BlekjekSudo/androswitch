//! View measure/layout/draw for app (DEX) views hosted in the window, plus
//! `View$MeasureSpec`, `ViewConfiguration`, `AnimationUtils` and `Trace`.
//!
//! **Measure and layout.**
//! - `View.measure(w, h)` calls the DEX `onMeasure` override when there is
//!   one. Otherwise it applies AOSP's default (`getDefaultSize`: the spec
//!   size).
//! - `setMeasuredDimension` stores the result; `getMeasuredWidth/Height`
//!   read it back.
//! - `View.layout(l, t, r, b)` stores the parent-relative frame
//!   (`getLeft`/`getTop`/…) and moves the host bounds to window coordinates
//!   (parent origin + l/t), so `view_tree`, hit-testing and screenshots agree.
//! - It calls `onSizeChanged` when the size changed, then `onLayout`.
//!
//! **Draw pass** (`Engine::draw_frame`). The host lays out its own views; for
//! each top-most DEX view in the content tree it runs `measure`/`layout` with
//! EXACTLY specs from the host bounds. It then records a transparent overlay
//! of each DEX view's `draw(Canvas)` (or `onDraw` + `dispatchDraw`) on a canvas
//! translated to the view's window origin. Host-only backgrounds/widgets stay
//! in the shared `rd-render::render_snapshot` base. The frame display list is
//! rasterized by `Engine::rasterize_frame` (graphics.rs).
//!
//! **ViewConfiguration** uses AOSP's dp constants scaled by the device density
//! (`android_res`). The timeouts are AOSP 14 values.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

const EXACTLY: i32 = 0x4000_0000;
/// MeasureSpec.AT_MOST (`2 << 30`).
const AT_MOST: i32 = i32::MIN;
const VIEW_CONFIG: &str = "Landroid/view/ViewConfiguration;";
const LAYOUT_INFLATER: &str = "Landroid/view/LayoutInflater;";
const WINDOW_INSETS: &str = "Landroid/view/WindowInsets;";
const SCROLLER: &str = "Landroid/widget/Scroller;";
const OVER_SCROLLER: &str = "Landroid/widget/OverScroller;";
const EDGE_EFFECT: &str = "Landroid/widget/EdgeEffect;";
const LINEAR_INTERPOLATOR: &str = "Landroid/view/animation/LinearInterpolator;";
const DECELERATE_INTERPOLATOR: &str = "Landroid/view/animation/DecelerateInterpolator;";
const ACCELERATE_INTERPOLATOR: &str = "Landroid/view/animation/AccelerateInterpolator;";
const ACCELERATE_DECELERATE_INTERPOLATOR: &str =
    "Landroid/view/animation/AccelerateDecelerateInterpolator;";

fn density() -> f32 {
    std::env::var("RD_DENSITY_DPI")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(280.0)
        / 160.0
}
fn dp(x: f32) -> i32 {
    (x * density() + 0.5) as i32
}
fn int(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Int(i)) => *i,
        _ => 0,
    }
}
fn bounds(vm: &Engine, v: ObjRef) -> Option<(i32, i32, i32, i32, Option<ObjRef>)> {
    match vm.fw.get(v) {
        Some(HostObj::View {
            x, y, w, h, parent, ..
        }) => Some((*x, *y, *w, *h, *parent)),
        _ => None,
    }
}
fn prop(vm: &Engine, v: ObjRef, k: &str) -> Option<i32> {
    match vm.view_props.get(&(v, k.to_string())) {
        Some(Value::Int(i)) => Some(*i),
        _ => None,
    }
}
fn set_prop(vm: &mut Engine, v: ObjRef, k: &str, x: i32) {
    vm.view_props.insert((v, k.to_string()), Value::Int(x));
}

/// `View.measure`: DEX `onMeasure` override or AOSP default.
pub(crate) fn measure(vm: &mut Engine, v: ObjRef, ws: i32, hs: i32) -> Result<(), VmExit> {
    // Android View.measure semantics (ADR 008 Stage 1b): same specs and no
    // pending requestLayout on this view -> the previous measured size is
    // still valid, skip onMeasure entirely. requestLayout bubbles the flag
    // to ancestors (see the requestLayout handler), so a changed child
    // re-measures its whole ancestor path, exactly like AOSP.
    if prop(vm, v, "rd_layout_requested").is_none()
        && prop(vm, v, "rd_wspec") == Some(ws)
        && prop(vm, v, "rd_hspec") == Some(hs)
        && prop(vm, v, "MeasuredWidth").is_some()
        && prop(vm, v, "MeasuredHeight").is_some()
    {
        return Ok(());
    }
    set_prop(vm, v, "rd_wspec", ws);
    set_prop(vm, v, "rd_hspec", hs);
    let measure_args = [Value::Int(ws), Value::Int(hs)];
    if crate::jcollections::guest_call(vm, v, "onMeasure", "(II)V", &measure_args)?.is_none() {
        // Platform containers such as FrameLayout and LinearLayout implement
        // onMeasure outside the APK DEX. Dispatch through the host framework
        // before falling back to View's getDefaultSize behavior.
        vm.invoke_instance(v, "onMeasure", "(II)V", &measure_args)?;
    }
    vm.view_props
        .remove(&(v, "rd_layout_requested".to_string()));
    Ok(())
}

/// `View.layout`: parent-relative frame + window-space host bounds.
pub(crate) fn layout(
    vm: &mut Engine,
    v: ObjRef,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) -> Result<(), VmExit> {
    let Some((_, _, ow, oh, parent)) = bounds(vm, v) else {
        return Ok(());
    };
    let (px, py) = parent
        .and_then(|p| bounds(vm, p))
        .map(|(x, y, ..)| (x, y))
        .unwrap_or((0, 0));
    let (nw, nh) = (r.saturating_sub(l).max(0), b.saturating_sub(t).max(0));
    // views laid out by the guest: window position = parent origin + frame
    let (ax, ay) = if prop(vm, v, "rd_host_root").is_some() {
        (l, t)
    } else {
        (px.saturating_add(l), py.saturating_add(t))
    };
    if let Some(HostObj::View { x, y, w, h, .. }) = vm.fw.objects.get_mut(&v) {
        *x = ax;
        *y = ay;
        *w = nw;
        *h = nh;
    }
    set_prop(vm, v, "rd_l", l);
    set_prop(vm, v, "rd_t", t);
    let changed = nw != ow || nh != oh || prop(vm, v, "rd_laid_out").is_none();
    set_prop(vm, v, "rd_laid_out", 1);
    if changed {
        crate::jcollections::guest_call(
            vm,
            v,
            "onSizeChanged",
            "(IIII)V",
            &[
                Value::Int(nw),
                Value::Int(nh),
                Value::Int(ow),
                Value::Int(oh),
            ],
        )?;
    }
    let layout_args = [
        Value::Int(changed as i32),
        Value::Int(l),
        Value::Int(t),
        Value::Int(r),
        Value::Int(b),
    ];
    if crate::jcollections::guest_call(vm, v, "onLayout", "(ZIIII)V", &layout_args)?.is_none() {
        // Platform containers lay out their children in native onLayout
        // methods that are not present in an APK's DEX files.
        vm.invoke_instance(v, "onLayout", "(ZIIII)V", &layout_args)?;
    }
    Ok(())
}

fn absolute_gravity(mut gravity: i32, layout_direction: i32) -> i32 {
    const RELATIVE_LAYOUT_DIRECTION: i32 = 0x0080_0000;
    const START: i32 = RELATIVE_LAYOUT_DIRECTION | 0x3;
    const END: i32 = RELATIVE_LAYOUT_DIRECTION | 0x5;
    const LEFT: i32 = 0x3;
    const RIGHT: i32 = 0x5;
    if gravity & RELATIVE_LAYOUT_DIRECTION != 0 {
        if gravity & START == START {
            gravity = (gravity & !START) | if layout_direction == 1 { RIGHT } else { LEFT };
        } else if gravity & END == END {
            gravity = (gravity & !END) | if layout_direction == 1 { LEFT } else { RIGHT };
        }
        gravity &= !RELATIVE_LAYOUT_DIRECTION;
    }
    gravity
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == "Landroid/view/Gravity;" && name == "getAbsoluteGravity" && args.len() == 2 {
        return Ok(Some(Value::Int(absolute_gravity(
            args[0].as_int()?,
            args[1].as_int()?,
        ))));
    }
    if class == "Landroid/view/Gravity;" && name == "apply" && args.len() == 6 {
        let gravity = absolute_gravity(args[0].as_int()?, args[5].as_int()?);
        let (width, height) = (args[1].as_int()?, args[2].as_int()?);
        let rect_obj = |value: &Value, label: &str| -> Result<ObjRef, VmExit> {
            value.as_ref()?.ok_or_else(|| {
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    format!("Gravity.apply {label} Rect == null"),
                ))
            })
        };
        let container = rect_obj(&args[3], "container")?;
        let out = rect_obj(&args[4], "outRect")?;
        let rect = |r: ObjRef, field: &str| {
            vm.heap
                .get_field(r, field)
                .ok()
                .and_then(|value| value.as_int().ok())
                .unwrap_or(0)
        };
        let (container_left, container_top, container_right, container_bottom) = (
            rect(container, "left"),
            rect(container, "top"),
            rect(container, "right"),
            rect(container, "bottom"),
        );
        let (left, right) = match gravity & 0x7 {
            0x7 => (container_left, container_right),
            0x1 => {
                let left = container_left.saturating_add(
                    container_right
                        .saturating_sub(container_left)
                        .saturating_sub(width)
                        / 2,
                );
                (left, left.saturating_add(width))
            }
            0x5 => (container_right.saturating_sub(width), container_right),
            _ => (container_left, container_left.saturating_add(width)),
        };
        let (top, bottom) = match gravity & 0x70 {
            0x70 => (container_top, container_bottom),
            0x10 => {
                let top = container_top.saturating_add(
                    container_bottom
                        .saturating_sub(container_top)
                        .saturating_sub(height)
                        / 2,
                );
                (top, top.saturating_add(height))
            }
            0x50 => (container_bottom.saturating_sub(height), container_bottom),
            _ => (container_top, container_top.saturating_add(height)),
        };
        for (field, value) in [
            ("left", left),
            ("top", top),
            ("right", right),
            ("bottom", bottom),
        ] {
            crate::err::put_field_result(vm.heap.put_field(out, field, Value::Int(value)))?;
        }
        return Ok(Some(Value::Null));
    }
    // D8 may keep the concrete ViewGroup subclass as the method-ref owner
    // for this inherited protected static helper (for example Toolbar).
    if name == "getChildMeasureSpec"
        && vm.cp.is_subtype(class, "Landroid/view/ViewGroup;")
        && args.len() == 3
    {
        let (spec, padding, child_dimension) =
            (args[0].as_int()?, args[1].as_int()?, args[2].as_int()?);
        return Ok(Some(Value::Int(crate::framework::child_measure_spec(
            spec,
            padding,
            child_dimension,
        ))));
    }
    if class == LAYOUT_INFLATER && name == "from" {
        let context = args
            .first()
            .ok_or_else(|| {
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "LayoutInflater.from(context): null context",
                ))
            })?
            .as_ref()?
            .ok_or_else(|| {
                VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "LayoutInflater.from(context): null context",
                ))
            })?;
        let service_name = crate::intrinsics::alloc_string(vm, "layout_inflater".to_string())?;
        let inflater = crate::m8::call_m8_instance(
            vm,
            "Landroid/content/Context;",
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            context,
            &[Value::Obj(service_name)],
        )?
        .ok_or_else(|| {
            crate::err::vm_error(
                "VM_TYPE_ERROR",
                "LayoutInflater.from: Context service shim returned no result",
            )
        })?;
        let Value::Obj(inflater) = inflater else {
            return Err(crate::err::vm_error(
                "VM_TYPE_ERROR",
                "LayoutInflater.from: layout_inflater service is not an object",
            )
            .into());
        };
        for field in ["mFactory", "mFactory2"] {
            if vm.heap.get_field(inflater, field).is_err() {
                crate::err::put_field_result(vm.heap.put_field(inflater, field, Value::Null))?;
            }
        }
        crate::err::put_field_result(vm.heap.put_field(inflater, "mContext", Value::Obj(context)))?;
        return Ok(Some(Value::Obj(inflater)));
    }
    let a = |i: usize| int(args.get(i));
    let is_view_class = vm.cp.is_subtype(class, "Landroid/view/View;");
    Ok(Some(match (class, name) {
        // WindowInsets.Type masks (AOSP constants, API 30+)
        ("Landroid/view/WindowInsets$Type;", t) => Value::Int(match t {
            "statusBars" => 1,
            "navigationBars" => 2,
            "captionBar" => 4,
            "ime" => 8,
            "systemGestures" => 16,
            "mandatorySystemGestures" => 32,
            "tappableElement" => 64,
            "displayCutout" => 128,
            "systemOverlays" => 256,
            "systemBars" => 1 | 2 | 4,
            _ => return Ok(None),
        }),
        ("Landroid/view/View$MeasureSpec;", "makeMeasureSpec" | "makeSafeMeasureSpec") => {
            Value::Int((a(0) & 0x3fff_ffff) | (a(1) & (0xC000_0000u32 as i32)))
        }
        ("Landroid/view/View$MeasureSpec;", "getMode") => {
            Value::Int(a(0) & (0xC000_0000u32 as i32))
        }
        ("Landroid/view/View$MeasureSpec;", "getSize") => Value::Int(a(0) & 0x3fff_ffff),
        (_, "resolveSize" | "resolveSizeAndState") if is_view_class => {
            let (size, spec) = (a(0), a(1));
            let (mode, ss) = (spec & (0xC000_0000u32 as i32), spec & 0x3fff_ffff);
            Value::Int(match mode {
                m if m == EXACTLY => ss,
                m if m == (0x8000_0000u32 as i32) => size.min(ss),
                _ => size,
            })
        }
        (_, "getDefaultSize") if is_view_class => {
            Value::Int(if a(1) & (0xC000_0000u32 as i32) == 0 {
                a(0)
            } else {
                a(1) & 0x3fff_ffff
            })
        }
        (_, "combineMeasuredStates") if is_view_class => Value::Int(a(0) | a(1)),
        (_, "generateViewId") if is_view_class => {
            let k = ("Lrd/View;".to_string(), "next_id".to_string());
            let raw = match vm.statics.get(&k) {
                Some(Value::Int(i)) if *i >= 1 => *i,
                _ => 1,
            };
            let n = if raw > 0x00ff_ffff { 1 } else { raw };
            let next = if n >= 0x00ff_ffff { 1 } else { n + 1 };
            vm.statics.insert(k, Value::Int(next));
            Value::Int(n)
        }
        (VIEW_CONFIG, "get") => {
            let k = ("Lrd/View;".to_string(), "view_configuration".to_string());
            if let Some(v) = vm.statics.get(&k) {
                v.clone()
            } else {
                let r = vm
                    .heap
                    .alloc_instance(VIEW_CONFIG.to_string(), Vec::new())?;
                vm.statics.insert(k, Value::Obj(r));
                Value::Obj(r)
            }
        }
        (VIEW_CONFIG, n) => match n {
            "getLongPressTimeout" => Value::Int(400),
            "getTapTimeout" => Value::Int(100),
            "getDoubleTapTimeout" => Value::Int(300),
            "getDoubleTapMinTime" => Value::Int(40),
            "getJumpTapTimeout" => Value::Int(500),
            "getKeyRepeatTimeout" => Value::Int(400),
            "getKeyRepeatDelay" => Value::Int(50),
            "getPressedStateDuration" => Value::Int(64),
            "getScrollBarFadeDuration" => Value::Int(250),
            "getScrollDefaultDelay" => Value::Int(300),
            "getZoomControlsTimeout" => Value::Long(3000),
            "getGlobalActionKeyTimeout" => Value::Long(500),
            "getMultiPressTimeout" => Value::Int(300),
            "getScrollFriction" => Value::Float(0.015),
            "getTouchSlop" => Value::Int(8),
            "getEdgeSlop" => Value::Int(12),
            "getMinimumFlingVelocity" => Value::Int(50),
            "getMaximumFlingVelocity" => Value::Int(8000),
            "getScrollBarSize" => Value::Int(4),
            "getFadingEdgeLength" => Value::Int(12),
            "getDefaultActionModeHideDuration" => Value::Long(2000),
            _ => return Ok(None),
        },
        ("Landroid/view/animation/AnimationUtils;", "currentAnimationTimeMillis") => {
            Value::Long(vm.fw.clock() as i64)
        }
        ("Landroid/os/Trace;", n) => match n {
            "isEnabled" => Value::Int(0),
            _ => Value::Null, // beginSection/endSection/async sections/counters: no tracer attached
        },
        _ => return Ok(None),
    }))
}

/// The focused view if it is `v` or one of its descendants.
fn focus_within(vm: &Engine, v: ObjRef) -> Option<ObjRef> {
    let f = vm.fw.focused_view?;
    let mut cur = Some(f);
    for _ in 0..64 {
        let c = cur?;
        if c == v {
            return Some(f);
        }
        cur = match vm.fw.objects.get(&c) {
            Some(HostObj::View { parent, .. }) => *parent,
            _ => None,
        };
    }
    None
}

/// Moves window focus to `to` (None = clear), calling `onFocusChanged` on the
/// view losing focus and then on the one gaining it. Re-requesting the
/// current focus is a no-op (AOSP: `requestFocus` on a focused view returns
/// true without callbacks) — this also breaks callback re-entrancy.
fn move_focus(vm: &mut Engine, to: Option<ObjRef>, dir: i32) -> Result<(), VmExit> {
    const SIG: &str = "(ZILandroid/graphics/Rect;)V";
    let from = vm.fw.focused_view;
    if from == to {
        return Ok(());
    }
    vm.fw.focused_view = to;
    if let Some(old) = from {
        vm.invoke_instance(
            old,
            "onFocusChanged",
            SIG,
            &[Value::Int(0), Value::Int(dir), Value::Null],
        )?;
    }
    if let Some(new) = to {
        vm.invoke_instance(
            new,
            "onFocusChanged",
            SIG,
            &[Value::Int(1), Value::Int(dir), Value::Null],
        )?;
    }
    Ok(())
}

fn scroller_value(vm: &Engine, recv: ObjRef, field: &str, default: i32) -> i32 {
    match vm.view_props.get(&(recv, format!("rd_scroller_{field}"))) {
        Some(Value::Int(value)) => *value,
        _ => default,
    }
}

fn set_scroller_value(vm: &mut Engine, recv: ObjRef, field: &str, value: i32) {
    vm.view_props
        .insert((recv, format!("rd_scroller_{field}")), Value::Int(value));
}

fn scroller_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let arg = |i: usize| int(args.get(i));
    let get = |field: &str| scroller_value(vm, recv, field, 0);
    let set_position = |vm: &mut Engine, x: i32, y: i32| {
        set_scroller_value(vm, recv, "curr_x", x);
        set_scroller_value(vm, recv, "curr_y", y);
    };
    let value = match (name, sig) {
        ("<init>", _) => {
            for field in [
                "start_x", "start_y", "curr_x", "curr_y", "final_x", "final_y",
            ] {
                set_scroller_value(vm, recv, field, 0);
            }
            set_scroller_value(vm, recv, "finished", 1);
            Value::Null
        }
        ("isFinished", "()Z") => Value::Int(i32::from(get("finished") != 0)),
        ("isOverScrolled", "()Z") => Value::Int(0),
        ("getStartX", "()I") => Value::Int(get("start_x")),
        ("getStartY", "()I") => Value::Int(get("start_y")),
        ("getCurrX", "()I") => Value::Int(get("curr_x")),
        ("getCurrY", "()I") => Value::Int(get("curr_y")),
        ("getFinalX", "()I") => Value::Int(get("final_x")),
        ("getFinalY", "()I") => Value::Int(get("final_y")),
        ("getCurrVelocity", "()F") => Value::Float(0.0),
        ("getDuration" | "timePassed", "()I") => Value::Int(0),
        ("computeScrollOffset", "()Z") => {
            if get("finished") != 0 {
                Value::Int(0)
            } else {
                set_position(vm, get("final_x"), get("final_y"));
                set_scroller_value(vm, recv, "finished", 1);
                Value::Int(1)
            }
        }
        ("abortAnimation", "()V") => {
            set_position(vm, get("final_x"), get("final_y"));
            set_scroller_value(vm, recv, "finished", 1);
            Value::Null
        }
        ("forceFinished", "(Z)V") => {
            set_scroller_value(vm, recv, "finished", i32::from(arg(0) != 0));
            Value::Null
        }
        ("startScroll", _) if args.len() >= 4 => {
            let (x, y) = (arg(0), arg(1));
            set_scroller_value(vm, recv, "start_x", x);
            set_scroller_value(vm, recv, "start_y", y);
            set_position(vm, x, y);
            set_scroller_value(vm, recv, "final_x", x.saturating_add(arg(2)));
            set_scroller_value(vm, recv, "final_y", y.saturating_add(arg(3)));
            set_scroller_value(vm, recv, "finished", 0);
            Value::Null
        }
        ("fling", _) if args.len() >= 8 => {
            let (x, y) = (arg(0), arg(1));
            let min_x = arg(4);
            let max_x = arg(5);
            let min_y = arg(6);
            let max_y = arg(7);
            let final_x = x.saturating_add(arg(2) / 4).clamp(min_x, max_x);
            let final_y = y.saturating_add(arg(3) / 4).clamp(min_y, max_y);
            set_scroller_value(vm, recv, "start_x", x);
            set_scroller_value(vm, recv, "start_y", y);
            set_position(vm, x, y);
            set_scroller_value(vm, recv, "final_x", final_x);
            set_scroller_value(vm, recv, "final_y", final_y);
            set_scroller_value(
                vm,
                recv,
                "finished",
                i32::from(x == final_x && y == final_y),
            );
            Value::Null
        }
        ("springBack", _) if args.len() >= 6 => {
            let (x, y) = (arg(0), arg(1));
            let final_x = x.clamp(arg(2), arg(3));
            let final_y = y.clamp(arg(4), arg(5));
            set_scroller_value(vm, recv, "start_x", x);
            set_scroller_value(vm, recv, "start_y", y);
            set_position(vm, x, y);
            set_scroller_value(vm, recv, "final_x", final_x);
            set_scroller_value(vm, recv, "final_y", final_y);
            let changed = x != final_x || y != final_y;
            set_scroller_value(vm, recv, "finished", i32::from(!changed));
            Value::Int(i32::from(changed))
        }
        ("setFinalX", "(I)V") => {
            set_scroller_value(vm, recv, "final_x", arg(0));
            set_scroller_value(vm, recv, "finished", 0);
            Value::Null
        }
        ("setFinalY", "(I)V") => {
            set_scroller_value(vm, recv, "final_y", arg(0));
            set_scroller_value(vm, recv, "finished", 0);
            Value::Null
        }
        ("extendDuration" | "setFriction", _) => Value::Null,
        ("notifyHorizontalEdgeReached" | "notifyVerticalEdgeReached", _) => Value::Null,
        ("isScrollingInDirection", "(FF)Z") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn interpolator_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let recognized = matches!(
        class,
        LINEAR_INTERPOLATOR
            | DECELERATE_INTERPOLATOR
            | ACCELERATE_INTERPOLATOR
            | ACCELERATE_DECELERATE_INTERPOLATOR
    );
    if !recognized {
        return Ok(None);
    }
    let factor_key = (recv, "rd_interpolator_factor".to_string());
    let factor = match vm.view_props.get(&factor_key) {
        Some(Value::Float(value)) => *value,
        _ => 1.0,
    };
    let arg_float = |index: usize| {
        args.get(index)
            .and_then(|value| value.as_float().ok())
            .unwrap_or(0.0)
    };
    match (name, sig) {
        ("<init>", _) => {
            if matches!(class, DECELERATE_INTERPOLATOR | ACCELERATE_INTERPOLATOR) {
                let value = if args.is_empty() { 1.0 } else { arg_float(0) };
                vm.view_props.insert(factor_key, Value::Float(value));
            }
            Ok(Some(Value::Null))
        }
        ("getInterpolation", "(F)F") => {
            let input = arg_float(0);
            let value = match class {
                LINEAR_INTERPOLATOR => input,
                DECELERATE_INTERPOLATOR => {
                    let inverse = 1.0 - input;
                    1.0 - if factor == 1.0 {
                        inverse * inverse
                    } else {
                        inverse.powf(2.0 * factor)
                    }
                }
                ACCELERATE_INTERPOLATOR => {
                    if factor == 1.0 {
                        input * input
                    } else {
                        input.powf(2.0 * factor)
                    }
                }
                _ => ((input + 1.0) * std::f32::consts::PI).cos() / 2.0 + 0.5,
            };
            Ok(Some(Value::Float(value)))
        }
        _ => Ok(None),
    }
}

fn edge_float(vm: &Engine, recv: ObjRef, field: &str, default: f32) -> f32 {
    match vm.view_props.get(&(recv, format!("rd_edge_{field}"))) {
        Some(Value::Float(value)) => *value,
        Some(Value::Int(value)) => *value as f32,
        _ => default,
    }
}

fn set_edge_float(vm: &mut Engine, recv: ObjRef, field: &str, value: f32) {
    vm.view_props
        .insert((recv, format!("rd_edge_{field}")), Value::Float(value));
}

fn edge_effect_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let distance = edge_float(vm, recv, "distance", 0.0);
    let f = |index: usize| {
        args.get(index)
            .and_then(|value| value.as_float().ok())
            .unwrap_or(0.0)
    };
    let i = |index: usize| int(args.get(index));
    let result = match (name, sig) {
        ("<init>", _) => {
            set_edge_float(vm, recv, "distance", 0.0);
            vm.view_props
                .insert((recv, "rd_edge_finished".to_string()), Value::Int(1));
            Value::Null
        }
        ("isFinished", "()Z") => Value::Int(i32::from(distance <= 0.0)),
        ("getDistance", "()F") => Value::Float(distance),
        ("onPull", "(F)V") | ("onPull", "(FF)V") => {
            let next = (distance + f(0).max(0.0)).clamp(0.0, 1.0);
            set_edge_float(vm, recv, "distance", next);
            vm.view_props.insert(
                (recv, "rd_edge_finished".to_string()),
                Value::Int(i32::from(next <= 0.0)),
            );
            Value::Null
        }
        ("onPullDistance", "(FF)F") => {
            let next = (distance + f(0)).clamp(0.0, 1.0);
            set_edge_float(vm, recv, "distance", next);
            vm.view_props.insert(
                (recv, "rd_edge_finished".to_string()),
                Value::Int(i32::from(next <= 0.0)),
            );
            Value::Float(next - distance)
        }
        ("onRelease" | "finish", "()V") => {
            set_edge_float(vm, recv, "distance", 0.0);
            vm.view_props
                .insert((recv, "rd_edge_finished".to_string()), Value::Int(1));
            Value::Null
        }
        ("onAbsorb", "(I)V") => {
            let next = if i(0) == 0 { 0.0 } else { 1.0 };
            set_edge_float(vm, recv, "distance", next);
            vm.view_props.insert(
                (recv, "rd_edge_finished".to_string()),
                Value::Int(i32::from(next <= 0.0)),
            );
            Value::Null
        }
        ("draw", "(Landroid/graphics/Canvas;)Z") => Value::Int(0),
        ("setSize", "(II)V") => {
            vm.view_props
                .insert((recv, "rd_edge_width".to_string()), Value::Int(i(0)));
            vm.view_props
                .insert((recv, "rd_edge_height".to_string()), Value::Int(i(1)));
            Value::Null
        }
        ("getMaxHeight", "()I") | ("getTargetHeight", "()I") => Value::Int(
            match vm.view_props.get(&(recv, "rd_edge_height".to_string())) {
                Some(Value::Int(value)) => *value,
                _ => 0,
            },
        ),
        ("setColor", "(I)V") => {
            vm.view_props
                .insert((recv, "rd_edge_color".to_string()), Value::Int(i(0)));
            Value::Null
        }
        ("getColor", "()I") => Value::Int(
            match vm.view_props.get(&(recv, "rd_edge_color".to_string())) {
                Some(Value::Int(value)) => *value,
                _ => 0,
            },
        ),
        ("setBlendMode", "(Landroid/graphics/BlendMode;)V") => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(result))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == SCROLLER || mref_class == OVER_SCROLLER {
        return scroller_intrinsic(vm, name, sig, recv, args);
    }
    if matches!(
        mref_class,
        LINEAR_INTERPOLATOR
            | DECELERATE_INTERPOLATOR
            | ACCELERATE_INTERPOLATOR
            | ACCELERATE_DECELERATE_INTERPOLATOR
    ) {
        return interpolator_intrinsic(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == EDGE_EFFECT {
        return edge_effect_intrinsic(vm, name, sig, recv, args);
    }
    if mref_class == WINDOW_INSETS {
        let value = match (name, sig) {
            ("<init>", _) => Value::Null,
            ("consumeDisplayCutout", "()Landroid/view/WindowInsets;")
            | ("consumeStableInsets", "()Landroid/view/WindowInsets;")
            | ("consumeSystemWindowInsets", "()Landroid/view/WindowInsets;") => Value::Obj(recv),
            ("getDisplayCutout", "()Landroid/view/DisplayCutout;") => Value::Null,
            ("getSystemGestureInsets", "()Landroid/graphics/Insets;")
            | ("getMandatorySystemGestureInsets", "()Landroid/graphics/Insets;")
            | ("getTappableElementInsets", "()Landroid/graphics/Insets;") => {
                Value::Obj(crate::graphics::new_platform_insets(vm, [0; 4])?)
            }
            ("getInsets", "(I)Landroid/graphics/Insets;")
            | ("getInsetsIgnoringVisibility", "(I)Landroid/graphics/Insets;") => {
                Value::Obj(crate::graphics::new_platform_insets(vm, [0; 4])?)
            }
            ("isVisible", "(I)Z") => Value::Int(0),
            ("getSystemWindowInsetLeft", "()I")
            | ("getSystemWindowInsetTop", "()I")
            | ("getSystemWindowInsetRight", "()I")
            | ("getSystemWindowInsetBottom", "()I")
            | ("getStableInsetLeft", "()I")
            | ("getStableInsetTop", "()I")
            | ("getStableInsetRight", "()I")
            | ("getStableInsetBottom", "()I") => Value::Int(0),
            ("hasSystemWindowInsets", "()Z")
            | ("hasStableInsets", "()Z")
            | ("isConsumed", "()Z")
            | ("isRound", "()Z") => Value::Int(0),
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    if mref_class == LAYOUT_INFLATER {
        return match (name, sig) {
            ("getFactory", "()Landroid/view/LayoutInflater$Factory;") => Ok(Some(
                vm.heap.get_field(recv, "mFactory").unwrap_or(Value::Null),
            )),
            ("getFactory2", "()Landroid/view/LayoutInflater$Factory2;") => Ok(Some(
                vm.heap.get_field(recv, "mFactory2").unwrap_or(Value::Null),
            )),
            ("setFactory", "(Landroid/view/LayoutInflater$Factory;)V") => {
                let factory = match args.first() {
                    Some(Value::Null | Value::Int(0)) | None => Value::Null,
                    Some(value) => value.clone(),
                };
                crate::err::put_field_result(vm.heap.put_field(recv, "mFactory", factory))?;
                Ok(Some(Value::Null))
            }
            ("setFactory2", "(Landroid/view/LayoutInflater$Factory2;)V") => {
                let factory = match args.first() {
                    Some(Value::Null | Value::Int(0)) | None => Value::Null,
                    Some(value) => value.clone(),
                };
                crate::err::put_field_result(vm.heap.put_field(recv, "mFactory", factory.clone()))?;
                crate::err::put_field_result(vm.heap.put_field(recv, "mFactory2", factory))?;
                Ok(Some(Value::Null))
            }
            ("getContext", "()Landroid/content/Context;") => Ok(Some(
                vm.heap.get_field(recv, "mContext").unwrap_or(Value::Null),
            )),
            ("cloneInContext", "(Landroid/content/Context;)Landroid/view/LayoutInflater;") => {
                let context = args
                    .first()
                    .ok_or_else(|| {
                        crate::err::vm_error("VM_TYPE_ERROR", "cloneInContext sem Context")
                    })?
                    .as_ref()
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                    .ok_or_else(|| {
                        VmExit::Exception(Throwable::new(
                            "Ljava/lang/NullPointerException;",
                            "LayoutInflater.cloneInContext(context): null context",
                        ))
                    })?;
                let clone = vm
                    .heap
                    .alloc_instance(LAYOUT_INFLATER.to_string(), Vec::new())
                    .map_err(crate::err::oom_throwable)?;
                for (field, value) in [
                    (
                        "mFactory",
                        vm.heap.get_field(recv, "mFactory").unwrap_or(Value::Null),
                    ),
                    (
                        "mFactory2",
                        vm.heap.get_field(recv, "mFactory2").unwrap_or(Value::Null),
                    ),
                    ("mContext", Value::Obj(context)),
                ] {
                    crate::err::put_field_result(vm.heap.put_field(clone, field, value))?;
                }
                Ok(Some(Value::Obj(clone)))
            }
            ("inflate", "(ILandroid/view/ViewGroup;)Landroid/view/View;")
            | ("inflate", "(ILandroid/view/ViewGroup;Z)Landroid/view/View;") => {
                let resid = args
                    .first()
                    .ok_or_else(|| crate::err::vm_error("VM_TYPE_ERROR", "inflate sem resource"))?
                    .as_int()
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                    as u32;
                let root = args
                    .get(1)
                    .ok_or_else(|| crate::err::vm_error("VM_TYPE_ERROR", "inflate sem root"))?
                    .as_ref()
                    .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
                let explicit_attach = sig == "(ILandroid/view/ViewGroup;Z)Landroid/view/View;";
                let attach = if explicit_attach {
                    args.get(2)
                        .ok_or_else(|| {
                            crate::err::vm_error("VM_TYPE_ERROR", "inflate sem attachToRoot")
                        })?
                        .as_int()
                        .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                        != 0
                } else {
                    root.is_some()
                };
                let context = vm
                    .heap
                    .get_field(recv, "mContext")
                    .ok()
                    .and_then(|value| value.as_ref().ok().flatten())
                    .or(vm.fw.current_activity);
                let view = crate::inflate::inflate_resource_into_with_context(
                    vm, resid, root, attach, context,
                )?;
                Ok(Some(Value::Obj(view)))
            }
            _ => Ok(None),
        };
    }
    if mref_class == VIEW_CONFIG {
        return Ok(Some(match name {
            "getScaledTouchSlop" => Value::Int(dp(8.0)),
            "getScaledPagingTouchSlop" | "getScaledWindowTouchSlop" => Value::Int(dp(16.0)),
            "getScaledDoubleTapSlop" => Value::Int(dp(100.0)),
            "getScaledMinimumFlingVelocity" => Value::Int(dp(50.0)),
            "getScaledMaximumFlingVelocity" => Value::Int(dp(8000.0)),
            "getScaledEdgeSlop" => Value::Int(dp(12.0)),
            "getScaledFadingEdgeLength" => Value::Int(dp(12.0)),
            "getScaledScrollBarSize" => Value::Int(dp(4.0)),
            "getScaledOverscrollDistance" => Value::Int(0),
            "getScaledOverflingDistance" => Value::Int(dp(6.0)),
            "getScaledHoverSlop" => Value::Int(dp(4.0)),
            "getScaledMinimumScalingSpan" => Value::Int(dp(170.0)),
            "getScaledHandwritingSlop" => Value::Int(dp(2.0)),
            "getScaledHandwritingGestureLineMargin" => Value::Int(dp(16.0)),
            "getScaledAmbiguousGestureMultiplier" => Value::Float(2.0),
            "getScaledHorizontalScrollFactor" | "getScaledVerticalScrollFactor" => {
                Value::Float(dp(64.0) as f32)
            }
            "getScaledMaximumDrawingCacheSize" => Value::Int(720 * 1440 * 4),
            "getScaledMinimumFlingVelocity2" => Value::Int(dp(50.0)),
            "hasPermanentMenuKey"
            | "isFadingMarqueeEnabled"
            | "shouldShowMenuShortcutsWhenKeyboardPresent" => Value::Int(0),
            _ if sig.ends_with(")I") => {
                // (inputDeviceId, axis, source) fling thresholds (API 34)
                Value::Int(if name.contains("Maximum") {
                    dp(8000.0)
                } else {
                    dp(50.0)
                })
            }
            _ => return Ok(None),
        }));
    }
    if mref_class == "Landroid/content/res/Configuration;" && name == "getLayoutDirection" {
        return Ok(Some(Value::Int(0))); // LTR
    }
    if !(mref_class.starts_with("Landroid/view/") || mref_class.starts_with("Landroid/widget/")) {
        return Ok(None);
    }
    let Some((x, y, w, h, _)) = bounds(vm, recv) else {
        // ViewParent.requestDisallowInterceptTouchEvent on a non-host parent
        return Ok(if name == "requestDisallowInterceptTouchEvent" {
            Some(Value::Null)
        } else {
            None
        });
    };
    let a = |i: usize| int(args.get(i));
    Ok(Some(match (name, sig) {
        ("measure", "(II)V") => {
            measure(vm, recv, a(0), a(1))?;
            Value::Null
        }
        ("layout", "(IIII)V") => {
            layout(vm, recv, a(0), a(1), a(2), a(3))?;
            Value::Null
        }
        ("setMeasuredDimension", "(II)V") => {
            set_prop(vm, recv, "MeasuredWidth", a(0) & 0x00ff_ffff);
            set_prop(vm, recv, "MeasuredHeight", a(1) & 0x00ff_ffff);
            Value::Null
        }
        ("getMeasuredWidth" | "getMeasuredWidthAndState", "()I") => {
            Value::Int(prop(vm, recv, "MeasuredWidth").unwrap_or(w))
        }
        ("getMeasuredHeight" | "getMeasuredHeightAndState", "()I") => {
            Value::Int(prop(vm, recv, "MeasuredHeight").unwrap_or(h))
        }
        ("getMeasuredState", "()I") => Value::Int(0),
        ("getRootWindowInsets", "()Landroid/view/WindowInsets;") => {
            if !vm.attached_views.contains(&recv) {
                Value::Null
            } else {
                let key = (
                    "Landroid/view/View;".to_string(),
                    "rd_empty_root_window_insets".to_string(),
                );
                let insets = match vm.statics.get(&key) {
                    Some(Value::Obj(insets)) => *insets,
                    _ => {
                        let insets = vm
                            .heap
                            .alloc_instance(WINDOW_INSETS.to_string(), Vec::new())?;
                        vm.statics.insert(key, Value::Obj(insets));
                        insets
                    }
                };
                Value::Obj(insets)
            }
        }
        (
            "dispatchApplyWindowInsets",
            "(Landroid/view/WindowInsets;)Landroid/view/WindowInsets;",
        ) => match args.first().cloned() {
            Some(Value::Obj(insets)) => vm.invoke_instance(
                recv,
                "onApplyWindowInsets",
                "(Landroid/view/WindowInsets;)Landroid/view/WindowInsets;",
                &[Value::Obj(insets)],
            )?,
            Some(Value::Null | Value::Int(0)) | None => Value::Null,
            Some(value) => value,
        },
        ("onApplyWindowInsets", "(Landroid/view/WindowInsets;)Landroid/view/WindowInsets;") => {
            args.first().cloned().unwrap_or(Value::Null)
        }
        (
            "computeSystemWindowInsets",
            "(Landroid/view/WindowInsets;Landroid/graphics/Rect;)Landroid/view/WindowInsets;",
        ) => {
            // The headless window currently supplies zero system insets. The
            // AOSP default reports those as local insets and consumes them;
            // WindowInsets.consumeSystemWindowInsets is modeled as identity.
            if let Some(Value::Obj(local)) = args.get(1) {
                for field in ["left", "top", "right", "bottom"] {
                    crate::err::put_field_result(vm.heap.put_field(*local, field, Value::Int(0)))?;
                }
            }
            match args.first() {
                Some(Value::Obj(insets)) => vm.invoke_instance(
                    *insets,
                    "consumeSystemWindowInsets",
                    "()Landroid/view/WindowInsets;",
                    &[],
                )?,
                Some(Value::Null | Value::Int(0)) | None => Value::Null,
                _ => Value::Null,
            }
        }
        ("setPaddingRelative", "(IIII)V") => {
            for (field, value) in [
                ("paddingStart", a(0)),
                ("paddingTop", a(1)),
                ("paddingEnd", a(2)),
                ("paddingBottom", a(3)),
                ("paddingLeft", a(0)), // headless device is LTR
                ("paddingRight", a(2)),
                ("paddingRelative", 1),
            ] {
                set_prop(vm, recv, field, value);
            }
            Value::Null
        }
        ("setPadding", "(IIII)V") => {
            for (field, value) in [
                ("paddingLeft", a(0)),
                ("paddingTop", a(1)),
                ("paddingRight", a(2)),
                ("paddingBottom", a(3)),
                ("paddingStart", a(0)),
                ("paddingEnd", a(2)),
                ("paddingRelative", 0),
            ] {
                set_prop(vm, recv, field, value);
            }
            Value::Null
        }
        ("getPaddingLeft", "()I") => Value::Int(prop(vm, recv, "paddingLeft").unwrap_or(0)),
        ("getPaddingTop", "()I") => Value::Int(prop(vm, recv, "paddingTop").unwrap_or(0)),
        ("getPaddingRight", "()I") => Value::Int(prop(vm, recv, "paddingRight").unwrap_or(0)),
        ("getPaddingBottom", "()I") => Value::Int(prop(vm, recv, "paddingBottom").unwrap_or(0)),
        ("getPaddingStart", "()I") => Value::Int(prop(vm, recv, "paddingStart").unwrap_or(0)),
        ("getPaddingEnd", "()I") => Value::Int(prop(vm, recv, "paddingEnd").unwrap_or(0)),
        ("isPaddingRelative", "()Z") => Value::Int(prop(vm, recv, "paddingRelative").unwrap_or(0)),
        ("setHorizontalScrollBarEnabled", "(Z)V") => {
            set_prop(vm, recv, "horizontalScrollBarEnabled", a(0));
            Value::Null
        }
        ("isHorizontalScrollBarEnabled", "()Z") => Value::Int(i32::from(
            prop(vm, recv, "horizontalScrollBarEnabled").unwrap_or(1) != 0,
        )),
        ("setVerticalScrollBarEnabled", "(Z)V") => {
            set_prop(vm, recv, "verticalScrollBarEnabled", a(0));
            Value::Null
        }
        ("isVerticalScrollBarEnabled", "()Z") => Value::Int(i32::from(
            prop(vm, recv, "verticalScrollBarEnabled").unwrap_or(1) != 0,
        )),
        ("setScrollbarFadingEnabled", "(Z)V") => {
            set_prop(vm, recv, "scrollbarFadingEnabled", a(0));
            Value::Null
        }
        ("isScrollbarFadingEnabled", "()Z") => Value::Int(i32::from(
            prop(vm, recv, "scrollbarFadingEnabled").unwrap_or(1) != 0,
        )),
        ("setScrollBarStyle", "(I)V") => {
            set_prop(vm, recv, "scrollBarStyle", a(0));
            Value::Null
        }
        ("getScrollBarStyle", "()I") => Value::Int(prop(vm, recv, "scrollBarStyle").unwrap_or(0)),
        ("getImportantForAccessibility", "()I") => {
            Value::Int(prop(vm, recv, "importantForAccessibility").unwrap_or(0))
        }
        ("getOutlineProvider", "()Landroid/view/ViewOutlineProvider;") => vm
            .view_props
            .get(&(recv, "outlineProvider".to_string()))
            .cloned()
            .unwrap_or(Value::Null),
        ("setOutlineProvider", "(Landroid/view/ViewOutlineProvider;)V") => {
            vm.view_props.insert(
                (recv, "outlineProvider".to_string()),
                args.first().cloned().unwrap_or(Value::Null),
            );
            Value::Null
        }
        ("getClipToOutline", "()Z") => {
            Value::Int(i32::from(prop(vm, recv, "clipToOutline").unwrap_or(0) != 0))
        }
        ("setClipToOutline", "(Z)V") => {
            set_prop(vm, recv, "clipToOutline", a(0));
            Value::Null
        }
        ("setImportantForAccessibility", "(I)V") => {
            set_prop(vm, recv, "importantForAccessibility", a(0));
            Value::Null
        }
        ("isLaidOut", "()Z") => {
            Value::Int(i32::from(prop(vm, recv, "rd_laid_out").unwrap_or(0) != 0))
        }
        ("requestLayout" | "forceLayout", "()V") => {
            // AOSP: requestLayout bubbles to the root, flagging each ancestor.
            let mut cur = Some(recv);
            for _ in 0..64 {
                let Some(v) = cur else { break };
                set_prop(vm, v, "rd_layout_requested", 1);
                cur = bounds(vm, v).and_then(|b| b.4);
            }
            vm.fw.traversal_pending = true;
            Value::Null
        }
        ("invalidate" | "postInvalidate" | "postInvalidateOnAnimation", _)
            if sig.ends_with(")V") =>
        {
            vm.fw.traversal_pending = true;
            Value::Null
        }
        // AOSP ViewGroup.drawChild: draw `child` on the parent's canvas,
        // translated to the child's parent-relative frame.
        ("drawChild", "(Landroid/graphics/Canvas;Landroid/view/View;J)Z") => {
            if let (Some(Value::Obj(c)), Some(Value::Obj(child))) = (args.first(), args.get(1)) {
                host_draw_child(vm, *c, *child)?;
            }
            Value::Int(0)
        }
        // AOSP: post-concat the ancestors' offsets (view transforms such as
        // rotation/scale are not modelled → pure window translation)
        ("transformMatrixToGlobal" | "transformMatrixToLocal", "(Landroid/graphics/Matrix;)V") => {
            if let Some(Value::Obj(m)) = args.first() {
                let s = if name == "transformMatrixToGlobal" {
                    1.0
                } else {
                    -1.0
                };
                vm.invoke_instance(
                    *m,
                    "postTranslate",
                    "(FF)Z",
                    &[Value::Float(s * x as f32), Value::Float(s * y as f32)],
                )?;
            }
            Value::Null
        }
        // View transforms (rotation/scale/translation properties) are not
        // modelled: the view matrix is the identity
        ("getMatrix" | "getInverseMatrix", "()Landroid/graphics/Matrix;") => {
            Value::Obj(crate::graphics::new_identity_matrix(vm)?)
        }
        ("hasIdentityMatrix", "()Z") => Value::Int(1),
        // View.post* — the attached view's handler is the main looper
        ("post" | "postOnAnimation", "(Ljava/lang/Runnable;)Z" | "(Ljava/lang/Runnable;)V") => {
            if let Some(Value::Obj(r)) = args.first() {
                view_post(vm, *r, 0)?;
            }
            if sig.ends_with('Z') {
                Value::Int(1)
            } else {
                Value::Null
            }
        }
        (
            "postDelayed" | "postOnAnimationDelayed",
            "(Ljava/lang/Runnable;J)Z" | "(Ljava/lang/Runnable;J)V",
        ) => {
            if let Some(Value::Obj(r)) = args.first() {
                let d = match args.get(1) {
                    Some(Value::Long(l)) => (*l).max(0) as u64,
                    Some(Value::Int(i)) => (*i).max(0) as u64,
                    _ => 0,
                };
                view_post(vm, *r, d)?;
            }
            if sig.ends_with('Z') {
                Value::Int(1)
            } else {
                Value::Null
            }
        }
        ("removeCallbacks", "(Ljava/lang/Runnable;)Z") => {
            if let Some(Value::Obj(r)) = args.first() {
                let r = *r;
                let looper = vm.main_looper_obj()?;
                if let Some(HostObj::Looper { queue }) = vm.fw.objects.get_mut(&looper) {
                    queue.retain(|e| e.is_message || e.runnable != r);
                }
            }
            Value::Int(1)
        }
        // API 33: the OnBackInvokedDispatcher of the view's window — the
        // dialog's for a view inside a dialog window, else the activity's
        ("findOnBackInvokedDispatcher", "()Landroid/window/OnBackInvokedDispatcher;") => {
            let mut root = recv;
            for _ in 0..512 {
                match bounds(vm, root) {
                    Some((_, _, _, _, Some(p))) => root = p,
                    _ => break,
                }
            }
            let owner = crate::m8::showing_dialog_roots(vm)
                .into_iter()
                .chain(
                    vm.fw
                        .m8
                        .dialogs
                        .keys()
                        .filter_map(|d| crate::m8::dialog_content(vm, *d).map(|c| (*d, c)))
                        .collect::<Vec<_>>(),
                )
                .find(|(_, c)| *c == root)
                .map(|(d, _)| d)
                .or(vm.fw.current_activity);
            match owner {
                Some(o) => crate::activity_extras::instance_intrinsic(
                    vm,
                    "Landroid/app/Activity;",
                    "getOnBackInvokedDispatcher",
                    "()Landroid/window/OnBackInvokedDispatcher;",
                    o,
                    &[],
                )?
                .unwrap_or(Value::Null),
                None => Value::Null,
            }
        }
        // Drawable.Callback (AOSP View implements it): scheduleDrawable
        // posts the drawable's frame runnable at uptime `when`; a view
        // redraws every frame here, so invalidate/unschedule need no state.
        ("scheduleDrawable", "(Landroid/graphics/drawable/Drawable;Ljava/lang/Runnable;J)V") => {
            if let Some(Value::Obj(r)) = args.get(1) {
                let when = match args.get(2) {
                    Some(Value::Long(l)) => (*l).max(0) as u64,
                    Some(Value::Int(i)) => (*i).max(0) as u64,
                    _ => 0,
                };
                view_post(vm, *r, when.saturating_sub(vm.fw.clock()))?;
            }
            Value::Null
        }
        ("unscheduleDrawable", "(Landroid/graphics/drawable/Drawable;Ljava/lang/Runnable;)V") => {
            if let Some(Value::Obj(r)) = args.get(1) {
                let r = *r;
                let looper = vm.main_looper_obj()?;
                if let Some(HostObj::Looper { queue }) = vm.fw.objects.get_mut(&looper) {
                    queue.retain(|e| e.is_message || e.runnable != r);
                }
            }
            Value::Null
        }
        (
            "unscheduleDrawable" | "invalidateDrawable",
            "(Landroid/graphics/drawable/Drawable;)V",
        ) => Value::Null,
        ("verifyDrawable", "(Landroid/graphics/drawable/Drawable;)Z") => Value::Int(0),
        // AOSP ViewGroup/View touch defaults (reached via `super.…`)
        ("dispatchTouchEvent", TOUCH_SIG) => match args.first() {
            Some(Value::Obj(ev)) => Value::Int(host_dispatch_touch(vm, recv, *ev)? as i32),
            _ => Value::Int(0),
        },
        ("onTouchEvent" | "onInterceptTouchEvent", TOUCH_SIG) => Value::Int(0),
        // AOSP ViewGroup.dispatchDraw (reached via `super.dispatchDraw`):
        // draw each visible child in order through the (overridable)
        // `drawChild(Canvas, View, long)`.
        ("dispatchDraw", "(Landroid/graphics/Canvas;)V") => {
            if let Some(Value::Obj(c)) = args.first() {
                host_dispatch_draw(vm, recv, *c)?;
            }
            Value::Null
        }
        // AOSP View.draw(Canvas) called directly (e.g. Compose drawing its
        // RippleHostView): onDraw, then dispatchDraw (DEX override or the
        // ViewGroup default)
        ("draw", "(Landroid/graphics/Canvas;)V") => {
            if let Some(Value::Obj(c)) = args.first() {
                let c = *c;
                crate::jcollections::guest_call(
                    vm,
                    recv,
                    "onDraw",
                    "(Landroid/graphics/Canvas;)V",
                    &[Value::Obj(c)],
                )?;
                if crate::jcollections::guest_call(
                    vm,
                    recv,
                    "dispatchDraw",
                    "(Landroid/graphics/Canvas;)V",
                    &[Value::Obj(c)],
                )?
                .is_none()
                {
                    host_dispatch_draw(vm, recv, c)?;
                }
            }
            Value::Null
        }
        // move the frame without resizing (AOSP: no onSizeChanged/onLayout)
        ("offsetLeftAndRight", "(I)V") | ("offsetTopAndBottom", "(I)V") => {
            let d = a(0);
            let horiz = name == "offsetLeftAndRight";
            let key = if horiz { "rd_l" } else { "rd_t" };
            let cur = prop(vm, recv, key).unwrap_or(if horiz { x } else { y });
            set_prop(vm, recv, key, cur.saturating_add(d));
            if let Some(HostObj::View { x, y, .. }) = vm.fw.objects.get_mut(&recv) {
                if horiz {
                    *x = x.saturating_add(d);
                } else {
                    *y = y.saturating_add(d);
                }
            }
            Value::Null
        }
        ("setLeft" | "setTop" | "setRight" | "setBottom", "(I)V") => {
            let (l, t) = (
                prop(vm, recv, "rd_l").unwrap_or(x),
                prop(vm, recv, "rd_t").unwrap_or(y),
            );
            let (mut fl, mut ft, mut fr, mut fb) = (l, t, l.saturating_add(w), t.saturating_add(h));
            match name {
                "setLeft" => fl = a(0),
                "setTop" => ft = a(0),
                "setRight" => fr = a(0),
                _ => fb = a(0),
            }
            layout(vm, recv, fl, ft, fr, fb)?;
            Value::Null
        }
        ("getLeft", "()I") => Value::Int(prop(vm, recv, "rd_l").unwrap_or(x)),
        ("getTop", "()I") => Value::Int(prop(vm, recv, "rd_t").unwrap_or(y)),
        ("getRight", "()I") => Value::Int(prop(vm, recv, "rd_l").unwrap_or(x).saturating_add(w)),
        ("getBottom", "()I") => Value::Int(prop(vm, recv, "rd_t").unwrap_or(y).saturating_add(h)),
        ("getLocationInWindow" | "getLocationOnScreen", "([I)V") => {
            if let Some(Value::Obj(arr)) = args.first() {
                if let Ok(e) = vm.heap.array_elems_mut(*arr) {
                    if e.len() >= 2 {
                        e[0] = Value::Int(x);
                        e[1] = Value::Int(y);
                    }
                }
            }
            Value::Null
        }
        ("isLayoutRequested", "()Z") => {
            Value::Int(prop(vm, recv, "rd_layout_requested").is_some() as i32)
        }
        ("isInTouchMode", "()Z") => Value::Int(1),
        ("setFocusable", "(Z)V") => {
            set_prop(vm, recv, "focusable", a(0));
            Value::Null
        }
        ("isFocusable", "()Z") => {
            Value::Int(i32::from(prop(vm, recv, "focusable").unwrap_or(0) != 0))
        }
        ("setFocusableInTouchMode", "(Z)V") => {
            set_prop(vm, recv, "focusableInTouchMode", a(0));
            Value::Null
        }
        ("isFocusableInTouchMode", "()Z") => Value::Int(i32::from(
            prop(vm, recv, "focusableInTouchMode").unwrap_or(0) != 0,
        )),
        ("setDescendantFocusability", "(I)V") => {
            set_prop(vm, recv, "descendantFocusability", a(0));
            Value::Null
        }
        ("getDescendantFocusability", "()I") => {
            Value::Int(prop(vm, recv, "descendantFocusability").unwrap_or(0x0002_0000))
        }
        ("hasWindowFocus", "()Z") => Value::Int(vm.attached_views.contains(&recv) as i32),
        // single-focus model of the window (AOSP ViewRootImpl.mFocusedView):
        // requestFocus moves focus and notifies onFocusChanged (virtual —
        // Compose's AndroidComposeView / app views override it)
        ("requestFocus", "()Z" | "(I)Z" | "(ILandroid/graphics/Rect;)Z") => {
            let dir = if sig == "()Z" { 130 } else { a(0) }; // FOCUS_DOWN
            let vis = matches!(
                vm.fw.objects.get(&recv),
                Some(HostObj::View {
                    vis: crate::framework::Vis::Visible,
                    ..
                })
            );
            if !vis {
                Value::Int(0)
            } else {
                move_focus(vm, Some(recv), dir)?;
                Value::Int(1)
            }
        }
        ("clearFocus", "()V") => {
            if vm.fw.focused_view == Some(recv) {
                move_focus(vm, None, 0)?;
            }
            Value::Null
        }
        ("isFocused", "()Z") => Value::Int((vm.fw.focused_view == Some(recv)) as i32),
        ("hasFocus", "()Z") => Value::Int(focus_within(vm, recv).is_some() as i32),
        ("findFocus", "()Landroid/view/View;") => {
            focus_within(vm, recv).map_or(Value::Null, Value::Obj)
        }
        ("onFocusChanged", "(ZILandroid/graphics/Rect;)V") => Value::Null,
        ("isAccessibilityFocused", "()Z") => Value::Int(0),
        // no floating text-selection toolbar window on the headless device:
        // AOSP returns null when no window callback starts the mode
        ("startActionMode", _) => Value::Null,
        // no scrolling ancestor scrolls on the headless window (AOSP: false
        // when no parent consumed the request)
        (
            "requestRectangleOnScreen",
            "(Landroid/graphics/Rect;)Z" | "(Landroid/graphics/Rect;Z)Z",
        ) => Value::Int(0),
        ("findFocus", _) | ("focusSearch", _) => Value::Null,
        // no content-capture / autofill / translation service on the headless device
        (
            "getContentCaptureSession"
            | "getAutofillId"
            | "getViewTranslationResponse"
            | "getRootSurfaceControl"
            | "getHandwritingDelegatorCallback",
            _,
        ) => Value::Null,
        (
            "isImportantForContentCapture"
            | "isImportantForAutofill"
            | "isAutofilled"
            | "isShowingLayoutBounds"
            | "isTemporarilyDetached"
            | "isInLayout",
            "()Z",
        ) => Value::Int(0),
        (
            "setImportantForContentCapture"
            | "setImportantForAutofill"
            | "setAutofillId"
            | "setContentCaptureSession"
            | "setHandwritingDelegatorCallback"
            | "setAutoHandwritingEnabled"
            | "setRevealOnFocusHint"
            | "setDefaultFocusHighlightEnabled"
            | "setFocusedByDefault"
            | "setKeyboardNavigationCluster"
            | "setAccessibilityPaneTitle"
            | "setScreenReaderFocusable"
            | "setAccessibilityHeading"
            | "setStateDescription"
            | "setSystemGestureExclusionRects"
            | "setPointerIcon"
            | "setTooltipText"
            | "setOnReceiveContentListener"
            | "setOnCapturedPointerListener",
            _,
        ) => Value::Null,
        ("setOnDragListener", _)
        | ("requestDisallowInterceptTouchEvent", _)
        | ("setOnHierarchyChangeListener", _) => Value::Null,
        (
            "dispatchKeyEventPreIme"
            | "dispatchGenericMotionEvent"
            | "dispatchCapturedPointerEvent",
            _,
        ) => Value::Int(0),
        ("generateDefaultLayoutParams", _) => {
            let lp = vm
                .heap
                .alloc_instance("Landroid/view/ViewGroup$LayoutParams;".into(), Vec::new())?;
            crate::err::put_field_result(vm.heap.put_field(lp, "width", Value::Int(-2)))?;
            crate::err::put_field_result(vm.heap.put_field(lp, "height", Value::Int(-2)))?;
            Value::Obj(lp)
        }
        ("addViewInLayout", _) => {
            // ViewGroup.addViewInLayout(child, index, params[, preventRequestLayout])
            let prevent = args
                .get(3)
                .and_then(|v| v.as_int().ok())
                .map(|i| i != 0)
                .unwrap_or(false);
            crate::framework::host_add_view(vm, recv, sig, args, prevent)?;
            Value::Int(1)
        }
        _ => return Ok(None),
    }))
}

impl Engine {
    /// Host draw pass: measure/layout DEX views in the current window and
    /// record the frame display list. Returns the number of recorded ops.
    /// Recorded Java stack (`rd_trace`) of an exception exit, if any; used to
    /// report failures of host-driven passes (draw, layout) with their call site.
    pub fn exit_trace(&self, e: &VmExit) -> String {
        let VmExit::Exception(t) = e else {
            return String::new();
        };
        let Some(obj) = t.obj else {
            return String::new();
        };
        match self.heap.get_field(obj, "rd_trace") {
            Ok(Value::Obj(s)) => self.heap.as_str(s).map(str::to_string).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Lay the window out before input/inspection, as a device does after
    /// the first traversal: if any DEX root under the content view is still
    /// 0-sized (no frame has run yet), run one `draw_frame` pass.
    pub fn ensure_laid_out(&mut self) -> Result<(), VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Ok(());
        };
        let Some(content) = self.window_content_of(activity) else {
            return Ok(());
        };
        let mut roots = Vec::new();
        self.collect_dex_roots(content, &mut roots, 0);
        roots.extend(
            crate::m8::showing_dialog_roots(self)
                .into_iter()
                .map(|(_, r)| r),
        );
        let needs_layout = roots
            .iter()
            .any(|r| matches!(bounds(self, *r), Some((_, _, w, h, _)) if w <= 0 || h <= 0));
        if needs_layout {
            self.draw_frame()?;
        }
        Ok(())
    }

    pub fn draw_frame(&mut self) -> Result<usize, VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Ok(0);
        };
        let Some(content) = self.window_content_of(activity) else {
            return Ok(0);
        };
        let (fw_, fh_) = (crate::graphics::FB_W as i32, crate::graphics::FB_H as i32);
        // 1. measure/layout top-most DEX views
        let mut dex_roots = Vec::new();
        self.collect_dex_roots(content, &mut dex_roots, 0);
        // Host-only window: the host renderer already draws every view. An
        // overlay frame would only paint its background over that rendering
        // (blank screenshots) and cost a full-screen rasterization.
        let dialogs = crate::m8::showing_dialog_roots(self);
        if dex_roots.is_empty() && dialogs.is_empty() {
            self.gfx.frame.clear();
            self.gfx.frame_valid = false;
            return Ok(0);
        }
        self.fw.in_traversal = true;
        let laid = self.layout_roots(&dex_roots, &dialogs);
        self.fw.in_traversal = false;
        laid?;
        // 2. let layout-triggered frame callbacks / posted work run
        let now = self.fw.clock();
        let _ = now;
        let _ = self.advance_clock(16);
        // 3. record the frame
        let canvas = self.begin_frame_canvas(fw_, fh_)?;
        self.record_view(canvas, content, 0)?;
        for (_, root) in &dialogs {
            // FLAG_DIM_BEHIND (Material dialog backgroundDimAmount 0.32)
            self.frame_fill(canvas, 0.0, 0.0, fw_ as f32, fh_ as f32, 0x5200_0000);
            self.record_view(canvas, *root, 0)?;
        }
        self.end_frame();
        Ok(self.frame_op_count())
    }

    /// One layout traversal (ViewRootImpl measure + layout) of the current
    /// activity window and every showing dialog window, without recording a
    /// frame. Run from `advance_clock` when a view requested a traversal.
    pub(crate) fn layout_pass(&mut self) -> Result<(), VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Ok(());
        };
        let Some(content) = self.window_content_of(activity) else {
            return Ok(());
        };
        let mut dex_roots = Vec::new();
        self.collect_dex_roots(content, &mut dex_roots, 0);
        let dialogs = crate::m8::showing_dialog_roots(self);
        self.layout_roots(&dex_roots, &dialogs)
    }

    fn layout_roots(
        &mut self,
        dex_roots: &[ObjRef],
        dialogs: &[(ObjRef, ObjRef)],
    ) -> Result<(), VmExit> {
        let (fw_, fh_) = (crate::graphics::FB_W as i32, crate::graphics::FB_H as i32);
        for v in dex_roots {
            let Some((x, y, w, h, _)) = bounds(self, *v) else {
                continue;
            };
            let (w, h) = (
                if w > 0 {
                    w
                } else {
                    fw_.saturating_sub(x).max(0)
                },
                if h > 0 {
                    h
                } else {
                    fh_.saturating_sub(y).max(0)
                },
            );
            measure(
                self,
                *v,
                EXACTLY | (w.max(0) & 0x3fff_ffff),
                EXACTLY | (h.max(0) & 0x3fff_ffff),
            )?;
            set_prop(self, *v, "rd_host_root", 1);
            layout(self, *v, x, y, x.saturating_add(w), y.saturating_add(h))?;
        }
        // 1b. dialog windows above the activity (WindowManager z-order)
        for (dlg, root) in dialogs {
            self.layout_dialog_window(*dlg, *root)?;
        }
        Ok(())
    }

    /// Measure/lay out a dialog's window content as a floating window:
    /// the window LayoutParams size (MATCH_PARENT → screen, WRAP_CONTENT →
    /// AT_MOST screen, px → EXACTLY), centered on screen (Gravity.CENTER).
    fn layout_dialog_window(&mut self, dialog: ObjRef, root: ObjRef) -> Result<(), VmExit> {
        let (fw_, fh_) = (crate::graphics::FB_W as i32, crate::graphics::FB_H as i32);
        let (lw, lh) = crate::m8::dialog_window_size(self, dialog);
        let spec = |want: i32, max: i32| match want {
            -1 => EXACTLY | max,
            w if w >= 0 => EXACTLY | w.min(max),
            _ => AT_MOST | max,
        };
        measure(self, root, spec(lw, fw_), spec(lh, fh_))?;
        let mw = (prop(self, root, "MeasuredWidth").unwrap_or(fw_) & 0x00ff_ffff).clamp(0, fw_);
        let mh = (prop(self, root, "MeasuredHeight").unwrap_or(fh_) & 0x00ff_ffff).clamp(0, fh_);
        let (x, y) = (fw_.saturating_sub(mw) / 2, fh_.saturating_sub(mh) / 2);
        if std::env::var("RD_TRACE_DRAW").is_ok() {
            let kids = match self.fw.get(root) {
                Some(HostObj::View { children, .. }) => children.len(),
                _ => 0,
            };
            eprintln!(
                "D/RustyDroid: dialog window {} spec=({lw},{lh}) measured={mw}x{mh} at ({x},{y}) children={kids}",
                self.heap.class_of(root).unwrap_or("?")
            );
        }
        set_prop(self, root, "rd_host_root", 1);
        layout(self, root, x, y, x.saturating_add(mw), y.saturating_add(mh))
    }

    pub(crate) fn collect_dex_roots(&self, v: ObjRef, out: &mut Vec<ObjRef>, depth: usize) {
        if depth > 256 {
            return;
        }
        let class = self.heap.class_of(v).unwrap_or("").to_string();
        if self.cp.find_class(&class).is_some() {
            out.push(v);
            return;
        }
        if let Some(HostObj::View { children, .. }) = self.fw.get(v) {
            for c in children.clone() {
                self.collect_dex_roots(c, out, depth + 1);
            }
        }
    }

    fn record_view(&mut self, canvas: ObjRef, v: ObjRef, depth: usize) -> Result<(), VmExit> {
        if depth > 256 {
            return Ok(());
        }
        let Some(HostObj::View {
            x,
            y,
            w,
            h,
            vis,
            children,
            ..
        }) = self.fw.get(v).cloned()
        else {
            return Ok(());
        };
        if vis != crate::framework::Vis::Visible {
            return Ok(());
        }
        let class = self.heap.class_of(v).unwrap_or("").to_string();
        if self.cp.find_class(&class).is_some() {
            let n = self.canvas_translate(canvas, x as f32, y as f32);
            let drew = crate::jcollections::guest_call(
                self,
                v,
                "draw",
                "(Landroid/graphics/Canvas;)V",
                &[Value::Obj(canvas)],
            )?
            .is_some();
            let mut dispatched = false;
            if !drew {
                crate::jcollections::guest_call(
                    self,
                    v,
                    "onDraw",
                    "(Landroid/graphics/Canvas;)V",
                    &[Value::Obj(canvas)],
                )?;
                dispatched = crate::jcollections::guest_call(
                    self,
                    v,
                    "dispatchDraw",
                    "(Landroid/graphics/Canvas;)V",
                    &[Value::Obj(canvas)],
                )?
                .is_some();
            }
            self.canvas_restore_to(canvas, n);
            // AOSP ViewGroup.dispatchDraw: without a DEX override the children
            // are drawn by the framework (e.g. ComposeView → AndroidComposeView).
            // Children translate to their own window origin.
            if !dispatched
                && self
                    .cp
                    .resolve_method(&class, "dispatchDraw", "(Landroid/graphics/Canvas;)V")
                    .is_none()
            {
                for c in children {
                    self.record_view(canvas, c, depth + 1)?;
                }
            }
            return Ok(());
        }
        let _ = (w, h);
        for c in children {
            self.record_view(canvas, c, depth + 1)?;
        }
        Ok(())
    }
}

/// AOSP `ViewGroup.drawChild`: draw `child` on the parent's canvas,
/// translated to the child's parent-relative frame.
fn host_draw_child(vm: &mut Engine, c: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    let visible = !matches!(
        vm.fw.get(child),
        Some(HostObj::View { vis, .. }) if *vis != crate::framework::Vis::Visible
    );
    if visible {
        let l = prop(vm, child, "rd_l").unwrap_or(0);
        let t = prop(vm, child, "rd_t").unwrap_or(0);
        let n = vm.canvas_translate(c, l as f32, t as f32);
        let drew = crate::jcollections::guest_call(
            vm,
            child,
            "draw",
            "(Landroid/graphics/Canvas;)V",
            &[Value::Obj(c)],
        )?
        .is_some();
        if !drew {
            crate::jcollections::guest_call(
                vm,
                child,
                "onDraw",
                "(Landroid/graphics/Canvas;)V",
                &[Value::Obj(c)],
            )?;
            crate::jcollections::guest_call(
                vm,
                child,
                "dispatchDraw",
                "(Landroid/graphics/Canvas;)V",
                &[Value::Obj(c)],
            )?;
        }
        vm.canvas_restore_to(c, n);
    }
    Ok(())
}

const TOUCH_SIG: &str = "(Landroid/view/MotionEvent;)Z";

fn truthy(v: Option<Value>) -> bool {
    matches!(v, Some(Value::Int(i)) if i != 0)
}

/// Maximum host-driven recursion depth for `dispatch_touch` / `host_dispatch_touch`
/// and `host_dispatch_draw` across nested ViewGroups.
const MAX_VIEW_DISPATCH_DEPTH: usize = 64;

/// Delivers `ev` (in `v`'s local coordinates) to `v` the way
/// `ViewRootImpl` does: a DEX `dispatchTouchEvent` override runs first,
/// otherwise the AOSP ViewGroup/View logic below.
pub(crate) fn dispatch_touch(vm: &mut Engine, v: ObjRef, ev: ObjRef) -> Result<bool, VmExit> {
    if vm.fw.view_dispatch_depth >= MAX_VIEW_DISPATCH_DEPTH {
        return Ok(false);
    }
    vm.fw.view_dispatch_depth += 1;
    let res = (|| {
        if let Some(r) = crate::jcollections::guest_call(
            vm,
            v,
            "dispatchTouchEvent",
            TOUCH_SIG,
            &[Value::Obj(ev)],
        )? {
            return Ok(truthy(Some(r)));
        }
        host_dispatch_touch_inner(vm, v, ev)
    })();
    vm.fw.view_dispatch_depth = vm.fw.view_dispatch_depth.saturating_sub(1);
    res
}

/// AOSP ViewGroup.dispatchTouchEvent / View.dispatchTouchEvent (stateless):
/// `onInterceptTouchEvent`, then the visible children under the point in
/// reverse drawing order (event offset to the child's frame), then the
/// view's own `onTouchEvent`. Subsequent events are routed again by
/// position (no persistent touch target), which matches taps and
/// in-bounds gestures.
pub(crate) fn host_dispatch_touch(vm: &mut Engine, v: ObjRef, ev: ObjRef) -> Result<bool, VmExit> {
    if vm.fw.view_dispatch_depth >= MAX_VIEW_DISPATCH_DEPTH {
        return Ok(false);
    }
    vm.fw.view_dispatch_depth += 1;
    let res = host_dispatch_touch_inner(vm, v, ev);
    vm.fw.view_dispatch_depth = vm.fw.view_dispatch_depth.saturating_sub(1);
    res
}

fn host_dispatch_touch_inner(vm: &mut Engine, v: ObjRef, ev: ObjRef) -> Result<bool, VmExit> {
    let Some(HostObj::View {
        x: px,
        y: py,
        children,
        ..
    }) = vm.fw.get(v).cloned()
    else {
        return Ok(false);
    };
    let intercepted = truthy(crate::jcollections::guest_call(
        vm,
        v,
        "onInterceptTouchEvent",
        TOUCH_SIG,
        &[Value::Obj(ev)],
    )?);
    if !intercepted {
        let (ex, ey) = crate::android_motion::xy(vm, ev);
        for c in children.iter().rev() {
            let Some(HostObj::View {
                x: cx,
                y: cy,
                w,
                h,
                vis,
                ..
            }) = vm.fw.get(*c).cloned()
            else {
                continue;
            };
            if vis != crate::framework::Vis::Visible {
                continue;
            }
            let (l, t) = (
                (cx as i64 - px as i64) as f32,
                (cy as i64 - py as i64) as f32,
            );
            if ex < l || ey < t || ex >= l + w as f32 || ey >= t + h as f32 {
                continue;
            }
            let e2 = crate::android_motion::copy_event(vm, ev)?;
            crate::android_motion::offset(vm, e2, -l, -t)?;
            if dispatch_touch(vm, *c, e2)? {
                return Ok(true);
            }
        }
    }
    // AOSP View.dispatchTouchEvent calls OnTouchListener before onTouchEvent.
    if let Some(Value::Obj(listener)) = vm
        .view_props
        .get(&(v, crate::framework::VIEW_ON_TOUCH_LISTENER.to_string()))
        .cloned()
    {
        let handled = vm.invoke_instance(
            listener,
            "onTouch",
            "(Landroid/view/View;Landroid/view/MotionEvent;)Z",
            &[Value::Obj(v), Value::Obj(ev)],
        )?;
        if truthy(Some(handled)) {
            return Ok(true);
        }
    }
    // AOSP View.onTouchEvent: a DEX override runs first; when none resolves
    // the framework default applies — a clickable view consumes the gesture
    // (PFLAG_PRESSED), performs the click on UP and returns true; a
    // non-clickable view returns false. Without this default the DOWN falls
    // through to the next view under the point: a full-screen sibling (e.g.
    // a ViewPager behind a TabLayout) steals the tap and the target view's
    // `performClick` (TabView → tab.select → setCurrentItem) never runs.
    if let Some(r) =
        crate::jcollections::guest_call(vm, v, "onTouchEvent", TOUCH_SIG, &[Value::Obj(ev)])?
    {
        return Ok(truthy(Some(r)));
    }
    view_on_touch_event_default(vm, v, ev)
}

/// AOSP `View.onTouchEvent` default (the framework implementation reached
/// when no DEX override resolves): CLICKABLE views consume DOWN/UP/CANCEL
/// and run `performClick` on UP over an active press; others return false.
/// The pressed flag lives in the same view-prop store as the touch listener
/// (`rd_pressed`), keeping `HostObj::View` unchanged.
pub(crate) fn view_on_touch_event_default(
    vm: &mut Engine,
    v: ObjRef,
    ev: ObjRef,
) -> Result<bool, VmExit> {
    let clickable = matches!(
        vm.fw.get(v),
        Some(HostObj::View {
            clickable: true,
            ..
        })
    );
    if !clickable {
        return Ok(false);
    }
    let action = match vm.heap.get_field(ev, "rd_action") {
        Ok(Value::Int(a)) => a,
        _ => crate::android_motion::ACTION_DOWN,
    };
    match action {
        crate::android_motion::ACTION_DOWN => {
            set_prop(vm, v, "rd_pressed", 1);
            Ok(true)
        }
        crate::android_motion::ACTION_UP => {
            let pressed = prop(vm, v, "rd_pressed").unwrap_or(0);
            set_prop(vm, v, "rd_pressed", 0);
            if pressed != 0 {
                // AOSP performClickInternal: the DEX `performClick` override
                // runs first (Material TabView.select and friends); when none
                // exists the host listener dispatch plays View.performClick
                // (setOnClickListener / android:onClick).
                match crate::jcollections::guest_call(vm, v, "performClick", "()Z", &[])? {
                    Some(_) => {}
                    None => {
                        vm.dispatch_click(v)?;
                    }
                }
            }
            Ok(true)
        }
        // ACTION_CANCEL (3): an interrupted gesture releases the press.
        3 => {
            set_prop(vm, v, "rd_pressed", 0);
            Ok(true)
        }
        // MOVE and other actions stay consumed while the press is active.
        _ => Ok(true),
    }
}

/// `View.post*`: enqueue `r` on the main looper after `delay_ms`.
fn view_post(vm: &mut Engine, r: ObjRef, delay_ms: u64) -> Result<(), VmExit> {
    let looper = vm.main_looper_obj()?;
    let when_ms = vm.fw.clock().saturating_add(delay_ms);
    if let Some(HostObj::Looper { queue }) = vm.fw.objects.get_mut(&looper) {
        queue.push(crate::framework::QueuedMsg {
            when_ms,
            runnable: r,
            handler: None,
            is_message: false,
            token: None,
        });
    }
    Ok(())
}

/// AOSP ViewGroup.dispatchDraw: each child in order through the
/// (overridable) `drawChild(Canvas, View, long)`.
fn host_dispatch_draw(vm: &mut Engine, recv: ObjRef, c: ObjRef) -> Result<(), VmExit> {
    if vm.fw.view_dispatch_depth >= MAX_VIEW_DISPATCH_DEPTH {
        return Ok(());
    }
    vm.fw.view_dispatch_depth += 1;
    let res = (|| {
        let kids = match vm.fw.get(recv) {
            Some(HostObj::View { children, .. }) => children.clone(),
            _ => Vec::new(),
        };
        for child in kids {
            let over = crate::jcollections::guest_call(
                vm,
                recv,
                "drawChild",
                "(Landroid/graphics/Canvas;Landroid/view/View;J)Z",
                &[Value::Obj(c), Value::Obj(child), Value::Long(0)],
            )?;
            if over.is_none() {
                host_draw_child(vm, c, child)?;
            }
        }
        Ok(())
    })();
    vm.fw.view_dispatch_depth = vm.fw.view_dispatch_depth.saturating_sub(1);
    res
}
