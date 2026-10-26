//! Host iterator navigation and backing-collection removal.
//! Snapshot traversal remains deterministic; removals are written through to
//! the source, with Java's one-remove-per-next/previous state machine.
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::{Engine, Value};

fn exception(class: &str, message: &str) -> VmExit {
    VmExit::Exception(Throwable::new(class, message))
}

pub(crate) fn bind(vm: &mut Engine, it: ObjRef, owner: ObjRef) -> Result<(), VmExit> {
    if matches!(
        vm.heap.class_of(owner)?,
        "Ljava/util/concurrent/CopyOnWriteArrayList;"
            | "Ljava/util/concurrent/CopyOnWriteArraySet;"
            | "Ljava/util/Collections$SingletonList;"
            | "Ljava/util/Collections$UnmodifiableSet;"
    ) {
        return Ok(());
    }
    crate::err::put_field_result(vm.heap.put_field(it, "rd$owner", Value::Obj(owner)))?;
    crate::err::put_field_result(vm.heap.put_field(it, "rd$last", Value::Int(-1)))?;
    Ok(())
}

pub(crate) fn navigate(vm: &mut Engine, it: ObjRef, name: &str) -> Result<Option<Value>, VmExit> {
    if name == "remove" {
        return remove(vm, it).map(Some);
    }
    let Some(HostObj::Iter { elems, pos }) = vm.fw.objects.get_mut(&it) else {
        return Ok(None);
    };
    let mut last = None;
    let out = match name {
        "hasNext" => Value::Int((*pos < elems.len()) as i32),
        "hasPrevious" => Value::Int((*pos > 0) as i32),
        "nextIndex" => Value::Int(*pos as i32),
        "previousIndex" => Value::Int(*pos as i32 - 1),
        "next" => {
            if *pos >= elems.len() {
                return Err(exception(
                    "Ljava/util/NoSuchElementException;",
                    "Iterator.next",
                ));
            }
            last = Some(*pos);
            *pos += 1;
            elems[*pos - 1].clone()
        }
        "previous" => {
            if *pos == 0 {
                return Err(exception(
                    "Ljava/util/NoSuchElementException;",
                    "Iterator.previous",
                ));
            }
            *pos -= 1;
            last = Some(*pos);
            elems[*pos].clone()
        }
        "add" | "set" => {
            return Err(exception(
                "Ljava/lang/UnsupportedOperationException;",
                "ListIterator.add/set on a snapshot",
            ))
        }
        _ => return Ok(None),
    };
    if let Some(i) = last {
        crate::err::put_field_result(vm.heap.put_field(it, "rd$last", Value::Int(i as i32)))?;
    }
    Ok(Some(out))
}

fn remove(vm: &mut Engine, it: ObjRef) -> Result<Value, VmExit> {
    let owner = match vm.heap.get_field(it, "rd$owner").unwrap_or(Value::Null) {
        Value::Obj(r) => r,
        _ => {
            return Err(exception(
                "Ljava/lang/UnsupportedOperationException;",
                "immutable iterator",
            ))
        }
    };
    let last = match vm.heap.get_field(it, "rd$last").unwrap_or(Value::Int(-1)) {
        Value::Int(i) if i >= 0 => i as usize,
        _ => {
            return Err(exception(
                "Ljava/lang/IllegalStateException;",
                "remove requires next/previous",
            ))
        }
    };
    let snapshot = match vm.fw.objects.get(&it) {
        Some(HostObj::Iter { elems, .. }) => elems.clone(),
        _ => return Ok(Value::Null),
    };
    let changed = || {
        exception(
            "Ljava/util/ConcurrentModificationException;",
            "iterator source changed",
        )
    };
    // Validate before touching the backing map, so stale iterators cannot
    // partially remove an entry and then report ConcurrentModificationException.
    if let Some(HostObj::List { elems }) = vm.fw.objects.get(&owner) {
        if *elems != snapshot || last >= elems.len() {
            return Err(changed());
        }
    }
    if let Value::Obj(map) = vm.heap.get_field(owner, "rd$map").unwrap_or(Value::Null) {
        let key = vm
            .heap
            .get_field(owner, &format!("rd$key{last}"))
            .unwrap_or(Value::Null);
        let entries = match vm.fw.objects.get(&map) {
            Some(HostObj::Map { entries }) => entries.clone(),
            _ => return Err(changed()),
        };
        let idx = entries
            .iter()
            .position(|(k, _)| *k == key)
            .ok_or_else(changed)?;
        if let Some(HostObj::Map { entries }) = vm.fw.objects.get_mut(&map) {
            entries.remove(idx);
        }
        // Keep the view's parallel key list aligned after removal.
        for i in last..snapshot.len().saturating_sub(1) {
            let key = vm
                .heap
                .get_field(owner, &format!("rd$key{}", i + 1))
                .unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(owner, &format!("rd$key{i}"), key))?;
        }
        crate::err::put_field_result(vm.heap.put_field(
            owner,
            &format!("rd$key{}", snapshot.len() - 1),
            Value::Null,
        ))?;
    }
    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&owner) {
        elems.remove(last);
        crate::jcollections::sublist_write_back(vm, owner);
    } else {
        // AbstractList template iterator over a DEX subclass.
        vm.invoke_instance(
            owner,
            "remove",
            "(I)Ljava/lang/Object;",
            &[Value::Int(last as i32)],
        )?;
    }
    if let Some(HostObj::Iter { elems, pos }) = vm.fw.objects.get_mut(&it) {
        elems.remove(last);
        if *pos > last {
            *pos -= 1;
        }
    }
    crate::err::put_field_result(vm.heap.put_field(it, "rd$last", Value::Int(-1)))?;
    Ok(Value::Null)
}
