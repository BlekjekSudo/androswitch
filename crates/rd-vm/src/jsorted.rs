//! `java.util.TreeMap` / `TreeSet` (+ `SortedMap`/`NavigableMap`/
//! `SortedSet`/`NavigableSet` calls on them).
//!
//! State reuses the host collection shapes, kept sorted:
//! - TreeMap → `HostObj::Map { entries }`;
//! - TreeSet → `HostObj::List { elems }`.
//!
//! Ordering is the comparator stored in the `rd_cmp` field, or natural
//! ordering (`arrays::compare`: boxes, strings, `Comparable.compareTo`).
//! Lookups use binary search, so each lookup makes O(log n) guest `compare`
//! calls.
//!
//! **Views are snapshots.** `keySet`/`values`/`entrySet`/`headMap`/`tailMap`/
//! `subMap`/`descending*` return copies, not live views. This is a documented
//! deviation, like `subList`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

const TREE_MAP: &str = "Ljava/util/TreeMap;";
const TREE_SET: &str = "Ljava/util/TreeSet;";

fn exc(c: &str, m: &str) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.to_string()))
}
fn cmp_of(vm: &Engine, r: ObjRef) -> Option<Value> {
    match vm.heap.get_field(r, "rd_cmp") {
        Ok(v @ Value::Obj(_)) => Some(v),
        _ => None,
    }
}
fn keys(vm: &Engine, r: ObjRef, map: bool) -> Vec<Value> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::Map { entries }) if map => entries.iter().map(|e| e.0.clone()).collect(),
        Some(HostObj::List { elems }) if !map => elems.clone(),
        _ => Vec::new(),
    }
}
/// (found, index): binary search of `k` in the sorted keys.
fn search(vm: &mut Engine, r: ObjRef, map: bool, k: &Value) -> Result<(bool, usize), VmExit> {
    let cmp = cmp_of(vm, r);
    let ks = keys(vm, r, map);
    if cmp.is_none() && matches!(k, Value::Null) {
        return Err(exc("Ljava/lang/NullPointerException;", "TreeMap/TreeSet null key"));
    }
    let (mut lo, mut hi) = (0usize, ks.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let c = crate::arrays::compare(vm, cmp.as_ref(), &ks[mid], k)?;
        if c == 0 {
            return Ok((true, mid));
        }
        if c < 0 { lo = mid + 1 } else { hi = mid }
    }
    Ok((false, lo))
}
fn entries(vm: &Engine, r: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    }
}
fn entry(vm: &mut Engine, k: Value, v: Value) -> Result<Value, VmExit> {
    let en = vm.heap.alloc_instance("Ljava/util/Map$Entry;".to_string(), vec![])?;
    crate::err::put_field_result(vm.heap.put_field(en, "entry_key", k))?;
    crate::err::put_field_result(vm.heap.put_field(en, "entry_value", v))?;
    Ok(Value::Obj(en))
}
fn new_tree(vm: &mut Engine, class: &str, cmp: Option<Value>, st: HostObj) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(class.to_string(), vec![])?;
    if let Some(c) = cmp {
        crate::err::put_field_result(vm.heap.put_field(r, "rd_cmp", c))?;
    }
    vm.fw.objects.insert(r, st);
    Ok(Value::Obj(r))
}
fn iter(vm: &mut Engine, elems: Vec<Value>) -> Result<Value, VmExit> {
    let it = vm.heap.alloc_instance("Ljava/util/Iterator;".to_string(), vec![])?;
    vm.fw.objects.insert(it, HostObj::Iter { elems, pos: 0 });
    Ok(Value::Obj(it))
}

/// Navigation: index of floor/ceiling/lower/higher for key `k`.
fn nav(vm: &mut Engine, r: ObjRef, map: bool, k: &Value, kind: &str) -> Result<Option<usize>, VmExit> {
    let (found, i) = search(vm, r, map, k)?;
    let n = keys(vm, r, map).len();
    Ok(match kind {
        "floor" => if found { Some(i) } else { i.checked_sub(1) },
        "lower" => i.checked_sub(1),
        "ceiling" => if i < n { Some(i) } else { None },
        _ /* higher */ => {
            let j = if found { i + 1 } else { i };
            if j < n { Some(j) } else { None }
        }
    })
}

