//! `android.view.Window` beyond the content slot: the decor view hierarchy
//! and the edge-to-edge/insets setters that AndroidX `WindowCompat`,
//! `enableEdgeToEdge` and Compose call in `onCreate`.
//!
//! As on a device, `getDecorView()` returns a `FrameLayout` decor whose
//! descendant `android.R.id.content` (`0x01020002`) is the content frame.
//! `setContentView` content is attached under that frame. As a result,
//! `decor.findViewById(android.R.id.content).getChildAt(0)` (androidx
//! `ComponentActivity.setContent`'s "existing ComposeView?" check) behaves as
//! on Android. The renderer keeps reading `Window.content`.
//!
//! The window flags, colours and fit-system-windows setters only record
//! state. The headless window has no system bars, so they don't change
//! layout.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

const WINDOW: &str = "Landroid/view/Window;";
pub const ID_CONTENT: i32 = 0x0102_0002;

fn new_host_view(vm: &mut Engine, class: &str, id: i32) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance(class.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.attach_host_state(class, r)?;
    if let Some(HostObj::View { id: i, w, h, .. }) = vm.fw.objects.get_mut(&r) {
        *i = id;
        *w = crate::framework::WINDOW_W;
        *h = crate::framework::WINDOW_H;
    }
    Ok(r)
}

/// `(decor, content_frame)` of a window, created on first use. Content set
/// earlier through `setContentView` is re-parented under the frame.
pub(crate) fn decor_of(vm: &mut Engine, window: ObjRef) -> Result<(ObjRef, ObjRef), VmExit> {
    if let Some(d) = vm.window_decor.get(&window) {
        return Ok(*d);
    }
    let decor = new_host_view(vm, "Landroid/widget/FrameLayout;", 0)?;
    let frame = new_host_view(vm, "Landroid/widget/FrameLayout;", ID_CONTENT)?;
    crate::framework::attach_child(vm, decor, frame)?;
    vm.window_decor.insert(window, (decor, frame));
    if let Some(HostObj::Window { content: Some(c) }) = vm.fw.get(window).cloned() {
        attach_content(vm, frame, c)?;
    }
    Ok((decor, frame))
}

/// Put `content` under the content frame (idempotent; detaches others).
pub(crate) fn attach_content(
    vm: &mut Engine,
    frame: ObjRef,
    content: ObjRef,
) -> Result<(), VmExit> {
    if let Some(HostObj::View { children, .. }) = vm.fw.objects.get_mut(&frame) {
        if children.contains(&content) {
            return Ok(());
        }
        children.clear();
    }
    crate::framework::attach_child(vm, frame, content)
}

/// Called by `set_content_view`: keep the decor tree in sync if it exists.
pub(crate) fn on_content_set(
    vm: &mut Engine,
    window: ObjRef,
    content: ObjRef,
) -> Result<(), VmExit> {
    if let Some((_, frame)) = vm.window_decor.get(&window).copied() {
        attach_content(vm, frame, content)?;
    }
    Ok(())
}

