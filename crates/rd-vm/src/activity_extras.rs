//! Remaining `android.app.Activity` platform surface used by AndroidX
//! `ComponentActivity`/`FragmentActivity`: configuration-change state,
//! results, back dispatch (API 33+ `OnBackInvokedDispatcher`), and the
//! superclass callback hooks.
//!
//! Semantics:
//! - **No configuration changes happen headless.**
//!   `getLastNonConfigurationInstance()` is null and
//!   `isChangingConfigurations()` is false.
//! - **`setResult`** is recorded per activity (`activity_results`).
//! - **`super.onBackPressed()`** finishes the activity, as AOSP's default
//!   does.
//! - **`getOnBackInvokedDispatcher()`** returns a per-activity host
//!   dispatcher whose registered callbacks are kept by priority. The BACK
//!   key invokes the highest-priority one first (`back_invoked_top`).
//! - **`super.onX` hooks** with no observable base behaviour are no-ops:
//!   `onConfigurationChanged`, `onNewIntent`, `onTrimMemory`,
//!   `onSaveInstanceState`, … Panel/menu queries answer "no menu".

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

pub(crate) const BACK_DISPATCHER: &str = "Landroid/window/OnBackInvokedDispatcher;";

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == BACK_DISPATCHER || vm.back_dispatchers.values().any(|d| *d == recv) {
        return back_dispatcher(vm, name, sig, recv, args);
    }
    // ContextWrapper / ContextThemeWrapper instantiated directly (e.g.
    // Compose `Dialog` wraps its context in `ContextThemeWrapper(ctx, theme)`):
    // the constructor records the base context; Context-family methods on
    // the wrapper are answered by the Context intrinsics (single app
    // context), and `getBaseContext` returns the recorded base.
    if matches!(
        mref_class,
        "Landroid/content/ContextWrapper;" | "Landroid/view/ContextThemeWrapper;"
    ) {
        match (name, sig) {
            ("<init>", "()V") => return Ok(Some(Value::Null)),
            (
                "<init>",
                "(Landroid/content/Context;)V"
                | "(Landroid/content/Context;I)V"
                | "(Landroid/content/Context;Landroid/content/res/Resources$Theme;)V",
            )
            | ("attachBaseContext", "(Landroid/content/Context;)V") => {
                if let Some(base @ Value::Obj(_)) = args.first() {
                    vm.view_props
                        .insert((recv, "ctx:mBase".to_string()), base.clone());
                }
                return Ok(Some(Value::Null));
            }
            ("getBaseContext", "()Landroid/content/Context;") => {
                if let Some(b) = vm.view_props.get(&(recv, "ctx:mBase".to_string())) {
                    return Ok(Some(b.clone()));
                }
            }
            _ => {}
        }
    }
    // Inherited Context queries must delegate to a wrapper's *actual* base,
    // not assume every receiver is an Activity. Compose Dialog accessibility
    // requests getPackageName through ContextThemeWrapper.
    if vm.cp.is_subtype(mref_class, "Landroid/content/Context;")
        && matches!(
            (name, sig),
            ("getPackageName", "()Ljava/lang/String;")
                | ("getApplicationContext", "()Landroid/content/Context;")
                | (
                    "getSharedPreferences",
                    "(Ljava/lang/String;I)Landroid/content/SharedPreferences;"
                )
        )
    {
        if let Some(Value::Obj(mut base)) =
            vm.view_props.get(&(recv, "ctx:mBase".to_string())).cloned()
        {
            let mut seen = vec![recv];
            loop {
                if seen.len() >= 64 || seen.contains(&base) {
                    return Err(crate::err::vm_error(
                        "VM_STACK_OVERFLOW",
                        "cyclic or over-deep ContextWrapper base chain",
                    )
                    .into());
                }
                seen.push(base);
                let class = vm.heap.class_of(base)?.to_string();
                // Honor application/library DEX overrides before unwrapping.
                if vm.cp.resolve_method(&class, name, sig).is_some() {
                    return vm.invoke_instance(base, name, sig, args).map(Some);
                }
                if let Some(Value::Obj(next)) =
                    vm.view_props.get(&(base, "ctx:mBase".to_string())).cloned()
                {
                    base = next;
                    continue;
                }
                return crate::framework::call_host_instance(
                    vm,
                    "Landroid/content/Context;",
                    name,
                    sig,
                    base,
                    args,
                );
            }
        }
    }
    let app_or_ctx = matches!(
        mref_class,
        "Landroid/app/Application;"
            | "Landroid/content/Context;"
            | "Landroid/content/ContextWrapper;"
    );
    if app_or_ctx {
        return Ok(match (name, sig) {
            (
                "registerComponentCallbacks" | "unregisterComponentCallbacks",
                "(Landroid/content/ComponentCallbacks;)V",
            ) => {
                // no configuration/trim-memory events are emitted headless
                Some(Value::Null)
            }
            _ => None,
        });
    }
    let receiver_class = vm.heap.class_of(recv)?.to_string();
    let activity_window_callback = mref_class == "Landroid/view/Window$Callback;"
        && vm.cp.is_subtype(&receiver_class, "Landroid/app/Activity;");
    if mref_class != "Landroid/app/Activity;" && !activity_window_callback {
        return Ok(None);
    }
    Ok(Some(match (name, sig) {
        ("getLastNonConfigurationInstance", "()Ljava/lang/Object;") => Value::Null,
        ("isChangingConfigurations", "()Z") => Value::Int(0),
        ("getActionBar", "()Landroid/app/ActionBar;") => Value::Null,
        ("setResult", "(I)V") | ("setResult", "(ILandroid/content/Intent;)V") => {
            let code = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            let data = args.get(1).and_then(|v| v.as_ref().ok().flatten());
            vm.activity_results.insert(recv, (code, data));
            Value::Null
        }
        ("onBackPressed", "()V") => {
            vm.finish_activity(recv)?;
            Value::Null
        }
        ("getOnBackInvokedDispatcher", "()Landroid/window/OnBackInvokedDispatcher;") => {
            let d = match vm.back_dispatchers.get(&recv) {
                Some(d) => *d,
                None => {
                    let d = vm
                        .heap
                        .alloc_instance(BACK_DISPATCHER.to_string(), Vec::new())?;
                    vm.back_dispatchers.insert(recv, d);
                    d
                }
            };
            Value::Obj(d)
        }
        (
            "invalidateOptionsMenu" | "reportFullyDrawn" | "onUserInteraction" | "onUserLeaveHint",
            "()V",
        )
        | ("overridePendingTransition", "(II)V")
        | ("onConfigurationChanged", "(Landroid/content/res/Configuration;)V")
        | ("onNewIntent", "(Landroid/content/Intent;)V")
        | ("onTrimMemory", "(I)V")
        | ("onSaveInstanceState", "(Landroid/os/Bundle;)V")
        | ("onActivityResult", "(IILandroid/content/Intent;)V")
        | ("onRequestPermissionsResult", "(I[Ljava/lang/String;[I)V")
        | ("onLowMemory", "()V")
        | ("onTopResumedActivityChanged", "(Z)V")
        | ("onWindowFocusChanged", "(Z)V")
        | ("onAttachedToWindow" | "onDetachedFromWindow" | "onContentChanged", "()V")
        | ("onPanelClosed", "(ILandroid/view/Menu;)V")
        | ("onCreatePanelView", "(I)Landroid/view/View;")
        | ("onMultiWindowModeChanged", _)
        | ("onPictureInPictureModeChanged", _) => Value::Null,
        // no options menu/panels headless
        ("onCreatePanelMenu", "(ILandroid/view/Menu;)Z")
        | ("onPreparePanel", "(ILandroid/view/View;Landroid/view/Menu;)Z")
        | ("onMenuItemSelected", "(ILandroid/view/MenuItem;)Z")
        | ("dispatchKeyShortcutEvent", "(Landroid/view/KeyEvent;)Z") => Value::Int(0),
        // super.dispatchKeyEvent: not consumed by the (empty) window
        ("dispatchKeyEvent", "(Landroid/view/KeyEvent;)Z") => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn back_dispatcher(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("registerOnBackInvokedCallback", "(ILandroid/window/OnBackInvokedCallback;)V") => {
            let prio = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            if let Some(cb) = args.get(1).and_then(|v| v.as_ref().ok().flatten()) {
                let list = vm.back_callbacks.entry(recv).or_default();
                list.retain(|(_, c)| *c != cb);
                list.push((prio, cb));
            }
            Ok(Some(Value::Null))
        }
        ("unregisterOnBackInvokedCallback", "(Landroid/window/OnBackInvokedCallback;)V") => {
            if let Some(cb) = args.first().and_then(|v| v.as_ref().ok().flatten()) {
                if let Some(list) = vm.back_callbacks.get_mut(&recv) {
                    list.retain(|(_, c)| *c != cb);
                }
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// The callback the BACK key should invoke for `activity`: the highest
/// priority, most recently registered (AOSP `WindowOnBackInvokedDispatcher`).
#[allow(dead_code)] // used by the BACK-key path behind feature gates
pub(crate) fn back_invoked_top(vm: &Engine, activity: ObjRef) -> Option<ObjRef> {
    let d = vm.back_dispatchers.get(&activity)?;
    let list = vm.back_callbacks.get(d)?;
    list.iter()
        .enumerate()
        .max_by_key(|(i, (p, _))| (*p, *i))
        .map(|(_, (_, c))| *c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    use rd_testsupport::vm::*;
    #[test]
    fn context_wrapper_delegates_package_query_to_real_base_dex_override() {
        let mut b = DexBuilder::new();
        let cls = b.class("LMyContext;", "Landroid/content/Context;");
        let text = b.intern("custom.base.package") as u16;
        let mut code = op21c(0x1a, 0, text);
        code.extend(op11x(0x11, 0));
        b.r#virtual(
            cls,
            "getPackageName",
            "Ljava/lang/String;",
            vec![],
            ACC_PUBLIC,
            Some(b.code(2, 1, 0, code)),
        );
        let dex = rd_dex::Dex::parse(b.finish()).unwrap();
        let mut e = Engine::new(vec![dex], Default::default());
        let base = e.heap.alloc_instance("LMyContext;".into(), vec![]).unwrap();
        let wrapper = e
            .heap
            .alloc_instance("Landroid/view/ContextThemeWrapper;".into(), vec![])
            .unwrap();
        instance_intrinsic(
            &mut e,
            "Landroid/view/ContextThemeWrapper;",
            "<init>",
            "(Landroid/content/Context;I)V",
            wrapper,
            &[Value::Obj(base), Value::Int(0)],
        )
        .unwrap();
        let got = instance_intrinsic(
            &mut e,
            "Landroid/content/Context;",
            "getPackageName",
            "()Ljava/lang/String;",
            wrapper,
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(got) = got else { panic!() };
        assert_eq!(e.heap.as_str(got).unwrap(), "custom.base.package");
    }
    #[test]
    fn wrapper_delegates_inherited_platform_method_and_rejects_cycles() {
        let mut b = DexBuilder::new();
        b.class("LMyActivity;", "Landroid/app/Activity;");
        let dex = rd_dex::Dex::parse(b.finish()).unwrap();
        let mut e = Engine::new(vec![dex], Default::default());
        e.fw.package_name = "app.package".into();
        let activity = e
            .heap
            .alloc_instance("LMyActivity;".into(), vec![])
            .unwrap();
        let wrapper = e
            .heap
            .alloc_instance("Landroid/view/ContextThemeWrapper;".into(), vec![])
            .unwrap();
        e.view_props
            .insert((wrapper, "ctx:mBase".into()), Value::Obj(activity));
        let got = instance_intrinsic(
            &mut e,
            "Landroid/content/Context;",
            "getPackageName",
            "()Ljava/lang/String;",
            wrapper,
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(got) = got else { panic!() };
        assert_eq!(e.heap.as_str(got).unwrap(), "app.package");
        e.view_props
            .insert((wrapper, "ctx:mBase".into()), Value::Obj(wrapper));
        let err = instance_intrinsic(
            &mut e,
            "Landroid/content/Context;",
            "getPackageName",
            "()Ljava/lang/String;",
            wrapper,
            &[],
        )
        .unwrap_err();
        assert!(matches!(err,VmExit::Error(e) if e.code=="VM_STACK_OVERFLOW"));
    }
}
