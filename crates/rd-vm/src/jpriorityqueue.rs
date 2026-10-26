//! `java.util.PriorityQueue` over the host list state (`HostObj::List`).
//!
//! The elements are kept sorted, head at index 0. Inserts are stable: an
//! element goes after the ones that compare equal. `poll`/`peek` therefore
//! return the least element, as the JDK binary heap does; the JDK leaves
//! iteration order unspecified. Ordering uses the queue's `Comparator`
//! (`compare(a, b)`) or the elements' natural ordering (`compareTo`), both
//! through virtual dispatch, so app comparators and host boxes/strings work.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

pub(crate) const PQ: &str = "Ljava/util/PriorityQueue;";

fn cmp_key(r: ObjRef) -> (ObjRef, String) {
    (r, "pq:cmp".to_string())
}

fn elems(vm: &mut Engine, r: ObjRef) -> &mut Vec<Value> {
    if !matches!(vm.fw.objects.get(&r), Some(HostObj::List { .. })) {
        vm.fw.objects.insert(r, HostObj::List { elems: Vec::new() });
    }
    match vm.fw.objects.get_mut(&r) {
        Some(HostObj::List { elems }) => elems,
        _ => unreachable!("list state inserted above"),
    }
}

fn compare(vm: &mut Engine, cmp: &Value, a: &Value, b: &Value) -> Result<i32, VmExit> {
    let r = match cmp {
        Value::Obj(c) => vm.invoke_instance(
            *c,
            "compare",
            "(Ljava/lang/Object;Ljava/lang/Object;)I",
            &[a.clone(), b.clone()],
        )?,
        _ => match a {
            Value::Obj(x) => vm.invoke_instance(
                *x,
                "compareTo",
                "(Ljava/lang/Object;)I",
                core::slice::from_ref(b),
            )?,
            _ => {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ClassCastException;",
                    "PriorityQueue element is not Comparable",
                )))
            }
        },
    };
    Ok(r.as_int()?)
}

fn insert(vm: &mut Engine, r: ObjRef, v: Value) -> Result<(), VmExit> {
    if matches!(v, Value::Null) {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "PriorityQueue does not permit null elements",
        )));
    }
    let cmp = vm
        .view_props
        .get(&cmp_key(r))
        .cloned()
        .unwrap_or(Value::Null);
    let cur = elems(vm, r).clone();
    // upper bound: first index whose element is > v (stable)
    let (mut lo, mut hi) = (0usize, cur.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if compare(vm, &cmp, &cur[mid], &v)? <= 0 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    elems(vm, r).insert(lo, v);
    Ok(())
}

fn nse() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/util/NoSuchElementException;",
        "PriorityQueue is empty",
    ))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_pq = recv_class == PQ || vm.view_props.contains_key(&cmp_key(recv));
    if !is_pq {
        return Ok(None);
    }
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let v = match (name, sig) {
        ("<init>", _) => {
            elems(vm, recv).clear();
            let mut cmp = Value::Null;
            let mut src = None;
            match sig {
                "()V" | "(I)V" => {}
                "(Ljava/util/Comparator;)V" => cmp = arg(0),
                "(ILjava/util/Comparator;)V" => cmp = arg(1),
                _ => {
                    // (Collection) / (PriorityQueue) / (SortedSet): copy,
                    // keeping a source PriorityQueue's comparator
                    if let Value::Obj(o) = arg(0) {
                        if let Some(c) = vm.view_props.get(&cmp_key(o)).cloned() {
                            cmp = c;
                        }
                        src = Some(arg(0));
                    }
                }
            }
            vm.view_props.insert(cmp_key(recv), cmp);
            if let Some(s) = src {
                for e in crate::jcollections::elems_of(vm, &s)? {
                    insert(vm, recv, e)?;
                }
            }
            Value::Null
        }
        ("add", "(Ljava/lang/Object;)Z") | ("offer", "(Ljava/lang/Object;)Z") => {
            insert(vm, recv, arg(0))?;
            Value::Int(1)
        }
        ("addAll", "(Ljava/util/Collection;)Z") => {
            let src = crate::jcollections::elems_of(vm, &arg(0))?;
            let changed = !src.is_empty();
            for e in src {
                insert(vm, recv, e)?;
            }
            Value::Int(changed as i32)
        }
        ("poll", "()Ljava/lang/Object;") => {
            let e = elems(vm, recv);
            if e.is_empty() {
                Value::Null
            } else {
                e.remove(0)
            }
        }
        ("remove", "()Ljava/lang/Object;") => {
            let e = elems(vm, recv);
            if e.is_empty() {
                return Err(nse());
            }
            e.remove(0)
        }
        ("peek", "()Ljava/lang/Object;") => elems(vm, recv).first().cloned().unwrap_or(Value::Null),
        ("element", "()Ljava/lang/Object;") => match elems(vm, recv).first().cloned() {
            Some(v) => v,
            None => return Err(nse()),
        },
        ("size", "()I") => Value::Int(elems(vm, recv).len() as i32),
        ("isEmpty", "()Z") => Value::Int(elems(vm, recv).is_empty() as i32),
        ("clear", "()V") => {
            elems(vm, recv).clear();
            Value::Null
        }
        ("comparator", "()Ljava/util/Comparator;") => vm
            .view_props
            .get(&cmp_key(recv))
            .cloned()
            .unwrap_or(Value::Null),
        ("contains", "(Ljava/lang/Object;)Z") | ("remove", "(Ljava/lang/Object;)Z") => {
            let x = arg(0);
            let cur = elems(vm, recv).clone();
            let mut found = None;
            for (i, e) in cur.iter().enumerate() {
                if crate::arrays::obj_equals(vm, &x, e)? {
                    found = Some(i);
                    break;
                }
            }
            if name == "remove" {
                if let Some(i) = found {
                    elems(vm, recv).remove(i);
                }
            }
            Value::Int(found.is_some() as i32)
        }
        ("iterator", "()Ljava/util/Iterator;") => {
            let snapshot = elems(vm, recv).clone();
            let it = vm
                .heap
                .alloc_instance("Ljava/util/Iterator;".to_string(), Vec::new())?;
            vm.fw.objects.insert(
                it,
                HostObj::Iter {
                    elems: snapshot,
                    pos: 0,
                },
            );
            Value::Obj(it)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
