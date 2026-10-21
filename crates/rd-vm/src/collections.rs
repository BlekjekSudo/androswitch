//! `java.util.Collections` statics that were missing from the host surface.
//!
//! Covered: `newSetFromMap`, `max`/`min` (± Comparator), `reverse`,
//! `singletonMap`, `emptyIterator`, `addAll(Collection, T...)`,
//! `list(Enumeration)`, and the `IdentityHashMap` hierarchy.
//!
//! These operate on host collection state (`HostObj::List` / `Map` / `Iter`).
//! Element comparison uses the guest `Comparator.compare` or
//! `Comparable.compareTo` (see `arrays::compare`).
//!
//! `newSetFromMap(m)` returns a fresh host `LinkedHashSet` instead of a view
//! backed by `m`. The universal idiom passes an empty, freshly created map,
//! where the two behave the same: a documented deviation.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::value::Value;

fn host_elems(vm: &Engine, v: Option<&Value>) -> Option<Vec<Value>> {
    match v {
        Some(Value::Obj(r)) => match vm.fw.objects.get(r) {
            Some(HostObj::List { elems }) | Some(HostObj::Iter { elems, .. }) => {
                Some(elems.clone())
            }
            Some(HostObj::Map { entries }) => {
                Some(entries.iter().map(|(k, _)| k.clone()).collect())
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn new_list(vm: &mut Engine, class: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}

/// Materialize JDK `Collections.EMPTY_*` fields on first `sget-object`.
/// AndroidX FragmentStore uses EMPTY_LIST as the no-fragments result; a null
/// fallback is not equivalent because callers immediately iterate it.
pub(crate) fn host_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if class != "Ljava/util/Collections;" {
        return Ok(None);
    }
    let (class, state) = match name {
        "EMPTY_LIST" => (
            "Ljava/util/Collections$SingletonList;",
            HostObj::List { elems: Vec::new() },
        ),
        "EMPTY_SET" | "EMPTY_SORTED_SET" => (
            "Ljava/util/Collections$UnmodifiableSet;",
            HostObj::List { elems: Vec::new() },
        ),
        "EMPTY_MAP" | "EMPTY_SORTED_MAP" => (
            "Ljava/util/Collections$UnmodifiableMap;",
            HostObj::Map {
                entries: Vec::new(),
            },
        ),
        _ => return Ok(None),
    };
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    vm.fw.objects.insert(r, state);
    Ok(Some(Value::Obj(r)))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != "Ljava/util/Collections;" {
        return Ok(None);
    }
    match (name, sig) {
        ("newSetFromMap", "(Ljava/util/Map;)Ljava/util/Set;") => {
            let init = host_elems(vm, args.first()).unwrap_or_default();
            Ok(Some(new_list(vm, "Ljava/util/LinkedHashSet;", init)?))
        }
        ("max", _) | ("min", _) => {
            let Some(items) = host_elems(vm, args.first()) else {
                return Ok(None);
            };
            if items.is_empty() {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/util/NoSuchElementException;",
                    String::new(),
                )));
            }
            let cmp = args.get(1).filter(|v| !matches!(v, Value::Null)).cloned();
            let mut best = items[0].clone();
            for it in &items[1..] {
                let c = crate::arrays::compare(vm, cmp.as_ref(), it, &best)?;
                if (name == "max" && c > 0) || (name == "min" && c < 0) {
                    best = it.clone();
                }
            }
            Ok(Some(best))
        }
        // Stable merge sort (the JDK's TimSort is stable too); a null
        // comparator means natural ordering.
        ("sort", "(Ljava/util/List;)V") | ("sort", "(Ljava/util/List;Ljava/util/Comparator;)V") => {
            let Some(Value::Obj(r)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Collections.sort(null)",
                )));
            };
            let Some(HostObj::List { elems }) = vm.fw.objects.get(r) else {
                return Ok(None);
            };
            let items = elems.clone();
            let cmp = args.get(1).filter(|v| !matches!(v, Value::Null)).cloned();
            let sorted = crate::arrays::merge_sort(vm, items, cmp.as_ref())?;
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(r) {
                *elems = sorted;
            }
            Ok(Some(Value::Null))
        }
        ("reverse", "(Ljava/util/List;)V") => {
            if let Some(Value::Obj(r)) = args.first() {
                if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(r) {
                    elems.reverse();
                    return Ok(Some(Value::Null));
                }
            }
            Ok(None)
        }
        ("singletonMap", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;") => {
            let r = vm
                .heap
                .alloc_instance("Ljava/util/HashMap;".to_string(), Vec::new())?;
            let e = vec![(args[0].clone(), args[1].clone())];
            vm.fw.objects.insert(r, HostObj::Map { entries: e });
            Ok(Some(Value::Obj(r)))
        }
        ("emptyIterator", "()Ljava/util/Iterator;") => {
            let r = vm
                .heap
                .alloc_instance("Ljava/util/Iterator;".to_string(), Vec::new())?;
            vm.fw.objects.insert(
                r,
                HostObj::Iter {
                    elems: Vec::new(),
                    pos: 0,
                },
            );
            Ok(Some(Value::Obj(r)))
        }
        ("list", "(Ljava/util/Enumeration;)Ljava/util/ArrayList;") => {
            let Some(items) = host_elems(vm, args.first()) else {
                return Ok(None);
            };
            Ok(Some(new_list(vm, "Ljava/util/ArrayList;", items)?))
        }
        ("addAll", "(Ljava/util/Collection;[Ljava/lang/Object;)Z") => {
            let (Some(Value::Obj(c)), Some(Value::Obj(a))) = (args.first(), args.get(1)) else {
                return Ok(None);
            };
            let add: Vec<Value> = vm.heap.as_array_elems(*a)?.0.to_vec();
            let is_set = vm
                .heap
                .class_of(*c)
                .map(|k| k.contains("Set"))
                .unwrap_or(false);
            let heap_snapshot: Vec<Value>;
            let mut changed = false;
            if let Some(HostObj::List { elems }) = vm.fw.objects.get(c) {
                heap_snapshot = elems.clone();
            } else {
                return Ok(None);
            }
            let mut out = heap_snapshot;
            for v in add {
                if is_set
                    && out
                        .iter()
                        .any(|x| crate::intrinsics::values_equal(&vm.heap, x, &v).unwrap_or(false))
                {
                    continue;
                }
                out.push(v);
                changed = true;
            }
            vm.fw.objects.insert(*c, HostObj::List { elems: out });
            Ok(Some(Value::Int(changed as i32)))
        }
        _ => Ok(None),
    }
}
