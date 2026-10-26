//! Runtime permission checks and requests, backed by the session's real
//! permission state.
//!
//! - `Context.checkSelfPermission` / `checkPermission` /
//!   `checkCallingOrSelfPermission` answer `PERMISSION_GRANTED` (0) or
//!   `PERMISSION_DENIED` (-1) from the grant set. The MCP server pushes that
//!   set with `Engine::set_granted_permissions`; it covers auto-granted
//!   normal permissions plus runtime grants.
//! - `Activity.requestPermissions(String[], int)` is asynchronous, as on a
//!   device. The result is queued and delivered on the next Looper drain
//!   through the activity's DEX `onRequestPermissionsResult(int, String[],
//!   int[])`; androidx `ComponentActivity` routes it to the
//!   ActivityResultRegistry. No dialog is modelled: the outcome is the
//!   current grant state, which an agent changes with
//!   `permissions_grant`/`permissions_revoke`.
//! - Without a session grant set (the plain CLI), every permission counts as
//!   granted. That is documented, and is what `adb install -g` gives.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::{ElemKind, ObjRef};
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

#[derive(Debug, Clone)]
pub struct PendingPermissionResult {
    pub activity: ObjRef,
    pub request_code: i32,
    pub permissions: ObjRef,
}

impl Engine {
    /// Session grant set: `None` means everything is granted (CLI default).
    pub fn set_granted_permissions<I: IntoIterator<Item = String>>(&mut self, granted: I) {
        self.granted_permissions = Some(granted.into_iter().collect());
    }

    pub(crate) fn permission_granted(&self, name: &str) -> bool {
        self.granted_permissions
            .as_ref()
            .map_or(true, |g| g.contains(name))
    }
}

fn is_context(vm: &Engine, class: &str) -> bool {
    vm.cp.is_subtype(class, "Landroid/content/Context;")
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !mref_class.starts_with("Landroid/") && !is_context(vm, mref_class) {
        return Ok(None);
    }
    let perm_of = |vm: &Engine, v: Option<&Value>| -> Option<String> {
        let r = v?.as_ref().ok()??;
        vm.heap.as_str(r).ok().map(str::to_string)
    };
    match (name, sig) {
        ("checkSelfPermission", "(Ljava/lang/String;)I")
        | ("checkCallingOrSelfPermission", "(Ljava/lang/String;)I")
        | ("checkPermission", "(Ljava/lang/String;II)I")
            if is_context(vm, recv_class) =>
        {
            let p = perm_of(vm, args.first()).ok_or_else(|| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "permission is null",
                ))
            })?;
            Ok(Some(Value::Int(if vm.permission_granted(&p) {
                0
            } else {
                -1
            })))
        }
        ("shouldShowRequestPermissionRationale", "(Ljava/lang/String;)Z")
            if is_context(vm, recv_class) =>
        {
            Ok(Some(Value::Int(0)))
        }
        ("requestPermissions", "([Ljava/lang/String;I)V") if is_context(vm, recv_class) => {
            let Some(arr) = args.first().and_then(|v| v.as_ref().ok().flatten()) else {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "permissions cannot be null",
                )));
            };
            let request_code = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
            if request_code < 0 {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "requestCode should be >= 0",
                )));
            }
            vm.pending_permission_results.push(PendingPermissionResult {
                activity: recv,
                request_code,
                permissions: arr,
            });
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// Deliver queued permission results (called when the main Looper drains).
pub(crate) fn deliver_pending(vm: &mut Engine) -> Result<usize, VmExit> {
    let pending = core::mem::take(&mut vm.pending_permission_results);
    let n = pending.len();
    for p in pending {
        let perms = vm.heap.as_array_elems(p.permissions)?.0.to_vec();
        let mut results = Vec::with_capacity(perms.len());
        for v in &perms {
            let granted = match v.as_ref()? {
                Some(r) => vm.permission_granted(vm.heap.as_str(r)?),
                None => false,
            };
            results.push(Value::Int(if granted { 0 } else { -1 }));
        }
        let grants = vm
            .heap
            .alloc_array(ElemKind::Int, results.len(), "I")
            .map_err(crate::err::oom_throwable)?;
        *vm.heap.array_elems_mut(grants)? = results;
        let class = vm.heap.class_of(p.activity)?.to_string();
        if let Some((d, def, m)) = vm.cp.resolve_method(
            &class,
            "onRequestPermissionsResult",
            "(I[Ljava/lang/String;[I)V",
        ) {
            vm.call(
                d,
                def,
                &m,
                vec![
                    Value::Obj(p.activity),
                    Value::Int(p.request_code),
                    Value::Obj(p.permissions),
                    Value::Obj(grants),
                ],
            )?;
        }
    }
    Ok(n)
}
