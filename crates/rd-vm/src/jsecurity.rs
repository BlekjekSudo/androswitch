//! `java.security.AccessController` and the small `Permission` subset used
//! by common Android libraries.
//!
//! The VM has no `SecurityManager` or per-domain permission checks. In this
//! single-threaded model, `doPrivileged` therefore invokes the supplied DEX
//! action inline and returns its result; `checkPermission` is a no-op. The
//! `RuntimePermission` object stores its name so ordinary `Permission.getName`
//! calls remain useful. Unknown APIs remain explicit `NOT_IMPLEMENTED` gaps.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const ACCESS_CONTROLLER: &str = "Ljava/security/AccessController;";
const PERMISSION: &str = "Ljava/security/Permission;";
const BASIC_PERMISSION: &str = "Ljava/security/BasicPermission;";
const RUNTIME_PERMISSION: &str = "Ljava/lang/RuntimePermission;";
const RUN_SIG: &str = "()Ljava/lang/Object;";

/// Static `AccessController` entry points currently used by Android libraries.
pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != ACCESS_CONTROLLER {
        return Ok(None);
    }

    match (name, sig) {
        ("doPrivileged", s) if do_privileged_signature(s) => {
            let action_value = args.first().ok_or_else(|| {
                crate::err::vm_error("INVALID_ARG", "AccessController.doPrivileged sem action")
            })?;
            let Some(action) = action_value.as_ref()? else {
                return Err(exception(
                    "Ljava/lang/NullPointerException;",
                    "AccessController.doPrivileged(action == null)",
                ));
            };
            // run() is resolved from the DEX receiver class, including bridge
            // methods emitted for lambdas and generic PrivilegedAction classes.
            vm.invoke_instance(action, "run", RUN_SIG, &[]).map(Some)
        }
        // Android has no active SecurityManager in this VM; permission checks
        // therefore grant access just as the platform path with no manager.
        ("checkPermission", "(Ljava/security/Permission;)V") => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}

/// `RuntimePermission(String)` plus the useful inherited Permission readers.
pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (recv_class, mref_class, name, sig) {
        (RUNTIME_PERMISSION, RUNTIME_PERMISSION, "<init>", "(Ljava/lang/String;)V") => {
            let name_value = args.first().ok_or_else(|| {
                crate::err::vm_error("INVALID_ARG", "RuntimePermission.<init> sem nome")
            })?;
            let Some(name_ref) = name_value.as_ref()? else {
                return Err(exception(
                    "Ljava/lang/NullPointerException;",
                    "RuntimePermission name == null",
                ));
            };
            let name = vm
                .heap
                .as_str(name_ref)
                .map_err(|e| crate::err::vm_error("INVALID_ARG", e))?;
            if name.is_empty() {
                return Err(exception(
                    "Ljava/lang/IllegalArgumentException;",
                    "RuntimePermission name cannot be empty",
                ));
            }
            crate::err::put_field_result(vm.heap.put_field(
                recv,
                "permission_name",
                name_value.clone(),
            ))?;
            Ok(Some(Value::Null))
        }
        (
            RUNTIME_PERMISSION,
            PERMISSION | BASIC_PERMISSION | RUNTIME_PERMISSION,
            "getName",
            "()Ljava/lang/String;",
        ) => Ok(Some(
            vm.heap
                .get_field(recv, "permission_name")
                .unwrap_or(Value::Null),
        )),
        (
            RUNTIME_PERMISSION,
            PERMISSION | BASIC_PERMISSION | RUNTIME_PERMISSION,
            "getActions",
            "()Ljava/lang/String;",
        ) => Ok(Some(Value::Obj(crate::intrinsics::alloc_string(
            vm,
            String::new(),
        )?))),
        _ => Ok(None),
    }
}

fn do_privileged_signature(sig: &str) -> bool {
    matches!(
        sig,
        "(Ljava/security/PrivilegedAction;)Ljava/lang/Object;"
            | "(Ljava/security/PrivilegedAction;Ljava/security/AccessControlContext;)Ljava/lang/Object;"
            | "(Ljava/security/PrivilegedAction;Ljava/security/AccessControlContext;[Ljava/security/Permission;)Ljava/lang/Object;"
            | "(Ljava/security/PrivilegedExceptionAction;)Ljava/lang/Object;"
            | "(Ljava/security/PrivilegedExceptionAction;Ljava/security/AccessControlContext;)Ljava/lang/Object;"
            | "(Ljava/security/PrivilegedExceptionAction;Ljava/security/AccessControlContext;[Ljava/security/Permission;)Ljava/lang/Object;"
    )
}

fn exception(class: &str, message: &str) -> VmExit {
    VmExit::Exception(Throwable::new(class, message))
}
