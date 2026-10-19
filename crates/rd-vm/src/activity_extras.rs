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
use crate::framework::HostObj;
use crate::heap::ObjRef;
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
    if mref_class != "Landroid/app/Activity;" {
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
            if let Some(HostObj::Activity { finished, .. }) = vm.fw.objects.get_mut(&recv) {
                *finished = true;
            }
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
