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
//! EXACTLY specs from the host bounds. It then records a frame: host view
//! backgrounds, and each DEX view's `draw(Canvas)` (or `onDraw` +
//! `dispatchDraw`) on a canvas translated to the view's window origin. The
//! frame display list is rasterized by `Engine::rasterize_frame` (graphics.rs).
//!
//! **ViewConfiguration** uses AOSP's dp constants scaled by the device density
//! (`android_res`). The timeouts are AOSP 14 values.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

const EXACTLY: i32 = 0x4000_0000;
const VIEW_CONFIG: &str = "Landroid/view/ViewConfiguration;";

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
    set_prop(vm, v, "rd_wspec", ws);
    set_prop(vm, v, "rd_hspec", hs);
    if crate::jcollections::guest_call(
        vm,
        v,
        "onMeasure",
        "(II)V",
        &[Value::Int(ws), Value::Int(hs)],
    )?
    .is_none()
    {
        set_prop(vm, v, "MeasuredWidth", ws & 0x3fff_ffff);
        set_prop(vm, v, "MeasuredHeight", hs & 0x3fff_ffff);
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
    let (nw, nh) = ((r - l).max(0), (b - t).max(0));
    // views laid out by the guest: window position = parent origin + frame
    let (ax, ay) = if prop(vm, v, "rd_host_root").is_some() {
        (l, t)
    } else {
        (px + l, py + t)
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
    crate::jcollections::guest_call(
        vm,
        v,
        "onLayout",
        "(ZIIII)V",
        &[
            Value::Int(changed as i32),
            Value::Int(l),
            Value::Int(t),
            Value::Int(r),
            Value::Int(b),
        ],
    )?;
    Ok(())
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| int(args.get(i));
    Ok(Some(match (class, name) {
        ("Landroid/view/View$MeasureSpec;", "makeMeasureSpec" | "makeSafeMeasureSpec") => {
            Value::Int((a(0) & 0x3fff_ffff) | (a(1) & (0xC000_0000u32 as i32)))
        }
        ("Landroid/view/View$MeasureSpec;", "getMode") => {
            Value::Int(a(0) & (0xC000_0000u32 as i32))
        }
        ("Landroid/view/View$MeasureSpec;", "getSize") => Value::Int(a(0) & 0x3fff_ffff),
        ("Landroid/view/View;", "resolveSize" | "resolveSizeAndState") => {
            let (size, spec) = (a(0), a(1));
            let (mode, ss) = (spec & (0xC000_0000u32 as i32), spec & 0x3fff_ffff);
            Value::Int(match mode {
                m if m == EXACTLY => ss,
                m if m == (0x8000_0000u32 as i32) => size.min(ss),
                _ => size,
            })
        }
        ("Landroid/view/View;", "getDefaultSize") => {
            Value::Int(if a(1) & (0xC000_0000u32 as i32) == 0 {
                a(0)
            } else {
                a(1) & 0x3fff_ffff
            })
        }
        ("Landroid/view/View;", "combineMeasuredStates") => Value::Int(a(0) | a(1)),
        ("Landroid/view/View;", "generateViewId") => {
            let k = ("Lrd/View;".to_string(), "next_id".to_string());
            let n = match vm.statics.get(&k) {
                Some(Value::Int(i)) => *i,
                _ => 1,
            };
            vm.statics.insert(k, Value::Int(n + 1));
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

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
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
        // AOSP ViewGroup.drawChild: draw `child` on the parent's canvas,
        // translated to the child's parent-relative frame.
        ("drawChild", "(Landroid/graphics/Canvas;Landroid/view/View;J)Z") => {
            if let (Some(Value::Obj(c)), Some(Value::Obj(child))) = (args.first(), args.get(1)) {
                host_draw_child(vm, *c, *child)?;
            }
            Value::Int(0)
        }
        // AOSP ViewGroup.dispatchDraw (reached via `super.dispatchDraw`):
        // draw each visible child in order through the (overridable)
        // `drawChild(Canvas, View, long)`.
        ("dispatchDraw", "(Landroid/graphics/Canvas;)V") => {
            if let Some(Value::Obj(c)) = args.first() {
                let c = *c;
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
            }
            Value::Null
        }
        // move the frame without resizing (AOSP: no onSizeChanged/onLayout)
        ("offsetLeftAndRight", "(I)V") | ("offsetTopAndBottom", "(I)V") => {
            let d = a(0);
            let horiz = name == "offsetLeftAndRight";
            let key = if horiz { "rd_l" } else { "rd_t" };
            let cur = prop(vm, recv, key).unwrap_or(if horiz { x } else { y });
            set_prop(vm, recv, key, cur + d);
            if let Some(HostObj::View { x, y, .. }) = vm.fw.objects.get_mut(&recv) {
                if horiz {
                    *x += d;
                } else {
                    *y += d;
                }
            }
            Value::Null
        }
        ("setLeft" | "setTop" | "setRight" | "setBottom", "(I)V") => {
            let (l, t) = (
                prop(vm, recv, "rd_l").unwrap_or(x),
                prop(vm, recv, "rd_t").unwrap_or(y),
            );
            let (mut fl, mut ft, mut fr, mut fb) = (l, t, l + w, t + h);
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
        ("getRight", "()I") => Value::Int(prop(vm, recv, "rd_l").unwrap_or(x) + w),
        ("getBottom", "()I") => Value::Int(prop(vm, recv, "rd_t").unwrap_or(y) + h),
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
        ("hasWindowFocus", "()Z") => Value::Int(vm.attached_views.contains(&recv) as i32),
        ("isFocused" | "hasFocus" | "isAccessibilityFocused", "()Z") => Value::Int(0),
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
        | ("setFocusableInTouchMode", _)
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
            let child = args.first().cloned().unwrap_or(Value::Null);
            crate::framework::call_host_instance(
                vm,
                "Landroid/view/ViewGroup;",
                "addView",
                "(Landroid/view/View;)V",
                recv,
                &[child],
            )?;
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
        if dex_roots.is_empty() {
            self.gfx.frame.clear();
            self.gfx.frame_valid = false;
            return Ok(0);
        }
        for v in &dex_roots {
            let Some((x, y, w, h, _)) = bounds(self, *v) else {
                continue;
            };
            let (w, h) = (
                if w > 0 { w } else { fw_ - x },
                if h > 0 { h } else { fh_ - y },
            );
            measure(self, *v, EXACTLY | w, EXACTLY | h)?;
            set_prop(self, *v, "rd_host_root", 1);
            layout(self, *v, x, y, x + w, y + h)?;
        }
        // 2. let layout-triggered frame callbacks / posted work run
        let now = self.fw.clock();
        let _ = now;
        let _ = self.advance_clock(16);
        // 3. record the frame
        let canvas = self.begin_frame_canvas(fw_, fh_)?;
        self.frame_fill(canvas, 0.0, 0.0, fw_ as f32, fh_ as f32, 0xFFFF_FFFF);
        self.record_view(canvas, content, 0)?;
        self.end_frame();
        Ok(self.frame_op_count())
    }

    fn collect_dex_roots(&self, v: ObjRef, out: &mut Vec<ObjRef>, depth: usize) {
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