pub(crate) fn instance_intrinsic(vm: &mut Engine, recv_class: &str, mref_class: &str, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let is_map = match (mref_class, recv_class) {
        (TREE_MAP, _) | (_, TREE_MAP) => true,
        (TREE_SET, _) | (_, TREE_SET) => false,
        // DEX subclasses (Compose's DepthSortedSet extends TreeSet, …): the
        // host-state check keeps the subtype walk off the hot path
        // (the method ref may name the subclass or an inherited JDK
        // declarer such as AbstractCollection)
        _ if !recv_class.starts_with("Ljava/")
            && matches!(vm.fw.objects.get(&recv), Some(HostObj::Map { .. } | HostObj::List { .. })) =>
        {
            if vm.cp.is_subtype(recv_class, TREE_SET) {
                false
            } else if vm.cp.is_subtype(recv_class, TREE_MAP) {
                true
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };
    let a0 = args.first().cloned().unwrap_or(Value::Null);
    if name == "<init>" {
        let mut cmp = None;
        let mut init: Vec<Value> = Vec::new();
        let mut init_map: Vec<(Value, Value)> = Vec::new();
        if sig.contains("Comparator") {
            cmp = Some(a0.clone()).filter(|v| matches!(v, Value::Obj(_)));
        } else if let Value::Obj(src) = a0 {
            if is_map {
                init_map = match vm.fw.objects.get(&src) {
                    Some(HostObj::Map { entries }) => entries.clone(),
                    _ => Vec::new(),
                };
                if sig.contains("SortedMap") {
                    cmp = cmp_of(vm, src);
                }
            } else {
                init = crate::jcollections::elems_of(vm, &a0)?;
                if sig.contains("SortedSet") {
                    cmp = cmp_of(vm, src);
                }
            }
        }
        if let Some(c) = cmp {
            crate::err::put_field_result(vm.heap.put_field(recv, "rd_cmp", c))?;
        }
        vm.fw.objects.insert(recv, if is_map { HostObj::Map { entries: Vec::new() } } else { HostObj::List { elems: Vec::new() } });
        for (k, v) in init_map {
            put(vm, recv, k, v)?;
        }
        for e in init {
            add(vm, recv, e)?;
        }
        return Ok(Some(Value::Null));
    }
    if !matches!(vm.fw.objects.get(&recv), Some(HostObj::Map { .. } | HostObj::List { .. })) {
        vm.fw.objects.insert(recv, if is_map { HostObj::Map { entries: Vec::new() } } else { HostObj::List { elems: Vec::new() } });
    }
    let ks = keys(vm, recv, is_map);
    let n = ks.len();
    let nse = || exc("Ljava/util/NoSuchElementException;", "empty");
    Ok(Some(match name {
        "size" => Value::Int(n as i32),
        "isEmpty" => Value::Int((n == 0) as i32),
        "clear" => {
            vm.fw.objects.insert(recv, if is_map { HostObj::Map { entries: Vec::new() } } else { HostObj::List { elems: Vec::new() } });
            Value::Null
        }
        "comparator" => cmp_of(vm, recv).unwrap_or(Value::Null),
        "contains" | "containsKey" => Value::Int(search(vm, recv, is_map, &a0)?.0 as i32),
        "iterator" | "descendingIterator" => {
            let mut e = ks;
            if name == "descendingIterator" {
                e.reverse();
            }
            iter(vm, e)?
        }
        "toArray" => crate::arrays::new_array(vm, "[Ljava/lang/Object;", crate::heap::ElemKind::Obj, ks)?,
        // ── set ──
        "add" if !is_map => Value::Int(add(vm, recv, a0)? as i32),
        "addAll" if !is_map => {
            let mut changed = false;
            for e in crate::jcollections::elems_of(vm, &a0)? {
                changed |= add(vm, recv, e)?;
            }
            Value::Int(changed as i32)
        }
        "remove" if !is_map => {
            let (found, i) = search(vm, recv, false, &a0)?;
            if found {
                if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&recv) {
                    elems.remove(i);
                }
            }
            Value::Int(found as i32)
        }
        "first" | "last" if !is_map => {
            if n == 0 {
                return Err(nse());
            }
            ks[if name == "first" { 0 } else { n - 1 }].clone()
        }
        "pollFirst" | "pollLast" if !is_map => {
            if n == 0 {
                Value::Null
            } else {
                let i = if name == "pollFirst" { 0 } else { n - 1 };
                if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&recv) {
                    elems.remove(i)
                } else {
                    Value::Null
                }
            }
        }
        "floor" | "ceiling" | "lower" | "higher" if !is_map => match nav(vm, recv, false, &a0, name)? {
            Some(i) => ks[i].clone(),
            None => Value::Null,
        },
        // ── map ──
        "put" => put(vm, recv, a0, args.get(1).cloned().unwrap_or(Value::Null))?,
        "get" | "getOrDefault" => {
            let (found, i) = search(vm, recv, true, &a0)?;
            if found { entries(vm, recv)[i].1.clone() } else { args.get(1).cloned().unwrap_or(Value::Null) }
        }
        "remove" => {
            let (found, i) = search(vm, recv, true, &a0)?;
            if found {
                match vm.fw.objects.get_mut(&recv) {
                    Some(HostObj::Map { entries }) => entries.remove(i).1,
                    _ => Value::Null,
                }
            } else {
                Value::Null
            }
        }
        "putAll" => {
            let src = match &a0 {
                Value::Obj(s) => match vm.fw.objects.get(s) {
                    Some(HostObj::Map { entries }) => entries.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            for (k, v) in src {
                put(vm, recv, k, v)?;
            }
            Value::Null
        }
        "containsValue" => {
            let mut hit = false;
            for (_, v) in entries(vm, recv) {
                if crate::arrays::obj_equals(vm, &v, &a0)? {
                    hit = true;
                    break;
                }
            }
            Value::Int(hit as i32)
        }
        "keySet" | "navigableKeySet" | "descendingKeySet" => {
            let mut e = ks;
            if name == "descendingKeySet" {
                e.reverse();
            }
            crate::collections::new_list(vm, "Ljava/util/LinkedHashSet;", e)?
        }
        "values" => {
            let v = entries(vm, recv).into_iter().map(|e| e.1).collect();
            crate::collections::new_list(vm, "Ljava/util/ArrayList;", v)?
        }
        "entrySet" => {
            let mut v = Vec::new();
            for (k, val) in entries(vm, recv) {
                v.push(entry(vm, k, val)?);
            }
            crate::collections::new_list(vm, "Ljava/util/LinkedHashSet;", v)?
        }
        "firstKey" | "lastKey" => {
            if n == 0 {
                return Err(nse());
            }
            ks[if name == "firstKey" { 0 } else { n - 1 }].clone()
        }
        "firstEntry" | "lastEntry" | "pollFirstEntry" | "pollLastEntry" => {
            if n == 0 {
                Value::Null
            } else {
                let i = if name.contains("First") { 0 } else { n - 1 };
                let (k, v) = entries(vm, recv)[i].clone();
                if name.starts_with("poll") {
                    if let Some(HostObj::Map { entries }) = vm.fw.objects.get_mut(&recv) {
                        entries.remove(i);
                    }
                }
                entry(vm, k, v)?
            }
        }
        "floorEntry" | "ceilingEntry" | "lowerEntry" | "higherEntry" | "floorKey" | "ceilingKey" | "lowerKey" | "higherKey" => {
            let kind = name.trim_end_matches("Entry").trim_end_matches("Key");
            match nav(vm, recv, true, &a0, kind)? {
                Some(i) => {
                    let (k, v) = entries(vm, recv)[i].clone();
                    if name.ends_with("Key") { k } else { entry(vm, k, v)? }
                }
                None => Value::Null,
            }
        }
        "headMap" | "tailMap" | "subMap" | "descendingMap" | "headSet" | "tailSet" | "subSet" | "descendingSet" => {
            let all = entries(vm, recv);
            let elems = ks.clone();
            let incl = |args: &[Value], i: usize, d: bool| match args.get(i) {
                Some(Value::Int(b)) => *b != 0,
                _ => d,
            };
            let (lo, hi): (usize, usize) = match name {
                "headMap" | "headSet" => {
                    let (f, i) = search(vm, recv, is_map, &a0)?;
                    (0, if f && incl(args, 1, false) { i + 1 } else { i })
                }
                "tailMap" | "tailSet" => {
                    let (f, i) = search(vm, recv, is_map, &a0)?;
                    (if f && !incl(args, 1, true) { i + 1 } else { i }, n)
                }
                "subMap" | "subSet" => {
                    let four = args.len() == 4;
                    let (f0, i0) = search(vm, recv, is_map, &a0)?;
                    let hi_key = args.get(if four { 2 } else { 1 }).cloned().unwrap_or(Value::Null);
                    let (f1, i1) = search(vm, recv, is_map, &hi_key)?;
                    let lo = if f0 && four && !incl(args, 1, true) { i0 + 1 } else { i0 };
                    let hi = if f1 && four && incl(args, 3, false) { i1 + 1 } else { i1 };
                    (lo, hi.max(lo))
                }
                _ => (0, n),
            };
            let cmp = cmp_of(vm, recv);
            if name.starts_with("descending") {
                if is_map {
                    let mut e = all;
                    e.reverse();
                    let r = vm.heap.alloc_instance("Ljava/util/LinkedHashMap;".to_string(), vec![])?;
                    vm.fw.objects.insert(r, HostObj::Map { entries: e });
                    Value::Obj(r)
                } else {
                    let mut e = elems;
                    e.reverse();
                    crate::collections::new_list(vm, "Ljava/util/LinkedHashSet;", e)?
                }
            } else if is_map {
                new_tree(vm, TREE_MAP, cmp, HostObj::Map { entries: all[lo..hi].to_vec() })?
            } else {
                new_tree(vm, TREE_SET, cmp, HostObj::List { elems: elems[lo..hi].to_vec() })?
            }
        }
        "equals" => Value::Int((Some(recv) == match a0 { Value::Obj(o) => Some(o), _ => None }) as i32),
        "hashCode" => Value::Int(recv as i32),
        _ => return Ok(None),
    }))
}

fn add(vm: &mut Engine, r: ObjRef, e: Value) -> Result<bool, VmExit> {
    let (found, i) = search(vm, r, false, &e)?;
    if found {
        return Ok(false);
    }
    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&r) {
        elems.insert(i, e);
    }
    Ok(true)
}

fn put(vm: &mut Engine, r: ObjRef, k: Value, v: Value) -> Result<Value, VmExit> {
    let (found, i) = search(vm, r, true, &k)?;
    match vm.fw.objects.get_mut(&r) {
        Some(HostObj::Map { entries }) => {
            if found {
                Ok(std::mem::replace(&mut entries[i].1, v))
            } else {
                entries.insert(i, (k, v));
                Ok(Value::Null)
            }
        }
        _ => Ok(Value::Null),
    }
}
