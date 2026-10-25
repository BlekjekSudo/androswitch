//! `java.lang` primitive wrappers: `Boolean`, `Byte`, `Short`, `Character`,
//! `Integer`, `Long`, `Float` and `Double`.
//!
//! Kotlin and d8/R8 box primitives everywhere. Some examples:
//! - `Boolean.FALSE` is the initial accumulator of kotlinx.coroutines'
//!   `CoroutineContext.fold`.
//! - Data-class `hashCode` uses `Boolean.hashCode(Z)`, `Long.hashCode(J)`,
//!   `Double.hashCode(D)` and so on.
//! - `equals` uses `Float.compare`/`Double.compare`.
//! - Generic code goes through `Number.xxxValue()`.
//!
//! The model is a host instance of the wrapper class with a `value` field.
//! The field holds `Int` for Z/B/S/C/I, `Long` for J, `Float` for F and
//! `Double` for D. `Boolean.TRUE` and `Boolean.FALSE` are canonical
//! singletons and `Boolean.valueOf` returns them, as on the JVM.
//!
//! Semantics follow the JDK. Float/Double `equals` and `hashCode` use the
//! canonical-NaN bit patterns (`floatToIntBits`/`doubleToLongBits`), so NaN
//! equals NaN and 0.0 does not equal -0.0. `compare` orders -0.0 before 0.0
//! and NaN last.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework_host::HostMethodKind;
use crate::heap::ObjRef;
use crate::value::Value;
use alloc::{
    format,
    string::{String, ToString},
    vec,
};

pub(crate) const BOOLEAN: &str = "Ljava/lang/Boolean;";
const BYTE: &str = "Ljava/lang/Byte;";
const SHORT: &str = "Ljava/lang/Short;";
const CHARACTER: &str = "Ljava/lang/Character;";
const INTEGER: &str = "Ljava/lang/Integer;";
const LONG: &str = "Ljava/lang/Long;";
const FLOAT: &str = "Ljava/lang/Float;";
const DOUBLE: &str = "Ljava/lang/Double;";

/// Primitive descriptor of a wrapper class, or `None`.
pub(crate) fn kind(class: &str) -> Option<char> {
    Some(match class {
        BOOLEAN => 'Z',
        BYTE => 'B',
        SHORT => 'S',
        CHARACTER => 'C',
        INTEGER => 'I',
        LONG => 'J',
        FLOAT => 'F',
        DOUBLE => 'D',
        _ => return None,
    })
}

/// Primitive class descriptor used by TYPE metadata and shared adapters.
pub fn primitive_descriptor(class: &str) -> Option<&'static str> {
    Some(match class {
        BOOLEAN => "Z",
        BYTE => "B",
        SHORT => "S",
        CHARACTER => "C",
        INTEGER => "I",
        LONG => "J",
        FLOAT => "F",
        DOUBLE => "D",
        "Ljava/lang/Void;" => "V",
        _ => return None,
    })
}

/// Descriptor of a supported platform static field; lookup does not allocate.
pub fn static_field_descriptor(class: &str, field: &str) -> Option<&'static str> {
    Some(match (class, field) {
        (BOOLEAN, "TRUE" | "FALSE") => BOOLEAN,
        (INTEGER, "MIN_VALUE" | "MAX_VALUE") => "I",
        (LONG, "MIN_VALUE" | "MAX_VALUE") => "J",
        (FLOAT, "NaN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" | "MIN_VALUE" | "MAX_VALUE") => {
            "F"
        }
        (DOUBLE, "NaN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" | "MIN_VALUE" | "MAX_VALUE") => {
            "D"
        }
        (class, "TYPE") if primitive_descriptor(class).is_some() => "Ljava/lang/Class;",
        _ => return None,
    })
}

