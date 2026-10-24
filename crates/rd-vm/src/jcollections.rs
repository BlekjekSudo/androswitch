//! `java.util` collection bulk operations, plus the `AbstractCollection` /
//! `AbstractList` template methods for app (DEX) subclasses.
//!
//! Two receiver shapes are served:
//!
//! 1. **Host collections**, i.e. receivers with `HostObj::List` state
//!    (ArrayList, LinkedList, HashSet, LinkedHashSet, …). This module adds:
//!    - `addAll(Collection)`, `addAll(int, Collection)`;
//!    - `removeAll`, `retainAll`, `containsAll`;
//!    - `toArray()`, `toArray(T[])`, `subList`, `lastIndexOf`;
//!    - `listIterator()`, `listIterator(int)`;
//!    - the `<init>(Collection)` constructor when the source is an app
//!      collection.
//!
//!    Element equality uses guest `equals`. Set receivers deduplicate.
//! 2. **DEX subclasses of `AbstractCollection`/`AbstractList`/`AbstractSet`**
//!    that inherit the JDK template methods. These are implemented on top of
//!    the subclass's own `size()`/`get(int)`/`iterator()`/`add`/`remove`, as
//!    in the JDK: `isEmpty`, `contains`, `containsAll`, `toArray`, `indexOf`,
//!    `lastIndexOf`, `iterator`/`listIterator` (AbstractList), `addAll`,
//!    `removeAll`, `retainAll`, `clear`, `equals`, `hashCode`, `toString`.
//!
//! Any argument collection can be read: a host one directly, an app one
//! through its own `toArray()`, `iterator()` or `size()`/`get(int)`.
//! Previously an app collection passed to a host collection raised
//! `VM_TYPE_ERROR`.
//!
//! **Deviations (documented).**
//! - `subList` returns a copy, not a live view.
//! - Host `ListIterator`s iterate a snapshot; `add`/`set` through
//!   them throw `UnsupportedOperationException` rather than silently
//!   diverging.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const ACC_ABSTRACT: u32 = 0x0400;

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.into()))
}

/// Call a concrete DEX implementation of `name+sig` on `recv`, if one exists.
/// Args are logical; none of the signatures used here take wide values.
pub(crate) fn guest_call(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let class = vm.heap.class_of(recv)?.to_string();
    let Some((d, def, m)) = vm.cp.resolve_method(&class, name, sig) else {
        return Ok(None);
    };
    if m.access_flags & ACC_ABSTRACT != 0 {
        return Ok(None);
    }
    let mut a = Vec::with_capacity(args.len() + 1);
    a.push(Value::Obj(recv));
    a.extend_from_slice(args);
    Ok(Some(vm.call(d, def, &m, a)?))
}

fn host_list(vm: &Engine, r: ObjRef) -> Option<Vec<Value>> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::List { elems }) => Some(elems.clone()),
        Some(HostObj::Iter { elems, pos }) => Some(elems[(*pos).min(elems.len())..].to_vec()),
        _ => None,
    }
}

fn is_host_state(vm: &Engine, r: ObjRef) -> bool {
    vm.fw.objects.contains_key(&r)
}

/// Elements of any Java `Collection`/`Iterable`: host or app-defined.
pub(crate) fn elems_of(vm: &mut Engine, v: &Value) -> Result<Vec<Value>, VmExit> {
    let r = match v {
        Value::Obj(r) => *r,
        _ => {
            return Err(exc(
                "Ljava/lang/NullPointerException;",
                "collection is null",
            ))
        }
    };
    if let Some(e) = host_list(vm, r) {
        return Ok(e);
    }
    if vm.heap.as_array_elems(r).is_ok() {
        return Ok(vm.heap.as_array_elems(r)?.0.to_vec());
    }
    if let Some(Value::Obj(a)) = guest_call(vm, r, "toArray", "()[Ljava/lang/Object;", &[])? {
        return Ok(vm.heap.as_array_elems(a)?.0.to_vec());
    }
    if let Some(it) = guest_call(vm, r, "iterator", "()Ljava/util/Iterator;", &[])? {
        return drain_iterator(vm, it);
    }
    // AbstractList contract: size() + get(int)
    if let Some(n) = guest_call(vm, r, "size", "()I", &[])? {
        let n = n.as_int()?;
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            match guest_call(vm, r, "get", "(I)Ljava/lang/Object;", &[Value::Int(i)])? {
                Some(e) => out.push(e),
                None => break,
            }
        }
        return Ok(out);
    }
    let c = vm.heap.class_of(r)?.to_string();
    Err(crate::err::vm_error(
        "VM_TYPE_ERROR",
        format!("{c}: collection is not iterable by the host"),
    )
    .into())
}

