//! `java.util.Arrays`: `copyOf`, `copyOfRange`, `fill`, `equals`,
//! `hashCode`, `toString`, `deepToString`, `sort` and `binarySearch` for
//! every element type.
//!
//! Object elements use their real Java semantics through guest calls:
//! - `equals`/`hashCode`/`compareTo` run the DEX override if the class
//!   declares one, or the host implementation (`Uri`, `UUID`, `BigDecimal`,
//!   collections, maps, Strings, boxes, …), falling back to identity.
//! - `sort(T[], Comparator)` calls `Comparator.compare`. The sort is a
//!   stable merge sort, matching TimSort's stability guarantee, and a
//!   comparator exception propagates.
//!
//! Float and double `equals`/`hashCode` use the canonical bit patterns,
//! like `Float.floatToIntBits`. The bounds errors are those of the JDK:
//! - NegativeArraySizeException;
//! - ArrayIndexOutOfBoundsException;
//! - IllegalArgumentException for `from > to`.

use std::collections::HashSet;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const ARRAYS: &str = "Ljava/util/Arrays;";

/// Maximum host->host reentrant call depth (`object_to_string`, `obj_equals`,
/// `obj_hash`, nested collections/maps) before throwing a catchable guest
/// `StackOverflowError` instead of exhausting the native Rust thread stack
/// (unoptimized debug frames of `instance_intrinsic_dispatch` are ~50 KiB).
const MAX_HOST_REENTRANT_DEPTH: usize = 16;

pub(crate) fn with_host_reentrant<T, F>(vm: &mut Engine, f: F) -> Result<T, VmExit>
where
    F: FnOnce(&mut Engine) -> Result<T, VmExit>,
{
    if vm.host_reentrant_depth >= MAX_HOST_REENTRANT_DEPTH {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/StackOverflowError;",
            "host reentrant call depth exceeded",
        )));
    }
    vm.host_reentrant_depth += 1;
    let res = f(vm);
    vm.host_reentrant_depth = vm.host_reentrant_depth.saturating_sub(1);
    res
}

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m))
}

pub(crate) fn elems(vm: &Engine, r: ObjRef) -> Result<(Vec<Value>, ElemKind, String), VmExit> {
    let (e, k) = vm.heap.as_array_elems(r)?;
    let desc = vm.heap.class_of(r)?.to_string();
    Ok((e.to_vec(), k, desc))
}

fn default_of(k: ElemKind) -> Value {
    match k {
        ElemKind::Int => Value::Int(0),
        ElemKind::Long => Value::Long(0),
        ElemKind::Float => Value::Float(0.0),
        ElemKind::Double => Value::Double(0.0),
        ElemKind::Obj => Value::Null,
    }
}

fn normalize_elem_for_desc(desc: &str, v: &Value) -> Result<Value, VmExit> {
    Ok(match desc {
        "[Z" => Value::Int(i32::from(v.as_int()? != 0)),
        "[B" => Value::Int(i32::from(v.as_int()? as i8)),
        "[S" => Value::Int(i32::from(v.as_int()? as i16)),
        "[C" => Value::Int(i32::from(v.as_int()? as u16)),
        _ => v.clone(),
    })
}

/// Charge `vm.heap` for `new_len` elements **before** allocating or resizing
/// any host `Vec<Value>`, preventing multi-GiB host `abort()` on huge sizes
/// such as `Arrays.copyOf(a, Integer.MAX_VALUE)`.
fn alloc_array_resized(
    vm: &mut Engine,
    desc: &str,
    k: ElemKind,
    new_len: usize,
    prefix: &[Value],
) -> Result<Value, VmExit> {
    let elem_class = if k == ElemKind::Obj {
        desc.strip_prefix('[').unwrap_or(desc).to_string()
    } else {
        desc.to_string()
    };
    let r = vm
        .heap
        .alloc_array(k, new_len, &elem_class)
        .map_err(crate::err::oom_throwable)?;
    let copy_len = prefix.len().min(new_len);
    let fill = default_of(k);
    let slot = vm
        .heap
        .array_elems_mut(r)
        .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
    slot.clear();
    slot.reserve(new_len);
    slot.extend_from_slice(&prefix[..copy_len]);
    slot.resize(new_len, fill);
    Ok(Value::Obj(r))
}