fn find_by_id(vm: &Engine, root: ObjRef, want: i32) -> Option<ObjRef> {
    let HostObj::View { id, children, .. } = vm.fw.get(root)? else {
        return None;
    };
    if *id == want {
        return Some(root);
    }
    children.iter().find_map(|c| find_by_id(vm, *c, want))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class != WINDOW {
        return Ok(None);
    }
    let v = match (name, sig) {
        ("getCallback", "()Landroid/view/Window$Callback;") => vm
            .view_props
            .get(&(recv, "win:callback".to_string()))
            .cloned()
            .unwrap_or(Value::Null),
        ("setCallback", "(Landroid/view/Window$Callback;)V") => {
            let callback = match args.first() {
                Some(Value::Null | Value::Int(0)) | None => Value::Null,
                Some(value) => value.clone(),
            };
            vm.view_props
                .insert((recv, "win:callback".to_string()), callback);
            Value::Null
        }
        ("getDecorView", "()Landroid/view/View;") | ("peekDecorView", "()Landroid/view/View;") => {
            Value::Obj(decor_of(vm, recv)?.0)
        }
        ("setContentView", "(Landroid/view/View;)V") => {
            let content = args
                .first()
                .ok_or_else(|| {
                    crate::err::vm_error("VM_TYPE_ERROR", "Window.setContentView sem View")
                })?
                .as_ref()
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?
                .ok_or_else(|| {
                    crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "Window.setContentView(view): null view",
                    )
                })?;
            let Some(HostObj::Window { content: slot }) = vm.fw.objects.get_mut(&recv) else {
                return Err(crate::err::vm_error("VM_TYPE_ERROR", "Window sem estado host").into());
            };
            *slot = Some(content);
            let (_, frame) = decor_of(vm, recv)?;
            attach_content(vm, frame, content)?;
            vm.fw.traversal_pending = true;
            Value::Null
        }
        ("findViewById", "(I)Landroid/view/View;") => {
            let (decor, _) = decor_of(vm, recv)?;
            let want = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            find_by_id(vm, decor, want).map_or(Value::Null, Value::Obj)
        }
        ("setDecorFitsSystemWindows", "(Z)V")
        | ("setStatusBarColor", "(I)V")
        | ("setNavigationBarColor", "(I)V")
        | ("setNavigationBarDividerColor", "(I)V")
        | ("setStatusBarContrastEnforced", "(Z)V")
        | ("setNavigationBarContrastEnforced", "(Z)V")
        | ("addFlags", "(I)V")
        | ("clearFlags", "(I)V")
        | ("setFlags", "(II)V")
        | ("setSoftInputMode", "(I)V")
        | ("setFormat", "(I)V")
        | ("setBackgroundDrawable", _)
        | ("setBackgroundDrawableResource", "(I)V")
        | ("setWindowAnimations", "(I)V")
        | ("setNavigationBarContrastEnforced$", _) => Value::Null,
        ("setGravity", "(I)V")
        | ("setDimAmount", "(F)V")
        | ("setElevation", "(F)V")
        | ("setTitle", "(Ljava/lang/CharSequence;)V") => Value::Null,
        // Window features (FEATURE_NO_TITLE, …): recorded per window
        ("requestFeature", "(I)Z") => {
            let f = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            let key = (recv, "win:features".to_string());
            let cur = match vm.view_props.get(&key) {
                Some(Value::Long(m)) => *m,
                _ => 0,
            };
            if (0..63).contains(&f) {
                vm.view_props.insert(key, Value::Long(cur | (1i64 << f)));
            }
            Value::Int(1)
        }
        ("hasFeature", "(I)Z") => {
            let f = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            let m = match vm.view_props.get(&(recv, "win:features".to_string())) {
                Some(Value::Long(m)) => *m,
                _ => 0,
            };
            Value::Int(((0..63).contains(&f) && m & (1i64 << f) != 0) as i32)
        }
        // Window attributes: one WindowManager.LayoutParams per window
        // (MATCH_PARENT × MATCH_PARENT by default, as for a PhoneWindow)
        ("getAttributes", "()Landroid/view/WindowManager$LayoutParams;") => {
            Value::Obj(window_attributes(vm, recv)?)
        }
        ("setAttributes", "(Landroid/view/WindowManager$LayoutParams;)V") => {
            if let Some(a @ Value::Obj(_)) = args.first() {
                vm.view_props
                    .insert((recv, "win:attrs".to_string()), a.clone());
            }
            Value::Null
        }
        ("setLayout", "(II)V") => {
            let a = window_attributes(vm, recv)?;
            let w = args.first().and_then(|v| v.as_int().ok()).unwrap_or(-1);
            let h = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(-1);
            crate::err::put_field_result(vm.heap.put_field(a, "width", Value::Int(w)))?;
            crate::err::put_field_result(vm.heap.put_field(a, "height", Value::Int(h)))?;
            Value::Null
        }
        ("superDispatchKeyEvent", "(Landroid/view/KeyEvent;)Z") => Value::Int(0),
        ("getStatusBarColor", "()I") | ("getNavigationBarColor", "()I") => Value::Int(0),
        ("isFloating", "()Z") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

pub(crate) fn window_attributes(vm: &mut Engine, window: ObjRef) -> Result<ObjRef, VmExit> {
    let key = (window, "win:attrs".to_string());
    if let Some(Value::Obj(a)) = vm.view_props.get(&key) {
        return Ok(*a);
    }
    const LP: &str = "Landroid/view/WindowManager$LayoutParams;";
    let a = vm.heap.alloc_instance(LP.to_string(), Vec::new())?;
    layout_params_intrinsic(vm, LP, "<init>", "()V", a, &[])?;
    vm.view_props.insert(key, Value::Obj(a));
    Ok(a)
}

/// `View.setSystemUiVisibility`/`getSystemUiVisibility` (pre-API-30
/// WindowCompat path): recorded, no system bars headless.
pub(crate) fn view_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(vm.fw.get(recv), Some(HostObj::View { .. })) {
        return Ok(None);
    }
    match (name, sig) {
        ("setSystemUiVisibility", "(I)V") => {
            let f = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            vm.system_ui_visibility.insert(recv, f);
            Ok(Some(Value::Null))
        }
        ("getSystemUiVisibility", "()I") => Ok(Some(Value::Int(
            vm.system_ui_visibility.get(&recv).copied().unwrap_or(0),
        ))),
        _ => Ok(None),
    }
}

