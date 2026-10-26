//! `android.util` value containers: the sparse-array family (`SparseArray`,
//! `SparseIntArray`, `SparseBooleanArray`, `SparseLongArray`,
//! `LongSparseArray`), plus `Size`, `SizeF`, `Pair`, `Range` and `Rational`.
//!
//! Sparse arrays keep a sorted key → value map in `Engine::sparse`, so
//! `keyAt`/`valueAt`/`indexOfKey` follow AOSP's ascending-key order.
//! `get(k)` on a missing key returns null/0/false, or the supplied default.
//! The value structs use heap fields (`Pair.first`/`second` are public in
//! AOSP).

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;
use alloc::collections::BTreeMap;

const SPARSE: &[&str] = &[
    "Landroid/util/SparseArray;",
    "Landroid/util/SparseIntArray;",
    "Landroid/util/SparseBooleanArray;",
    "Landroid/util/SparseLongArray;",
    "Landroid/util/LongSparseArray;",
];
const ARRAY_MAP: &str = "Landroid/util/ArrayMap;";
const SIMPLE_ARRAY_MAP: &str = "Landroid/util/SimpleArrayMap;";
const ARRAY_SET: &str = "Landroid/util/ArraySet;";

fn key(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Int(i)) => *i as i64,
        Some(Value::Long(l)) => *l,
        _ => 0,
    }
}
fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
fn get(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}
fn oob(i: i64) -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
        format!("index={i}"),
    ))
}

fn default_value(class: &str) -> Value {
    match class {
        "Landroid/util/SparseIntArray;" | "Landroid/util/SparseBooleanArray;" => Value::Int(0),
        "Landroid/util/SparseLongArray;" => Value::Long(0),
        _ => Value::Null,
    }
}

fn sparse(
    vm: &mut Engine,
    class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let m = vm.sparse.entry(recv).or_default();
    let nth = |m: &BTreeMap<i64, Value>, i: i64| -> Result<(i64, Value), VmExit> {
        m.iter()
            .nth(i.max(0) as usize)
            .filter(|_| i >= 0)
            .map(|(k, v)| (*k, v.clone()))
            .ok_or_else(|| oob(i))
    };
    Ok(Some(match name {
        "<init>" => {
            m.clear();
            Value::Null
        }
        "put" | "append" => {
            m.insert(key(a(0)), a(1).cloned().unwrap_or(Value::Null));
            Value::Null
        }
        "get" => match m.get(&key(a(0))) {
            Some(v) => v.clone(),
            None => a(1).cloned().unwrap_or_else(|| default_value(class)),
        },
        "contains" => Value::Int(m.contains_key(&key(a(0))) as i32),
        "delete" | "remove" => {
            m.remove(&key(a(0)));
            Value::Null
        }
        "removeAt" => {
            let (k, _) = nth(m, key(a(0)))?;
            m.remove(&k);
            Value::Null
        }
        "removeAtRange" => {
            let (i, n) = (key(a(0)), key(a(1)));
            let ks: Vec<i64> = m
                .keys()
                .skip(i.max(0) as usize)
                .take(n.max(0) as usize)
                .copied()
                .collect();
            for k in ks {
                m.remove(&k);
            }
            Value::Null
        }
        "size" => Value::Int(m.len() as i32),
        "isEmpty" => Value::Int(m.is_empty() as i32),
        "clear" => {
            m.clear();
            Value::Null
        }
        "keyAt" => {
            let (k, _) = nth(m, key(a(0)))?;
            if class == "Landroid/util/LongSparseArray;" {
                Value::Long(k)
            } else {
                Value::Int(k as i32)
            }
        }
        "valueAt" => nth(m, key(a(0)))?.1,
        "setValueAt" => {
            let (k, _) = nth(m, key(a(0)))?;
            m.insert(k, a(1).cloned().unwrap_or(Value::Null));
            Value::Null
        }
        "indexOfKey" => Value::Int(
            m.keys()
                .position(|k| *k == key(a(0)))
                .map(|i| i as i32)
                .unwrap_or(-1),
        ),
        "indexOfValue" => {
            let v = a(0).cloned().unwrap_or(Value::Null);
            Value::Int(
                m.values()
                    .position(|x| *x == v)
                    .map(|i| i as i32)
                    .unwrap_or(-1),
            )
        }
        "clone" => {
            let copy = m.clone();
            let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
            vm.sparse.insert(r, copy);
            Value::Obj(r)
        }
        "putAll" => {
            if let Some(Value::Obj(o)) = a(0) {
                let other = vm.sparse.get(o).cloned().unwrap_or_default();
                vm.sparse.entry(recv).or_default().extend(other);
            }
            Value::Null
        }
        "toString" => {
            let s = format!(
                "{{{}}}",
                m.iter()
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
}

fn array_throw(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}

fn array_hash(vm: &mut Engine, value: &Value) -> Result<i32, VmExit> {
    Ok(match value {
        Value::Null => 0,
        Value::Obj(_) => crate::arrays::obj_hash(vm, value)?,
        Value::Int(x) => *x,
        Value::Long(x) => (*x ^ ((*x as u64) >> 32) as i64) as i32,
        Value::Float(x) => {
            if x.is_nan() {
                0x7fc0_0000
            } else {
                x.to_bits() as i32
            }
        }
        Value::Double(x) => {
            let bits = if x.is_nan() {
                0x7ff8_0000_0000_0000u64
            } else {
                x.to_bits()
            };
            ((bits ^ (bits >> 32)) & 0xffff_ffff) as u32 as i32
        }
        _ => 0,
    })
}

fn array_equal(vm: &mut Engine, needle: &Value, candidate: &Value) -> Result<bool, VmExit> {
    if crate::intrinsics::values_equal(&vm.heap, needle, candidate)? {
        return Ok(true);
    }
    let Value::Obj(recv) = needle else {
        return Ok(false);
    };
    if matches!(candidate, Value::Null) {
        return Ok(false);
    }
    match crate::jcollections::guest_call(
        vm,
        *recv,
        "equals",
        "(Ljava/lang/Object;)Z",
        core::slice::from_ref(candidate),
    )? {
        Some(value) => Ok(value.as_int().unwrap_or(0) != 0),
        None => Ok(false),
    }
}

/// Locate `needle` in a hash-ordered key slice. `Ok(index)` is an equal
/// key; `Err(index)` is the AOSP insertion point: after every existing key
/// with the same signed hash, before the first greater hash.
fn array_find_in(
    vm: &mut Engine,
    keys: &[Value],
    needle: &Value,
) -> Result<Result<usize, usize>, VmExit> {
    let target = array_hash(vm, needle)?;
    let mut hashes = Vec::with_capacity(keys.len());
    for key in keys {
        hashes.push(array_hash(vm, key)?);
    }
    let insert = hashes
        .iter()
        .position(|hash| *hash > target)
        .unwrap_or(hashes.len());
    for (index, hash) in hashes.iter().enumerate() {
        if *hash == target && array_equal(vm, needle, &keys[index])? {
            return Ok(Ok(index));
        }
    }
    Ok(Err(insert))
}

fn array_contains_linear(
    vm: &mut Engine,
    values: &[Value],
    needle: &Value,
) -> Result<bool, VmExit> {
    for value in values {
        if array_equal(vm, needle, value)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn array_map_entries(vm: &Engine, recv: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.objects.get(&recv) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    }
}

fn array_map_store(vm: &mut Engine, recv: ObjRef, entries: Vec<(Value, Value)>) {
    vm.fw.objects.insert(recv, HostObj::Map { entries });
}

fn array_map_put_into(
    vm: &mut Engine,
    entries: &mut Vec<(Value, Value)>,
    key: Value,
    value: Value,
) -> Result<Value, VmExit> {
    let keys: Vec<Value> = entries.iter().map(|(k, _)| k.clone()).collect();
    match array_find_in(vm, &keys, &key)? {
        Ok(index) => Ok(core::mem::replace(&mut entries[index].1, value)),
        Err(index) => {
            entries.insert(index, (key, value));
            Ok(Value::Null)
        }
    }
}

fn array_map_source_entries(
    vm: &mut Engine,
    value: Option<&Value>,
) -> Result<Vec<(Value, Value)>, VmExit> {
    let Some(Value::Obj(source)) = value else {
        return Err(array_throw(
            "Ljava/lang/NullPointerException;",
            "map == null",
        ));
    };
    let source = *source;
    if let Some(HostObj::Map { entries }) = vm.fw.objects.get(&source) {
        return Ok(entries.clone());
    }
    let entry_set = vm.invoke_instance(source, "entrySet", "()Ljava/util/Set;", &[])?;
    let entries = crate::jcollections::elems_of(vm, &entry_set)?;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let Value::Obj(entry) = entry else {
            continue;
        };
        let key = vm.invoke_instance(entry, "getKey", "()Ljava/lang/Object;", &[])?;
        let value = vm.invoke_instance(entry, "getValue", "()Ljava/lang/Object;", &[])?;
        out.push((key, value));
    }
    Ok(out)
}

fn array_value_string(
    vm: &mut Engine,
    this: ObjRef,
    this_label: &str,
    value: &Value,
) -> Result<String, VmExit> {
    Ok(match value {
        Value::Null => "null".to_string(),
        Value::Obj(obj) if *obj == this => this_label.to_string(),
        Value::Obj(obj) => crate::intrinsics::object_to_string(vm, *obj)?,
        Value::Int(x) => x.to_string(),
        Value::Long(x) => x.to_string(),
        Value::Float(x) => crate::repr::java_float(*x),
        Value::Double(x) => crate::repr::java_double(*x),
        other => format!("{other:?}"),
    })
}

fn array_map_view(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    entries: &[(Value, Value)],
) -> Result<Value, VmExit> {
    let mut elems = Vec::with_capacity(entries.len());
    let mut keys = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        keys.push(key.clone());
        elems.push(match name {
            "keySet" => key.clone(),
            "values" => value.clone(),
            _ => {
                let entry = vm
                    .heap
                    .alloc_instance("Ljava/util/Map$Entry;".to_string(), Vec::new())
                    .map_err(crate::err::oom_throwable)?;
                put(vm, entry, "entry_key", key.clone())?;
                put(vm, entry, "entry_value", value.clone())?;
                Value::Obj(entry)
            }
        });
    }
    let view = vm
        .heap
        .alloc_instance("Ljava/util/HashSet;".to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.objects.insert(view, HostObj::List { elems });
    put(vm, view, "rd$map", Value::Obj(recv))?;
    for (index, key) in keys.into_iter().enumerate() {
        put(vm, view, &format!("rd$key{index}"), key)?;
    }
    Ok(Value::Obj(view))
}

fn array_map(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Value::Null);
    if name == "<init>" {
        let source = if matches!(
            sig,
            "(Ljava/util/Map;)V"
                | "(Landroid/util/ArrayMap;)V"
                | "(Landroid/util/SimpleArrayMap;)V"
        ) {
            array_map_source_entries(vm, args.first())?
        } else {
            Vec::new()
        };
        let mut entries = Vec::new();
        for (key, value) in source {
            array_map_put_into(vm, &mut entries, key, value)?;
        }
        array_map_store(vm, recv, entries);
        return Ok(Some(Value::Null));
    }

    let entries_snapshot = array_map_entries(vm, recv);
    let keys_snapshot: Vec<Value> = entries_snapshot.iter().map(|(k, _)| k.clone()).collect();
    Ok(Some(match name {
        "put" | "append" => {
            let mut entries = entries_snapshot;
            let old = array_map_put_into(vm, &mut entries, arg(0), arg(1))?;
            array_map_store(vm, recv, entries);
            old
        }
        "putAll" => {
            let source = array_map_source_entries(vm, args.first())?;
            let mut entries = entries_snapshot;
            for (key, value) in source {
                array_map_put_into(vm, &mut entries, key, value)?;
            }
            array_map_store(vm, recv, entries);
            Value::Null
        }
        "get" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) => entries_snapshot[index].1.clone(),
            Err(_) => Value::Null,
        },
        "getOrDefault" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) => entries_snapshot[index].1.clone(),
            Err(_) => arg(1),
        },
        "putIfAbsent" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) if !matches!(entries_snapshot[index].1, Value::Null) => {
                entries_snapshot[index].1.clone()
            }
            Ok(index) => {
                let mut entries = entries_snapshot;
                entries[index].1 = arg(1);
                array_map_store(vm, recv, entries);
                Value::Null
            }
            Err(index) => {
                let mut entries = entries_snapshot;
                entries.insert(index, (arg(0), arg(1)));
                array_map_store(vm, recv, entries);
                Value::Null
            }
        },
        "remove" if args.len() >= 2 => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) if array_equal(vm, &entries_snapshot[index].1.clone(), &arg(1))? => {
                let mut entries = entries_snapshot;
                entries.remove(index);
                array_map_store(vm, recv, entries);
                Value::Int(1)
            }
            _ => Value::Int(0),
        },
        "remove" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) => {
                let mut entries = entries_snapshot;
                let old = entries.remove(index).1;
                array_map_store(vm, recv, entries);
                old
            }
            Err(_) => Value::Null,
        },
        "replace" if args.len() >= 3 => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) if array_equal(vm, &entries_snapshot[index].1.clone(), &arg(1))? => {
                let mut entries = entries_snapshot;
                entries[index].1 = arg(2);
                array_map_store(vm, recv, entries);
                Value::Int(1)
            }
            _ => Value::Int(0),
        },
        "replace" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) => {
                let mut entries = entries_snapshot;
                let old = core::mem::replace(&mut entries[index].1, arg(1));
                array_map_store(vm, recv, entries);
                old
            }
            Err(_) => Value::Null,
        },
        "containsKey" => Value::Int(array_find_in(vm, &keys_snapshot, &arg(0))?.is_ok() as i32),
        "containsValue" => {
            let mut found = false;
            for (_, value) in &entries_snapshot {
                if array_equal(vm, &arg(0), value)? {
                    found = true;
                    break;
                }
            }
            Value::Int(found as i32)
        }
        "indexOfKey" => match array_find_in(vm, &keys_snapshot, &arg(0))? {
            Ok(index) => Value::Int(index as i32),
            Err(index) => Value::Int(-(index as i32) - 1),
        },
        "indexOfValue" => {
            let mut found = -1i32;
            for (index, (_, value)) in entries_snapshot.iter().enumerate() {
                if array_equal(vm, &arg(0), value)? {
                    found = index as i32;
                    break;
                }
            }
            Value::Int(found)
        }
        "keyAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Some((key, _)) = usize::try_from(index)
                .ok()
                .and_then(|i| entries_snapshot.get(i))
            else {
                return Err(oob(index as i64));
            };
            key.clone()
        }
        "valueAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Some((_, value)) = usize::try_from(index)
                .ok()
                .and_then(|i| entries_snapshot.get(i))
            else {
                return Err(oob(index as i64));
            };
            value.clone()
        }
        "setValueAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Ok(index_usize) = usize::try_from(index) else {
                return Err(oob(index as i64));
            };
            if index_usize >= entries_snapshot.len() {
                return Err(oob(index as i64));
            }
            let mut entries = entries_snapshot;
            let old = core::mem::replace(&mut entries[index_usize].1, arg(1));
            array_map_store(vm, recv, entries);
            old
        }
        "removeAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Ok(index_usize) = usize::try_from(index) else {
                return Err(oob(index as i64));
            };
            if index_usize >= entries_snapshot.len() {
                return Err(oob(index as i64));
            }
            let mut entries = entries_snapshot;
            let old = entries.remove(index_usize).1;
            array_map_store(vm, recv, entries);
            old
        }
        "size" => Value::Int(entries_snapshot.len() as i32),
        "isEmpty" => Value::Int(entries_snapshot.is_empty() as i32),
        "clear" => {
            array_map_store(vm, recv, Vec::new());
            Value::Null
        }
        "ensureCapacity" => Value::Null,
        "removeAll" | "retainAll" => {
            // ArrayMap's collection bulk operations act on the keys.
            let source = crate::jcollections::elems_of(vm, &arg(0))?;
            let mut kept = Vec::with_capacity(entries_snapshot.len());
            let mut changed = false;
            for entry in entries_snapshot {
                let inside = array_contains_linear(vm, &source, &entry.0)?;
                if inside == (name == "retainAll") {
                    kept.push(entry);
                } else {
                    changed = true;
                }
            }
            array_map_store(vm, recv, kept);
            Value::Int(changed as i32)
        }
        "keySet" | "values" | "entrySet" => array_map_view(vm, recv, name, &entries_snapshot)?,
        "hashCode" => {
            let mut hash = 0i32;
            for (key, value) in &entries_snapshot {
                hash = hash.wrapping_add(array_hash(vm, key)? ^ array_hash(vm, value)?);
            }
            Value::Int(hash)
        }
        "equals" => {
            let Some(Value::Obj(other)) = args.first() else {
                return Ok(Some(Value::Int(0)));
            };
            if *other == recv {
                Value::Int(1)
            } else {
                // AOSP: equality is against any `java.util.Map`, not only
                // another ArrayMap.
                let other_class = vm.heap.class_of(*other)?.to_string();
                let host_map = matches!(vm.fw.objects.get(other), Some(HostObj::Map { .. }));
                if !host_map && !vm.cp.is_subtype(&other_class, "Ljava/util/Map;") {
                    return Ok(Some(Value::Int(0)));
                }
                let other_entries = if host_map {
                    array_map_entries(vm, *other)
                } else {
                    match array_map_source_entries(vm, args.first()) {
                        Ok(entries) => entries,
                        Err(_) => return Ok(Some(Value::Int(0))),
                    }
                };
                if other_entries.len() != entries_snapshot.len() {
                    Value::Int(0)
                } else {
                    let other_keys: Vec<Value> =
                        other_entries.iter().map(|(k, _)| k.clone()).collect();
                    let mut equal = true;
                    for (key, value) in &entries_snapshot {
                        match array_find_in(vm, &other_keys, key)? {
                            Ok(index)
                                if array_equal(vm, value, &other_entries[index].1.clone())? => {}
                            _ => {
                                equal = false;
                                break;
                            }
                        }
                    }
                    Value::Int(equal as i32)
                }
            }
        }
        "toString" => {
            let mut parts = Vec::with_capacity(entries_snapshot.len());
            for (key, value) in &entries_snapshot {
                parts.push(format!(
                    "{}={}",
                    array_value_string(vm, recv, "(this Map)", key)?,
                    array_value_string(vm, recv, "(this Map)", value)?
                ));
            }
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!("{{{}}}", parts.join(", ")),
            )?)
        }
        _ => return Ok(None),
    }))
}