pub(crate) fn new_array(
    vm: &mut Engine,
    desc: &str,
    k: ElemKind,
    items: Vec<Value>,
) -> Result<Value, VmExit> {
    // alloc_array wants the element class for Obj arrays, the full desc otherwise
    let elem_class = if k == ElemKind::Obj {
        desc.strip_prefix('[').unwrap_or(desc).to_string()
    } else {
        desc.to_string()
    };
    let r = vm
        .heap
        .alloc_array(k, items.len(), &elem_class)
        .map_err(crate::err::oom_throwable)?;
    let slot = vm
        .heap
        .array_elems_mut(r)
        .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
    slot.clear();
    slot.extend(items);
    Ok(Value::Obj(r))
}

fn arr_arg(args: &[Value], i: usize) -> Result<ObjRef, VmExit> {
    match args.get(i) {
        Some(Value::Obj(r)) => Ok(*r),
        _ => Err(exc("Ljava/lang/NullPointerException;", "array is null")),
    }
}

/// Guest `a.equals(b)` supporting DEX overrides, host classes (`Uri`, `UUID`,
/// `BigDecimal`, collections, maps, …), Strings, primitive boxes, and identity.
pub(crate) fn obj_equals(vm: &mut Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    let is_null = |v: &Value| matches!(v, Value::Null);
    if is_null(a) || is_null(b) {
        return Ok(is_null(a) && is_null(b));
    }
    if crate::intrinsics::values_equal(&vm.heap, a, b)? {
        return Ok(true);
    }
    let (Value::Obj(x), Value::Obj(_)) = (a, b) else {
        return Ok(false);
    };
    // Fast-reject String and primitive boxes that already ran through values_equal
    if vm.heap.as_str(*x).is_ok() {
        return Ok(false);
    }
    let class = vm.heap.class_of(*x)?.to_string();
    if crate::boxes::kind(&class).is_some() {
        return Ok(false);
    }
    with_host_reentrant(vm, |vm| {
        let res = vm.invoke_instance(
            *x,
            "equals",
            "(Ljava/lang/Object;)Z",
            std::slice::from_ref(b),
        )?;
        Ok(matches!(res, Value::Int(v) if v != 0))
    })
}

/// Guest `o.hashCode()` (0 for null), dispatching DEX overrides and host classes.
pub(crate) fn obj_hash(vm: &mut Engine, v: &Value) -> Result<i32, VmExit> {
    let Value::Obj(r) = v else { return Ok(0) };
    if let Ok(s) = vm.heap.as_str(*r) {
        return Ok(s
            .encode_utf16()
            .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)));
    }
    let class = vm.heap.class_of(*r)?.to_string();
    if let Some(k) = crate::boxes::kind(&class) {
        let inner = vm.heap.get_field(*r, "value").unwrap_or(Value::Int(0));
        return Ok(prim_hash(k, &inner));
    }
    with_host_reentrant(vm, |vm| {
        Ok(vm
            .invoke_instance(*r, "hashCode", "()I", &[])?
            .as_int()
            .unwrap_or(0))
    })
}

fn prim_hash(k: char, v: &Value) -> i32 {
    match (k, v) {
        ('J', Value::Long(l)) => (*l ^ ((*l as u64) >> 32) as i64) as i32,
        ('D', Value::Double(d)) => {
            let b = if d.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                d.to_bits() as i64
            };
            (b ^ ((b as u64) >> 32) as i64) as i32
        }
        ('F', Value::Float(f)) => {
            if f.is_nan() {
                0x7fc0_0000
            } else {
                f.to_bits() as i32
            }
        }
        ('Z', Value::Int(x)) => {
            if *x != 0 {
                1231
            } else {
                1237
            }
        }
        (_, Value::Int(x)) => *x,
        _ => 0,
    }
}

fn elem_hash(k: ElemKind, desc: &str, v: &Value) -> i32 {
    let kind = match (k, desc) {
        (ElemKind::Long, _) => 'J',
        (ElemKind::Double, _) => 'D',
        (ElemKind::Float, _) => 'F',
        (_, "[Z") => 'Z',
        _ => 'I',
    };
    prim_hash(kind, v)
}

