//! One private detail-message slot for interpreted, host, and JNI throwables.
//! Application fields named `message` cannot shadow java.lang.Throwable state.
use crate::{err, Engine, ObjRef, Value, VmExit};
#[cfg(not(feature = "desktop-runtime"))]
use alloc::string::String;
use alloc::{string::ToString, vec};
pub(crate) const SLOT: &str = "java.lang.Throwable#detailMessage";

pub(crate) fn set(vm: &mut Engine, object: ObjRef, value: Value) -> Result<(), VmExit> {
    err::put_field_result(vm.heap.put_field(object, SLOT, value))
}
pub(crate) fn stored(vm: &Engine, object: ObjRef) -> Result<Value, VmExit> {
    if !vm
        .cp
        .is_subtype(vm.heap.class_of(object)?, "Ljava/lang/Throwable;")
    {
        return Err(err::vm_error(
            "VM_TYPE_ERROR",
            "Throwable message receiver is not Throwable",
        )
        .into());
    }
    // new-instance initializes inherited fields to Java defaults, even before
    // a constructor has written a message (e.g. an allocated RemoteException).
    let value = vm.heap.get_field(object, SLOT).unwrap_or(Value::Null);
    if let Some(reference) = value.as_ref()? {
        vm.heap.as_str(reference)?;
    }
    Ok(value)
}
#[cfg(not(feature = "desktop-runtime"))]
pub(crate) fn message(vm: &Engine, object: ObjRef) -> Result<Option<String>, VmExit> {
    stored(vm, object)?
        .as_ref()?
        .map(|r| vm.heap.as_str(r).map(ToString::to_string))
        .transpose()
        .map_err(Into::into)
}
pub(crate) fn materialize(
    vm: &mut Engine,
    class: &str,
    message: Option<&str>,
) -> Result<ObjRef, VmExit> {
    let object = vm
        .heap
        .alloc_instance(class.to_string(), vec![])
        .map_err(err::oom_throwable)?;
    let message = match message {
        Some(text) => Value::Obj(
            vm.heap
                .alloc_string(text.to_string())
                .map_err(err::oom_throwable)?,
        ),
        None => Value::Null,
    };
    set(vm, object, message)?;
    Ok(object)
}