fn array_set_elems(vm: &Engine, recv: ObjRef) -> Vec<Value> {
    match vm.fw.objects.get(&recv) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => Vec::new(),
    }
}

fn array_set_store(vm: &mut Engine, recv: ObjRef, elems: Vec<Value>) {
    vm.fw.objects.insert(recv, HostObj::List { elems });
}

fn array_set_add_into(
    vm: &mut Engine,
    elems: &mut Vec<Value>,
    value: Value,
) -> Result<bool, VmExit> {
    match array_find_in(vm, elems, &value)? {
        Ok(_) => Ok(false),
        Err(index) => {
            elems.insert(index, value);
            Ok(true)
        }
    }
}

fn array_set(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Value::Null);
    if name == "<init>" {
        let source = if matches!(
            sig,
            "(Ljava/util/Collection;)V" | "(Landroid/util/ArraySet;)V"
        ) {
            crate::jcollections::elems_of(vm, &arg(0))?
        } else if sig == "([Ljava/lang/Object;)V" {
            // `ArraySet(T[] array)` tolerates a null array.
            match args.first() {
                Some(Value::Obj(array)) => vm.heap.as_array_elems(*array)?.0.to_vec(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let mut elems = Vec::new();
        for value in source {
            array_set_add_into(vm, &mut elems, value)?;
        }
        array_set_store(vm, recv, elems);
        return Ok(Some(Value::Null));
    }

    let elems_snapshot = array_set_elems(vm, recv);
    Ok(Some(match name {
        "add" | "append" => {
            let mut elems = elems_snapshot;
            let changed = array_set_add_into(vm, &mut elems, arg(0))?;
            array_set_store(vm, recv, elems);
            Value::Int(changed as i32)
        }
        "addAll" => {
            let source = crate::jcollections::elems_of(vm, &arg(0))?;
            let mut elems = elems_snapshot;
            let mut changed = false;
            for value in source {
                changed |= array_set_add_into(vm, &mut elems, value)?;
            }
            array_set_store(vm, recv, elems);
            Value::Int(changed as i32)
        }
        "remove" => match array_find_in(vm, &elems_snapshot, &arg(0))? {
            Ok(index) => {
                let mut elems = elems_snapshot;
                elems.remove(index);
                array_set_store(vm, recv, elems);
                Value::Int(1)
            }
            Err(_) => Value::Int(0),
        },
        "removeAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Ok(index_usize) = usize::try_from(index) else {
                return Err(oob(index as i64));
            };
            if index_usize >= elems_snapshot.len() {
                return Err(oob(index as i64));
            }
            let mut elems = elems_snapshot;
            let old = elems.remove(index_usize);
            array_set_store(vm, recv, elems);
            old
        }
        "removeAll" | "retainAll" => {
            let source = crate::jcollections::elems_of(vm, &arg(0))?;
            let mut kept = Vec::with_capacity(elems_snapshot.len());
            let mut changed = false;
            for value in elems_snapshot {
                let inside = array_contains_linear(vm, &source, &value)?;
                if inside == (name == "retainAll") {
                    kept.push(value);
                } else {
                    changed = true;
                }
            }
            array_set_store(vm, recv, kept);
            Value::Int(changed as i32)
        }
        "contains" => Value::Int(array_find_in(vm, &elems_snapshot, &arg(0))?.is_ok() as i32),
        "containsAll" => {
            let source = crate::jcollections::elems_of(vm, &arg(0))?;
            let mut all = true;
            for value in &source {
                if array_find_in(vm, &elems_snapshot, value)?.is_err() {
                    all = false;
                    break;
                }
            }
            Value::Int(all as i32)
        }
        "indexOf" => match array_find_in(vm, &elems_snapshot, &arg(0))? {
            Ok(index) => Value::Int(index as i32),
            Err(index) => Value::Int(-(index as i32) - 1),
        },
        "valueAt" => {
            let index = arg(0).as_int().unwrap_or(0);
            let Some(value) = usize::try_from(index)
                .ok()
                .and_then(|i| elems_snapshot.get(i))
            else {
                return Err(oob(index as i64));
            };
            value.clone()
        }
        "size" => Value::Int(elems_snapshot.len() as i32),
        "isEmpty" => Value::Int(elems_snapshot.is_empty() as i32),
        "clear" => {
            array_set_store(vm, recv, Vec::new());
            Value::Null
        }
        "ensureCapacity" => Value::Null,
        "iterator" => {
            let iterator = vm
                .heap
                .alloc_instance("Ljava/util/Iterator;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            vm.fw.objects.insert(
                iterator,
                HostObj::Iter {
                    elems: elems_snapshot,
                    pos: 0,
                },
            );
            crate::jiterator::bind(vm, iterator, recv)?;
            Value::Obj(iterator)
        }
        "toArray" => crate::jcollections::to_array(
            vm,
            elems_snapshot,
            if sig == "()[Ljava/lang/Object;" {
                None
            } else {
                args.first()
            },
        )?,
        "hashCode" => {
            // AOSP ArraySet.hashCode sums the stored element hashes.
            let mut hash = 0i32;
            for value in &elems_snapshot {
                hash = hash.wrapping_add(array_hash(vm, value)?);
            }
            Value::Int(hash)
        }
        "equals" => {
            // AOSP: `other instanceof Set`, same size, every element
            // contained. A non-Set (or an element source that cannot be
            // read as one) is simply unequal.
            let Some(Value::Obj(other)) = args.first() else {
                return Ok(Some(Value::Int(0)));
            };
            if *other == recv {
                Value::Int(1)
            } else {
                let other_class = vm.heap.class_of(*other)?.to_string();
                if !vm.cp.is_subtype(&other_class, "Ljava/util/Set;") {
                    Value::Int(0)
                } else {
                    match crate::jcollections::elems_of(vm, &Value::Obj(*other)) {
                        Ok(other_elems) if other_elems.len() == elems_snapshot.len() => {
                            let mut equal = true;
                            for value in &elems_snapshot {
                                if !array_contains_linear(vm, &other_elems, value)? {
                                    equal = false;
                                    break;
                                }
                            }
                            Value::Int(equal as i32)
                        }
                        _ => Value::Int(0),
                    }
                }
            }
        }
        "toString" => {
            // AOSP ArraySet renders with braces, like a Map: `{a, b}`,
            // `{}` when empty, and `(this Set)` for a self reference.
            let mut parts = Vec::with_capacity(elems_snapshot.len());
            for value in &elems_snapshot {
                parts.push(array_value_string(vm, recv, "(this Set)", value)?);
            }
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!("{{{}}}", parts.join(", ")),
            )?)
        }
        _ => return Ok(None),
    }))
}