/// Exact declarations implemented by this module, shared with JNI lookup.
pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance, Static};
    let k = kind(class)?;
    if name == "<init>" {
        return ((sig == format!("({k})V"))
            || (k != 'C' && sig == "(Ljava/lang/String;)V")
            || (k == 'F' && sig == "(D)V"))
            .then_some(Constructor);
    }
    if matches!(
        (name, sig),
        ("toString", "()Ljava/lang/String;")
            | ("hashCode", "()I")
            | ("equals", "(Ljava/lang/Object;)Z")
    ) || (name == "compareTo"
        && (sig == "(Ljava/lang/Object;)I" || sig == format!("({class})I")))
        || (!matches!(k, 'Z' | 'C')
            && matches!(
                (name, sig),
                ("byteValue", "()B")
                    | ("shortValue", "()S")
                    | ("intValue", "()I")
                    | ("longValue", "()J")
                    | ("floatValue", "()F")
                    | ("doubleValue", "()D")
            ))
        || (k == 'Z' && (name, sig) == ("booleanValue", "()Z"))
        || (k == 'C' && (name, sig) == ("charValue", "()C"))
    {
        return Some(Instance);
    }
    if (name == "valueOf" && sig == format!("({k}){class}"))
        || (name == "hashCode" && sig == format!("({k})I"))
        || (name == "compare" && sig == format!("({k}{k})I"))
        || (name == "toString" && sig == format!("({k})Ljava/lang/String;"))
        || (k == 'Z'
            && matches!(
                (name, sig),
                ("parseBoolean", "(Ljava/lang/String;)Z")
                    | ("valueOf", "(Ljava/lang/String;)Ljava/lang/Boolean;")
                    | ("logicalAnd" | "logicalOr" | "logicalXor", "(ZZ)Z")
            ))
        || (k == 'F'
            && matches!(
                (name, sig),
                ("floatToIntBits" | "floatToRawIntBits", "(F)I")
                    | ("intBitsToFloat", "(I)F")
                    | ("isNaN" | "isInfinite" | "isFinite", "(F)Z")
            ))
        || (k == 'D'
            && matches!(
                (name, sig),
                ("doubleToLongBits" | "doubleToRawLongBits", "(D)J")
                    | ("longBitsToDouble", "(J)D")
                    | ("isNaN" | "isInfinite" | "isFinite", "(D)Z")
            ))
    {
        return Some(Static);
    }
    None
}

fn npe(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", msg))
}

/// Narrow an int to the wrapper's width (JLS 5.1.3), as `valueOf` sees it.
fn narrow(k: char, v: i32) -> i32 {
    match k {
        'Z' => (v != 0) as i32,
        'B' => v as i8 as i32,
        'S' => v as i16 as i32,
        'C' => v as u16 as i32,
        _ => v,
    }
}

/// Box `v` (already of the right primitive kind) into `class`.
pub fn box_value(vm: &mut Engine, class: &str, v: Value) -> Result<ObjRef, VmExit> {
    let k = kind(class).ok_or_else(|| npe("not a wrapper class"))?;
    if k == 'Z' {
        return boolean_obj(vm, v.as_int()? != 0);
    }
    let v = match k {
        'J' => Value::Long(v.as_long()?),
        'F' => Value::Float(v.as_float()?),
        'D' => Value::Double(v.as_double()?),
        _ => Value::Int(narrow(k, v.as_int()?)),
    };
    let cache = match v {
        Value::Int(n)
            if k == 'B'
                || (k == 'C' && (0..128).contains(&n))
                || (matches!(k, 'S' | 'I') && (-128..128).contains(&n)) =>
        {
            Some(n as i64)
        }
        Value::Long(n) if (-128..128).contains(&n) => Some(n),
        _ => None,
    }
    .map(|n| (class.to_string(), n));
    if let Some(r) = cache.as_ref().and_then(|key| vm.boxed_cache.get(key)) {
        return Ok(*r);
    }
    let object = vm
        .heap
        .alloc_instance(class.to_string(), vec![("value".to_string(), v)])
        .map_err(crate::err::oom_throwable)?;
    if let Some(key) = cache {
        vm.boxed_cache.insert(key, object);
    }
    Ok(object)
}

