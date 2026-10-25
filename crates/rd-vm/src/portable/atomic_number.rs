//! API26 scalar AtomicInteger/AtomicLong contracts, using GC-owned instance slots.
//! Exclusive Engine dispatch and the JNI VM gate serialize each intrinsic.
//! Like AtomicBoolean, volatile fences cover the native boundary; no guest
//! callback or yield occurs inside a read/modify/write. Functional update
//! callbacks and Java9 access modes are intentionally not advertised here.
use crate::{err, Engine, ObjRef, Value, VmExit};
use alloc::string::ToString;
use core::sync::atomic::{fence, Ordering};

pub(crate) const INTEGER: &str = "Ljava/util/concurrent/atomic/AtomicInteger;";
pub(crate) const LONG: &str = "Ljava/util/concurrent/atomic/AtomicLong;";
pub(crate) fn supports(class: &str) -> bool {
    matches!(class, INTEGER | LONG)
}
fn slot(class: &str) -> &'static str {
    if class == INTEGER {
        "java.util.concurrent.atomic.AtomicInteger#value"
    } else {
        "java.util.concurrent.atomic.AtomicLong#value"
    }
}
fn value(class: &str, n: i64) -> Value {
    if class == INTEGER {
        Value::Int(n as i32)
    } else {
        Value::Long(n)
    }
}
fn number(class: &str, value: &Value) -> Result<i64, VmExit> {
    (if class == INTEGER {
        value.as_int().map(i64::from)
    } else {
        value.as_long()
    })
    .map_err(|e| err::vm_error("VM_TYPE_ERROR", e).into())
}
pub(crate) fn attach(vm: &mut Engine, class: &str, object: ObjRef) -> Result<(), VmExit> {
    err::put_field_result(vm.heap.put_field(object, slot(class), value(class, 0)))?;
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
    // This is on every intrinsic dispatch path; unrelated framework calls must
    // not allocate a copied class name or traverse both atomic hierarchies.
    if !matches!(
        name,
        "<init>"
            | "get"
            | "set"
            | "lazySet"
            | "getAndSet"
            | "compareAndSet"
            | "weakCompareAndSet"
            | "getAndIncrement"
            | "getAndDecrement"
            | "incrementAndGet"
            | "decrementAndGet"
            | "getAndAdd"
            | "addAndGet"
            | "intValue"
            | "longValue"
            | "floatValue"
            | "doubleValue"
            | "byteValue"
            | "shortValue"
            | "toString"
    ) {
        return Ok(None);
    }
    let runtime = vm.heap.class_of(receiver)?;
    let class = if vm.cp.is_subtype(runtime, INTEGER) {
        INTEGER
    } else if vm.cp.is_subtype(runtime, LONG) {
        LONG
    } else {
        return Ok(None);
    };
    let (get, set, cas, update) = if class == INTEGER {
        ("()I", "(I)V", "(II)Z", "(I)I")
    } else {
        ("()J", "(J)V", "(JJ)Z", "(J)J")
    };
    let supported = match name {
        "<init>" => sig == "()V" || sig == set,
        "get" | "getAndIncrement" | "getAndDecrement" | "incrementAndGet" | "decrementAndGet" => {
            sig == get
        }
        "set" | "lazySet" => sig == set,
        "compareAndSet" | "weakCompareAndSet" => sig == cas,
        "getAndSet" | "getAndAdd" | "addAndGet" => sig == update,
        "intValue" => sig == "()I",
        "longValue" => sig == "()J",
        "floatValue" => sig == "()F",
        "doubleValue" => sig == "()D",
        "byteValue" => sig == "()B",
        "shortValue" => sig == "()S",
        "toString" => sig == "()Ljava/lang/String;",
        _ => false,
    };
    if !supported
        || (name == "<init>" && declaring != class)
        || (declaring != class
            && !vm.cp.is_subtype(declaring, class)
            && !(declaring == "Ljava/lang/Number;"
                && matches!(
                    name,
                    "intValue"
                        | "longValue"
                        | "floatValue"
                        | "doubleValue"
                        | "byteValue"
                        | "shortValue"
                        | "toString"
                ))
            && !(declaring == "Ljava/lang/Object;" && name == "toString"))
    {
        return Ok(None);
    }
    if name != "<init>"
        && receiver_class != class
        && vm.cp.resolve_method(runtime, name, sig).is_some()
    {
        return Ok(None); // Preserve real DEX overrides and invoke-super dispatch.
    }
    let argument = |i: usize| -> Result<i64, VmExit> {
        number(
            class,
            args.get(i)
                .ok_or_else(|| err::vm_error("VM_TYPE_ERROR", "missing atomic argument"))?,
        )
    };
    fence(Ordering::SeqCst);
    let current = number(class, &vm.heap.get_field(receiver, slot(class))?)?;
    let mut next = None;
    let out = match name {
        "<init>" if sig == "()V" => {
            next = Some(0);
            Value::Null
        }
        "<init>" | "set" | "lazySet" => {
            next = Some(argument(0)?);
            Value::Null
        }
        "get" => value(class, current),
        "getAndSet" => {
            next = Some(argument(0)?);
            value(class, current)
        }
        "compareAndSet" | "weakCompareAndSet" => {
            let matched = current == argument(0)?;
            let update = argument(1)?;
            if matched {
                next = Some(update);
            }
            Value::Int(matched as i32)
        }
        "getAndIncrement" | "getAndDecrement" | "incrementAndGet" | "decrementAndGet"
        | "getAndAdd" | "addAndGet" => {
            let delta = match name {
                "getAndIncrement" | "incrementAndGet" => 1,
                "getAndDecrement" | "decrementAndGet" => -1,
                _ => argument(0)?,
            };
            let n = if class == INTEGER {
                (current as i32).wrapping_add(delta as i32) as i64
            } else {
                current.wrapping_add(delta)
            };
            next = Some(n);
            value(
                class,
                if name.starts_with("getAnd") {
                    current
                } else {
                    n
                },
            )
        }
        "intValue" => Value::Int(current as i32),
        "longValue" => Value::Long(current),
        "floatValue" => Value::Float(current as f32),
        "doubleValue" => Value::Double(current as f64),
        "byteValue" | "shortValue" => {
            // Number's inherited conversions call the virtual intValue method.
            let n = vm
                .invoke_instance(receiver, "intValue", "()I", &[])?
                .as_int()?;
            Value::Int(if name == "byteValue" {
                n as i8 as i32
            } else {
                n as i16 as i32
            })
        }
        "toString" => Value::Obj(crate::intrinsics::alloc_string(vm, current.to_string())?),
        _ => unreachable!("descriptor checked above"),
    };
    if let Some(n) = next {
        err::put_field_result(vm.heap.put_field(receiver, slot(class), value(class, n)))?;
    }
    fence(Ordering::SeqCst);
    Ok(Some(out))
}