fn drain_iterator(vm: &mut Engine, it: Value) -> Result<Vec<Value>, VmExit> {
    let Value::Obj(it) = it else {
        return Ok(Vec::new());
    };
    if let Some(e) = host_list(vm, it) {
        return Ok(e);
    }
    let mut out = Vec::new();
    loop {
        let has = guest_call(vm, it, "hasNext", "()Z", &[])?.map(|v| v.as_int().unwrap_or(0) != 0);
        match has {
            Some(true) => match guest_call(vm, it, "next", "()Ljava/lang/Object;", &[])? {
                Some(e) => out.push(e),
                None => break,
            },
            _ => break,
        }
        if out.len() > 10_000_000 {
            return Err(exc(
                "Ljava/lang/OutOfMemoryError;",
                "iterator does not terminate",
            ));
        }
    }
    Ok(out)
}

fn contains(vm: &mut Engine, items: &[Value], x: &Value) -> Result<bool, VmExit> {
    for e in items {
        if crate::arrays::obj_equals(vm, x, e)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_set(vm: &Engine, r: ObjRef) -> bool {
    vm.heap
        .class_of(r)
        .map(|c| c.contains("Set"))
        .unwrap_or(false)
}

/// Most recent `subList` views tracked for write-through (older views fall
/// back to snapshot semantics).
const MAX_SUBLIST_VIEWS: usize = 1024;

fn register_sublist(vm: &mut Engine, view: &Value, parent: ObjRef, offset: usize, len: usize) {
    let Value::Obj(v) = view else { return };
    vm.fw.sublists.insert(*v, (parent, offset, len));
    vm.fw.sublist_order.push_back(*v);
    while vm.fw.sublist_order.len() > MAX_SUBLIST_VIEWS {
        if let Some(old) = vm.fw.sublist_order.pop_front() {
            vm.fw.sublists.remove(&old);
        }
    }
}

/// `List.subList` is a *view* (JDK): before an operation on it, re-read
/// its window of the parent so parent writes are visible through the view.
pub(crate) fn sublist_refresh(vm: &mut Engine, view: ObjRef) {
    let Some(&(parent, off, len)) = vm.fw.sublists.get(&view) else {
        return;
    };
    sublist_refresh(vm, parent);
    let window = match vm.fw.objects.get(&parent) {
        Some(HostObj::List { elems }) if off + len <= elems.len() => elems[off..off + len].to_vec(),
        _ => return,
    };
    if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&view) {
        *elems = window;
    }
}

/// After an operation on a `subList` view, write its contents back into
/// the parent's window (and up the chain for nested views), so idioms such
/// as `list.subList(a, b).clear()`, `Collections.sort(list.subList(..))`
/// or `subList.set(i, x)` modify the backing list as on the JDK.
pub(crate) fn sublist_write_back(vm: &mut Engine, view: ObjRef) {
    let mut cur = view;
    while let Some(&(parent, off, len)) = vm.fw.sublists.get(&cur) {
        let now = match vm.fw.objects.get(&cur) {
            Some(HostObj::List { elems }) => elems.clone(),
            _ => return,
        };
        let new_len = now.len();
        match vm.fw.objects.get_mut(&parent) {
            Some(HostObj::List { elems }) if off + len <= elems.len() => {
                if new_len == len && elems[off..off + len] == now[..] {
                    return;
                }
                elems.splice(off..off + len, now);
            }
            _ => return,
        }
        vm.fw.sublists.insert(cur, (parent, off, new_len));
        cur = parent;
    }
}

fn new_host(vm: &mut Engine, class: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}

fn new_iter(
    vm: &mut Engine,
    owner: ObjRef,
    elems: Vec<Value>,
    pos: usize,
) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Ljava/util/ListIterator;".to_string(), Vec::new())?;
    vm.fw.objects.insert(r, HostObj::Iter { elems, pos });
    crate::jiterator::bind(vm, r, owner)?;
    Ok(Value::Obj(r))
}