const BASE64: &str = "Landroid/util/Base64;";
const BASE64_NO_PADDING: i32 = 1;
const BASE64_NO_WRAP: i32 = 2;
const BASE64_CRLF: i32 = 4;
const BASE64_URL_SAFE: i32 = 8;
const BASE64_NO_CLOSE: i32 = 16;

fn base64_bad() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/IllegalArgumentException;",
        "bad base-64",
    ))
}

fn base64_alphabet(flags: i32) -> &'static [u8; 64] {
    const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const WEB_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    if flags & BASE64_URL_SAFE != 0 {
        WEB_SAFE
    } else {
        STANDARD
    }
}

fn base64_newline(flags: i32) -> &'static [u8] {
    if flags & BASE64_CRLF != 0 {
        b"\r\n"
    } else {
        b"\n"
    }
}

fn base64_push_group(output: &mut Vec<u8>, chunk: &[u8], flags: i32) {
    debug_assert!(!chunk.is_empty() && chunk.len() <= 3);
    let alphabet = base64_alphabet(flags);
    let padding = flags & BASE64_NO_PADDING == 0;
    let b0 = u32::from(chunk[0]);
    let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
    let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
    let triple = (b0 << 16) | (b1 << 8) | b2;
    output.push(alphabet[(triple >> 18) as usize & 63]);
    output.push(alphabet[(triple >> 12) as usize & 63]);
    if chunk.len() > 1 {
        output.push(alphabet[(triple >> 6) as usize & 63]);
    } else if padding {
        output.push(b'=');
    }
    if chunk.len() > 2 {
        output.push(alphabet[triple as usize & 63]);
    } else if padding {
        output.push(b'=');
    }
}

fn base64_encode(input: &[u8], flags: i32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        base64_push_group(&mut raw, chunk, flags);
    }
    if flags & BASE64_NO_WRAP != 0 || raw.is_empty() {
        return raw;
    }
    let newline = base64_newline(flags);
    let mut wrapped = Vec::with_capacity(raw.len() + (raw.len() / 76 + 1) * newline.len());
    for line in raw.chunks(76) {
        wrapped.extend_from_slice(line);
        wrapped.extend_from_slice(newline);
    }
    wrapped
}

fn base64_decode(input: &[u8], flags: i32) -> Result<Vec<u8>, VmExit> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Sextets(usize),
        ExpectSecondPadding,
        Done,
    }
    let web_safe = flags & BASE64_URL_SAFE != 0;
    let mut state = State::Sextets(0);
    let mut accumulator = 0u32;
    let mut output = Vec::with_capacity(input.len() * 3 / 4 + 3);
    for &byte in input {
        if byte == b'=' {
            match state {
                State::Sextets(2) => state = State::ExpectSecondPadding,
                State::Sextets(3) => {
                    output.push((accumulator >> 10) as u8);
                    output.push((accumulator >> 2) as u8);
                    state = State::Done;
                }
                State::ExpectSecondPadding => {
                    output.push((accumulator >> 4) as u8);
                    state = State::Done;
                }
                _ => return Err(base64_bad()),
            }
            continue;
        }
        let sextet = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'+' if !web_safe => 62,
            b'/' if !web_safe => 63,
            b'-' if web_safe => 62,
            b'_' if web_safe => 63,
            // Android's decoder skips whitespace and every other byte that
            // is not in the selected alphabet.
            _ => continue,
        };
        let State::Sextets(count) = state else {
            return Err(base64_bad());
        };
        accumulator = (accumulator << 6) | sextet;
        if count == 3 {
            output.push((accumulator >> 16) as u8);
            output.push((accumulator >> 8) as u8);
            output.push(accumulator as u8);
            accumulator = 0;
            state = State::Sextets(0);
        } else {
            state = State::Sextets(count + 1);
        }
    }
    match state {
        State::Sextets(0) | State::Done => Ok(output),
        State::Sextets(2) => {
            output.push((accumulator >> 4) as u8);
            Ok(output)
        }
        State::Sextets(3) => {
            output.push((accumulator >> 10) as u8);
            output.push((accumulator >> 2) as u8);
            Ok(output)
        }
        _ => Err(base64_bad()),
    }
}

fn base64_flags(args: &[Value]) -> i32 {
    args.last().and_then(|v| v.as_int().ok()).unwrap_or(0)
}

fn base64_input_slice(vm: &Engine, args: &[Value]) -> Result<Vec<u8>, VmExit> {
    let Some(Value::Obj(input)) = args.first() else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "input == null",
        )));
    };
    let values = vm.heap.as_array_elems(*input)?.0;
    let (offset, len) = if args.len() >= 4 {
        (args[1].as_int().unwrap_or(0), args[2].as_int().unwrap_or(0))
    } else {
        (0, values.len() as i32)
    };
    let offset = usize::try_from(offset).ok();
    let len = usize::try_from(len).ok();
    let Some((offset, len)) = offset.zip(len) else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/ArrayIndexOutOfBoundsException;",
            "negative offset or length",
        )));
    };
    let Some(end) = offset.checked_add(len).filter(|end| *end <= values.len()) else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/ArrayIndexOutOfBoundsException;",
            format!("offset={offset}, len={len}, array length={}", values.len()),
        )));
    };
    Ok(values[offset..end]
        .iter()
        .map(|value| value.as_int().unwrap_or(0) as u8)
        .collect())
}

fn base64_static(vm: &mut Engine, name: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let flags = base64_flags(args);
    Ok(Some(match name {
        "decode" => {
            let input = if args.len() < 4 {
                match args.first() {
                    Some(Value::Obj(text)) if vm.heap.as_str(*text).is_ok() => {
                        vm.heap.as_str(*text)?.as_bytes().to_vec()
                    }
                    _ => base64_input_slice(vm, args)?,
                }
            } else {
                base64_input_slice(vm, args)?
            };
            crate::jlstring::byte_array(vm, &base64_decode(&input, flags)?)?
        }
        "encode" => {
            let input = base64_input_slice(vm, args)?;
            crate::jlstring::byte_array(vm, &base64_encode(&input, flags))?
        }
        "encodeToString" => {
            let input = base64_input_slice(vm, args)?;
            // The encoder emits only the ASCII Base64 alphabet, padding and
            // line breaks; decode that byte alphabet directly instead of
            // relying on a panicking UTF-8 assertion.
            let encoded = base64_encode(&input, flags)
                .iter()
                .map(|byte| char::from(*byte))
                .collect::<String>();
            Value::Obj(crate::intrinsics::alloc_string(vm, encoded)?)
        }
        _ => return Ok(None),
    }))
}

const BASE64_INPUT_STREAM: &str = "Landroid/util/Base64InputStream;";
const BASE64_OUTPUT_STREAM: &str = "Landroid/util/Base64OutputStream;";
const B64_STREAM_FLAGS: &str = "rd_b64_flags";
const B64_STREAM_CLOSED: &str = "rd_b64_closed";
const B64_STREAM_PENDING_LEN: &str = "rd_b64_pending_len";
const B64_STREAM_PENDING_0: &str = "rd_b64_pending_0";
const B64_STREAM_PENDING_1: &str = "rd_b64_pending_1";
const B64_STREAM_COLUMN: &str = "rd_b64_column";
const B64_STREAM_DECODED: &str = "rd_b64_decoded";
const B64_STREAM_POS: &str = "rd_b64_pos";

fn b64_stream_throw(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}

fn b64_stream_int(vm: &Engine, recv: ObjRef, field: &str) -> i32 {
    match get(vm, recv, field) {
        Value::Int(value) => value,
        _ => 0,
    }
}

fn b64_stream_set_int(
    vm: &mut Engine,
    recv: ObjRef,
    field: &str,
    value: i32,
) -> Result<(), VmExit> {
    put(vm, recv, field, Value::Int(value))
}

fn b64_stream_wrapped(vm: &Engine, recv: ObjRef, field: &str) -> Result<ObjRef, VmExit> {
    match get(vm, recv, field) {
        Value::Obj(wrapped) => Ok(wrapped),
        _ => Err(b64_stream_throw(
            "Ljava/io/IOException;",
            "stream has no wrapped stream",
        )),
    }
}

