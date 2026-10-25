//! Basic ConcurrentHashMap contracts. The VM/JNI gate serializes each commit;
//! guest equals callbacks run before version validation, with the old entries
//! rooted on the receiver. Reentrant mutations restart lookup, never overwrite
//! newer data. This is a bounded linear map, not a lock-free implementation.
use super::*;
use core::sync::atomic::{fence, Ordering};
const CLASS: &str = "Ljava/util/concurrent/ConcurrentHashMap;";
const ENTRIES: &str = "java.util.concurrent.ConcurrentHashMap#entries";
const VERSION: &str = "java.util.concurrent.ConcurrentHashMap#version";
const ACTIVE: &str = "java.util.concurrent.ConcurrentHashMap#operation";
const LIMIT: usize = 65536;
pub(super) fn attach(vm: &mut Engine, class: &str, object: ObjRef) -> Result<bool> {
    if !vm.cp.is_subtype(class, CLASS) {
        return Ok(false);
    }
    write(vm, object, ENTRIES, Value::Null)?;
    write(vm, object, ACTIVE, Value::Null)?;
    write(vm, object, VERSION, Value::Long(0))?;
    Ok(true)
}
pub(super) fn metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<rd_vm::framework_host::HostMethodKind> {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance};
    if class != CLASS {
        return None;
    }
    match (name, sig) {
        ("<init>", "()V" | "(I)V" | "(IF)V" | "(IFI)V") => Some(Constructor),
        ("get" | "remove", "(Ljava/lang/Object;)Ljava/lang/Object;")
        | (
            "put" | "putIfAbsent" | "replace",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        )
        | ("remove", "(Ljava/lang/Object;Ljava/lang/Object;)Z")
        | ("replace", "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z")
        | ("containsKey" | "containsValue" | "contains", "(Ljava/lang/Object;)Z")
        | ("size", "()I")
        | ("isEmpty", "()Z")
        | ("clear", "()V") => Some(Instance),
        _ => None,
    }
}
fn reference(value: &Value) -> Result<()> {
    if value.as_ref()?.is_none() {
        return Err(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "ConcurrentHashMap null key/value",
        )
        .into());
    }
    Ok(())
}
fn version(vm: &Engine, object: ObjRef) -> Result<i64> {
    vm.heap
        .get_field(object, VERSION)?
        .as_long()
        .map_err(|e| error("VM_TYPE_ERROR", e))
}
fn commit(vm: &mut Engine, object: ObjRef, entries: Vec<Value>) -> Result<()> {
    if entries.len() / 2 > LIMIT {
        return Err(error(
            "COLLECTION_BUDGET",
            "ConcurrentHashMap 65536 entries",
        ));
    }
    let array = vm.heap.alloc_array(
        rd_vm::heap::ElemKind::Obj,
        entries.len(),
        "Ljava/lang/Object;",
    )?;
    *vm.heap.array_elems_mut(array)? = entries;
    let next = version(vm, object)?.wrapping_add(1);
    // Slots already exist, and no guest call/GC/yield occurs between writes.
    fence(Ordering::SeqCst);
    write(vm, object, ENTRIES, Value::Obj(array))?;
    write(vm, object, VERSION, Value::Long(next))?;
    fence(Ordering::SeqCst);
    Ok(())
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
    if !vm.cp.is_subtype(CLASS, class) && !vm.cp.is_subtype(class, CLASS) {
        return Ok(None);
    }
    if name == "<init>"
        && metadata(class, name, sig) == Some(rd_vm::framework_host::HostMethodKind::Constructor)
    {
        let bad_load = if args.len() >= 2 {
            let load = args[1].as_float()?;
            load.is_nan() || load <= 0.0
        } else {
            false
        };
        if (!args.is_empty() && int(args, 0)? < 0)
            || bad_load
            || (args.len() >= 3 && int(args, 2)? <= 0)
        {
            return Err(Throwable::new(
                "Ljava/lang/IllegalArgumentException;",
                "ConcurrentHashMap constructor arguments",
            )
            .into());
        }
        commit(vm, object, Vec::new())?;
        return Ok(Some(Value::Null));
    }
    if metadata(CLASS, name, sig) != Some(rd_vm::framework_host::HostMethodKind::Instance) {
        return Ok(None);
    }
    for (index, value) in args.iter().enumerate() {
        if name == "remove" && index == 1 && matches!(value, Value::Null) {
            return Ok(Some(Value::Int(0)));
        }
        reference(value)?;
    }
    let current = vm
        .heap
        .get_field(object, ENTRIES)?
        .as_ref()?
        .ok_or_else(|| error("API_UNAVAILABLE", "uninitialized ConcurrentHashMap"))?;
    let previous = vm.heap.get_field(object, ACTIVE)?;
    let guard = vm.heap.alloc_instance(
        "Ljava/lang/Object;".into(),
        vec![
            ("snapshot".into(), Value::Obj(current)),
            ("previous".into(), previous.clone()),
        ],
    )?;
    write(vm, object, ACTIVE, Value::Obj(guard))?;
    let result = (|| -> Result<Value> {
        for _ in 0..128 {
            fence(Ordering::SeqCst);
            let before = version(vm, object)?;
            let array = obj(&vm.heap.get_field(object, ENTRIES)?)?;
            // Root every retried snapshot before callbacks can clear the map.
            write(vm, guard, "snapshot", Value::Obj(array))?;
            let mut entries = vm.heap.as_array_elems(array)?.0.to_vec();
            let count = entries.len() / 2;
            if name == "size" {
                return Ok(Value::Int(count as i32));
            }
            if name == "isEmpty" {
                return Ok(Value::Int(i32::from(count == 0)));
            }
            if name == "clear" {
                commit(vm, object, Vec::new())?;
                return Ok(Value::Null);
            }
            let value_search = matches!(name, "containsValue" | "contains");
            let mut found = None;
            for (index, pair) in entries.chunks_exact(2).enumerate() {
                if collections::equals(vm, &args[0], &pair[usize::from(value_search)])? {
                    found = Some(index * 2);
                    break;
                }
                if version(vm, object)? != before {
                    break;
                }
            }
            let conditional = sig.ends_with(")Z") && matches!(name, "remove" | "replace");
            let equal = if conditional {
                match found {
                    Some(i) => collections::equals(vm, &args[1], &entries[i + 1])?,
                    None => false,
                }
            } else {
                true
            };
            // Native-backed equals can release the JNI lease; revalidate after
            // every callback and commit without any further guest call-out.
            if version(vm, object)? != before {
                continue;
            }
            let old = found.map(|i| entries[i + 1].clone()).unwrap_or(Value::Null);
            let out = match name {
                "get" => old,
                "containsKey" | "containsValue" | "contains" => {
                    Value::Int(i32::from(found.is_some()))
                }
                "put" | "putIfAbsent" => {
                    if name == "put" || found.is_none() {
                        if let Some(i) = found {
                            entries[i + 1] = args[1].clone();
                        } else {
                            entries.extend_from_slice(&args[..2]);
                        }
                        commit(vm, object, entries)?;
                    }
                    old
                }
                "remove" => {
                    if equal {
                        if let Some(i) = found {
                            drop(entries.drain(i..i + 2));
                            commit(vm, object, entries)?;
                        }
                    }
                    if conditional {
                        Value::Int(i32::from(equal))
                    } else {
                        old
                    }
                }
                "replace" => {
                    if equal {
                        if let Some(i) = found {
                            entries[i + 1] = args[if conditional { 2 } else { 1 }].clone();
                            commit(vm, object, entries)?;
                        }
                    }
                    if conditional {
                        Value::Int(i32::from(equal))
                    } else {
                        old
                    }
                }
                _ => return Err(error("API_UNAVAILABLE", "ConcurrentHashMap method")),
            };
            return Ok(out);
        }
        Err(error(
            "COLLECTION_REENTRANCY_BUDGET",
            "128 ConcurrentHashMap callback mutation retries",
        ))
    })();
    // Preserve the previous operation's root chain on every error/exception.
    write(vm, object, ACTIVE, previous)?;
    result.map(Some)
}