/// Canonical `Boolean.TRUE` / `Boolean.FALSE`. They are stored in the static
/// table, so `sget`, `valueOf` and `TRUE == valueOf(true)` all see the same
/// identity.
pub(crate) fn boolean_obj(vm: &mut Engine, b: bool) -> Result<ObjRef, VmExit> {
    let key = (
        BOOLEAN.to_string(),
        if b { "TRUE" } else { "FALSE" }.to_string(),
    );
    if let Some(Value::Obj(r)) = vm.statics.get(&key) {
        return Ok(*r);
    }
    let r = vm
        .heap
        .alloc_instance(
            BOOLEAN.to_string(),
            vec![("value".to_string(), Value::Int(b as i32))],
        )
        .map_err(crate::err::oom_throwable)?;
    vm.statics.insert(key, Value::Obj(r));
    Ok(r)
}

/// Host static constants of JDK classes that are not in any DEX. Called by
/// `sget` before falling back to the type default.
pub fn host_static(vm: &mut Engine, owner: &str, field: &str) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (owner, field) {
        (BOOLEAN, "TRUE") => Value::Obj(boolean_obj(vm, true)?),
        (BOOLEAN, "FALSE") => Value::Obj(boolean_obj(vm, false)?),
        (INTEGER, "MAX_VALUE") => Value::Int(i32::MAX),
        (INTEGER, "MIN_VALUE") => Value::Int(i32::MIN),
        (LONG, "MAX_VALUE") => Value::Long(i64::MAX),
        (LONG, "MIN_VALUE") => Value::Long(i64::MIN),
        (FLOAT, "NaN") => Value::Float(f32::NAN),
        (FLOAT, "POSITIVE_INFINITY") => Value::Float(f32::INFINITY),
        (FLOAT, "NEGATIVE_INFINITY") => Value::Float(f32::NEG_INFINITY),
        (FLOAT, "MAX_VALUE") => Value::Float(f32::MAX),
        (FLOAT, "MIN_VALUE") => Value::Float(f32::from_bits(1)),
        (DOUBLE, "NaN") => Value::Double(f64::NAN),
        (DOUBLE, "POSITIVE_INFINITY") => Value::Double(f64::INFINITY),
        (DOUBLE, "NEGATIVE_INFINITY") => Value::Double(f64::NEG_INFINITY),
        (DOUBLE, "MAX_VALUE") => Value::Double(f64::MAX),
        (DOUBLE, "MIN_VALUE") => Value::Double(f64::from_bits(1)),
        // Integer.TYPE == int.class, …
        (c, "TYPE") if kind(c).is_some() => {
            let k = kind(c).unwrap_or('V').to_string();
            Value::Obj(vm.class_object_for(&k)?)
        }
        ("Ljava/lang/Void;", "TYPE") => Value::Obj(vm.class_object_for("V")?),
        _ => return Ok(None),
    }))
}

fn float_bits(v: f32) -> i32 {
    if v.is_nan() {
        0x7fc0_0000
    } else {
        v.to_bits() as i32
    }
}

fn double_bits(v: f64) -> i64 {
    if v.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        v.to_bits() as i64
    }
}

fn long_hash(v: i64) -> i32 {
    (v ^ ((v as u64) >> 32) as i64) as i32
}

fn float_compare(a: f32, b: f32) -> i32 {
    // Float.compare: total order on floatToIntBits for the equal/NaN cases
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        float_bits(a).cmp(&float_bits(b)) as i32
    }
}

fn double_compare(a: f64, b: f64) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        double_bits(a).cmp(&double_bits(b)) as i32
    }
}