pub(crate) fn to_array(
    vm: &mut Engine,
    items: Vec<Value>,
    given: Option<&Value>,
) -> Result<Value, VmExit> {
    match given {
        Some(Value::Obj(a)) => {
            let (cur, _, desc) = crate::arrays::elems(vm, *a)?;
            if cur.len() >= items.len() {
                let n = items.len();
                let dst = vm
                    .heap
                    .array_elems_mut(*a)
                    .map_err(|e| crate::err::vm_error("INVALID_FORMAT", e))?;
                for (i, v) in items.into_iter().enumerate() {
                    dst[i] = v;
                }
                if dst.len() > n {
                    dst[n] = Value::Null;
                }
                Ok(Value::Obj(*a))
            } else {
                crate::arrays::new_array(vm, &desc, ElemKind::Obj, items)
            }
        }
        Some(_) => Err(exc("Ljava/lang/NullPointerException;", "toArray(null)")),
        None => crate::arrays::new_array(vm, "[Ljava/lang/Object;", ElemKind::Obj, items),
    }
}

fn range(len: usize, from: i32, to: i32) -> Result<(usize, usize), VmExit> {
    if from < 0 || to as i64 > len as i64 || from > to {
        return Err(exc(
            "Ljava/lang/IndexOutOfBoundsException;",
            format!("fromIndex: {from}, toIndex: {to}, size: {len}"),
        ));
    }
    Ok((from as usize, to as usize))
}

fn to_string(vm: &mut Engine, me: ObjRef, items: &[Value]) -> Result<String, VmExit> {
    let mut s = String::from("[");
    for (i, e) in items.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        match e {
            Value::Obj(r) if *r == me => s.push_str("(this Collection)"),
            Value::Obj(r) => s.push_str(&crate::intrinsics::object_to_string(vm, *r)?),
            Value::Null => s.push_str("null"),
            other => s.push_str(&format!("{other:?}")),
        }
    }
    s.push(']');
    Ok(s)
}