/// `android.view.ViewGroup$LayoutParams` family (`MarginLayoutParams`,
/// `FrameLayout`/`LinearLayout`/`RelativeLayout`/`WindowManager`
/// LayoutParams). Constructors store plain heap fields (`width`, `height`,
/// `weight`, `gravity`, the margins), so DEX `iget`/`iput` read and write
/// them directly. A DEX subclass that calls `super(w, h)` gets the same.
pub(crate) fn layout_params_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !(mref_class.starts_with("Landroid/") && mref_class.ends_with("LayoutParams;")) {
        return Ok(None);
    }
    let put = |vm: &mut Engine, f: &str, v: Value| -> Result<(), VmExit> {
        crate::err::put_field_result(vm.heap.put_field(recv, f, v))?;
        Ok(())
    };
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    match (name, sig) {
        ("<init>", _) => {
            for f in [
                "leftMargin",
                "topMargin",
                "rightMargin",
                "bottomMargin",
                "gravity",
            ] {
                put(vm, f, Value::Int(if f == "gravity" { -1 } else { 0 }))?;
            }
            put(vm, "weight", Value::Float(0.0))?;
            match sig {
                "(II)V" => {
                    put(vm, "width", Value::Int(int(0)))?;
                    put(vm, "height", Value::Int(int(1)))?;
                }
                "(IIF)V" => {
                    put(vm, "width", Value::Int(int(0)))?;
                    put(vm, "height", Value::Int(int(1)))?;
                    let w = args.get(2).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
                    put(vm, "weight", Value::Float(w))?;
                }
                "(III)V" => {
                    put(vm, "width", Value::Int(int(0)))?;
                    put(vm, "height", Value::Int(int(1)))?;
                    put(vm, "gravity", Value::Int(int(2)))?;
                }
                "()V" => {
                    // WindowManager.LayoutParams(): MATCH_PARENT both ways
                    put(vm, "width", Value::Int(-1))?;
                    put(vm, "height", Value::Int(-1))?;
                }
                _ => {
                    // copy ctor (LayoutParams source) or (Context, AttributeSet)
                    let src = args.first().and_then(|v| v.as_ref().ok().flatten());
                    let from = |f: &str| src.and_then(|s| vm.heap.get_field(s, f).ok());
                    let (w, h) = (from("width"), from("height"));
                    let copied: Vec<(&str, Option<Value>)> = [
                        "leftMargin",
                        "topMargin",
                        "rightMargin",
                        "bottomMargin",
                        "gravity",
                        "weight",
                    ]
                    .iter()
                    .map(|f| (*f, from(f)))
                    .collect();
                    put(vm, "width", w.unwrap_or(Value::Int(-2)))?;
                    put(vm, "height", h.unwrap_or(Value::Int(-2)))?;
                    for (f, v) in copied {
                        if let Some(v) = v {
                            put(vm, f, v)?;
                        }
                    }
                }
            }
            Ok(Some(Value::Null))
        }
        ("setMargins", "(IIII)V") => {
            for (i, f) in ["leftMargin", "topMargin", "rightMargin", "bottomMargin"]
                .iter()
                .enumerate()
            {
                put(vm, f, Value::Int(int(i)))?;
            }
            Ok(Some(Value::Null))
        }
        ("setMarginStart", "(I)V") => {
            put(vm, "leftMargin", Value::Int(int(0)))?;
            Ok(Some(Value::Null))
        }
        ("setMarginEnd", "(I)V") => {
            put(vm, "rightMargin", Value::Int(int(0)))?;
            Ok(Some(Value::Null))
        }
        ("getMarginStart", "()I") => Ok(Some(
            vm.heap
                .get_field(recv, "leftMargin")
                .unwrap_or(Value::Int(0)),
        )),
        ("getMarginEnd", "()I") => Ok(Some(
            vm.heap
                .get_field(recv, "rightMargin")
                .unwrap_or(Value::Int(0)),
        )),
        _ => Ok(None),
    }
}
