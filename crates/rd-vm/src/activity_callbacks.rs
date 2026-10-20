//! `Application.ActivityLifecycleCallbacks`, registered on the Application
//! or (API 29+) on a single Activity.
//!
//! AndroidX Lifecycle on API 29+ drives `ON_CREATE`/`ON_START`/`ON_RESUME`/…
//! from `ReportFragment.LifecycleCallbacks`, which is registered with
//! `Activity.registerActivityLifecycleCallbacks` and listens to
//! `onActivityPostCreated`, `onActivityPostStarted`, `onActivityPrePaused`,
//! and so on. Without dispatch, an app's Lifecycle stays INITIALIZED:
//! `repeatOnLifecycle`, `collectAsStateWithLifecycle`, `LiveData` observers
//! and Compose lifecycle effects never run.
//!
//! The dispatch order follows AOSP `Activity.performX` /
//! `Activity.dispatchActivityX`:
//! - **pre** (`onActivityPreX`): application callbacks, then the
//!   activity's own;
//! - **the activity's `onX`**;
//! - **main** (`onActivityX`, sent from `super.onX`): application, then
//!   the activity's own;
//! - **post** (`onActivityPostX`): the activity's own, then the
//!   application's.
//!
//! For pause/stop/destroy, AOSP sends `main` before `onX` returns. That
//! timing isn't observable here, so `main` runs after `onX`.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

const CB_SIG: &str = "(Landroid/app/Application$ActivityLifecycleCallbacks;)V";

#[derive(Debug, Default, Clone)]
pub(crate) struct Registry {
    /// (activity it's scoped to, or None = Application-wide; callback)
    pub entries: Vec<(Option<ObjRef>, ObjRef)>,
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let scope = match mref_class {
        "Landroid/app/Activity;" => Some(recv),
        "Landroid/app/Application;" => None,
        _ => return Ok(None),
    };
    if sig != CB_SIG {
        return Ok(None);
    }
    let cb = args.first().and_then(|v| v.as_ref().ok().flatten());
    match name {
        "registerActivityLifecycleCallbacks" => {
            if let Some(cb) = cb {
                vm.activity_callbacks.entries.push((scope, cb));
            }
            Ok(Some(Value::Null))
        }
        "unregisterActivityLifecycleCallbacks" => {
            if let Some(cb) = cb {
                if let Some(p) = vm
                    .activity_callbacks
                    .entries
                    .iter()
                    .position(|(s, c)| *s == scope && *c == cb)
                {
                    vm.activity_callbacks.entries.remove(p);
                }
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// Lifecycle method name → callback event stem ("onCreate" → "Created").
pub(crate) fn event_of(step: &str) -> Option<&'static str> {
    Some(match step {
        "onCreate" => "Created",
        "onStart" => "Started",
        "onResume" => "Resumed",
        "onPause" => "Paused",
        "onStop" => "Stopped",
        "onDestroy" => "Destroyed",
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Pre,
    Main,
    Post,
}

fn targets(vm: &Engine, activity: ObjRef, phase: Phase) -> Vec<ObjRef> {
    let app: Vec<ObjRef> = vm
        .activity_callbacks
        .entries
        .iter()
        .filter(|(s, _)| s.is_none())
        .map(|(_, c)| *c)
        .collect();
    let own: Vec<ObjRef> = vm
        .activity_callbacks
        .entries
        .iter()
        .filter(|(s, _)| *s == Some(activity))
        .map(|(_, c)| *c)
        .collect();
    match phase {
        Phase::Post => own.into_iter().chain(app).collect(),
        _ => app.into_iter().chain(own).collect(),
    }
}

pub(crate) fn dispatch(
    vm: &mut Engine,
    activity: ObjRef,
    event: &str,
    phase: Phase,
    bundle: Option<Value>,
) -> Result<(), VmExit> {
    let method = match phase {
        Phase::Pre => format!("onActivityPre{event}"),
        Phase::Main => format!("onActivity{event}"),
        Phase::Post => format!("onActivityPost{event}"),
    };
    let with_bundle = event == "Created";
    let sig = if with_bundle {
        "(Landroid/app/Activity;Landroid/os/Bundle;)V"
    } else {
        "(Landroid/app/Activity;)V"
    };
    for cb in targets(vm, activity, phase) {
        let class = vm.heap.class_of(cb)?.to_string();
        // only DEX implementations: the Pre/Post interface defaults are
        // empty in the platform
        if let Some((d, def, m)) = vm.cp.resolve_method(&class, &method, sig) {
            let mut slots = vec![Value::Obj(cb), Value::Obj(activity)];
            if with_bundle {
                slots.push(bundle.clone().unwrap_or(Value::Null));
            }
            vm.call(d, def, &m, slots)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::event_of;

    #[test]
    fn events() {
        assert_eq!(event_of("onCreate"), Some("Created"));
        assert_eq!(event_of("onPause"), Some("Paused"));
        assert_eq!(event_of("onRestart"), None);
    }
}
