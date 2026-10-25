//! View attach-to-window lifecycle.
//!
//! On a device, `ActivityThread.handleResumeActivity` → `makeVisible` →
//! `WindowManager.addView(decor)` → `ViewRootImpl` dispatches
//! `onAttachedToWindow` down the tree. After that `isAttachedToWindow()` is
//! true, `View.OnAttachStateChangeListener`s fire, and views added later
//! under an attached parent are attached on insertion. Compose depends on
//! this: `AbstractComposeView` creates its composition from
//! `onAttachedToWindow`/attach listeners, and the `ViewCompositionStrategy`
//! disposes it on detach.
//!
//! What runs on attach, per view, in pre-order (parent before children, as
//! in `ViewGroup.dispatchAttachedToWindow`):
//! 1. its DEX `onAttachedToWindow()` override, if the app/library declares
//!    one;
//! 2. then each registered listener's `onViewAttachedToWindow(View)`.
//!
//! Detach mirrors this (`onDetachedFromWindow`, `onViewDetachedFromWindow`)
//! when a view is removed from an attached parent or the activity finishes.

use std::collections::HashSet;

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::{HostObj, MAX_VIEW_DEPTH};
use crate::heap::ObjRef;
use crate::value::Value;

const LISTENER: &str = "Landroid/view/View$OnAttachStateChangeListener;";

fn children(vm: &Engine, v: ObjRef) -> Vec<ObjRef> {
    match vm.fw.get(v) {
        Some(HostObj::View { children, .. }) => children.clone(),
        _ => Vec::new(),
    }
}

fn parent_of(vm: &Engine, v: ObjRef) -> Option<ObjRef> {
    match vm.fw.get(v) {
        Some(HostObj::View { parent, .. }) => *parent,
        _ => None,
    }
}

/// Call a DEX override of `name()V` on `view`. Host (android/*)
/// implementations are skipped because they're no-ops headless.
fn call_override(vm: &mut Engine, view: ObjRef, name: &str) -> Result<(), VmExit> {
    let class = vm.heap.class_of(view)?.to_string();
    if let Some((d, def, m)) = vm.cp.resolve_method(&class, name, "()V") {
        let declaring = vm.cp.dexes[d].type_str(def.class_idx).to_string();
        if !declaring.starts_with("Landroid/") {
            vm.call(d, def, &m, vec![Value::Obj(view)])?;
        }
    }
    Ok(())
}

fn notify(vm: &mut Engine, view: ObjRef, attached: bool) -> Result<(), VmExit> {
    let listeners = vm.attach_listeners.get(&view).cloned().unwrap_or_default();
    let method = if attached {
        "onViewAttachedToWindow"
    } else {
        "onViewDetachedFromWindow"
    };
    for l in listeners {
        let class = vm.heap.class_of(l)?.to_string();
        if let Some((d, def, m)) = vm
            .cp
            .resolve_method(&class, method, "(Landroid/view/View;)V")
        {
            vm.call(d, def, &m, vec![Value::Obj(l), Value::Obj(view)])?;
        }
    }
    Ok(())
}

/// Attach `root` and its subtree (pre-order). Already-attached views are
/// skipped. Bounded by `MAX_VIEW_DEPTH` and cycle-guarded on an explicit heap
/// worklist so deep or corrupted View graphs never exhaust the native stack.
pub(crate) fn dispatch_attach(vm: &mut Engine, root: ObjRef) -> Result<(), VmExit> {
    let mut stack = vec![(root, 0usize)];
    let mut seen = HashSet::new();
    while let Some((cur, depth)) = stack.pop() {
        if depth > MAX_VIEW_DEPTH || !seen.insert(cur) {
            continue;
        }
        if vm.attached_views.insert(cur) {
            call_override(vm, cur, "onAttachedToWindow")?;
            notify(vm, cur, true)?;
        }
        // children may have been added by onAttachedToWindow (Compose adds its
        // AndroidComposeView there): read them after the callback and push in
        // reverse so pre-order visits child 0, 1, 2, ...
        for c in children(vm, cur).into_iter().rev() {
            stack.push((c, depth + 1));
        }
    }
    Ok(())
}

/// Detach `root` and its subtree (post-order: children before parent).
/// Iterative and bounded by `MAX_VIEW_DEPTH` + `seen` cycle guard.
pub(crate) fn dispatch_detach(vm: &mut Engine, root: ObjRef) -> Result<(), VmExit> {
    let mut stack = vec![(root, 0usize, false)];
    let mut seen = HashSet::new();
    while let Some((cur, depth, expanded)) = stack.pop() {
        if !expanded {
            if depth > MAX_VIEW_DEPTH || !seen.insert(cur) {
                continue;
            }
            stack.push((cur, depth, true));
            for c in children(vm, cur).into_iter().rev() {
                stack.push((c, depth + 1, false));
            }
        } else if vm.attached_views.remove(&cur) {
            call_override(vm, cur, "onDetachedFromWindow")?;
            notify(vm, cur, false)?;
        }
    }
    Ok(())
}

