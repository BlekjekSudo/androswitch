//! Android Pair values are ordinary GC-scanned heap fields. Component methods
//! use shared object dispatch; unavailable component behavior remains an error.
use super::*;
const CLASS: &str = "Landroid/util/Pair;";
pub(super) fn metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<rd_vm::framework_host::HostMethodKind> {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance, Static};
    match (class, name, sig) {
        (CLASS, "<init>", "(Ljava/lang/Object;Ljava/lang/Object;)V") => Some(Constructor),
        (CLASS, "create", "(Ljava/lang/Object;Ljava/lang/Object;)Landroid/util/Pair;") => {
            Some(Static)
        }
        (CLASS, "equals", "(Ljava/lang/Object;)Z")
        | (CLASS, "hashCode", "()I")
        | (CLASS, "toString", "()Ljava/lang/String;") => Some(Instance),
        _ => None,
    }
}
pub(super) fn attach(vm: &mut Engine, class: &str, object: ObjRef) -> Result<bool> {
    if !vm.cp.is_subtype(class, CLASS) {
        return Ok(false);
    }
    write(vm, object, "first", Value::Null)?;
    write(vm, object, "second", Value::Null)?;
    Ok(true)
}
fn assign(vm: &mut Engine, object: ObjRef, args: &[Value]) -> Result<()> {
    for (i, field) in ["first", "second"].into_iter().enumerate() {
        let value = args
            .get(i)
            .ok_or_else(|| error("VM_TYPE_ERROR", "missing Pair component"))?;
        value.as_ref().map_err(|e| error("VM_TYPE_ERROR", e))?;
        write(vm, object, field, value.clone())?;
    }
    Ok(())
}
pub(super) fn create(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>> {
    if metadata(class, name, sig) != Some(rd_vm::framework_host::HostMethodKind::Static) {
        return Ok(None);
    }
    let object = shell(vm, CLASS)?;
    assign(vm, object, args)?;
    Ok(Some(Value::Obj(object)))
}
fn hash(vm: &mut Engine, object: ObjRef, field: &str) -> Result<i32> {
    let value = vm.heap.get_field(object, field)?;
    match value.as_ref()? {
        None => Ok(0),
        Some(value) => vm
            .invoke_instance(value, "hashCode", "()I", &[])?
            .as_int()
            .map_err(|e| error("VM_TYPE_ERROR", e)),
    }
}
fn component_text(vm: &mut Engine, object: ObjRef, field: &str) -> Result<String> {
    let value = vm.heap.get_field(object, field)?;
    let Some(value) = value.as_ref()? else {
        return Ok("null".into());
    };
    let out = vm.invoke_instance(value, "toString", "()Ljava/lang/String;", &[])?;
    if matches!(out, Value::Null) {
        return Ok("null".into());
    }
    text(vm, &out)
}
pub(super) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    object: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    if !vm.cp.is_subtype(vm.heap.class_of(object)?, CLASS) {
        return Ok(None);
    }
    if name == "<init>"
        && metadata(class, name, sig) == Some(rd_vm::framework_host::HostMethodKind::Constructor)
    {
        assign(vm, object, args)?;
        return Ok(Some(Value::Null));
    }
    if metadata(CLASS, name, sig) != Some(rd_vm::framework_host::HostMethodKind::Instance) {
        return Ok(None);
    }
    let value = match name {
        "equals" => {
            let Some(other) = args[0].as_ref()? else {
                return Ok(Some(Value::Int(0)));
            };
            if !vm.cp.is_subtype(vm.heap.class_of(other)?, CLASS) {
                return Ok(Some(Value::Int(0)));
            }
            let left = vm.heap.get_field(object, "first")?;
            let right = vm.heap.get_field(other, "first")?;
            // API26 delegates to the argument pair's components first.
            let first = collections::equals(vm, &right, &left)?;
            let second = if first {
                let left = vm.heap.get_field(object, "second")?;
                let right = vm.heap.get_field(other, "second")?;
                collections::equals(vm, &right, &left)?
            } else {
                false
            };
            Value::Int(i32::from(second))
        }
        "hashCode" => Value::Int(hash(vm, object, "first")? ^ hash(vm, object, "second")?),
        "toString" => {
            let first = component_text(vm, object, "first")?;
            let second = component_text(vm, object, "second")?;
            string(vm, format!("Pair{{{first} {second}}}"))?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