fn b64_stream_array_bytes(vm: &Engine, value: Option<&Value>) -> Result<Vec<u8>, VmExit> {
    let Some(Value::Obj(array)) = value else {
        return Err(b64_stream_throw(
            "Ljava/lang/NullPointerException;",
            "buffer == null",
        ));
    };
    Ok(vm
        .heap
        .as_array_elems(*array)?
        .0
        .iter()
        .map(|value| value.as_int().unwrap_or(0) as u8)
        .collect())
}

fn b64_stream_write_data(vm: &Engine, sig: &str, args: &[Value]) -> Result<Vec<u8>, VmExit> {
    match sig {
        "(I)V" => Ok(vec![args
            .first()
            .and_then(|value| value.as_int().ok())
            .unwrap_or(0) as u8]),
        "([B)V" => b64_stream_array_bytes(vm, args.first()),
        "([BII)V" => {
            let all = b64_stream_array_bytes(vm, args.first())?;
            let offset = args
                .get(1)
                .and_then(|value| value.as_int().ok())
                .unwrap_or(0);
            let len = args
                .get(2)
                .and_then(|value| value.as_int().ok())
                .unwrap_or(0);
            let offset = usize::try_from(offset).ok();
            let len = usize::try_from(len).ok();
            let Some((offset, len)) = offset.zip(len) else {
                return Err(b64_stream_throw(
                    "Ljava/lang/IndexOutOfBoundsException;",
                    "negative offset or length",
                ));
            };
            let Some(end) = offset.checked_add(len).filter(|end| *end <= all.len()) else {
                return Err(b64_stream_throw(
                    "Ljava/lang/IndexOutOfBoundsException;",
                    format!("offset={offset}, len={len}, array length={}", all.len()),
                ));
            };
            Ok(all[offset..end].to_vec())
        }
        _ => Ok(Vec::new()),
    }
}

fn b64_stream_save_pending(
    vm: &mut Engine,
    recv: ObjRef,
    pending: &[u8],
    column: i32,
) -> Result<(), VmExit> {
    b64_stream_set_int(vm, recv, B64_STREAM_PENDING_LEN, pending.len() as i32)?;
    b64_stream_set_int(
        vm,
        recv,
        B64_STREAM_PENDING_0,
        i32::from(*pending.first().unwrap_or(&0)),
    )?;
    b64_stream_set_int(
        vm,
        recv,
        B64_STREAM_PENDING_1,
        i32::from(*pending.get(1).unwrap_or(&0)),
    )?;
    b64_stream_set_int(vm, recv, B64_STREAM_COLUMN, column)
}

fn b64_stream_pending(vm: &Engine, recv: ObjRef) -> Vec<u8> {
    match b64_stream_int(vm, recv, B64_STREAM_PENDING_LEN) {
        1 => vec![b64_stream_int(vm, recv, B64_STREAM_PENDING_0) as u8],
        2.. => vec![
            b64_stream_int(vm, recv, B64_STREAM_PENDING_0) as u8,
            b64_stream_int(vm, recv, B64_STREAM_PENDING_1) as u8,
        ],
        _ => Vec::new(),
    }
}

fn b64_stream_write_encoded(vm: &mut Engine, recv: ObjRef, encoded: &[u8]) -> Result<(), VmExit> {
    if encoded.is_empty() {
        return Ok(());
    }
    let out = b64_stream_wrapped(vm, recv, "out")?;
    let bytes = crate::jlstring::byte_array(vm, encoded)?;
    vm.invoke_instance(out, "write", "([B)V", &[bytes])?;
    Ok(())
}

fn base64_output_stream(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(Ljava/io/OutputStream;I)V") => {
            let Some(out @ Value::Obj(_)) = args.first() else {
                return Err(b64_stream_throw(
                    "Ljava/lang/NullPointerException;",
                    "out == null",
                ));
            };
            put(vm, recv, "out", out.clone())?;
            b64_stream_set_int(vm, recv, B64_STREAM_FLAGS, base64_flags(args))?;
            b64_stream_set_int(vm, recv, B64_STREAM_CLOSED, 0)?;
            b64_stream_save_pending(vm, recv, &[], 0)?;
        }
        ("write", "(I)V" | "([B)V" | "([BII)V") => {
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Err(b64_stream_throw("Ljava/io/IOException;", "Stream closed"));
            }
            let flags = b64_stream_int(vm, recv, B64_STREAM_FLAGS);
            let mut pending = b64_stream_pending(vm, recv);
            let mut column = b64_stream_int(vm, recv, B64_STREAM_COLUMN);
            let mut encoded = Vec::new();
            for byte in b64_stream_write_data(vm, sig, args)? {
                pending.push(byte);
                if pending.len() == 3 {
                    base64_push_group(&mut encoded, &pending, flags);
                    column += 4;
                    if flags & BASE64_NO_WRAP == 0 && column == 76 {
                        encoded.extend_from_slice(base64_newline(flags));
                        column = 0;
                    }
                    pending.clear();
                }
            }
            b64_stream_save_pending(vm, recv, &pending, column)?;
            b64_stream_write_encoded(vm, recv, &encoded)?;
        }
        ("flush", "()V") => {
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Err(b64_stream_throw("Ljava/io/IOException;", "Stream closed"));
            }
            let out = b64_stream_wrapped(vm, recv, "out")?;
            vm.invoke_instance(out, "flush", "()V", &[])?;
        }
        ("close", "()V") => {
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Ok(Some(Value::Null));
            }
            let flags = b64_stream_int(vm, recv, B64_STREAM_FLAGS);
            let pending = b64_stream_pending(vm, recv);
            let mut column = b64_stream_int(vm, recv, B64_STREAM_COLUMN);
            let mut encoded = Vec::new();
            if !pending.is_empty() {
                let start = encoded.len();
                base64_push_group(&mut encoded, &pending, flags);
                column += (encoded.len() - start) as i32;
            }
            if flags & BASE64_NO_WRAP == 0 && column > 0 {
                encoded.extend_from_slice(base64_newline(flags));
            }
            b64_stream_save_pending(vm, recv, &[], 0)?;
            b64_stream_set_int(vm, recv, B64_STREAM_CLOSED, 1)?;
            b64_stream_write_encoded(vm, recv, &encoded)?;
            let out = b64_stream_wrapped(vm, recv, "out")?;
            if flags & BASE64_NO_CLOSE == 0 {
                vm.invoke_instance(out, "close", "()V", &[])?;
            } else {
                vm.invoke_instance(out, "flush", "()V", &[])?;
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(Value::Null))
}

fn base64_input_load(vm: &mut Engine, recv: ObjRef) -> Result<Vec<u8>, VmExit> {
    if let Value::Obj(decoded) = get(vm, recv, B64_STREAM_DECODED) {
        return b64_stream_array_bytes(vm, Some(&Value::Obj(decoded)));
    }
    let input = b64_stream_wrapped(vm, recv, "in")?;
    let scratch = crate::jlstring::byte_array(vm, &[0; 8192])?;
    let Value::Obj(scratch_ref) = scratch else {
        return Err(b64_stream_throw("Ljava/io/IOException;", "scratch buffer"));
    };
    let mut encoded = Vec::new();
    let mut zero_reads = 0usize;
    loop {
        let n = vm
            .invoke_instance(
                input,
                "read",
                "([BII)I",
                &[scratch.clone(), Value::Int(0), Value::Int(8192)],
            )?
            .as_int()
            .unwrap_or(-1);
        if n < 0 {
            break;
        }
        if n == 0 {
            zero_reads += 1;
            if zero_reads > 100 {
                break;
            }
            continue;
        }
        zero_reads = 0;
        let bytes = vm.heap.as_array_elems(scratch_ref)?.0;
        let take = (n as usize).min(bytes.len());
        encoded.extend(
            bytes[..take]
                .iter()
                .map(|value| value.as_int().unwrap_or(0) as u8),
        );
    }
    let decoded = base64_decode(&encoded, b64_stream_int(vm, recv, B64_STREAM_FLAGS))?;
    let decoded_value = crate::jlstring::byte_array(vm, &decoded)?;
    put(vm, recv, B64_STREAM_DECODED, decoded_value)?;
    Ok(decoded)
}

