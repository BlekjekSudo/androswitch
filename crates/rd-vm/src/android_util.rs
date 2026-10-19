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
use crate::heap::ObjRef;
use crate::value::Value;
use std::collections::BTreeMap;

const SPARSE: &[&str] = &[
    "Landroid/util/SparseArray;",
    "Landroid/util/SparseIntArray;",
    "Landroid/util/SparseBooleanArray;",
    "Landroid/util/SparseLongArray;",
    "Landroid/util/LongSparseArray;",
];

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
    VmExit::Exception(Throwable::new("Ljava/lang/ArrayIndexOutOfBoundsException;", format!("index={i}")))
}

fn default_value(class: &str) -> Value {
    match class {
        "Landroid/util/SparseIntArray;" | "Landroid/util/SparseBooleanArray;" => Value::Int(0),
        "Landroid/util/SparseLongArray;" => Value::Long(0),
        _ => Value::Null,
    }
}

fn sparse(vm: &mut Engine, class: &str, name: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let m = vm.sparse.entry(recv).or_insert_with(BTreeMap::new);
    let nth = |m: &BTreeMap<i64, Value>, i: i64| -> Result<(i64, Value), VmExit> {
        m.iter().nth(i.max(0) as usize).filter(|_| i >= 0).map(|(k, v)| (*k, v.clone())).ok_or_else(|| oob(i))
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
            let ks: Vec<i64> = m.keys().skip(i.max(0) as usize).take(n.max(0) as usize).copied().collect();
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
            if class == "Landroid/util/LongSparseArray;" { Value::Long(k) } else { Value::Int(k as i32) }
        }
        "valueAt" => nth(m, key(a(0)))?.1,
        "setValueAt" => {
            let (k, _) = nth(m, key(a(0)))?;
            m.insert(k, a(1).cloned().unwrap_or(Value::Null));
            Value::Null
        }
        "indexOfKey" => Value::Int(m.keys().position(|k| *k == key(a(0))).map(|i| i as i32).unwrap_or(-1)),
        "indexOfValue" => {
            let v = a(0).cloned().unwrap_or(Value::Null);
            Value::Int(m.values().position(|x| *x == v).map(|i| i as i32).unwrap_or(-1))
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
            let s = format!("{{{}}}", m.iter().map(|(k, v)| format!("{k}={v:?}")).collect::<Vec<_>>().join(", "));
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
}

pub(crate) fn static_intrinsic(vm: &mut Engine, class: &str, name: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        ("Landroid/util/Pair;", "create") | ("Landroid/util/Range;", "create") => {
            let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
            let (f0, f1) = if class.ends_with("Pair;") { ("first", "second") } else { ("rd_lower", "rd_upper") };
            put(vm, r, f0, args.first().cloned().unwrap_or(Value::Null))?;
            put(vm, r, f1, args.get(1).cloned().unwrap_or(Value::Null))?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

pub(crate) fn instance_intrinsic(vm: &mut Engine, mref_class: &str, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
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
            Value::Obj(o) => vm.heap.class_of(*o).ok() == vm.heap.class_of(recv).ok() && fields.iter().all(|f| get(vm, *o, f) == get(vm, recv, f)),
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
                "Landroid/util/Size;" | "Landroid/util/SizeF;" => format!("{}x{}", fmt(&get(vm, recv, "rd_w")), fmt(&get(vm, recv, "rd_h"))),
                "Landroid/util/Rational;" => format!("{}/{}", fmt(&get(vm, recv, "rd_num")), fmt(&get(vm, recv, "rd_den"))),
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
    if b == 0 { a } else { gcd(b, a % b) }
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
                Ok(x @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_))) => Some(num(&x)),
                _ => None,
            },
            _ => None,
        }
    };
    if let (Some(x), Some(y)) = (unbox(vm, a), unbox(vm, b)) {
        return Ok(x.partial_cmp(&y).map(|o| o as i32).unwrap_or(0));
    }
    match a {
        Value::Obj(r) => match crate::jcollections::guest_call(vm, *r, "compareTo", "(Ljava/lang/Object;)I", &[b.clone()])? {
            Some(Value::Int(i)) => Ok(i),
            _ => Ok(0),
        },
        _ => Ok(0),
    }
}