fn prim_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => {
            (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits()
        }
        (Value::Double(x), Value::Double(y)) => {
            (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits()
        }
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Long(x), Value::Long(y)) => x == y,
        _ => false,
    }
}

fn elem_to_string(
    vm: &mut Engine,
    desc: &str,
    v: &Value,
    deep: bool,
    deja_vu: &mut HashSet<ObjRef>,
) -> Result<String, VmExit> {
    Ok(match v {
        Value::Null => "null".into(),
        Value::Int(x) => match desc {
            "[Z" => (*x != 0).to_string(),
            "[C" => char::from_u32(*x as u32)
                .map(String::from)
                .unwrap_or_default(),
            _ => x.to_string(),
        },
        Value::Long(x) => x.to_string(),
        Value::Float(f) => crate::numerics::java_double_to_string(*f as f64, true),
        Value::Double(d) => crate::numerics::java_double_to_string(*d, false),
        Value::Obj(r) => {
            if deep && vm.heap.as_array_elems(*r).is_ok() {
                if deja_vu.contains(r) {
                    "[...]".into()
                } else {
                    array_to_string_inner(vm, *r, true, deja_vu)?
                }
            } else {
                crate::intrinsics::object_to_string(vm, *r)?
            }
        }
        _ => String::new(),
    })
}

fn array_to_string_inner(
    vm: &mut Engine,
    r: ObjRef,
    deep: bool,
    deja_vu: &mut HashSet<ObjRef>,
) -> Result<String, VmExit> {
    with_host_reentrant(vm, |vm| {
        let (items, _, desc) = elems(vm, r)?;
        if deep {
            deja_vu.insert(r);
        }
        let mut parts = Vec::with_capacity(items.len());
        let mut err = None;
        for v in &items {
            match elem_to_string(vm, &desc, v, deep, deja_vu) {
                Ok(s) => parts.push(s),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        if deep {
            deja_vu.remove(&r);
        }
        if let Some(e) = err {
            return Err(e);
        }
        Ok(format!("[{}]", parts.join(", ")))
    })
}

fn array_to_string(vm: &mut Engine, r: ObjRef, deep: bool) -> Result<String, VmExit> {
    let mut deja_vu = HashSet::new();
    array_to_string_inner(vm, r, deep, &mut deja_vu)
}

/// Compare two elements: Comparator if given, else natural ordering.
pub(crate) fn compare(
    vm: &mut Engine,
    cmp: Option<&Value>,
    a: &Value,
    b: &Value,
) -> Result<i32, VmExit> {
    if let Some(Value::Obj(c)) = cmp {
        let class = vm.heap.class_of(*c)?.to_string();
        if let Some((d, def, m)) =
            vm.cp
                .resolve_method(&class, "compare", "(Ljava/lang/Object;Ljava/lang/Object;)I")
        {
            return Ok(vm
                .call(d, def, &m, vec![Value::Obj(*c), a.clone(), b.clone()])?
                .as_int()
                .unwrap_or(0));
        }
        if let Some(v) = crate::intrinsics::call_instance_intrinsic_mref(
            vm,
            &class,
            &class,
            "compare",
            "(Ljava/lang/Object;Ljava/lang/Object;)I",
            *c,
            &[a.clone(), b.clone()],
        )? {
            return Ok(v.as_int().unwrap_or(0));
        }
        return Err(exc(
            "Ljava/lang/AbstractMethodError;",
            format!("{class}.compare"),
        ));
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y) as i32),
        (Value::Long(x), Value::Long(y)) => Ok(x.cmp(y) as i32),
        (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y) as i32),
        (Value::Double(x), Value::Double(y)) => Ok(x.total_cmp(y) as i32),
        (Value::Obj(x), Value::Obj(y)) => {
            if let (Ok(s), Ok(t)) = (vm.heap.as_str(*x), vm.heap.as_str(*y)) {
                let (u, w): (Vec<u16>, Vec<u16>) =
                    (s.encode_utf16().collect(), t.encode_utf16().collect());
                return Ok(u.cmp(&w) as i32);
            }
            let class = vm.heap.class_of(*x)?.to_string();
            if crate::boxes::kind(&class).is_some() {
                let (p, q) = (
                    vm.heap.get_field(*x, "value")?,
                    vm.heap.get_field(*y, "value")?,
                );
                return compare(vm, None, &p, &q);
            }
            if let Some((d, def, m)) =
                vm.cp
                    .resolve_method(&class, "compareTo", "(Ljava/lang/Object;)I")
            {
                return Ok(vm
                    .call(d, def, &m, vec![a.clone(), b.clone()])?
                    .as_int()
                    .unwrap_or(0));
            }
            let typed_sig = format!("({class})I");
            for cand_sig in ["(Ljava/lang/Object;)I", typed_sig.as_str()] {
                if let Some(v) = crate::intrinsics::call_instance_intrinsic_mref(
                    vm,
                    &class,
                    &class,
                    "compareTo",
                    cand_sig,
                    *x,
                    std::slice::from_ref(b),
                )? {
                    return Ok(v.as_int().unwrap_or(0));
                }
            }
            Err(exc(
                "Ljava/lang/ClassCastException;",
                format!("{class} cannot be cast to java.lang.Comparable"),
            ))
        }
        (Value::Null, _) | (_, Value::Null) => Err(exc(
            "Ljava/lang/NullPointerException;",
            "null element in sort",
        )),
        _ => Ok(0),
    }
}