const COLLECTION_TYPES: &[&str] = &[
    "Ljava/lang/Iterable;",
    "Ljava/util/Collection;",
    "Ljava/util/List;",
    "Ljava/util/Set;",
    "Ljava/util/ArrayList;",
    "Ljava/util/LinkedList;",
    "Ljava/util/HashSet;",
    "Ljava/util/LinkedHashSet;",
    "Ljava/util/AbstractCollection;",
    "Ljava/util/AbstractList;",
    "Ljava/util/AbstractSet;",
    "Ljava/util/AbstractSequentialList;",
    "Ljava/util/concurrent/CopyOnWriteArrayList;",
    "Ljava/util/concurrent/CopyOnWriteArraySet;",
];

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // ListIterator navigation over host iterator state
    if matches!(
        mref_class,
        "Ljava/util/ListIterator;" | "Ljava/util/Iterator;"
    ) {
        return crate::jiterator::navigate(vm, recv, name);
    }

    if !COLLECTION_TYPES.contains(&mref_class) {
        return Ok(None);
    }
    // `android.util.ArraySet` also stores its elements in HostObj::List,
    // but in signed Java-hash order. Its `addAll` must therefore defer to
    // android_util instead of appending through this insertion-order path,
    // including when named through Collection/Set and inherited by a DEX
    // subclass that does not override it.
    if name == "addAll" && sig == "(Ljava/util/Collection;)Z" {
        let recv_class = vm.heap.class_of(recv)?.to_string();
        const ARRAY_SET: &str = "Landroid/util/ArraySet;";
        if recv_class == ARRAY_SET
            || (vm.cp.find_class(&recv_class).is_some()
                && vm.cp.is_subtype(&recv_class, ARRAY_SET)
                && vm.cp.resolve_method(&recv_class, name, sig).is_none())
        {
            return Ok(None);
        }
    }
    let host = match vm.fw.objects.get(&recv) {
        Some(HostObj::List { elems }) => Some(elems.clone()),
        None if name == "<init>"
            && sig == "(Ljava/util/Collection;)V"
            && !mref_class.contains("Abstract")
            && !matches!(
                mref_class,
                "Ljava/util/Collection;" | "Ljava/util/List;" | "Ljava/util/Set;"
            ) =>
        {
            Some(Vec::new())
        }
        _ => None,
    };
    if let Some(mut items) = host {
        let set = is_set(vm, recv);
        let r = match (name, sig) {
            ("<init>", "(Ljava/util/Collection;)V") => {
                let src = elems_of(vm, args.first().unwrap_or(&Value::Null))?;
                let mut out: Vec<Value> = Vec::new();
                for e in src {
                    if set && contains(vm, &out, &e)? {
                        continue;
                    }
                    out.push(e);
                }
                vm.fw.objects.insert(recv, HostObj::List { elems: out });
                return Ok(Some(Value::Null));
            }
            ("addIfAbsent", "(Ljava/lang/Object;)Z") => {
                let value = args.first().cloned().unwrap_or(Value::Null);
                if contains(vm, &items, &value)? {
                    Value::Int(0)
                } else {
                    items.push(value);
                    Value::Int(1)
                }
            }
            ("addAllAbsent", "(Ljava/util/Collection;)I") => {
                let source = elems_of(vm, args.first().unwrap_or(&Value::Null))?;
                let mut added = 0;
                for value in source {
                    if !contains(vm, &items, &value)? {
                        items.push(value);
                        added += 1;
                    }
                }
                Value::Int(added)
            }
            ("addAll", "(Ljava/util/Collection;)Z") | ("addAll", "(ILjava/util/Collection;)Z") => {
                let (at, src_v) = if sig.starts_with("(I") {
                    let i = args[0].as_int()?;
                    if i < 0 || i as usize > items.len() {
                        return Err(exc(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("Index: {i}, Size: {}", items.len()),
                        ));
                    }
                    (i as usize, &args[1])
                } else {
                    (items.len(), &args[0])
                };
                let src = elems_of(vm, src_v)?;
                let mut add = Vec::new();
                for e in src {
                    if set && (contains(vm, &items, &e)? || contains(vm, &add, &e)?) {
                        continue;
                    }
                    add.push(e);
                }
                let changed = !add.is_empty();
                items.splice(at..at, add);
                Value::Int(changed as i32)
            }
            ("removeAll", "(Ljava/util/Collection;)Z")
            | ("retainAll", "(Ljava/util/Collection;)Z") => {
                let other = elems_of(vm, &args[0])?;
                let keep_matching = name == "retainAll";
                let before = items.len();
                let mut kept = Vec::with_capacity(before);
                for e in items.drain(..) {
                    if contains(vm, &other, &e)? == keep_matching {
                        kept.push(e);
                    }
                }
                items = kept;
                Value::Int((items.len() != before) as i32)
            }
            ("containsAll", "(Ljava/util/Collection;)Z") => {
                let other = elems_of(vm, &args[0])?;
                let mut all = true;
                for e in &other {
                    if !contains(vm, &items, e)? {
                        all = false;
                        break;
                    }
                }
                return Ok(Some(Value::Int(all as i32)));
            }
            ("toArray", "()[Ljava/lang/Object;") => return Ok(Some(to_array(vm, items, None)?)),
            ("toArray", "([Ljava/lang/Object;)[Ljava/lang/Object;") => {
                return Ok(Some(to_array(vm, items, args.first())?))
            }
            ("subList", "(II)Ljava/util/List;") => {
                let (a, b) = range(items.len(), args[0].as_int()?, args[1].as_int()?)?;
                let view = new_host(vm, "Ljava/util/ArrayList;", items[a..b].to_vec())?;
                register_sublist(vm, &view, recv, a, b - a);
                return Ok(Some(view));
            }
            ("lastIndexOf", "(Ljava/lang/Object;)I") => {
                let x = args.first().cloned().unwrap_or(Value::Null);
                let mut found = -1;
                for (i, e) in items.iter().enumerate().rev() {
                    if crate::arrays::obj_equals(vm, &x, e)? {
                        found = i as i32;
                        break;
                    }
                }
                return Ok(Some(Value::Int(found)));
            }
            ("listIterator", "()Ljava/util/ListIterator;") => {
                return Ok(Some(new_iter(vm, recv, items, 0)?))
            }
            ("listIterator", "(I)Ljava/util/ListIterator;") => {
                let i = args[0].as_int()?;
                if i < 0 || i as usize > items.len() {
                    return Err(exc(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        format!("Index: {i}"),
                    ));
                }
                return Ok(Some(new_iter(vm, recv, items, i as usize)?));
            }
            _ => return Ok(None),
        };
        vm.fw.objects.insert(recv, HostObj::List { elems: items });
        return Ok(Some(r));
    }
    // ── DEX subclasses inheriting the JDK template methods ──────────────
    if is_host_state(vm, recv) {
        return Ok(None);
    }
    let rc = vm.heap.class_of(recv)?.to_string();
    if rc.starts_with("Ljava/") {
        return Ok(None);
    }
    let is_list = matches!(
        mref_class,
        "Ljava/util/AbstractList;" | "Ljava/util/List;" | "Ljava/util/AbstractSequentialList;"
    ) || crate::jlclass::assignable(vm, "Ljava/util/AbstractList;", &rc);
    let me = Value::Obj(recv);
    Ok(Some(match (name, sig) {
        ("<init>", "()V") if mref_class.starts_with("Ljava/util/Abstract") => Value::Null,
        ("isEmpty", "()Z") => match guest_call(vm, recv, "size", "()I", &[])? {
            Some(n) => Value::Int((n.as_int()? == 0) as i32),
            None => return Ok(None),
        },
        ("iterator", "()Ljava/util/Iterator;")
        | ("listIterator", "()Ljava/util/ListIterator;")
        | ("listIterator", "(I)Ljava/util/ListIterator;")
            if is_list =>
        {
            // AbstractList: iterate via size()/get(int)
            let n = match guest_call(vm, recv, "size", "()I", &[])? {
                Some(n) => n.as_int()?,
                None => return Ok(None),
            };
            let mut items = Vec::new();
            for i in 0..n {
                match guest_call(vm, recv, "get", "(I)Ljava/lang/Object;", &[Value::Int(i)])? {
                    Some(e) => items.push(e),
                    None => return Ok(None),
                }
            }
            let start = if sig == "(I)Ljava/util/ListIterator;" {
                args[0].as_int()?.max(0) as usize
            } else {
                0
            };
            new_iter(vm, recv, items, start.min(n.max(0) as usize))?
        }
        ("contains", "(Ljava/lang/Object;)Z") => {
            let items = elems_of(vm, &me)?;
            Value::Int(contains(vm, &items, &args[0])? as i32)
        }
        ("containsAll", "(Ljava/util/Collection;)Z") => {
            let items = elems_of(vm, &me)?;
            let other = elems_of(vm, &args[0])?;
            let mut all = true;
            for e in &other {
                if !contains(vm, &items, e)? {
                    all = false;
                    break;
                }
            }
            Value::Int(all as i32)
        }
        ("toArray", "()[Ljava/lang/Object;") => {
            let items = elems_of(vm, &me)?;
            to_array(vm, items, None)?
        }
        ("toArray", "([Ljava/lang/Object;)[Ljava/lang/Object;") => {
            let items = elems_of(vm, &me)?;
            to_array(vm, items, args.first())?
        }
        ("indexOf", "(Ljava/lang/Object;)I") | ("lastIndexOf", "(Ljava/lang/Object;)I") => {
            let items = elems_of(vm, &me)?;
            let idx: Vec<usize> = if name == "indexOf" {
                (0..items.len()).collect()
            } else {
                (0..items.len()).rev().collect()
            };
            let mut found = -1;
            for i in idx {
                if crate::arrays::obj_equals(vm, &args[0], &items[i])? {
                    found = i as i32;
                    break;
                }
            }
            Value::Int(found)
        }
        ("addAll", "(Ljava/util/Collection;)Z") => {
            let src = elems_of(vm, &args[0])?;
            let mut changed = false;
            for e in src {
                match guest_call(
                    vm,
                    recv,
                    "add",
                    "(Ljava/lang/Object;)Z",
                    std::slice::from_ref(&e),
                )? {
                    Some(v) => changed |= v.as_int().unwrap_or(0) != 0,
                    None => match guest_call(vm, recv, "size", "()I", &[])? {
                        Some(n) if is_list => {
                            if guest_call(vm, recv, "add", "(ILjava/lang/Object;)V", &[n, e])?
                                .is_none()
                            {
                                return Err(exc(
                                    "Ljava/lang/UnsupportedOperationException;",
                                    "add",
                                ));
                            }
                            changed = true;
                        }
                        _ => return Err(exc("Ljava/lang/UnsupportedOperationException;", "add")),
                    },
                }
            }
            Value::Int(changed as i32)
        }
        ("addAll", "(ILjava/util/Collection;)Z") => {
            let start = args[0].as_int()?;
            let src = elems_of(vm, &args[1])?;
            let changed = !src.is_empty();
            for (at, e) in (start..).zip(src) {
                if guest_call(
                    vm,
                    recv,
                    "add",
                    "(ILjava/lang/Object;)V",
                    &[Value::Int(at), e],
                )?
                .is_none()
                {
                    return Err(exc(
                        "Ljava/lang/UnsupportedOperationException;",
                        "add(int, E)",
                    ));
                }
            }
            Value::Int(changed as i32)
        }
        ("removeAll", "(Ljava/util/Collection;)Z") | ("retainAll", "(Ljava/util/Collection;)Z") => {
            let items = elems_of(vm, &me)?;
            let other = elems_of(vm, &args[0])?;
            let mut changed = false;
            for e in items {
                let inside = contains(vm, &other, &e)?;
                if inside == (name == "removeAll") {
                    if guest_call(vm, recv, "remove", "(Ljava/lang/Object;)Z", &[e])?.is_none() {
                        return Err(exc("Ljava/lang/UnsupportedOperationException;", "remove"));
                    }
                    changed = true;
                }
            }
            Value::Int(changed as i32)
        }
        ("clear", "()V") => {
            if is_list {
                if let Some(n) = guest_call(vm, recv, "size", "()I", &[])? {
                    for i in (0..n.as_int()?).rev() {
                        if guest_call(
                            vm,
                            recv,
                            "remove",
                            "(I)Ljava/lang/Object;",
                            &[Value::Int(i)],
                        )?
                        .is_none()
                        {
                            return Err(exc(
                                "Ljava/lang/UnsupportedOperationException;",
                                "remove(int)",
                            ));
                        }
                    }
                }
            } else {
                for e in elems_of(vm, &me)? {
                    if guest_call(vm, recv, "remove", "(Ljava/lang/Object;)Z", &[e])?.is_none() {
                        return Err(exc("Ljava/lang/UnsupportedOperationException;", "remove"));
                    }
                }
            }
            Value::Null
        }
        ("subList", "(II)Ljava/util/List;") if is_list => {
            let items = elems_of(vm, &me)?;
            let (a, b) = range(items.len(), args[0].as_int()?, args[1].as_int()?)?;
            let view = new_host(vm, "Ljava/util/ArrayList;", items[a..b].to_vec())?;
            if let Value::Obj(parent) = me {
                if vm.fw.objects.contains_key(&parent) {
                    register_sublist(vm, &view, parent, a, b - a);
                }
            }
            view
        }
        ("hashCode", "()I") => {
            let items = elems_of(vm, &me)?;
            let mut h: i32 = if is_list { 1 } else { 0 };
            for e in &items {
                let eh = crate::arrays::obj_hash(vm, e)?;
                h = if is_list {
                    h.wrapping_mul(31).wrapping_add(eh)
                } else {
                    h.wrapping_add(eh)
                };
            }
            Value::Int(h)
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let Some(Value::Obj(o)) = args.first().cloned() else {
                return Ok(Some(Value::Int(0)));
            };
            if o == recv {
                return Ok(Some(Value::Int(1)));
            }
            let a = elems_of(vm, &me)?;
            let b = match elems_of(vm, &Value::Obj(o)) {
                Ok(b) => b,
                Err(_) => return Ok(Some(Value::Int(0))),
            };
            let mut eq = a.len() == b.len();
            if eq {
                for i in 0..a.len() {
                    let ok = if is_list {
                        crate::arrays::obj_equals(vm, &a[i], &b[i])?
                    } else {
                        contains(vm, &b, &a[i])?
                    };
                    if !ok {
                        eq = false;
                        break;
                    }
                }
            }
            Value::Int(eq as i32)
        }
        ("toString", "()Ljava/lang/String;") => {
            let items = elems_of(vm, &me)?;
            let s = to_string(vm, recv, &items)?;
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
}

/// `equals`/`hashCode`/`toString` for receivers with host List/Set state
/// (ArrayList, Collections$SingletonList, HashSet, …), with JDK semantics:
/// a List compares in order with any List, a Set compares membership with
/// any Set. Called whatever the invoke's declared class is (`Object`,
/// `List`, `Collection`, …).
pub(crate) fn host_object_method(
    vm: &mut Engine,
    recv_class: &str,
    recv: ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        (name, sig),
        ("equals", "(Ljava/lang/Object;)Z")
            | ("hashCode", "()I")
            | ("toString", "()Ljava/lang/String;")
    ) {
        return Ok(None);
    }
    let items = match vm.fw.objects.get(&recv) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => return Ok(None),
    };
    let is_set = vm.cp.is_subtype(recv_class, "Ljava/util/Set;");
    Ok(Some(match name {
        "hashCode" => {
            let mut h: i32 = if is_set { 0 } else { 1 };
            for e in &items {
                let eh = crate::arrays::obj_hash(vm, e)?;
                h = if is_set {
                    h.wrapping_add(eh)
                } else {
                    h.wrapping_mul(31).wrapping_add(eh)
                };
            }
            Value::Int(h)
        }
        "toString" => {
            let s = to_string(vm, recv, &items)?;
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => {
            let Some(Value::Obj(o)) = args.first().cloned() else {
                return Ok(Some(Value::Int(0)));
            };
            if o == recv {
                return Ok(Some(Value::Int(1)));
            }
            let oc = vm.heap.class_of(o)?.to_string();
            let want = if is_set {
                "Ljava/util/Set;"
            } else {
                "Ljava/util/List;"
            };
            if !vm.cp.is_subtype(&oc, want) {
                return Ok(Some(Value::Int(0)));
            }
            let b = match elems_of(vm, &Value::Obj(o)) {
                Ok(b) => b,
                Err(_) => return Ok(Some(Value::Int(0))),
            };
            let mut eq = items.len() == b.len();
            if eq {
                for i in 0..items.len() {
                    let ok = if is_set {
                        contains(vm, &b, &items[i])?
                    } else {
                        crate::arrays::obj_equals(vm, &items[i], &b[i])?
                    };
                    if !ok {
                        eq = false;
                        break;
                    }
                }
            }
            Value::Int(eq as i32)
        }
    }))
}