fn base64_input_stream(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(Ljava/io/InputStream;I)V") => {
            let Some(input @ Value::Obj(_)) = args.first() else {
                return Err(b64_stream_throw(
                    "Ljava/lang/NullPointerException;",
                    "in == null",
                ));
            };
            put(vm, recv, "in", input.clone())?;
            b64_stream_set_int(vm, recv, B64_STREAM_FLAGS, base64_flags(args))?;
            b64_stream_set_int(vm, recv, B64_STREAM_CLOSED, 0)?;
            b64_stream_set_int(vm, recv, B64_STREAM_POS, 0)?;
            put(vm, recv, B64_STREAM_DECODED, Value::Null)?;
            Ok(Some(Value::Null))
        }
        ("read", "()I") => {
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Ok(Some(Value::Int(-1)));
            }
            let decoded = base64_input_load(vm, recv)?;
            let pos = b64_stream_int(vm, recv, B64_STREAM_POS).max(0) as usize;
            let Some(byte) = decoded.get(pos) else {
                return Ok(Some(Value::Int(-1)));
            };
            b64_stream_set_int(vm, recv, B64_STREAM_POS, (pos + 1) as i32)?;
            Ok(Some(Value::Int(i32::from(*byte))))
        }
        ("read", "([B)I" | "([BII)I") => {
            let Some(Value::Obj(array)) = args.first() else {
                return Err(b64_stream_throw(
                    "Ljava/lang/NullPointerException;",
                    "buffer == null",
                ));
            };
            let array_len = vm.heap.as_array_elems(*array)?.0.len();
            let (offset, len) = if sig == "([B)I" {
                (0, array_len as i32)
            } else {
                (
                    args.get(1)
                        .and_then(|value| value.as_int().ok())
                        .unwrap_or(0),
                    args.get(2)
                        .and_then(|value| value.as_int().ok())
                        .unwrap_or(0),
                )
            };
            let offset = usize::try_from(offset).ok();
            let len = usize::try_from(len).ok();
            let Some((offset, len)) = offset.zip(len) else {
                return Err(b64_stream_throw(
                    "Ljava/lang/IndexOutOfBoundsException;",
                    "negative offset or length",
                ));
            };
            if offset
                .checked_add(len)
                .filter(|end| *end <= array_len)
                .is_none()
            {
                return Err(b64_stream_throw(
                    "Ljava/lang/IndexOutOfBoundsException;",
                    format!("offset={offset}, len={len}, array length={array_len}"),
                ));
            }
            if len == 0 {
                return Ok(Some(Value::Int(0)));
            }
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Ok(Some(Value::Int(-1)));
            }
            let decoded = base64_input_load(vm, recv)?;
            let pos = b64_stream_int(vm, recv, B64_STREAM_POS).max(0) as usize;
            if pos >= decoded.len() {
                return Ok(Some(Value::Int(-1)));
            }
            let count = len.min(decoded.len() - pos);
            {
                let elems = vm.heap.array_elems_mut(*array)?;
                for (index, byte) in decoded[pos..pos + count].iter().enumerate() {
                    elems[offset + index] = Value::Int(i32::from(*byte as i8));
                }
            }
            b64_stream_set_int(vm, recv, B64_STREAM_POS, (pos + count) as i32)?;
            Ok(Some(Value::Int(count as i32)))
        }
        ("available", "()I") => {
            if let Value::Obj(decoded_ref) = get(vm, recv, B64_STREAM_DECODED) {
                let decoded = b64_stream_array_bytes(vm, Some(&Value::Obj(decoded_ref)))?;
                let pos = b64_stream_int(vm, recv, B64_STREAM_POS).max(0) as usize;
                return Ok(Some(Value::Int(decoded.len().saturating_sub(pos) as i32)));
            }
            let input = b64_stream_wrapped(vm, recv, "in")?;
            let encoded_available = vm
                .invoke_instance(input, "available", "()I", &[])?
                .as_int()
                .unwrap_or(0)
                .max(0);
            Ok(Some(Value::Int((encoded_available * 3) / 4)))
        }
        ("close", "()V") => {
            if b64_stream_int(vm, recv, B64_STREAM_CLOSED) != 0 {
                return Ok(Some(Value::Null));
            }
            b64_stream_set_int(vm, recv, B64_STREAM_CLOSED, 1)?;
            if b64_stream_int(vm, recv, B64_STREAM_FLAGS) & BASE64_NO_CLOSE == 0 {
                let input = b64_stream_wrapped(vm, recv, "in")?;
                vm.invoke_instance(input, "close", "()V", &[])?;
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// `android.util.Base64` compile-time flag constants for `sget` and
/// reflection paths that do not inline them.
pub(crate) fn host_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if let Some(value) = crate::android_json::host_static(vm, class, name)? {
        return Ok(Some(value));
    }
    if class == ARRAY_MAP && name == "EMPTY" {
        // `ArrayMap.EMPTY` is a canonical empty instance; the sget path
        // caches this first materialization in the class statics, so its
        // identity is stable across reads.
        let r = vm.heap.alloc_instance(ARRAY_MAP.to_string(), Vec::new())?;
        vm.fw.objects.insert(
            r,
            HostObj::Map {
                entries: Vec::new(),
            },
        );
        return Ok(Some(Value::Obj(r)));
    }
    if class != BASE64 {
        return Ok(None);
    }
    Ok(Some(Value::Int(match name {
        "DEFAULT" => 0,
        "NO_PADDING" => BASE64_NO_PADDING,
        "NO_WRAP" => BASE64_NO_WRAP,
        "CRLF" => BASE64_CRLF,
        "URL_SAFE" => BASE64_URL_SAFE,
        "NO_CLOSE" => BASE64_NO_CLOSE,
        _ => return Ok(None),
    })))
}

const LRU_CACHE: &str = "Landroid/util/LruCache;";
const LRU_MAX_SIZE: &str = "rd_lru_max_size";
const LRU_SIZE: &str = "rd_lru_size";
const LRU_PUT_COUNT: &str = "rd_lru_put_count";
const LRU_CREATE_COUNT: &str = "rd_lru_create_count";
const LRU_EVICTION_COUNT: &str = "rd_lru_eviction_count";
const LRU_HIT_COUNT: &str = "rd_lru_hit_count";
const LRU_MISS_COUNT: &str = "rd_lru_miss_count";

fn lru_throw(class: &str, message: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, message.into()))
}

fn lru_int_field(vm: &Engine, recv: ObjRef, field: &str) -> i32 {
    match get(vm, recv, field) {
        Value::Int(value) => value,
        _ => 0,
    }
}

fn lru_set_int_field(vm: &mut Engine, recv: ObjRef, field: &str, value: i32) -> Result<(), VmExit> {
    put(vm, recv, field, Value::Int(value))
}

fn lru_increment(vm: &mut Engine, recv: ObjRef, field: &str) -> Result<(), VmExit> {
    lru_set_int_field(
        vm,
        recv,
        field,
        lru_int_field(vm, recv, field).wrapping_add(1),
    )
}

fn lru_entries(vm: &mut Engine, recv: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.objects.get(&recv) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => {
            vm.fw.objects.insert(
                recv,
                HostObj::Map {
                    entries: Vec::new(),
                },
            );
            Vec::new()
        }
    }
}

fn lru_set_entries(vm: &mut Engine, recv: ObjRef, entries: Vec<(Value, Value)>) {
    vm.fw.objects.insert(recv, HostObj::Map { entries });
}

