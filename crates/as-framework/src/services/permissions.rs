//! v0.1 capability policy: declared storage permissions map to the shared SD
//! sandbox already exposed by the VFS. Unimplemented services are denied.
use super::*;
fn granted(android: &Android, permission: &str) -> bool {
    matches!(
        permission,
        "android.permission.READ_EXTERNAL_STORAGE" | "android.permission.WRITE_EXTERNAL_STORAGE"
    ) && android
        .apk
        .manifest
        .uses_permissions
        .iter()
        .any(|p| p.name == permission && p.max_sdk.is_none_or(|n| n >= 26))
}
pub(super) fn call(
    android: &Android,
    vm: &mut Engine,
    name: &str,
    sig: &str,
    r: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    let class = vm.heap.class_of(r)?.to_string();
    if !vm.cp.is_subtype(&class, "Landroid/content/Context;") {
        return Ok(None);
    }
    let value = match (name, sig) {
        ("checkSelfPermission", "(Ljava/lang/String;)I") => {
            Value::Int(if granted(android, &text(vm, &args[0])?) {
                0
            } else {
                -1
            })
        }
        ("enforceCallingOrSelfPermission", "(Ljava/lang/String;Ljava/lang/String;)V") => {
            let permission = text(vm, &args[0])?;
            if !granted(android, &permission) {
                return Err(Throwable::new("Ljava/lang/SecurityException;", permission).into());
            }
            Value::Null
        }
        ("requestPermissions", "([Ljava/lang/String;I)V")
            if vm.cp.is_subtype(&class, "Landroid/app/Activity;") =>
        {
            let names = vm.heap.as_array_elems(obj(&args[0])?)?.0.to_vec();
            let results = names
                .iter()
                .map(|v| {
                    Ok(Value::Int(if granted(android, &text(vm, v)?) {
                        0
                    } else {
                        -1
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            let array =
                vm.heap
                    .alloc_array_zeroed(rd_vm::heap::ElemKind::Int, results.len(), "[I")?;
            vm.heap.array_elems_mut(array)?.clone_from_slice(&results);
            if vm
                .cp
                .resolve_method(
                    &class,
                    "onRequestPermissionsResult",
                    "(I[Ljava/lang/String;[I)V",
                )
                .is_some()
            {
                android.callback(
                    vm,
                    r,
                    "onRequestPermissionsResult",
                    "(I[Ljava/lang/String;[I)V",
                    &[args[1].clone(), args[0].clone(), Value::Obj(array)],
                )?;
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