/// Stable merge sort with a fallible comparator.
pub(crate) fn merge_sort(
    vm: &mut Engine,
    v: Vec<Value>,
    cmp: Option<&Value>,
) -> Result<Vec<Value>, VmExit> {
    if v.len() <= 1 {
        return Ok(v);
    }
    let mut right = v;
    let left = right.drain(..right.len() / 2).collect::<Vec<_>>();
    let left = merge_sort(vm, left, cmp)?;
    let right = merge_sort(vm, right, cmp)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if compare(vm, cmp, &right[j], &left[i])? < 0 {
            out.push(right[j].clone());
            j += 1;
        } else {
            out.push(left[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    Ok(out)
}

fn check_range(len: usize, from: i32, to: i32) -> Result<(usize, usize), VmExit> {
    if from > to {
        return Err(exc(
            "Ljava/lang/IllegalArgumentException;",
            format!("fromIndex({from}) > toIndex({to})"),
        ));
    }
    if from < 0 {
        return Err(exc(
            "Ljava/lang/ArrayIndexOutOfBoundsException;",
            format!("Array index out of range: {from}"),
        ));
    }
    if to as usize > len {
        return Err(exc(
            "Ljava/lang/ArrayIndexOutOfBoundsException;",
            format!("Array index out of range: {to}"),
        ));
    }
    Ok((from as usize, to as usize))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != ARRAYS {
        return Ok(None);
    }
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let class_desc = |vm: &Engine, v: Option<&Value>| -> Option<String> {
        match v {
            Some(Value::Obj(c)) => match vm.heap.get_field(*c, "class_name") {
                Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
                _ => None,
            },
            _ => None,
        }
    };
    Ok(Some(match name {
        "copyOf" => {
            if args.len() == 3 && matches!(args.get(2), Some(Value::Null)) {
                return Err(exc("Ljava/lang/NullPointerException;", "newType is null"));
            }
            let a = arr_arg(args, 0)?;
            let n = int(1);
            if n < 0 {
                return Err(exc("Ljava/lang/NegativeArraySizeException;", n.to_string()));
            }
            let (items, k, desc) = elems(vm, a)?;
            let desc = class_desc(vm, args.get(2)).unwrap_or(desc);
            alloc_array_resized(vm, &desc, k, n as usize, &items)?
        }
        "copyOfRange" => {
            if args.len() == 4 && matches!(args.get(3), Some(Value::Null)) {
                return Err(exc("Ljava/lang/NullPointerException;", "newType is null"));
            }
            let a = arr_arg(args, 0)?;
            let (items, k, desc) = elems(vm, a)?;
            let (from, to) = (int(1), int(2));
            if from > to {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("{from} > {to}"),
                ));
            }
            if from < 0 || from as usize > items.len() {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("{from}"),
                ));
            }
            let desc = class_desc(vm, args.get(3)).unwrap_or(desc);
            let from_u = from as usize;
            let new_len = (i64::from(to) - i64::from(from)) as usize;
            let copy_end = (to as usize).min(items.len());
            alloc_array_resized(vm, &desc, k, new_len, &items[from_u..copy_end])?
        }
        "fill" => {
            let a = arr_arg(args, 0)?;
            let len = vm.heap.as_array_elems(a)?.0.len();
            let desc = vm.heap.class_of(a)?.to_string();
            let (from, to, raw_v) = if args.len() == 4 {
                let (f, t) = check_range(len, int(1), int(2))?;
                (f, t, &args[3])
            } else {
                (0, len, &args[1])
            };
            let v = normalize_elem_for_desc(&desc, raw_v)?;
            let slot = vm
                .heap
                .array_elems_mut(a)
                .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
            for e in &mut slot[from..to] {
                *e = v.clone();
            }
            Value::Null
        }
        "equals" if args.len() == 2 => {
            let (a, b) = (args[0].clone(), args[1].clone());
            let r = match (&a, &b) {
                (Value::Null, Value::Null) => true,
                (Value::Obj(x), Value::Obj(y)) if x == y => true,
                (Value::Obj(x), Value::Obj(y)) => {
                    let (ia, ka, _) = elems(vm, *x)?;
                    let (ib, _, _) = elems(vm, *y)?;
                    if ia.len() != ib.len() {
                        false
                    } else if ka == ElemKind::Obj {
                        let mut all = true;
                        for (p, q) in ia.iter().zip(&ib) {
                            if !obj_equals(vm, p, q)? {
                                all = false;
                                break;
                            }
                        }
                        all
                    } else {
                        ia.iter().zip(&ib).all(|(p, q)| prim_eq(p, q))
                    }
                }
                _ => false,
            };
            Value::Int(r as i32)
        }
        "hashCode" => match args.first() {
            Some(Value::Obj(a)) => {
                let (items, k, desc) = elems(vm, *a)?;
                let mut h: i32 = 1;
                for v in &items {
                    let eh = if k == ElemKind::Obj {
                        obj_hash(vm, v)?
                    } else {
                        elem_hash(k, &desc, v)
                    };
                    h = h.wrapping_mul(31).wrapping_add(eh);
                }
                Value::Int(h)
            }
            _ => Value::Int(0),
        },
        "toString" | "deepToString" => match args.first() {
            Some(Value::Obj(a)) => {
                let s = array_to_string(vm, *a, name == "deepToString")?;
                Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
            }
            _ => Value::Obj(crate::intrinsics::alloc_string(vm, "null".into())?),
        },
        "sort" => {
            let a = arr_arg(args, 0)?;
            let len = vm.heap.as_array_elems(a)?.0.len();
            let (from, to, cmp) = match sig {
                s if s.starts_with("([Ljava/lang/Object;II")
                    || (args.len() >= 3 && matches!(args.get(1), Some(Value::Int(_)))) =>
                {
                    let (f, t) = check_range(len, int(1), int(2))?;
                    (f, t, args.get(3).cloned())
                }
                _ => (0, len, args.get(1).cloned()),
            };
            let cmp = cmp.filter(|c| !matches!(c, Value::Null));
            let items = vm.heap.as_array_elems(a)?.0[from..to].to_vec();
            let sorted = merge_sort(vm, items, cmp.as_ref())?;
            let slot = vm
                .heap
                .array_elems_mut(a)
                .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
            slot[from..to].clone_from_slice(&sorted);
            Value::Null
        }
        "binarySearch" if args.len() == 2 => {
            let a = arr_arg(args, 0)?;
            let items = vm.heap.as_array_elems(a)?.0.to_vec();
            let key = args[1].clone();
            let (mut lo, mut hi) = (0i32, items.len() as i32 - 1);
            let mut found = None;
            while lo <= hi {
                let mid = ((lo + hi) as u32 >> 1) as i32;
                let c = compare(vm, None, &items[mid as usize], &key)?;
                match c.cmp(&0) {
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid - 1,
                    std::cmp::Ordering::Equal => {
                        found = Some(mid);
                        break;
                    }
                }
            }
            Value::Int(found.unwrap_or(-(lo + 1)))
        }
        _ => return Ok(None),
    }))
}
