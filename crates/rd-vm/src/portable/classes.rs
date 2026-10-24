//! Class names use existing canonical Class objects and DEX nesting metadata.
use crate::{err, Engine, ObjRef, Value, VmExit};
use alloc::string::ToString;

pub(crate) fn call_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
) -> Result<Option<Value>, VmExit> {
    if (class, name, sig) != ("Ljava/lang/Class;", "getSimpleName", "()Ljava/lang/String;") {
        return Ok(None);
    }
    if vm.heap.class_of(receiver)? != "Ljava/lang/Class;" {
        return Err(err::vm_error("VM_TYPE_ERROR", "getSimpleName receiver is not Class").into());
    }
    let descriptor = vm
        .heap
        .get_field(receiver, "class_name")?
        .as_ref()?
        .ok_or_else(|| err::vm_error("INVALID_CLASS_METADATA", "Class descriptor is missing"))?;
    let descriptor = vm.heap.as_str(descriptor)?.to_string();
    let name = vm.cp.class_simple_name(&descriptor)?;
    Ok(Some(Value::Obj(crate::intrinsics::alloc_string(vm, name)?)))
}
