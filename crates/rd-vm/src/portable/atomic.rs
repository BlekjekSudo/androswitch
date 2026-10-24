//! API 26 AtomicBoolean backed by a budgeted, GC-owned instance slot.
//!
//! An intrinsic runs under exclusive `&mut Engine` with no call-out or yield.
//! Horizon JNI callbacks also acquire the VM gate before entering the engine;
//! read/modify/write cannot interleave with another VM operation. SeqCst fences
//! retain volatile ordering at this boundary. This is not an implementation of
//! Java Thread or a claim that the engine supports concurrent mutable access.
//! Contract: docs.oracle.com/javase/8/docs/api/java/util/concurrent/atomic/AtomicBoolean.html
use crate::{err, Engine, ObjRef, Value, VmExit};
use alloc::string::ToString;
use core::sync::atomic::{fence, Ordering};

pub(crate) const CLASS: &str = "Ljava/util/concurrent/atomic/AtomicBoolean;";
const SLOT: &str = "java.util.concurrent.atomic.AtomicBoolean#value";

pub(crate) fn attach(vm: &mut Engine, object: ObjRef) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(object, SLOT, Value::Int(0)))?;
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
        ("<init>", "()V" | "(Z)V")
            | ("get", "()Z")
            | ("set" | "lazySet", "(Z)V")
            | ("getAndSet", "(Z)Z")
            | ("compareAndSet" | "weakCompareAndSet", "(ZZ)Z")
            | ("toString", "()Ljava/lang/String;")
    ) {
        return Ok(None);
    }
    let runtime_class = vm.heap.class_of(receiver)?.to_string();
    if !vm.cp.is_subtype(&runtime_class, CLASS) {
        return if declaring == CLASS {
            Err(err::vm_error("VM_TYPE_ERROR", "AtomicBoolean receiver type").into())
        } else {
            Ok(None)
        };
    }
    if (name == "<init>" && declaring != CLASS)
        || (declaring != CLASS
            && !vm.cp.is_subtype(declaring, CLASS)
            && !(declaring == "Ljava/lang/Object;" && name == "toString"))
    {
        return Ok(None);
    }
    if name != "<init>"
        && receiver_class != CLASS
        && vm.cp.resolve_method(&runtime_class, name, sig).is_some()
    {
        // A real DEX toString override retains normal virtual dispatch.
        return Ok(None);
    }
    let argument = |i: usize| -> Result<bool, VmExit> {
        args.get(i)
            .ok_or_else(|| err::vm_error("VM_TYPE_ERROR", "missing boolean argument"))?
            .as_int()
            .map(|v| v != 0)
            .map_err(|e| err::vm_error("VM_TYPE_ERROR", e).into())
    };
    fence(Ordering::SeqCst);
    let current = match vm.heap.get_field(receiver, SLOT)? {
        Value::Int(0) => false,
        Value::Int(1) => true,
        _ => return Err(err::vm_error("VM_TYPE_ERROR", "invalid AtomicBoolean state").into()),
    };
    let mut next = None;
    let value = match (name, sig) {
        ("<init>", "()V") => {
            next = Some(false);
            Value::Null
        }
        ("<init>" | "set" | "lazySet", "(Z)V") => {
            next = Some(argument(0)?);
            Value::Null
        }
        ("get", "()Z") => Value::Int(current as i32),
        ("getAndSet", "(Z)Z") => {
            next = Some(argument(0)?);
            Value::Int(current as i32)
        }
        ("compareAndSet" | "weakCompareAndSet", "(ZZ)Z") => {
            let matches = current == argument(0)?;
            let update = argument(1)?;
            if matches {
                next = Some(update);
            }
            // A weak CAS is permitted to succeed without spurious failure.
            Value::Int(matches as i32)
        }
        ("toString", "()Ljava/lang/String;") => Value::Obj(crate::intrinsics::alloc_string(
            vm,
            if current { "true" } else { "false" }.to_string(),
        )?),
        _ => unreachable!("supported descriptors checked above"),
    };
    if let Some(next) = next {
        crate::err::put_field_result(vm.heap.put_field(receiver, SLOT, Value::Int(next as i32)))?;
    }
    fence(Ordering::SeqCst);
    Ok(Some(value))
}