/// `makeVisible`: attach the activity's window tree (decor if created, else
/// the content root).
pub(crate) fn attach_activity_window(vm: &mut Engine, activity: ObjRef) -> Result<(), VmExit> {
    let w = crate::framework::ensure_window(vm, activity)?;
    let root = match vm.window_decor.get(&w) {
        Some((decor, _)) => Some(*decor),
        None => match vm.fw.get(w) {
            Some(HostObj::Window { content }) => *content,
            _ => None,
        },
    };
    if let Some(r) = root {
        vm.window_attached.insert(w);
        dispatch_attach(vm, r)?;
        // floating ViewTreeObservers merge into the window's; first traversal
        crate::vto::merge_into_root(vm, r)?;
        crate::vto::traversal(vm, r)?;
    }
    Ok(())
}

/// Called after a child is inserted: attach it if the parent is attached.
pub(crate) fn on_child_added(vm: &mut Engine, parent: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    if vm.attached_views.contains(&parent) {
        dispatch_attach(vm, child)?;
    }
    Ok(())
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !mref_class.starts_with("Landroid/view/") && !mref_class.starts_with("Landroid/widget/") {
        return Ok(None);
    }
    if !matches!(vm.fw.get(recv), Some(HostObj::View { .. })) {
        return Ok(None);
    }
    // View(Context, …) constructors: remember the Context (side effect only;
    // the framework constructor still runs)
    if name == "<init>" && sig.starts_with("(Landroid/content/Context;") {
        if let Some(c) = args.first().and_then(|v| v.as_ref().ok().flatten()) {
            vm.view_props
                .insert((recv, "rd_context".to_string()), Value::Obj(c));
        }
        return Ok(None);
    }
    match (name, sig) {
        ("getContext", "()Landroid/content/Context;") => {
            // the constructor Context, else (inflated/host-created views) the
            // activity hosting the window
            let c = match vm.view_props.get(&(recv, "rd_context".to_string())) {
                Some(v) => v.clone(),
                None => vm.fw.current_activity.map_or(Value::Null, Value::Obj),
            };
            Ok(Some(c))
        }
        ("addOnAttachStateChangeListener", s) if s == format!("({LISTENER})V") => {
            if let Some(l) = args.first().and_then(|v| v.as_ref().ok().flatten()) {
                let list = vm.attach_listeners.entry(recv).or_default();
                if !list.contains(&l) {
                    list.push(l);
                }
            }
            Ok(Some(Value::Null))
        }
        ("removeOnAttachStateChangeListener", s) if s == format!("({LISTENER})V") => {
            if let Some(l) = args.first().and_then(|v| v.as_ref().ok().flatten()) {
                if let Some(list) = vm.attach_listeners.get_mut(&recv) {
                    list.retain(|x| *x != l);
                }
            }
            Ok(Some(Value::Null))
        }
        // view-tree navigation from the host hierarchy. The decor's real
        // parent is ViewRootImpl (not a View); headless it is null.
        ("getParent", "()Landroid/view/ViewParent;") => {
            Ok(Some(parent_of(vm, recv).map_or(Value::Null, Value::Obj)))
        }
        ("getRootView", "()Landroid/view/View;") => {
            let mut cur = recv;
            for _ in 0..=MAX_VIEW_DEPTH {
                let Some(p) = parent_of(vm, cur) else { break };
                cur = p;
            }
            Ok(Some(Value::Obj(cur)))
        }
        // window token: a Binder while attached (AttachInfo.mWindowToken),
        // null when detached
        ("getWindowToken" | "getApplicationWindowToken", "()Landroid/os/IBinder;") => {
            if !vm.attached_views.contains(&recv) {
                return Ok(Some(Value::Null));
            }
            let t = match vm.window_token {
                Some(t) => t,
                None => {
                    let t = vm
                        .heap
                        .alloc_instance("Landroid/os/Binder;".to_string(), Vec::new())?;
                    vm.window_token = Some(t);
                    t
                }
            };
            Ok(Some(Value::Obj(t)))
        }
        // AOSP: mAttachInfo.mHandler (the window's main-looper Handler) while
        // attached, null otherwise
        ("getHandler", "()Landroid/os/Handler;") => {
            if !vm.attached_views.contains(&recv) {
                return Ok(Some(Value::Null));
            }
            let key = (
                "Landroid/view/View;".to_string(),
                "rd_attach_handler".to_string(),
            );
            if let Some(Value::Obj(h)) = vm.statics.get(&key) {
                return Ok(Some(Value::Obj(*h)));
            }
            let looper = vm.main_looper_obj()?;
            let h = vm
                .heap
                .alloc_instance("Landroid/os/Handler;".to_string(), Vec::new())?;
            vm.attach_host_state("Landroid/os/Handler;", h)?;
            crate::framework::call_host_instance(
                vm,
                "Landroid/os/Handler;",
                "<init>",
                "(Landroid/os/Looper;)V",
                h,
                &[Value::Obj(looper)],
            )?;
            vm.statics.insert(key, Value::Obj(h));
            Ok(Some(Value::Obj(h)))
        }
        ("isAttachedToWindow", "()Z") => {
            Ok(Some(Value::Int(vm.attached_views.contains(&recv) as i32)))
        }
        _ => Ok(None),
    }
}
