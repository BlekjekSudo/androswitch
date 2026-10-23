//! Unhandled intrinsic lookup returns `None` so the shared interpreter can
//! resolve the actual DEX method or report its exact unresolved descriptor.
//! This is not a successful implementation of any Android service.
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::string::String;

pub fn alloc_string(vm: &mut Engine, text: String) -> Result<ObjRef, VmExit> {
    if let Some(object) = vm.portable.strings.get(&text) {
        return Ok(*object);
    }
    let object = vm.heap.alloc_string(text.clone())?;
    vm.portable.strings.insert(text, object);
    Ok(object)
}

pub fn call_native_loader(
    _vm: &mut Engine,
    _class: &str,
    _name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(None)
}
pub fn call_static_intrinsic(
    _vm: &mut Engine,
    _class: &str,
    _name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(None)
}
#[allow(clippy::too_many_arguments)]
pub fn call_instance_intrinsic_mref(
    _vm: &mut Engine,
    _receiver_class: &str,
    _declaring_class: &str,
    _name: &str,
    _sig: &str,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(None)
}