/// Hash of a primitive of kind `k` (the static `X.hashCode(prim)` contract).
fn prim_hash(k: char, v: &Value) -> Result<i32, VmExit> {
    Ok(match k {
        'Z' => {
            if v.as_int()? != 0 {
                1231
            } else {
                1237
            }
        }
        'J' => long_hash(v.as_long()?),
        'F' => float_bits(v.as_float()?),
        'D' => long_hash(double_bits(v.as_double()?)),
        _ => v.as_int()?,
    })
}

fn prim_compare(k: char, a: &Value, b: &Value) -> Result<i32, VmExit> {
    Ok(match k {
        'J' => a.as_long()?.cmp(&b.as_long()?) as i32,
        'F' => float_compare(a.as_float()?, b.as_float()?),
        'D' => double_compare(a.as_double()?, b.as_double()?),
        'B' | 'S' | 'C' => a.as_int()?.wrapping_sub(b.as_int()?),
        _ => a.as_int()?.cmp(&b.as_int()?) as i32,
    })
}

fn prim_to_string(k: char, v: &Value) -> Result<String, VmExit> {
    Ok(match k {
        'Z' => (v.as_int()? != 0).to_string(),
        'C' => char::from_u32(v.as_int()? as u32)
            .unwrap_or('\u{FFFD}')
            .to_string(),
        'J' => v.as_long()?.to_string(),
        'F' => crate::repr::java_float(v.as_float()?),
        'D' => crate::repr::java_double(v.as_double()?),
        _ => v.as_int()?.to_string(),
    })
}

/// Read a wrapper's primitive as f64/i64 for the `Number.xxxValue()` family.
fn numeric(k: char, v: &Value) -> Result<(i64, f64), VmExit> {
    Ok(match k {
        'J' => {
            let x = v.as_long()?;
            (x, x as f64)
        }
        'F' => {
            let x = v.as_float()? as f64;
            (java_d2l(x), x)
        }
        'D' => {
            let x = v.as_double()?;
            (java_d2l(x), x)
        }
        _ => {
            let x = v.as_int()? as i64;
            (x, x as f64)
        }
    })
}

/// JLS 5.1.3 double→long (saturating, NaN → 0). Rust `as` matches it.
fn java_d2l(x: f64) -> i64 {
    x as i64
}

fn int_value(k: char, v: &Value) -> Result<i32, VmExit> {
    Ok(match k {
        'F' => v.as_float()? as i32,
        'D' => v.as_double()? as i32,
        _ => numeric(k, v)?.0 as i32,
    })
}

