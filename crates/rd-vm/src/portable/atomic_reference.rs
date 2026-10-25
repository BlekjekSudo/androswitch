//! Scalar API26 AtomicReference over GC-scanned identity references. VM/JNI
//! dispatch holds the same gate as the scalar atomic implementations. RMW never
//! calls guest code; only toString delegates to the referenced object's method.
use crate::{err, Engine, ObjRef, Value, VmExit};
use core::sync::atomic::{fence, Ordering};
pub(crate) const CLASS: &str = "Ljava/util/concurrent/atomic/AtomicReference;";
const SLOT: &str = "java.util.concurrent.atomic.AtomicReference#value";
pub(crate) fn attach(vm: &mut Engine, receiver: ObjRef) -> Result<(), VmExit> {
    err::put_field_result(vm.heap.put_field(receiver, SLOT, Value::Null))?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn call_instance(
    vm: &mut Engine,
    receiver_class: &str,
    declaring: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        (name, sig),
        ("<init>", "()V" | "(Ljava/lang/Object;)V")
            | ("get", "()Ljava/lang/Object;")
            | ("set" | "lazySet", "(Ljava/lang/Object;)V")
            | ("getAndSet", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | (
                "compareAndSet" | "weakCompareAndSet",
                "(Ljava/lang/Object;Ljava/lang/Object;)Z"
            )
            | ("toString", "()Ljava/lang/String;")
    ) {
        return Ok(None);
    }
    let runtime = vm.heap.class_of(receiver)?;
    if !vm.cp.is_subtype(runtime, CLASS)
        || (name == "<init>" && declaring != CLASS)
        || !(declaring == CLASS
            || vm.cp.is_subtype(declaring, CLASS)
            || (declaring == "Ljava/lang/Object;" && name == "toString"))
        || (name != "<init>"
            && receiver_class != CLASS
            && vm.cp.resolve_method(runtime, name, sig).is_some())
    {
        return Ok(None);
    }
    let argument = |index: usize| -> Result<Value, VmExit> {
        let value = args
            .get(index)
            .ok_or_else(|| err::vm_error("VM_TYPE_ERROR", "missing atomic reference argument"))?;
        value
            .as_ref()
            .map_err(|e| err::vm_error("VM_TYPE_ERROR", e))?;
        Ok(value.clone())
    };
    fence(Ordering::SeqCst);
    let current = vm.heap.get_field(receiver, SLOT)?;
    let identity = current
        .as_ref()
        .map_err(|e| err::vm_error("VM_TYPE_ERROR", e))?;
    let mut next = None;
    let out = match name {
        "<init>" if sig == "()V" => {
            next = Some(Value::Null);
            Value::Null
        }
        "<init>" | "set" | "lazySet" => {
            next = Some(argument(0)?);
            Value::Null
        }
        "get" => current,
        "getAndSet" => {
            next = Some(argument(0)?);
            current
        }
        "compareAndSet" | "weakCompareAndSet" => {
            let expected = argument(0)?
                .as_ref()
                .map_err(|e| err::vm_error("VM_TYPE_ERROR", e))?;
            let update = argument(1)?;
            let equal = identity == expected;
            if equal {
                next = Some(update);
            }
            Value::Int(i32::from(equal))
        }
        "toString" => {
            // toString is not an RMW operation. Release the volatile boundary
            // before a real, possibly reentrant/GC-triggering DEX callback.
            fence(Ordering::SeqCst);
            return Ok(Some(if let Some(object) = identity {
                vm.invoke_instance(object, "toString", "()Ljava/lang/String;", &[])?
            } else {
                Value::Obj(crate::intrinsics::alloc_string(vm, "null".into())?)
            }));
        }
        _ => unreachable!("descriptor checked above"),
    };
    if let Some(value) = next {
        err::put_field_result(vm.heap.put_field(receiver, SLOT, value))?;
    }
    fence(Ordering::SeqCst);
    Ok(Some(out))
}
