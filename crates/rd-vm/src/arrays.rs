//! `java.util.Arrays`: `copyOf`, `copyOfRange`, `fill`, `equals`,
//! `hashCode`, `toString`, `deepToString`, `sort` and `binarySearch` for
//! every element type.
//!
//! Object elements use their real Java semantics through guest calls:
//! - `equals`/`hashCode`/`compareTo` run the DEX override if the class
//!   declares one. Strings and boxes are compared by value, and other
//!   objects fall back to identity.
//! - `sort(T[], Comparator)` calls `Comparator.compare`. The sort is a
//!   stable merge sort, matching TimSort's stability guarantee, and a
//!   comparator exception propagates.
//!
//! Float and double `equals`/`hashCode` use the canonical bit patterns,
//! like `Float.floatToIntBits`. The bounds errors are those of the JDK:
//! - NegativeArraySizeException;
//! - ArrayIndexOutOfBoundsException;
//! - IllegalArgumentException for `from > to`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const ARRAYS: &str = "Ljava/util/Arrays;";

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

pub(crate) fn new_array(vm: &mut Engine, desc: &str, k: ElemKind, items: Vec<Value>) -> Result<Value, VmExit> {
    // alloc_array wants the element class for Obj arrays, the full desc otherwise
    let elem_class = if k == ElemKind::Obj { desc.strip_prefix('[').unwrap_or(desc).to_string() } else { desc.to_string() };
    let r = vm.heap.alloc_array(k, items.len(), &elem_class)?;
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

/// Guest `a.equals(b)`.
pub(crate) fn obj_equals(vm: &mut Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    if crate::intrinsics::values_equal(&vm.heap, a, b)? {
        return Ok(true);
    }
    let (Value::Obj(x), Value::Obj(_)) = (a, b) else { return Ok(false) };
    let class = vm.heap.class_of(*x)?.to_string();
    if let Some((d, def, m)) = vm.cp.resolve_method(&class, "equals", "(Ljava/lang/Object;)Z") {
        let r = vm.call(d, def, &m, vec![a.clone(), b.clone()])?;
        return Ok(matches!(r, Value::Int(v) if v != 0));
    }
    Ok(false)
}

/// Guest `o.hashCode()` (0 for null).
pub(crate) fn obj_hash(vm: &mut Engine, v: &Value) -> Result<i32, VmExit> {
    let Value::Obj(r) = v else { return Ok(0) };
    if let Ok(s) = vm.heap.as_str(*r) {
        return Ok(s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)));
    }
    let class = vm.heap.class_of(*r)?.to_string();
    if let Some(k) = crate::boxes::kind(&class) {
        let inner = vm.heap.get_field(*r, "value").unwrap_or(Value::Int(0));
        return Ok(prim_hash(k, &inner));
    }
    if let Some((d, def, m)) = vm.cp.resolve_method(&class, "hashCode", "()I") {
        return Ok(vm.call(d, def, &m, vec![v.clone()])?.as_int().unwrap_or(0));
    }
    Ok(*r as i32)
}

