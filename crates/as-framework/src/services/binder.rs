//! Process-local Binder interface ownership. No kernel Binder, remote service,
//! Parcel transport or calling-process identity is synthesized.
use super::*;
const BINDER: &str = "Landroid/os/Binder;";
const IBINDER: &str = "Landroid/os/IBinder;";
const INTERFACE: &str = "Landroid/os/IInterface;";
const OWNER: &str = "android.os.Binder#owner";
const DESCRIPTOR: &str = "android.os.Binder#descriptor";
const INITIALIZED: &str = "android.os.Binder#initialized";

pub(super) fn attach(vm: &mut Engine, class: &str, receiver: ObjRef) -> Result<bool> {
    if !vm.cp.is_subtype(class, BINDER) {
        return Ok(false);
    }
    for field in [OWNER, DESCRIPTOR] {
        write(vm, receiver, field, Value::Null)?;
    }
    write(vm, receiver, INITIALIZED, Value::Int(0))?;
    Ok(true)
}
pub(super) fn metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<rd_vm::framework_host::HostMethodKind> {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance};
    match (class, name, sig) {
        (BINDER, "<init>", "()V") => Some(Constructor),
        (BINDER, "attachInterface", "(Landroid/os/IInterface;Ljava/lang/String;)V") => {
            Some(Instance)
        }
        (
            BINDER | IBINDER,
            "queryLocalInterface",
            "(Ljava/lang/String;)Landroid/os/IInterface;",
        )
        | (BINDER | IBINDER, "getInterfaceDescriptor", "()Ljava/lang/String;")
        | (BINDER | IBINDER, "pingBinder" | "isBinderAlive", "()Z") => Some(Instance),
        _ => None,
    }
}
pub(super) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    if !vm.cp.is_subtype(vm.heap.class_of(receiver)?, BINDER) {
        return Ok(None);
    }
    if (class, name, sig) == (BINDER, "<init>", "()V") {
        write(vm, receiver, OWNER, Value::Null)?;
        write(vm, receiver, DESCRIPTOR, Value::Null)?;
        write(vm, receiver, INITIALIZED, Value::Int(1))?;
        return Ok(Some(Value::Null));
    }
    if metadata(BINDER, name, sig).is_none() {
        return Ok(None);
    }
    if vm.heap.get_field(receiver, INITIALIZED)?.as_int()? != 1 {
        return Err(error("API_UNAVAILABLE", "uninitialized local Binder"));
    }
    let value = match (name, sig) {
        ("attachInterface", "(Landroid/os/IInterface;Ljava/lang/String;)V") => {
            if let Some(owner) = args[0].as_ref()? {
                if !vm.cp.is_subtype(vm.heap.class_of(owner)?, INTERFACE) {
                    return Err(Throwable::new(
                        "Ljava/lang/ClassCastException;",
                        "Binder owner must implement IInterface",
                    )
                    .into());
                }
            }
            if let Some(descriptor) = args[1].as_ref()? {
                vm.heap.as_str(descriptor)?;
            }
            write(vm, receiver, OWNER, args[0].clone())?;
            write(vm, receiver, DESCRIPTOR, args[1].clone())?;
            Value::Null
        }
        ("getInterfaceDescriptor", "()Ljava/lang/String;") => {
            vm.heap.get_field(receiver, DESCRIPTOR)?
        }
        ("queryLocalInterface", "(Ljava/lang/String;)Landroid/os/IInterface;") => {
            let expected = vm.heap.get_field(receiver, DESCRIPTOR)?.as_ref()?;
            let requested = args[0].as_ref()?;
            let matches = match (expected, requested) {
                (Some(a), Some(b)) => vm.heap.as_str(a)? == vm.heap.as_str(b)?,
                _ => false,
            };
            if matches {
                vm.heap.get_field(receiver, OWNER)?
            } else {
                Value::Null
            }
        }
        ("pingBinder" | "isBinderAlive", "()Z") => Value::Int(1),
        _ => return Ok(None),
    };
    Ok(Some(value))
}
