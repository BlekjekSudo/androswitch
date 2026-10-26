//! Scope adapter for shared upstream intrinsics. No host-state fallback.
use crate::{Engine, Value, VmExit};

pub(crate) fn pin(vm: &mut Engine, values: &[Value]) {
    for value in values {
        if let Value::Obj(reference) = value {
            vm.heap.pin_allocation(*reference);
        }
    }
}

pub(crate) fn with_values(
    vm: &mut Engine,
    args: &[Value],
    operation: impl FnOnce(&mut Engine) -> Result<Option<Value>, VmExit>,
) -> Result<Option<Value>, VmExit> {
    let mark = vm.heap.gc_scope_depth();
    vm.heap.gc_host_enter(false);
    pin(vm, args);
    let result = operation(vm);
    let returned = match &result {
        Ok(Some(Value::Obj(reference))) => Some(*reference),
        Err(VmExit::Exception(exception)) => exception.obj,
        _ => None,
    };
    #[cfg(feature = "std")]
    {
        vm.heap.gc_scope_restore(mark);
        if let Some(reference) = returned {
            vm.heap.gc_pin_return(reference);
        }
    }
    #[cfg(not(feature = "std"))]
    vm.heap.gc_scope_restore_return(mark, returned);
    result
}