fn prim_hash(k: char, v: &Value) -> i32 {
    match (k, v) {
        ('J', Value::Long(l)) => (*l ^ ((*l as u64) >> 32) as i64) as i32,
        ('D', Value::Double(d)) => {
            let b = if d.is_nan() { 0x7ff8_0000_0000_0000 } else { d.to_bits() as i64 };
            (b ^ ((b as u64) >> 32) as i64) as i32
        }
        ('F', Value::Float(f)) => if f.is_nan() { 0x7fc0_0000 } else { f.to_bits() as i32 },
        ('Z', Value::Int(x)) => if *x != 0 { 1231 } else { 1237 },
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
        (Value::Float(x), Value::Float(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        (Value::Double(x), Value::Double(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Long(x), Value::Long(y)) => x == y,
        _ => false,
    }
}

fn elem_to_string(vm: &mut Engine, desc: &str, v: &Value, deep: bool) -> Result<String, VmExit> {
    Ok(match v {
        Value::Null => "null".into(),
        Value::Int(x) => match desc {
            "[Z" => (*x != 0).to_string(),
            "[C" => char::from_u32(*x as u32).map(String::from).unwrap_or_default(),
            _ => x.to_string(),
        },
        Value::Long(x) => x.to_string(),
        Value::Float(f) => crate::numerics::java_double_to_string(*f as f64, true),
        Value::Double(d) => crate::numerics::java_double_to_string(*d, false),
        Value::Obj(r) => {
            if deep && vm.heap.as_array_elems(*r).is_ok() {
                array_to_string(vm, *r, true)?
            } else {
                crate::intrinsics::object_to_string(vm, *r)?
            }
        }
        _ => String::new(),
    })
}

fn array_to_string(vm: &mut Engine, r: ObjRef, deep: bool) -> Result<String, VmExit> {
    let (items, _, desc) = elems(vm, r)?;
    let mut parts = Vec::with_capacity(items.len());
    for v in &items {
        parts.push(elem_to_string(vm, &desc, v, deep)?);
    }
    Ok(format!("[{}]", parts.join(", ")))
}

/// Compare two elements: Comparator if given, else natural ordering.
pub(crate) fn compare(vm: &mut Engine, cmp: Option<&Value>, a: &Value, b: &Value) -> Result<i32, VmExit> {
    if let Some(Value::Obj(c)) = cmp {
        let class = vm.heap.class_of(*c)?.to_string();
        let Some((d, def, m)) = vm.cp.resolve_method(&class, "compare", "(Ljava/lang/Object;Ljava/lang/Object;)I") else {
            return Err(exc("Ljava/lang/AbstractMethodError;", format!("{class}.compare")));
        };
        return Ok(vm.call(d, def, &m, vec![Value::Obj(*c), a.clone(), b.clone()])?.as_int().unwrap_or(0));
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y) as i32),
        (Value::Long(x), Value::Long(y)) => Ok(x.cmp(y) as i32),
        (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y) as i32),
        (Value::Double(x), Value::Double(y)) => Ok(x.total_cmp(y) as i32),
        (Value::Obj(x), Value::Obj(y)) => {
            if let (Ok(s), Ok(t)) = (vm.heap.as_str(*x), vm.heap.as_str(*y)) {
                let (u, w): (Vec<u16>, Vec<u16>) = (s.encode_utf16().collect(), t.encode_utf16().collect());
                return Ok(u.cmp(&w) as i32);
            }
            let class = vm.heap.class_of(*x)?.to_string();
            if crate::boxes::kind(&class).is_some() {
                let (p, q) = (vm.heap.get_field(*x, "value")?, vm.heap.get_field(*y, "value")?);
                return compare(vm, None, &p, &q);
            }
            match vm.cp.resolve_method(&class, "compareTo", "(Ljava/lang/Object;)I") {
                Some((d, def, m)) => Ok(vm.call(d, def, &m, vec![a.clone(), b.clone()])?.as_int().unwrap_or(0)),
                None => Err(exc("Ljava/lang/ClassCastException;", format!("{class} cannot be cast to java.lang.Comparable"))),
            }
        }
        (Value::Null, _) | (_, Value::Null) => Err(exc("Ljava/lang/NullPointerException;", "null element in sort")),
        _ => Ok(0),
    }
}

/// Stable merge sort with a fallible comparator.
fn merge_sort(vm: &mut Engine, v: Vec<Value>, cmp: Option<&Value>) -> Result<Vec<Value>, VmExit> {
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
        return Err(exc("Ljava/lang/IllegalArgumentException;", format!("fromIndex({from}) > toIndex({to})")));
    }
    if from < 0 {
        return Err(exc("Ljava/lang/ArrayIndexOutOfBoundsException;", format!("Array index out of range: {from}")));
    }
    if to as usize > len {
        return Err(exc("Ljava/lang/ArrayIndexOutOfBoundsException;", format!("Array index out of range: {to}")));
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
            let a = arr_arg(args, 0)?;
            let n = int(1);
            if n < 0 {
                return Err(exc("Ljava/lang/NegativeArraySizeException;", n.to_string()));
            }
            let (mut items, k, desc) = elems(vm, a)?;
            let desc = class_desc(vm, args.get(2)).unwrap_or(desc);
            items.resize(n as usize, default_of(k));
            new_array(vm, &desc, k, items)?
        }
        "copyOfRange" => {
            let a = arr_arg(args, 0)?;
            let (items, k, desc) = elems(vm, a)?;
            let (from, to) = (int(1), int(2));
            if from > to {
                return Err(exc("Ljava/lang/IllegalArgumentException;", format!("{from} > {to}")));
            }
            if from < 0 || from as usize > items.len() {
                return Err(exc("Ljava/lang/ArrayIndexOutOfBoundsException;", format!("{from}")));
            }
            let desc = class_desc(vm, args.get(3)).unwrap_or(desc);
            let mut out: Vec<Value> = items[from as usize..(to as usize).min(items.len())].to_vec();
            out.resize((to - from) as usize, default_of(k));
            new_array(vm, &desc, k, out)?
        }
        "fill" => {
            let a = arr_arg(args, 0)?;
            let len = vm.heap.as_array_elems(a)?.0.len();
            let (from, to, v) = if args.len() == 4 {
                let (f, t) = check_range(len, int(1), int(2))?;
                (f, t, args[3].clone())
            } else {
                (0, len, args[1].clone())
            };
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
                    let eh = if k == ElemKind::Obj { obj_hash(vm, v)? } else { elem_hash(k, &desc, v) };
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
                s if s.starts_with("([Ljava/lang/Object;II") || (args.len() >= 3 && matches!(args.get(1), Some(Value::Int(_)))) => {
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
                if c < 0 {
                    lo = mid + 1;
                } else if c > 0 {
                    hi = mid - 1;
                } else {
                    found = Some(mid);
                    break;
                }
            }
            Value::Int(found.unwrap_or(-(lo + 1)))
        }
        _ => return Ok(None),
    }))
}