fn lru_find(vm: &Engine, entries: &[(Value, Value)], key: &Value) -> Result<Option<usize>, VmExit> {
    for (index, (candidate, _)) in entries.iter().enumerate() {
        if crate::intrinsics::values_equal(&vm.heap, candidate, key)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn lru_guest_method(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let class = vm.heap.class_of(recv)?.to_string();
    if vm.cp.resolve_method(&class, name, sig).is_none() {
        return Ok(None);
    }
    vm.invoke_instance(recv, name, sig, args).map(Some)
}

fn lru_size_of(vm: &mut Engine, recv: ObjRef, key: &Value, value: &Value) -> Result<i32, VmExit> {
    let size = match lru_guest_method(
        vm,
        recv,
        "sizeOf",
        "(Ljava/lang/Object;Ljava/lang/Object;)I",
        &[key.clone(), value.clone()],
    )? {
        Some(Value::Int(size)) => size,
        Some(other) => other.as_int()?,
        None => 1,
    };
    if size < 0 {
        return Err(lru_throw(
            "Ljava/lang/IllegalStateException;",
            format!("Negative size: {key:?}={value:?}"),
        ));
    }
    Ok(size)
}

fn lru_create(vm: &mut Engine, recv: ObjRef, key: &Value) -> Result<Value, VmExit> {
    Ok(lru_guest_method(
        vm,
        recv,
        "create",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        core::slice::from_ref(key),
    )?
    .unwrap_or(Value::Null))
}

fn lru_entry_removed(
    vm: &mut Engine,
    recv: ObjRef,
    evicted: bool,
    key: &Value,
    old_value: &Value,
    new_value: &Value,
) -> Result<(), VmExit> {
    lru_guest_method(
        vm,
        recv,
        "entryRemoved",
        "(ZLjava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)V",
        &[
            Value::Int(evicted as i32),
            key.clone(),
            old_value.clone(),
            new_value.clone(),
        ],
    )?;
    Ok(())
}

fn lru_trim_to_size(vm: &mut Engine, recv: ObjRef, max_size: i32) -> Result<(), VmExit> {
    loop {
        let mut entries = lru_entries(vm, recv);
        let size = lru_int_field(vm, recv, LRU_SIZE);
        if size < 0 || (entries.is_empty() && size != 0) {
            return Err(lru_throw(
                "Ljava/lang/IllegalStateException;",
                "LruCache.sizeOf() is reporting inconsistent values!",
            ));
        }
        if size <= max_size || entries.is_empty() {
            return Ok(());
        }
        let (key, value) = entries.remove(0);
        lru_set_entries(vm, recv, entries);
        let evicted_size = lru_size_of(vm, recv, &key, &value)?;
        lru_set_int_field(
            vm,
            recv,
            LRU_SIZE,
            lru_int_field(vm, recv, LRU_SIZE).wrapping_sub(evicted_size),
        )?;
        lru_increment(vm, recv, LRU_EVICTION_COUNT)?;
        lru_entry_removed(vm, recv, true, &key, &value, &Value::Null)?;
    }
}

fn lru_require_key(key: &Value) -> Result<(), VmExit> {
    if matches!(key, Value::Null) {
        Err(lru_throw("Ljava/lang/NullPointerException;", "key == null"))
    } else {
        Ok(())
    }
}

fn lru_cache(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Value::Null);
    Ok(Some(match (name, sig) {
        ("<init>", "(I)V") => {
            let max_size = arg(0).as_int()?;
            if max_size <= 0 {
                return Err(lru_throw(
                    "Ljava/lang/IllegalArgumentException;",
                    "maxSize <= 0",
                ));
            }
            lru_set_entries(vm, recv, Vec::new());
            for (field, value) in [
                (LRU_MAX_SIZE, max_size),
                (LRU_SIZE, 0),
                (LRU_PUT_COUNT, 0),
                (LRU_CREATE_COUNT, 0),
                (LRU_EVICTION_COUNT, 0),
                (LRU_HIT_COUNT, 0),
                (LRU_MISS_COUNT, 0),
            ] {
                lru_set_int_field(vm, recv, field, value)?;
            }
            Value::Null
        }
        ("get", "(Ljava/lang/Object;)Ljava/lang/Object;") => {
            let key = arg(0);
            lru_require_key(&key)?;
            let mut entries = lru_entries(vm, recv);
            if let Some(index) = lru_find(vm, &entries, &key)? {
                let entry = entries.remove(index);
                let value = entry.1.clone();
                entries.push(entry); // Access order: now most recently used.
                lru_set_entries(vm, recv, entries);
                lru_increment(vm, recv, LRU_HIT_COUNT)?;
                return Ok(Some(value));
            }
            lru_increment(vm, recv, LRU_MISS_COUNT)?;
            let created = lru_create(vm, recv, &key)?;
            if matches!(created, Value::Null) {
                return Ok(Some(Value::Null));
            }
            lru_increment(vm, recv, LRU_CREATE_COUNT)?;
            let mut entries = lru_entries(vm, recv);
            if let Some(index) = lru_find(vm, &entries, &key)? {
                // `create` itself populated this key. AOSP restores that
                // value and reports the created value as removed.
                let entry = entries.remove(index);
                let existing = entry.1.clone();
                entries.push(entry);
                lru_set_entries(vm, recv, entries);
                lru_entry_removed(vm, recv, false, &key, &created, &existing)?;
                return Ok(Some(existing));
            }
            let created_size = lru_size_of(vm, recv, &key, &created)?;
            let mut entries = lru_entries(vm, recv);
            entries.push((key, created.clone()));
            lru_set_entries(vm, recv, entries);
            lru_set_int_field(
                vm,
                recv,
                LRU_SIZE,
                lru_int_field(vm, recv, LRU_SIZE).wrapping_add(created_size),
            )?;
            let max_size = lru_int_field(vm, recv, LRU_MAX_SIZE);
            lru_trim_to_size(vm, recv, max_size)?;
            created
        }
        ("put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;") => {
            let key = arg(0);
            let value = arg(1);
            if matches!(key, Value::Null) || matches!(value, Value::Null) {
                return Err(lru_throw(
                    "Ljava/lang/NullPointerException;",
                    "key == null || value == null",
                ));
            }
            lru_increment(vm, recv, LRU_PUT_COUNT)?;
            let new_size = lru_size_of(vm, recv, &key, &value)?;
            let mut entries = lru_entries(vm, recv);
            let previous = if let Some(index) = lru_find(vm, &entries, &key)? {
                let (existing_key, previous) = entries.remove(index);
                entries.push((existing_key, value.clone()));
                Some(previous)
            } else {
                entries.push((key.clone(), value.clone()));
                None
            };
            lru_set_entries(vm, recv, entries);
            let mut size = lru_int_field(vm, recv, LRU_SIZE).wrapping_add(new_size);
            if let Some(previous) = &previous {
                size = size.wrapping_sub(lru_size_of(vm, recv, &key, previous)?);
            }
            lru_set_int_field(vm, recv, LRU_SIZE, size)?;
            if let Some(previous) = &previous {
                lru_entry_removed(vm, recv, false, &key, previous, &value)?;
            }
            let max_size = lru_int_field(vm, recv, LRU_MAX_SIZE);
            lru_trim_to_size(vm, recv, max_size)?;
            previous.unwrap_or(Value::Null)
        }
        ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;") => {
            let key = arg(0);
            lru_require_key(&key)?;
            let mut entries = lru_entries(vm, recv);
            let Some(index) = lru_find(vm, &entries, &key)? else {
                return Ok(Some(Value::Null));
            };
            let (_, previous) = entries.remove(index);
            lru_set_entries(vm, recv, entries);
            let previous_size = lru_size_of(vm, recv, &key, &previous)?;
            lru_set_int_field(
                vm,
                recv,
                LRU_SIZE,
                lru_int_field(vm, recv, LRU_SIZE).wrapping_sub(previous_size),
            )?;
            lru_entry_removed(vm, recv, false, &key, &previous, &Value::Null)?;
            previous
        }
        ("trimToSize", "(I)V") => {
            lru_trim_to_size(vm, recv, arg(0).as_int()?)?;
            Value::Null
        }
        ("resize", "(I)V") => {
            let max_size = arg(0).as_int()?;
            if max_size <= 0 {
                return Err(lru_throw(
                    "Ljava/lang/IllegalArgumentException;",
                    "maxSize <= 0",
                ));
            }
            lru_set_int_field(vm, recv, LRU_MAX_SIZE, max_size)?;
            lru_trim_to_size(vm, recv, max_size)?;
            Value::Null
        }
        ("evictAll", "()V") => {
            lru_trim_to_size(vm, recv, -1)?;
            Value::Null
        }
        ("size", "()I") => Value::Int(lru_int_field(vm, recv, LRU_SIZE)),
        ("maxSize", "()I") => Value::Int(lru_int_field(vm, recv, LRU_MAX_SIZE)),
        ("hitCount", "()I") => Value::Int(lru_int_field(vm, recv, LRU_HIT_COUNT)),
        ("missCount", "()I") => Value::Int(lru_int_field(vm, recv, LRU_MISS_COUNT)),
        ("createCount", "()I") => Value::Int(lru_int_field(vm, recv, LRU_CREATE_COUNT)),
        ("putCount", "()I") => Value::Int(lru_int_field(vm, recv, LRU_PUT_COUNT)),
        ("evictionCount", "()I") => Value::Int(lru_int_field(vm, recv, LRU_EVICTION_COUNT)),
        ("snapshot", "()Ljava/util/Map;") => {
            let entries = lru_entries(vm, recv);
            let map = vm
                .heap
                .alloc_instance("Ljava/util/LinkedHashMap;".to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            vm.fw.objects.insert(map, HostObj::Map { entries });
            put(vm, map, "rd$linked_map_access_order", Value::Int(1))?;
            Value::Obj(map)
        }
        ("toString", "()Ljava/lang/String;") => {
            let hits = lru_int_field(vm, recv, LRU_HIT_COUNT);
            let misses = lru_int_field(vm, recv, LRU_MISS_COUNT);
            let accesses = hits.wrapping_add(misses);
            let hit_percent = if accesses != 0 {
                (i64::from(hits) * 100 / i64::from(accesses)) as i32
            } else {
                0
            };
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!(
                    "LruCache[maxSize={},hits={},misses={},hitRate={}%]",
                    lru_int_field(vm, recv, LRU_MAX_SIZE),
                    hits,
                    misses,
                    hit_percent
                ),
            )?)
        }
        _ => return Ok(None),
    }))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(value) = crate::android_json::static_intrinsic(vm, class, name, args)? {
        return Ok(Some(value));
    }
    if class == BASE64 {
        return base64_static(vm, name, args);
    }
    match (class, name) {
        ("Landroid/util/Pair;", "create") | ("Landroid/util/Range;", "create") => {
            let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
            let (f0, f1) = if class.ends_with("Pair;") {
                ("first", "second")
            } else {
                ("rd_lower", "rd_upper")
            };
            put(vm, r, f0, args.first().cloned().unwrap_or(Value::Null))?;
            put(vm, r, f1, args.get(1).cloned().unwrap_or(Value::Null))?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // This dispatcher is consulted for host calls on *every* receiver class
    // (including hot java.lang.String operations). Keep the non-Android path
    // free of receiver-class allocation, method resolution, and subtype
    // walks: only do that work after an exact Android class/method gate says
    // this module could actually handle the call.
    if matches!(
        mref_class,
        "Landroid/util/JsonReader;" | "Landroid/util/JsonWriter;"
    ) || matches!(
        recv_class,
        "Landroid/util/JsonReader;" | "Landroid/util/JsonWriter;"
    ) {
        if let Some(value) = crate::android_json::instance_intrinsic(
            vm, mref_class, recv_class, name, sig, recv, args,
        )? {
            return Ok(Some(value));
        }
    }
    // LruCache's state operations are final, so a DEX subclass reference
    // can safely use the same host implementation while its create/sizeOf/
    // entryRemoved overrides remain guest methods.
    let lru_final_method = matches!(
        name,
        "<init>"
            | "get"
            | "put"
            | "remove"
            | "trimToSize"
            | "resize"
            | "evictAll"
            | "size"
            | "maxSize"
            | "hitCount"
            | "missCount"
            | "createCount"
            | "putCount"
            | "evictionCount"
            | "snapshot"
    );
    if mref_class == LRU_CACHE || (lru_final_method && vm.cp.is_subtype(recv_class, LRU_CACHE)) {
        return lru_cache(vm, name, sig, recv, args);
    }
    if matches!(mref_class, ARRAY_MAP | SIMPLE_ARRAY_MAP)
        || matches!(recv_class, ARRAY_MAP | SIMPLE_ARRAY_MAP)
    {
        return array_map(vm, name, sig, recv, args);
    }
    if mref_class == ARRAY_SET || recv_class == ARRAY_SET {
        return array_set(vm, name, sig, recv, args);
    }
    // DEX subclasses inherit ArrayMap/ArraySet's host state operations when
    // they do not declare the method themselves. Gate this expensive subtype
    // check on a DEX-defined receiver plus an Array* operation name so
    // platform receivers (String, collections, streams) stay on the cheap
    // exact-class path above.
    let array_container_method = matches!(
        name,
        "put"
            | "append"
            | "putAll"
            | "get"
            | "getOrDefault"
            | "putIfAbsent"
            | "remove"
            | "replace"
            | "containsKey"
            | "containsValue"
            | "indexOfKey"
            | "indexOfValue"
            | "keyAt"
            | "valueAt"
            | "setValueAt"
            | "removeAt"
            | "size"
            | "isEmpty"
            | "clear"
            | "ensureCapacity"
            | "keySet"
            | "values"
            | "entrySet"
            | "hashCode"
            | "equals"
            | "toString"
            | "add"
            | "addAll"
            | "removeAll"
            | "retainAll"
            | "contains"
            | "containsAll"
            | "indexOf"
            | "iterator"
            | "toArray"
    );
    if array_container_method && vm.cp.find_class(recv_class).is_some() {
        if vm.cp.is_subtype(recv_class, ARRAY_MAP)
            && vm.cp.resolve_method(recv_class, name, sig).is_none()
        {
            return array_map(vm, name, sig, recv, args);
        }
        if vm.cp.is_subtype(recv_class, ARRAY_SET)
            && vm.cp.resolve_method(recv_class, name, sig).is_none()
        {
            return array_set(vm, name, sig, recv, args);
        }
    }
    // Base64 streams: first require one of the exact operations implemented
    // below. A platform receiver that is not the stream class itself cannot
    // be an unloaded subtype; only a DEX-defined receiver can need the
    // subtype + guest-override checks.
    let base64_input_method = matches!(
        (name, sig),
        ("<init>", "(Ljava/io/InputStream;I)V")
            | ("read", "()I" | "([B)I" | "([BII)I")
            | ("available", "()I")
            | ("close", "()V")
    );
    if base64_input_method {
        let exact_stream = mref_class == BASE64_INPUT_STREAM || recv_class == BASE64_INPUT_STREAM;
        let dex_subclass = !exact_stream
            && vm.cp.find_class(recv_class).is_some()
            && vm.cp.is_subtype(recv_class, BASE64_INPUT_STREAM)
            && vm.cp.resolve_method(recv_class, name, sig).is_none();
        if exact_stream || dex_subclass {
            return base64_input_stream(vm, name, sig, recv, args);
        }
    }
    let base64_output_method = matches!(
        (name, sig),
        ("<init>", "(Ljava/io/OutputStream;I)V")
            | ("write", "(I)V" | "([B)V" | "([BII)V")
            | ("flush", "()V")
            | ("close", "()V")
    );
    if base64_output_method {
        let exact_stream = mref_class == BASE64_OUTPUT_STREAM || recv_class == BASE64_OUTPUT_STREAM;
        let dex_subclass = !exact_stream
            && vm.cp.find_class(recv_class).is_some()
            && vm.cp.is_subtype(recv_class, BASE64_OUTPUT_STREAM)
            && vm.cp.resolve_method(recv_class, name, sig).is_none();
        if exact_stream || dex_subclass {
            return base64_output_stream(vm, name, sig, recv, args);
        }
    }
    if !mref_class.starts_with("Landroid/util/") {
        return Ok(None);
    }
    if let Some(c) = SPARSE.iter().find(|c| **c == mref_class) {
        return sparse(vm, c, name, recv, args);
    }
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let fields: &[&str] = match mref_class {
        "Landroid/util/Size;" | "Landroid/util/SizeF;" => &["rd_w", "rd_h"],
        "Landroid/util/Pair;" => &["first", "second"],
        "Landroid/util/Range;" => &["rd_lower", "rd_upper"],
        "Landroid/util/Rational;" => &["rd_num", "rd_den"],
        _ => return Ok(None),
    };
    let same = |vm: &Engine, o: &Value| -> bool {
        match o {
            Value::Obj(o) => {
                vm.heap.class_of(*o).ok() == vm.heap.class_of(recv).ok()
                    && fields.iter().all(|f| get(vm, *o, f) == get(vm, recv, f))
            }
            _ => false,
        }
    };
    let hash = |vm: &Engine| -> i32 {
        fields.iter().fold(0i32, |h, f| {
            let x = match get(vm, recv, f) {
                Value::Int(i) => i,
                Value::Float(x) => x.to_bits() as i32,
                Value::Obj(r) => r as i32,
                _ => 0,
            };
            h.wrapping_mul(31).wrapping_add(x)
        })
    };
    Ok(Some(match name {
        "<init>" => {
            let (mut x, mut y) = (a(0), a(1));
            if mref_class == "Landroid/util/Rational;" {
                // AOSP normalizes by gcd and keeps the sign on the numerator
                if let (Value::Int(n), Value::Int(d)) = (&x, &y) {
                    let (mut n, mut d) = (*n as i64, *d as i64);
                    if d < 0 {
                        n = -n;
                        d = -d;
                    }
                    let g = gcd(n.abs(), d.abs()).max(1);
                    x = Value::Int((n / g) as i32);
                    y = Value::Int((d / g) as i32);
                }
            }
            put(vm, recv, fields[0], x)?;
            put(vm, recv, fields[1], y)?;
            Value::Null
        }
        "getWidth" => get(vm, recv, "rd_w"),
        "getHeight" => get(vm, recv, "rd_h"),
        "getLower" => get(vm, recv, "rd_lower"),
        "getUpper" => get(vm, recv, "rd_upper"),
        "getNumerator" => get(vm, recv, "rd_num"),
        "getDenominator" => get(vm, recv, "rd_den"),
        "doubleValue" | "floatValue" | "intValue" | "longValue" => {
            let (n, d) = (num(&get(vm, recv, "rd_num")), num(&get(vm, recv, "rd_den")));
            let v = if d == 0.0 { f64::NAN } else { n / d };
            match name {
                "doubleValue" => Value::Double(v),
                "floatValue" => Value::Float(v as f32),
                "intValue" => Value::Int(v as i32),
                _ => Value::Long(v as i64),
            }
        }
        "contains" | "clamp" if mref_class == "Landroid/util/Range;" => {
            let (lo, hi, v) = (get(vm, recv, "rd_lower"), get(vm, recv, "rd_upper"), a(0));
            let c_lo = compare(vm, &v, &lo)?;
            let c_hi = compare(vm, &v, &hi)?;
            if name == "contains" {
                Value::Int((c_lo >= 0 && c_hi <= 0) as i32)
            } else if c_lo < 0 {
                lo
            } else if c_hi > 0 {
                hi
            } else {
                v
            }
        }
        "equals" => Value::Int(same(vm, &a(0)) as i32),
        "hashCode" => Value::Int(hash(vm)),
        "clone" => {
            let r = vm.heap.alloc_instance(mref_class.to_string(), Vec::new())?;
            for f in fields {
                let v = get(vm, recv, f);
                put(vm, r, f, v)?;
            }
            Value::Obj(r)
        }
        "toString" => {
            let s = match mref_class {
                "Landroid/util/Size;" | "Landroid/util/SizeF;" => format!(
                    "{}x{}",
                    fmt(&get(vm, recv, "rd_w")),
                    fmt(&get(vm, recv, "rd_h"))
                ),
                "Landroid/util/Rational;" => format!(
                    "{}/{}",
                    fmt(&get(vm, recv, "rd_num")),
                    fmt(&get(vm, recv, "rd_den"))
                ),
                _ => return Ok(None),
            };
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => {
            let _ = sig;
            return Ok(None);
        }
    }))
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}
fn num(v: &Value) -> f64 {
    match v {
        Value::Int(i) => *i as f64,
        Value::Long(l) => *l as f64,
        Value::Float(f) => *f as f64,
        Value::Double(d) => *d,
        _ => 0.0,
    }
}
fn fmt(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        other => format!("{other:?}"),
    }
}
/// `Comparable.compareTo` for boxed numbers (fast path) or any guest object.
fn compare(vm: &mut Engine, a: &Value, b: &Value) -> Result<i32, VmExit> {
    let unbox = |vm: &Engine, v: &Value| -> Option<f64> {
        match v {
            Value::Obj(r) => match vm.heap.get_field(*r, "value") {
                Ok(x @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_))) => {
                    Some(num(&x))
                }
                _ => None,
            },
            _ => None,
        }
    };
    if let (Some(x), Some(y)) = (unbox(vm, a), unbox(vm, b)) {
        return Ok(x.partial_cmp(&y).map(|o| o as i32).unwrap_or(0));
    }
    match a {
        Value::Obj(r) => match crate::jcollections::guest_call(
            vm,
            *r,
            "compareTo",
            "(Ljava/lang/Object;)I",
            core::slice::from_ref(b),
        )? {
            Some(Value::Int(i)) => Ok(i),
            _ => Ok(0),
        },
        _ => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;

    #[test]
    fn android_base64_flags_and_wrapping_match_aosp() {
        assert_eq!(base64_encode(b"hello", 0), b"aGVsbG8=\n");
        assert_eq!(base64_encode(b"hello", BASE64_NO_WRAP), b"aGVsbG8=");
        assert_eq!(
            base64_encode(b"hello", BASE64_NO_WRAP | BASE64_NO_PADDING),
            b"aGVsbG8"
        );
        assert_eq!(
            base64_encode(&[0xfb, 0xff], BASE64_NO_WRAP | BASE64_URL_SAFE),
            b"-_8="
        );
        assert_eq!(base64_encode(b"hello", BASE64_CRLF), b"aGVsbG8=\r\n");
        assert_eq!(base64_encode(b"", 0), b"");

        let wrapped = base64_encode(&[b'a'; 58], 0);
        assert_eq!(&wrapped[..4], b"YWFh");
        assert_eq!(&wrapped[72..76], b"YWFh");
        assert_eq!(wrapped[76], b'\n');
        assert_eq!(&wrapped[77..], b"YQ==\n");
    }

    #[test]
    fn android_base64_decode_skips_garbage_and_validates_padding() {
        assert_eq!(base64_decode(b"aGVs bG8=\n", 0).unwrap(), b"hello");
        assert_eq!(base64_decode(b"TQ", 0).unwrap(), b"M");
        assert_eq!(base64_decode(b"TWE", 0).unwrap(), b"Ma");
        assert_eq!(
            base64_decode(b"-_8", BASE64_URL_SAFE).unwrap(),
            vec![0xfb, 0xff]
        );
        assert!(base64_decode(b"T", 0).is_err());
        assert!(base64_decode(b"TQ=", 0).is_err());
        assert!(base64_decode(b"TWE==", 0).is_err());
        assert!(base64_decode(b"TQ==QQ", 0).is_err());
    }

    #[test]
    fn lru_cache_access_order_eviction_and_stats_match_aosp() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let cache = vm
            .heap
            .alloc_instance(LRU_CACHE.to_string(), Vec::new())
            .unwrap();
        let call = |vm: &mut Engine, name: &str, sig: &str, args: &[Value]| {
            instance_intrinsic(vm, LRU_CACHE, LRU_CACHE, name, sig, cache, args)
                .unwrap()
                .unwrap()
        };
        let text = |vm: &mut Engine, value: &str| {
            Value::Obj(vm.heap.alloc_string(value.to_string()).unwrap())
        };

        call(&mut vm, "<init>", "(I)V", &[Value::Int(2)]);
        let a = text(&mut vm, "a");
        let one = text(&mut vm, "one");
        assert_eq!(
            call(
                &mut vm,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[a.clone(), one],
            ),
            Value::Null
        );
        let b = text(&mut vm, "b");
        let two = text(&mut vm, "two");
        call(
            &mut vm,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[b.clone(), two],
        );
        // Accessing `a` makes `b` the eldest entry.
        let a2 = text(&mut vm, "a");
        assert!(matches!(
            call(
                &mut vm,
                "get",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                &[a2],
            ),
            Value::Obj(_)
        ));
        let c = text(&mut vm, "c");
        let three = text(&mut vm, "three");
        call(
            &mut vm,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[c.clone(), three],
        );
        let b2 = text(&mut vm, "b");
        assert_eq!(
            call(
                &mut vm,
                "get",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                &[b2],
            ),
            Value::Null
        );
        assert_eq!(call(&mut vm, "size", "()I", &[]), Value::Int(2));
        assert_eq!(call(&mut vm, "hitCount", "()I", &[]), Value::Int(1));
        assert_eq!(call(&mut vm, "missCount", "()I", &[]), Value::Int(1));
        assert_eq!(call(&mut vm, "putCount", "()I", &[]), Value::Int(3));
        assert_eq!(call(&mut vm, "evictionCount", "()I", &[]), Value::Int(1));

        let snapshot = call(&mut vm, "snapshot", "()Ljava/util/Map;", &[]);
        let Value::Obj(snapshot) = snapshot else {
            panic!("snapshot did not return a Map");
        };
        let Some(HostObj::Map { entries }) = vm.fw.objects.get(&snapshot) else {
            panic!("snapshot has no host map");
        };
        let keys = entries
            .iter()
            .map(|(key, _)| match key {
                Value::Obj(key) => vm.heap.as_str(*key).unwrap().to_string(),
                other => panic!("snapshot key: {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(keys, ["a", "c"]);

        call(&mut vm, "resize", "(I)V", &[Value::Int(1)]);
        assert_eq!(call(&mut vm, "size", "()I", &[]), Value::Int(1));
        assert!(instance_intrinsic(
            &mut vm,
            LRU_CACHE,
            LRU_CACHE,
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            cache,
            &[Value::Null, Value::Null],
        )
        .is_err());
    }

    #[test]
    fn array_map_and_array_set_keep_signed_hash_order() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let text = |vm: &mut Engine, value: &str| {
            Value::Obj(vm.heap.alloc_string(value.to_string()).unwrap())
        };
        let boxed = |vm: &mut Engine, value: i32| {
            Value::Obj(
                crate::boxes::box_value(vm, "Ljava/lang/Integer;", Value::Int(value)).unwrap(),
            )
        };
        let unboxed = |vm: &Engine, value: &Value| match value {
            Value::Obj(obj) => vm.heap.get_field(*obj, "value").unwrap().as_int().unwrap(),
            other => panic!("expected Integer, got {other:?}"),
        };

        let map = vm
            .heap
            .alloc_instance(ARRAY_MAP.to_string(), Vec::new())
            .unwrap();
        let call_map = |vm: &mut Engine, name: &str, sig: &str, args: &[Value]| {
            instance_intrinsic(vm, ARRAY_MAP, ARRAY_MAP, name, sig, map, args)
                .unwrap()
                .unwrap()
        };
        call_map(&mut vm, "<init>", "()V", &[]);
        for (key, value) in [(2, "two"), (-1, "minus"), (0, "zero"), (1, "one")] {
            let key = boxed(&mut vm, key);
            let value = text(&mut vm, value);
            call_map(
                &mut vm,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[key, value],
            );
        }
        for (key, value) in [("Aa", "aa"), ("BB", "bb")] {
            let key = text(&mut vm, key);
            let value = text(&mut vm, value);
            call_map(
                &mut vm,
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[key, value],
            );
        }
        let ordered_int_keys: Vec<i32> = (0..4)
            .map(|index| {
                let key = call_map(
                    &mut vm,
                    "keyAt",
                    "(I)Ljava/lang/Object;",
                    &[Value::Int(index)],
                );
                unboxed(&vm, &key)
            })
            .collect();
        assert_eq!(ordered_int_keys, [-1, 0, 1, 2]);
        let aa_key = call_map(&mut vm, "keyAt", "(I)Ljava/lang/Object;", &[Value::Int(4)]);
        let Value::Obj(aa_key) = aa_key else {
            panic!("keyAt(4) was not a String");
        };
        assert_eq!(vm.heap.as_str(aa_key).unwrap(), "Aa");
        let bb = text(&mut vm, "BB");
        assert_eq!(
            call_map(
                &mut vm,
                "indexOfKey",
                "(Ljava/lang/Object;)I",
                core::slice::from_ref(&bb),
            ),
            Value::Int(5)
        );
        let bb_value = call_map(
            &mut vm,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            core::slice::from_ref(&bb),
        );
        let Value::Obj(bb_value) = bb_value else {
            panic!("BB value was not a String");
        };
        assert_eq!(vm.heap.as_str(bb_value).unwrap(), "bb");

        let set = vm
            .heap
            .alloc_instance(ARRAY_SET.to_string(), Vec::new())
            .unwrap();
        let call_set = |vm: &mut Engine, name: &str, sig: &str, args: &[Value]| {
            instance_intrinsic(vm, ARRAY_SET, ARRAY_SET, name, sig, set, args)
                .unwrap()
                .unwrap()
        };
        call_set(&mut vm, "<init>", "()V", &[]);
        for value in [2, -1, 0] {
            let boxed_value = boxed(&mut vm, value);
            assert_eq!(
                call_set(
                    &mut vm,
                    "add",
                    "(Ljava/lang/Object;)Z",
                    core::slice::from_ref(&boxed_value),
                ),
                Value::Int(1)
            );
        }
        let duplicate = boxed(&mut vm, 2);
        assert_eq!(
            call_set(
                &mut vm,
                "add",
                "(Ljava/lang/Object;)Z",
                core::slice::from_ref(&duplicate),
            ),
            Value::Int(0)
        );
        let ordered_values: Vec<i32> = (0..3)
            .map(|index| {
                let value = call_set(
                    &mut vm,
                    "valueAt",
                    "(I)Ljava/lang/Object;",
                    &[Value::Int(index)],
                );
                unboxed(&vm, &value)
            })
            .collect();
        assert_eq!(ordered_values, [-1, 0, 2]);
        let iterator = call_set(&mut vm, "iterator", "()Ljava/util/Iterator;", &[]);
        let Value::Obj(iterator) = iterator else {
            panic!("iterator was not an object");
        };
        assert_eq!(
            vm.invoke_instance(iterator, "hasNext", "()Z", &[]).unwrap(),
            Value::Int(1)
        );
        let first = vm
            .invoke_instance(iterator, "next", "()Ljava/lang/Object;", &[])
            .unwrap();
        assert_eq!(unboxed(&vm, &first), -1);
    }

    #[test]
    fn array_set_object_forms_and_array_map_empty_follow_aosp() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let boxed = |vm: &mut Engine, value: i32| {
            Value::Obj(
                crate::boxes::box_value(vm, "Ljava/lang/Integer;", Value::Int(value)).unwrap(),
            )
        };
        let set = vm
            .heap
            .alloc_instance(ARRAY_SET.to_string(), Vec::new())
            .unwrap();
        let call_set = |vm: &mut Engine, name: &str, sig: &str, args: &[Value]| {
            instance_intrinsic(vm, ARRAY_SET, ARRAY_SET, name, sig, set, args)
                .unwrap()
                .unwrap()
        };
        call_set(&mut vm, "<init>", "()V", &[]);
        assert_eq!(
            match call_set(&mut vm, "toString", "()Ljava/lang/String;", &[]) {
                Value::Obj(o) => vm.heap.as_str(o).unwrap().to_string(),
                other => panic!("toString: {other:?}"),
            },
            "{}"
        );
        for value in [3, 1] {
            let v = boxed(&mut vm, value);
            call_set(
                &mut vm,
                "add",
                "(Ljava/lang/Object;)Z",
                core::slice::from_ref(&v),
            );
        }
        // hashCode is the sum of element hashes (Integer hash is itself).
        assert_eq!(call_set(&mut vm, "hashCode", "()I", &[]), Value::Int(4));
        // toString uses AOSP braces in signed-hash order.
        assert_eq!(
            match call_set(&mut vm, "toString", "()Ljava/lang/String;", &[]) {
                Value::Obj(o) => vm.heap.as_str(o).unwrap().to_string(),
                other => panic!("toString: {other:?}"),
            },
            "{1, 3}"
        );
        // equals against any java.util.Set host state, order-insensitive.
        let hash_set = vm
            .heap
            .alloc_instance("Ljava/util/HashSet;".to_string(), Vec::new())
            .unwrap();
        let three = boxed(&mut vm, 3);
        let one = boxed(&mut vm, 1);
        vm.fw.objects.insert(
            hash_set,
            HostObj::List {
                elems: vec![three, one],
            },
        );
        assert_eq!(
            call_set(
                &mut vm,
                "equals",
                "(Ljava/lang/Object;)Z",
                &[Value::Obj(hash_set)],
            ),
            Value::Int(1)
        );

        // ArrayMap.EMPTY materializes an empty ArrayMap instance.
        let empty = host_static(&mut vm, ARRAY_MAP, "EMPTY").unwrap().unwrap();
        let Value::Obj(empty) = empty else {
            panic!("EMPTY was not an object");
        };
        assert_eq!(vm.heap.class_of(empty).unwrap(), ARRAY_MAP);
        assert_eq!(
            instance_intrinsic(&mut vm, ARRAY_MAP, ARRAY_MAP, "size", "()I", empty, &[])
                .unwrap()
                .unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn array_set_collection_add_all_keeps_signed_hash_order() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let boxed = |vm: &mut Engine, value: i32| {
            Value::Obj(
                crate::boxes::box_value(vm, "Ljava/lang/Integer;", Value::Int(value)).unwrap(),
            )
        };
        let source = vm
            .heap
            .alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())
            .unwrap();
        vm.invoke_instance(source, "<init>", "()V", &[]).unwrap();
        for value in [2, -1, 0] {
            let boxed_value = boxed(&mut vm, value);
            vm.invoke_instance(
                source,
                "add",
                "(Ljava/lang/Object;)Z",
                core::slice::from_ref(&boxed_value),
            )
            .unwrap();
        }

        let set = vm
            .heap
            .alloc_instance(ARRAY_SET.to_string(), Vec::new())
            .unwrap();
        instance_intrinsic(&mut vm, ARRAY_SET, ARRAY_SET, "<init>", "()V", set, &[])
            .unwrap()
            .unwrap();
        // Name the call through Collection, as invoke-interface does. The
        // generic collection layer must not append in source order.
        assert_eq!(
            crate::intrinsics::call_instance_intrinsic_mref(
                &mut vm,
                ARRAY_SET,
                "Ljava/util/Collection;",
                "addAll",
                "(Ljava/util/Collection;)Z",
                set,
                &[Value::Obj(source)],
            )
            .unwrap(),
            Some(Value::Int(1))
        );
        let ordered: Vec<i32> = (0..3)
            .map(|index| {
                let value = instance_intrinsic(
                    &mut vm,
                    ARRAY_SET,
                    ARRAY_SET,
                    "valueAt",
                    "(I)Ljava/lang/Object;",
                    set,
                    &[Value::Int(index)],
                )
                .unwrap()
                .unwrap();
                let Value::Obj(obj) = value else {
                    panic!("valueAt({index}) was not an Integer");
                };
                vm.heap.get_field(obj, "value").unwrap().as_int().unwrap()
            })
            .collect();
        assert_eq!(ordered, [-1, 0, 2]);
    }

    #[test]
    fn array_set_typed_to_array_preserves_component_and_reuses_large_array() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let text = |vm: &mut Engine, value: &str| {
            Value::Obj(vm.heap.alloc_string(value.to_string()).unwrap())
        };
        let set = vm
            .heap
            .alloc_instance(ARRAY_SET.to_string(), Vec::new())
            .unwrap();
        let call_set = |vm: &mut Engine, name: &str, sig: &str, args: &[Value]| {
            instance_intrinsic(vm, ARRAY_SET, ARRAY_SET, name, sig, set, args)
                .unwrap()
                .unwrap()
        };
        call_set(&mut vm, "<init>", "()V", &[]);
        for value in ["b", "a"] {
            let element = text(&mut vm, value);
            call_set(
                &mut vm,
                "add",
                "(Ljava/lang/Object;)Z",
                core::slice::from_ref(&element),
            );
        }

        let large = crate::arrays::new_array(
            &mut vm,
            "[Ljava/lang/String;",
            crate::heap::ElemKind::Obj,
            vec![Value::Null; 3],
        )
        .unwrap();
        let reused = call_set(
            &mut vm,
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            core::slice::from_ref(&large),
        );
        assert_eq!(reused, large);
        let Value::Obj(large_ref) = large else {
            panic!("large array fixture was not an object");
        };
        let (elements, _, descriptor) = crate::arrays::elems(&mut vm, large_ref).unwrap();
        assert_eq!(descriptor, "[Ljava/lang/String;");
        let rendered: Vec<Option<String>> = elements
            .iter()
            .map(|value| match value {
                Value::Obj(obj) => vm.heap.as_str(*obj).ok().map(str::to_string),
                Value::Null => None,
                other => panic!("unexpected ArraySet element {other:?}"),
            })
            .collect();
        assert_eq!(
            rendered,
            [Some("a".to_string()), Some("b".to_string()), None]
        );

        let small = crate::arrays::new_array(
            &mut vm,
            "[Ljava/lang/String;",
            crate::heap::ElemKind::Obj,
            vec![Value::Null],
        )
        .unwrap();
        let replaced = call_set(
            &mut vm,
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
            core::slice::from_ref(&small),
        );
        assert_ne!(replaced, small);
        let Value::Obj(replaced_ref) = replaced else {
            panic!("typed toArray did not return an array object");
        };
        let (elements, _, descriptor) = crate::arrays::elems(&mut vm, replaced_ref).unwrap();
        assert_eq!(descriptor, "[Ljava/lang/String;");
        assert_eq!(elements.len(), 2);
    }

    #[test]
    fn android_base64_static_dispatch_uses_offsets_and_constants() {
        let mut vm = Engine::new(Vec::new(), crate::VmConfig::default());
        let input = crate::jlstring::byte_array(&mut vm, b"xxhello!").unwrap();
        let encoded = static_intrinsic(
            &mut vm,
            BASE64,
            "encodeToString",
            &[
                input,
                Value::Int(2),
                Value::Int(5),
                Value::Int(BASE64_NO_WRAP),
            ],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(encoded) = encoded else {
            panic!("encodeToString did not return a String");
        };
        assert_eq!(vm.heap.as_str(encoded).unwrap(), "aGVsbG8=");

        let encoded_ref = Value::Obj(encoded);
        let decoded = static_intrinsic(&mut vm, BASE64, "decode", &[encoded_ref, Value::Int(0)])
            .unwrap()
            .unwrap();
        let Value::Obj(decoded) = decoded else {
            panic!("decode did not return byte[]");
        };
        let values = vm.heap.as_array_elems(decoded).unwrap().0;
        assert_eq!(
            values
                .iter()
                .map(|value| value.as_int().unwrap() as u8)
                .collect::<Vec<_>>(),
            b"hello"
        );
        assert_eq!(
            host_static(&mut vm, BASE64, "URL_SAFE").unwrap(),
            Some(Value::Int(BASE64_URL_SAFE))
        );
        assert!(
            static_intrinsic(&mut vm, BASE64, "decode", &[Value::Null, Value::Int(0)],).is_err()
        );
    }
}
