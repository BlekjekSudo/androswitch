//! `android.view.ViewTreeObserver`.
//!
//! As in AOSP:
//! - **Attached views** share their window's observer.
//! - **A detached tree** gets a *floating* observer keyed by its current
//!   root. When the tree is attached, its listeners merge into the window
//!   observer (`View.dispatchAttachedToWindow` → `mFloatingTreeObserver.merge`).
//! - **Traversals** (the first frame after `makeVisible`, or explicit
//!   `dispatchOnGlobalLayout`) fire listeners in order: global-layout, then
//!   pre-draw, then draw.
//! - **`isAlive`** is false only for an observer that was merged away.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const VTO: &str = "Landroid/view/ViewTreeObserver;";

#[derive(Debug, Default, Clone)]
pub(crate) struct Observer {
    /// (listener kind = interface simple name, listener object)
    pub listeners: Vec<(String, ObjRef)>,
    pub alive: bool,
}

fn root_of(vm: &Engine, mut v: ObjRef) -> ObjRef {
    for _ in 0..=crate::framework::MAX_VIEW_DEPTH {
        let Some(HostObj::View {
            parent: Some(p), ..
        }) = vm.fw.get(v)
        else {
            break;
        };
        v = *p;
    }
    v
}

/// The observer for `view`. Attached views share their root's observer;
/// detached trees get one keyed by their root.
pub(crate) fn observer_for(vm: &mut Engine, view: ObjRef) -> Result<ObjRef, VmExit> {
    let root = root_of(vm, view);
    if let Some(o) = vm.vto_by_root.get(&root) {
        return Ok(*o);
    }
    let o = vm.heap.alloc_instance(VTO.to_string(), Vec::new())?;
    vm.vto_by_root.insert(root, o);
    vm.vto.insert(
        o,
        Observer {
            listeners: Vec::new(),
            alive: true,
        },
    );
    Ok(o)
}

/// After `root` is attached to a window: merge the floating observers of
/// trees now under it, and kill the merged ones.
pub(crate) fn merge_into_root(vm: &mut Engine, root: ObjRef) -> Result<(), VmExit> {
    let target = observer_for(vm, root)?;
    let stale: Vec<(ObjRef, ObjRef)> = vm
        .vto_by_root
        .iter()
        .filter(|(r, o)| **r != root && **o != target)
        .map(|(r, o)| (*r, *o))
        .collect();
    for (r, o) in stale {
        if root_of(vm, r) != root {
            continue; // still a detached tree
        }
        vm.vto_by_root.remove(&r);
        let moved = vm.vto.get_mut(&o).map(|ob| {
            ob.alive = false;
            std::mem::take(&mut ob.listeners)
        });
        if let (Some(moved), Some(t)) = (moved, vm.vto.get_mut(&target)) {
            t.listeners.extend(moved);
        }
    }
    Ok(())
}

fn fire(vm: &mut Engine, obs: ObjRef, kind: &str, method: &str, sig: &str) -> Result<(), VmExit> {
    let ls: Vec<ObjRef> = vm
        .vto
        .get(&obs)
        .map(|o| {
            o.listeners
                .iter()
                .filter(|(k, _)| k == kind)
                .map(|(_, l)| *l)
                .collect()
        })
        .unwrap_or_default();
    for l in ls {
        let class = vm.heap.class_of(l)?.to_string();
        if let Some((d, def, m)) = vm.cp.resolve_method(&class, method, sig) {
            vm.call(d, def, &m, vec![Value::Obj(l)])?;
        }
    }
    Ok(())
}

/// One traversal: global layout, then pre-draw, then draw (ViewRootImpl
/// `performTraversals` order).
pub(crate) fn traversal(vm: &mut Engine, root: ObjRef) -> Result<(), VmExit> {
    let obs = observer_for(vm, root)?;
    fire(vm, obs, "OnGlobalLayoutListener", "onGlobalLayout", "()V")?;
    fire(vm, obs, "OnPreDrawListener", "onPreDraw", "()Z")?;
    fire(vm, obs, "OnDrawListener", "onDraw", "()V")?;
    Ok(())
}

fn kind_of(sig: &str) -> Option<String> {
    // "(Landroid/view/ViewTreeObserver$OnGlobalLayoutListener;)V"
    let inner = sig.strip_prefix("(Landroid/view/ViewTreeObserver$")?;
    let (k, _) = inner.split_once(';')?;
    Some(k.to_string())
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "getViewTreeObserver"
        && sig == "()Landroid/view/ViewTreeObserver;"
        && matches!(vm.fw.get(recv), Some(HostObj::View { .. }))
    {
        return Ok(Some(Value::Obj(observer_for(vm, recv)?)));
    }
    if mref_class != VTO || !vm.vto.contains_key(&recv) {
        return Ok(None);
    }
    let listener = args.first().and_then(|v| v.as_ref().ok().flatten());
    if let Some(kind) = kind_of(sig) {
        if let Some(rest) = name.strip_prefix("add") {
            let _ = rest;
            if let (Some(l), Some(o)) = (listener, vm.vto.get_mut(&recv)) {
                o.listeners.push((kind, l));
            }
            return Ok(Some(Value::Null));
        }
        if name.starts_with("remove") {
            // removeGlobalOnLayoutListener is the deprecated alias
            if let (Some(l), Some(o)) = (listener, vm.vto.get_mut(&recv)) {
                if let Some(p) = o.listeners.iter().position(|(k, x)| *k == kind && *x == l) {
                    o.listeners.remove(p);
                }
            }
            return Ok(Some(Value::Null));
        }
    }
    match (name, sig) {
        ("isAlive", "()Z") => Ok(Some(Value::Int(vm.vto[&recv].alive as i32))),
        ("dispatchOnGlobalLayout", "()V") => {
            fire(vm, recv, "OnGlobalLayoutListener", "onGlobalLayout", "()V")?;
            Ok(Some(Value::Null))
        }
        ("dispatchOnPreDraw", "()Z") => {
            fire(vm, recv, "OnPreDrawListener", "onPreDraw", "()Z")?;
            Ok(Some(Value::Int(0)))
        }
        ("dispatchOnDraw", "()V") => {
            fire(vm, recv, "OnDrawListener", "onDraw", "()V")?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::kind_of;

    #[test]
    fn listener_kind_from_signature() {
        assert_eq!(
            kind_of("(Landroid/view/ViewTreeObserver$OnGlobalLayoutListener;)V").as_deref(),
            Some("OnGlobalLayoutListener")
        );
        assert_eq!(kind_of("()Z"), None);
    }
}
