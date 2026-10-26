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

use crate::collection_factories::{self as state, Contents};
use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
#[cfg(not(feature = "std"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

fn host_elems(vm: &mut Engine, v: Option<&Value>) -> Result<Option<Vec<Value>>, VmExit> {
    let Some(Value::Obj(r)) = v else {
        return Ok(None);
    };
    let values = match state::snapshot(vm, *r)? {
        Some(Contents::List(elems) | Contents::Set(elems) | Contents::Iterator { elems, .. }) => {
            Some(elems)
        }
        Some(Contents::Map(entries)) => Some(entries.into_iter().map(|(k, _)| k).collect()),
        None => None,
    };
    if let Some(values) = &values {
        crate::shared_call::pin(vm, values);
    }
    Ok(values)
}

pub(crate) fn new_list(vm: &mut Engine, class: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
    let contents = if class.contains("Set") {
        Contents::Set(elems)
    } else {
        Contents::List(elems)
    };
    Ok(Value::Obj(state::create(vm, class, contents)?))
}

/// Materialize JDK `Collections.EMPTY_*` fields on first `sget-object`.
/// AndroidX FragmentStore uses EMPTY_LIST as the no-fragments result; a null
/// fallback is not equivalent because callers immediately iterate it.
#[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
pub(crate) fn host_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if class != "Ljava/util/Collections;" {
        return Ok(None);
    }
    if crate::collection_factories::static_field_descriptor(class, name).is_some() {
        return crate::collection_factories::static_field(vm, class, name);
    }
    let (class, state) = match name {
        "EMPTY_LIST" => (
            "Ljava/util/Collections$SingletonList;",
            Contents::List(Vec::new()),
        ),
        "EMPTY_SET" | "EMPTY_SORTED_SET" => (
            "Ljava/util/Collections$UnmodifiableSet;",
            Contents::List(Vec::new()),
        ),
        "EMPTY_MAP" | "EMPTY_SORTED_MAP" => (
            "Ljava/util/Collections$UnmodifiableMap;",
            Contents::Map(Vec::new()),
        ),
        _ => return Ok(None),
    };
    let r = state::create(vm, class, state)?;
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
            let init = host_elems(vm, args.first())?.unwrap_or_default();
            Ok(Some(new_list(vm, "Ljava/util/LinkedHashSet;", init)?))
        }
        ("max", _) | ("min", _) => {
            let Some(items) = host_elems(vm, args.first())? else {
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
            let Some(Contents::List(items)) = state::snapshot(vm, *r)? else {
                return Ok(None);
            };
            crate::shared_call::pin(vm, &items);
            let cmp = args.get(1).filter(|v| !matches!(v, Value::Null)).cloned();
            let sorted = crate::arrays::merge_sort(vm, items, cmp.as_ref())?;
            crate::jcollections::put_list(vm, *r, sorted)?;
            Ok(Some(Value::Null))
        }
        ("reverse", "(Ljava/util/List;)V") => {
            if let Some(Value::Obj(r)) = args.first() {
                if let Some(Contents::List(mut elems)) = state::snapshot(vm, *r)? {
                    elems.reverse();
                    crate::jcollections::put_list(vm, *r, elems)?;
                    return Ok(Some(Value::Null));
                }
            }
            Ok(None)
        }
        ("singletonMap", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;") => {
            let e = vec![(args[0].clone(), args[1].clone())];
            let r = state::create(vm, state::MAP, Contents::Map(e))?;
            Ok(Some(Value::Obj(r)))
        }
        ("emptyIterator", "()Ljava/util/Iterator;") => {
            let r = state::create(
                vm,
                "Ljava/util/Iterator;",
                Contents::Iterator {
                    elems: Vec::new(),
                    pos: 0,
                },
            )?;
            Ok(Some(Value::Obj(r)))
        }
        ("list", "(Ljava/util/Enumeration;)Ljava/util/ArrayList;") => {
            let Some(items) = host_elems(vm, args.first())? else {
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
            if let Some(Contents::List(elems) | Contents::Set(elems)) = state::snapshot(vm, *c)? {
                heap_snapshot = elems;
            } else {
                return Ok(None);
            }
            let mut out = heap_snapshot;
            for v in add {
                if is_set
                    && out
                        .iter()
                        .any(|x| crate::java_values::values_equal(&vm.heap, x, &v).unwrap_or(false))
                {
                    continue;
                }
                out.push(v);
                changed = true;
            }
            crate::jcollections::put_list(vm, *c, out)?;
            Ok(Some(Value::Int(changed as i32)))
        }
        _ => Ok(None),
    }
}

pub fn method_metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<crate::framework_host::HostMethodKind> {
    if class != state::CLASS {
        return None;
    }
    matches!(
        (name, sig),
        (
            "max" | "min",
            "(Ljava/util/Collection;)Ljava/lang/Object;"
                | "(Ljava/util/Collection;Ljava/util/Comparator;)Ljava/lang/Object;"
        ) | (
            "sort",
            "(Ljava/util/List;)V" | "(Ljava/util/List;Ljava/util/Comparator;)V"
        ) | ("reverse", "(Ljava/util/List;)V")
            | (
                "singletonMap",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;"
            )
            | ("emptyIterator", "()Ljava/util/Iterator;")
            | ("list", "(Ljava/util/Enumeration;)Ljava/util/ArrayList;")
            | ("addAll", "(Ljava/util/Collection;[Ljava/lang/Object;)Z")
    )
    .then_some(crate::framework_host::HostMethodKind::Static)
}

pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if method_metadata(class, name, sig).is_none() {
        return Ok(None);
    }
    let expected = match name {
        "emptyIterator" => 0,
        "singletonMap" | "addAll" => 2,
        _ if sig.contains("Comparator;") => 2,
        _ => 1,
    };
    if args.len() != expected {
        return Err(
            crate::err::vm_error("INVALID_ARG", "Collections utility argument count").into(),
        );
    }
    crate::shared_call::with_values(vm, args, |vm| {
        if matches!(name, "sort" | "reverse" | "addAll") {
            let Some(Value::Obj(owner)) = args.first() else {
                return Err(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "collection is null",
                )
                .into());
            };
            if state::rejects_mutation(vm.heap.class_of(*owner)?, "clear", "()V") {
                return Err(Throwable::new(
                    "Ljava/lang/UnsupportedOperationException;",
                    "immutable collection utility",
                )
                .into());
            }
            return crate::jcollections::with_product_call(vm, *owner, args, |vm| {
                static_intrinsic(vm, class, name, sig, args)
            });
        }
        static_intrinsic(vm, class, name, sig, args)
    })
}