/// Static methods of the wrappers. Returns `Ok(None)` when the call is not a
/// wrapper static this module models.
pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if method_metadata(class, name, sig) != Some(HostMethodKind::Static) {
        return Ok(None);
    }
    let Some(k) = kind(class) else {
        return Ok(None);
    };
    let p = k.to_string();
    let a0 = || args.first().cloned().unwrap_or(Value::Null);
    let v = match name {
        "valueOf" if sig == format!("({p}){class}") => Value::Obj(box_value(vm, class, a0())?),
        "hashCode" if sig == format!("({p})I") => Value::Int(prim_hash(k, &a0())?),
        "compare" if sig == format!("({p}{p})I") => {
            Value::Int(prim_compare(k, &args[0], &args[1])?)
        }
        "toString" if sig == format!("({p})Ljava/lang/String;") => {
            let s = prim_to_string(k, &a0())?;
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        "parseBoolean" | "valueOf" if k == 'Z' && sig.starts_with("(Ljava/lang/String;)") => {
            let b = match a0().as_ref()? {
                Some(r) => vm.heap.as_str(r)?.eq_ignore_ascii_case("true"),
                None => false,
            };
            if name == "parseBoolean" {
                Value::Int(b as i32)
            } else {
                Value::Obj(boolean_obj(vm, b)?)
            }
        }
        "logicalAnd" if k == 'Z' => {
            Value::Int(((args[0].as_int()? != 0) & (args[1].as_int()? != 0)) as i32)
        }
        "logicalOr" if k == 'Z' => {
            Value::Int(((args[0].as_int()? != 0) | (args[1].as_int()? != 0)) as i32)
        }
        "logicalXor" if k == 'Z' => {
            Value::Int(((args[0].as_int()? != 0) ^ (args[1].as_int()? != 0)) as i32)
        }
        "floatToIntBits" if k == 'F' => Value::Int(float_bits(args[0].as_float()?)),
        "floatToRawIntBits" if k == 'F' => Value::Int(args[0].as_float()?.to_bits() as i32),
        "intBitsToFloat" if k == 'F' => Value::Float(f32::from_bits(args[0].as_int()? as u32)),
        "doubleToLongBits" if k == 'D' => Value::Long(double_bits(args[0].as_double()?)),
        "doubleToRawLongBits" if k == 'D' => Value::Long(args[0].as_double()?.to_bits() as i64),
        "longBitsToDouble" if k == 'D' => Value::Double(f64::from_bits(args[0].as_long()? as u64)),
        "isNaN" if k == 'F' && sig == "(F)Z" => Value::Int(args[0].as_float()?.is_nan() as i32),
        "isNaN" if k == 'D' && sig == "(D)Z" => Value::Int(args[0].as_double()?.is_nan() as i32),
        "isInfinite" if k == 'F' && sig == "(F)Z" => {
            Value::Int(args[0].as_float()?.is_infinite() as i32)
        }
        "isInfinite" if k == 'D' && sig == "(D)Z" => {
            Value::Int(args[0].as_double()?.is_infinite() as i32)
        }
        "isFinite" if k == 'F' => Value::Int(args[0].as_float()?.is_finite() as i32),
        "isFinite" if k == 'D' => Value::Int(args[0].as_double()?.is_finite() as i32),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// Instance methods on a wrapper receiver. Returns `Ok(None)` when not modelled.
pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        method_metadata(recv_class, name, sig),
        Some(HostMethodKind::Instance | HostMethodKind::Constructor)
    ) {
        return Ok(None);
    }
    let Some(k) = kind(recv_class) else {
        return Ok(None);
    };
    // `new Float(f)`, `new Integer("12")`, …: the (deprecated but still
    // emitted) wrapper constructors.
    if name == "<init>" {
        let v = match args.first() {
            Some(Value::Obj(sr)) if sig == "(Ljava/lang/String;)V" => {
                let s = vm.heap.as_str(*sr)?.to_string();
                match k {
                    'Z' => Value::Int(s.eq_ignore_ascii_case("true") as i32),
                    'J' => Value::Long(crate::primitive_parse::parse_radix(
                        &s,
                        10,
                        i64::MIN as i128,
                        i64::MAX as i128,
                    )? as i64),
                    'F' => Value::Float(crate::primitive_parse::parse_float32(&s)?),
                    'D' => Value::Double(crate::primitive_parse::parse_float(&s)?),
                    _ => {
                        let (min, max) = match k {
                            'B' => (i8::MIN as i128, i8::MAX as i128),
                            'S' => (i16::MIN as i128, i16::MAX as i128),
                            _ => (i32::MIN as i128, i32::MAX as i128),
                        };
                        Value::Int(crate::primitive_parse::parse_radix(&s, 10, min, max)? as i32)
                    }
                }
            }
            Some(Value::Null | Value::Int(0)) if sig == "(Ljava/lang/String;)V" && k == 'Z' => {
                Value::Int(0)
            }
            Some(value) if sig == "(Ljava/lang/String;)V" => {
                if matches!(k, 'F' | 'D') {
                    return Err(npe("wrapper constructor string == null"));
                }
                let _ = crate::primitive_parse::string_argument(vm, value)?;
                return Ok(None);
            }
            Some(a) if k == 'F' && sig == "(D)V" => Value::Float(a.as_double()? as f32),
            Some(a) => match k {
                'J' => Value::Long(a.as_long()?),
                'F' => Value::Float(a.as_float()?),
                'D' => Value::Double(a.as_double()?),
                _ => Value::Int(narrow(k, a.as_int()?)),
            },
            None => return Ok(None),
        };
        crate::err::put_field_result(vm.heap.put_field(recv, "value", v))?;
        return Ok(Some(Value::Null));
    }
    let mine = vm.heap.get_field(recv, "value")?;
    let numeric_kind = !matches!(k, 'Z' | 'C');
    let v = match (name, sig) {
        ("booleanValue", "()Z") if k == 'Z' => Value::Int((mine.as_int()? != 0) as i32),
        ("charValue", "()C") if k == 'C' => Value::Int(mine.as_int()?),
        ("intValue", "()I") if numeric_kind => Value::Int(int_value(k, &mine)?),
        ("longValue", "()J") if numeric_kind => Value::Long(numeric(k, &mine)?.0),
        ("shortValue", "()S") if numeric_kind => Value::Int(int_value(k, &mine)? as i16 as i32),
        ("byteValue", "()B") if numeric_kind => Value::Int(int_value(k, &mine)? as i8 as i32),
        ("floatValue", "()F") if numeric_kind => Value::Float(match k {
            'F' => mine.as_float()?,
            'J' => mine.as_long()? as f32,
            _ => numeric(k, &mine)?.1 as f32,
        }),
        ("doubleValue", "()D") if numeric_kind => Value::Double(numeric(k, &mine)?.1),
        ("hashCode", "()I") => Value::Int(prim_hash(k, &mine)?),
        ("toString", "()Ljava/lang/String;") => {
            let s = prim_to_string(k, &mine)?;
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match args.first().map(|a| a.as_ref()).transpose()?.flatten() {
                Some(r) if vm.heap.class_of(r)? == recv_class => {
                    let other = vm.heap.get_field(r, "value")?;
                    match k {
                        'J' => mine.as_long()? == other.as_long()?,
                        'F' => float_bits(mine.as_float()?) == float_bits(other.as_float()?),
                        'D' => double_bits(mine.as_double()?) == double_bits(other.as_double()?),
                        _ => mine.as_int()? == other.as_int()?,
                    }
                }
                _ => false,
            };
            Value::Int(eq as i32)
        }
        ("compareTo", s) if s == "(Ljava/lang/Object;)I" || s == format!("({recv_class})I") => {
            let Some(r) = args.first().map(|a| a.as_ref()).transpose()?.flatten() else {
                return Err(npe("compareTo(null)"));
            };
            if vm.heap.class_of(r)? != recv_class {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ClassCastException;",
                    format!(
                        "{} cannot be compared to {recv_class}",
                        vm.heap.class_of(r)?
                    ),
                )));
            }
            let other = vm.heap.get_field(r, "value")?;
            Value::Int(prim_compare(k, &mine, &other)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_double_semantics_match_jdk() {
        assert_eq!(float_compare(-0.0, 0.0), -1);
        assert_eq!(float_compare(f32::NAN, f32::INFINITY), 1);
        assert_eq!(float_compare(f32::NAN, f32::NAN), 0);
        assert_eq!(double_compare(0.0, -0.0), 1);
        assert_eq!(double_bits(f64::NAN), 0x7ff8_0000_0000_0000);
        assert_eq!(long_hash(1), 1);
        assert_eq!(long_hash(-1), 0);
        assert_eq!(prim_hash('Z', &Value::Int(1)).unwrap(), 1231);
        assert_eq!(prim_hash('Z', &Value::Int(0)).unwrap(), 1237);
        // Double.hashCode(1.0) == 1072693248 on the JVM
        assert_eq!(prim_hash('D', &Value::Double(1.0)).unwrap(), 1_072_693_248);
        // Float.hashCode(1.0f) == 1065353216
        assert_eq!(prim_hash('F', &Value::Float(1.0)).unwrap(), 1_065_353_216);
        assert_eq!(narrow('B', 200), -56);
        assert_eq!(narrow('C', -1), 0xffff);
    }
}
