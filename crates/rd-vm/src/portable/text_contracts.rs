//! Shared Java text/numeric/array contracts, independent of APK identity.
use crate::{err, intrinsics, Engine, ObjRef, Throwable, Value, VmExit};
#[cfg(not(feature = "std"))]
use alloc::format;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
type Result<T> = core::result::Result<T, VmExit>;
fn exception(class: &str, detail: &str) -> VmExit {
    Throwable::new(class, detail).into()
}
fn object(value: &Value) -> Result<ObjRef> {
    value
        .as_ref()?
        .ok_or_else(|| exception("Ljava/lang/NullPointerException;", "null argument"))
}
fn text(vm: &Engine, value: &Value) -> Result<String> {
    Ok(vm.heap.as_str(object(value)?)?.into())
}
fn out(vm: &mut Engine, value: String) -> Result<Option<Value>> {
    Ok(Some(Value::Obj(intrinsics::alloc_string(vm, value)?)))
}
fn utf16(value: &[u16]) -> Result<String> {
    String::from_utf16(value).map_err(|_| {
        err::vm_error(
            "API_UNAVAILABLE",
            "isolated UTF16 surrogate requires a UTF16 String heap",
        )
        .into()
    })
}
pub(super) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>> {
    // The shipping std VM uses the complete inherited numerics family.
    // Retain the earlier small core-only fallback for the isolated parser VM.
    #[cfg(not(feature = "std"))]
    {
        let number = match (class, name, sig) {
            ("Ljava/lang/Double;", "toString", "(D)Ljava/lang/String;") => {
                Some(crate::repr::java_double(args[0].as_double()?))
            }
            ("Ljava/lang/Float;", "toString", "(F)Ljava/lang/String;") => {
                Some(crate::repr::java_float(args[0].as_float()?))
            }
            ("Ljava/lang/Integer;", "toHexString", "(I)Ljava/lang/String;") => {
                Some(format!("{:x}", args[0].as_int()? as u32))
            }
            ("Ljava/lang/Long;", "toHexString", "(J)Ljava/lang/String;") => {
                Some(format!("{:x}", args[0].as_long()? as u64))
            }
            _ => None,
        };
        if let Some(value) = number {
            return out(vm, value);
        }
        if matches!(class, "Ljava/lang/Math;" | "Ljava/lang/StrictMath;") && name == "abs" {
            return Ok(Some(match sig {
                "(I)I" => Value::Int(args[0].as_int()?.wrapping_abs()),
                "(J)J" => Value::Long(args[0].as_long()?.wrapping_abs()),
                "(F)F" => Value::Float(args[0].as_float()?.abs()),
                "(D)D" => Value::Double(args[0].as_double()?.abs()),
                _ => return Ok(None),
            }));
        }
        if matches!(class, "Ljava/lang/Math;" | "Ljava/lang/StrictMath;")
            && matches!(name, "min" | "max")
        {
            // Rust's floating min/max suppress a single NaN; Java propagates it.
            // Equal signed zero also has an ordering in Java (negative < positive).
            let minimum = name == "min";
            macro_rules! floating {
                ($a:expr, $b:expr, $kind:ident) => {{
                    let (a, b) = ($a, $b);
                    Value::$kind(if a.is_nan() {
                        a
                    } else if b.is_nan() {
                        b
                    } else if a == 0.0 && b == 0.0 {
                        if minimum {
                            if a.is_sign_negative() || b.is_sign_negative() {
                                -0.0
                            } else {
                                0.0
                            }
                        } else if a.is_sign_negative() && b.is_sign_negative() {
                            a
                        } else {
                            0.0
                        }
                    } else if (minimum && a < b) || (!minimum && a > b) {
                        a
                    } else {
                        b
                    })
                }};
            }
            return Ok(Some(match sig {
                "(II)I" => {
                    let (a, b) = (args[0].as_int()?, args[1].as_int()?);
                    Value::Int(if minimum { a.min(b) } else { a.max(b) })
                }
                "(JJ)J" => {
                    let (a, b) = (args[0].as_long()?, args[1].as_long()?);
                    Value::Long(if minimum { a.min(b) } else { a.max(b) })
                }
                "(FF)F" => floating!(args[0].as_float()?, args[1].as_float()?, Float),
                "(DD)D" => floating!(args[0].as_double()?, args[1].as_double()?, Double),
                _ => return Ok(None),
            }));
        }
    }
    if (class, name, sig)
        == (
            "Ljava/lang/System;",
            "arraycopy",
            "(Ljava/lang/Object;ILjava/lang/Object;II)V",
        )
    {
        let source = object(&args[0])?;
        let target = object(&args[2])?;
        let source_type = vm.heap.class_of(source)?.to_string();
        let target_type = vm.heap.class_of(target)?.to_string();
        if !source_type.starts_with('[') || !target_type.starts_with('[') {
            return Err(exception(
                "Ljava/lang/ArrayStoreException;",
                "arraycopy requires arrays",
            ));
        }
        let (source_values, source_kind) = vm.heap.as_array_elems(source)?;
        let (target_values, target_kind) = vm.heap.as_array_elems(target)?;
        let references =
            source_kind == crate::heap::ElemKind::Obj && target_kind == crate::heap::ElemKind::Obj;
        if !references && source_type != target_type {
            return Err(exception(
                "Ljava/lang/ArrayStoreException;",
                "primitive array component mismatch",
            ));
        }
        let (a, b, n) = (args[1].as_int()?, args[3].as_int()?, args[4].as_int()?);
        if a < 0
            || b < 0
            || n < 0
            || (a as usize)
                .checked_add(n as usize)
                .map_or(true, |end| end > source_values.len())
            || (b as usize)
                .checked_add(n as usize)
                .map_or(true, |end| end > target_values.len())
        {
            return Err(exception(
                "Ljava/lang/IndexOutOfBoundsException;",
                "arraycopy range",
            ));
        }
        // Snapshot before writing: overlapping ranges use memmove semantics.
        let copy = source_values[a as usize..a as usize + n as usize].to_vec();
        for (index, value) in copy.into_iter().enumerate() {
            if references {
                if let Some(r) = value.as_ref()? {
                    if !vm.cp.is_subtype(vm.heap.class_of(r)?, &target_type[1..]) {
                        return Err(exception(
                            "Ljava/lang/ArrayStoreException;",
                            "reference array component mismatch",
                        ));
                    }
                }
            }
            vm.heap.array_elems_mut(target)?[b as usize + index] = value;
        }
        return Ok(Some(Value::Null));
    }
    Ok(None)
}
pub(super) fn call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    if class != "Ljava/lang/String;" {
        return Ok(None);
    }
    if name == "<init>" {
        let value = match sig {
            "()V" => String::new(),
            "(Ljava/lang/String;)V" => text(vm, &args[0])?,
            "([C)V" => {
                let values = vm.heap.as_array_elems(object(&args[0])?)?.0;
                let units = values
                    .iter()
                    .map(|v| v.as_int().map(|v| v as u16))
                    .collect::<core::result::Result<Vec<_>, _>>()?;
                utf16(&units)?
            }
            _ => return Ok(None),
        };
        vm.heap.initialize_string(receiver, value)?;
        return Ok(Some(Value::Null));
    }
    if !matches!(
        (name, sig),
        (
            "substring",
            "(II)Ljava/lang/String;" | "(I)Ljava/lang/String;"
        ) | ("indexOf" | "lastIndexOf", "(Ljava/lang/String;)I")
            | ("startsWith" | "endsWith", "(Ljava/lang/String;)Z")
            | ("replace", "(CC)Ljava/lang/String;")
            | ("codePointAt", "(I)I")
    ) {
        return Ok(None);
    }
    let value = vm.heap.as_str(receiver)?.to_string();
    let units: Vec<u16> = value.encode_utf16().collect();
    let result = match (name, sig) {
        ("substring", "(II)Ljava/lang/String;") | ("substring", "(I)Ljava/lang/String;") => {
            let start = args[0].as_int()?;
            let end = if args.len() == 1 {
                units.len() as i32
            } else {
                args[1].as_int()?
            };
            if start < 0 || end < start || end as usize > units.len() {
                return Err(exception(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    "substring range",
                ));
            }
            return out(vm, utf16(&units[start as usize..end as usize])?);
        }
        ("indexOf" | "lastIndexOf", "(Ljava/lang/String;)I") => {
            let needle: Vec<_> = text(vm, &args[0])?.encode_utf16().collect();
            let mut positions = 0..=units.len().saturating_sub(needle.len());
            let valid = |i: &usize| {
                units.get(*i..i.saturating_add(needle.len())) == Some(needle.as_slice())
            };
            let at = if name == "indexOf" {
                positions.find(valid)
            } else {
                positions.rfind(valid)
            };
            Value::Int(at.map_or(-1, |i| i as i32))
        }
        ("startsWith" | "endsWith", "(Ljava/lang/String;)Z") => {
            let needle = text(vm, &args[0])?;
            Value::Int(i32::from(if name == "startsWith" {
                value.starts_with(&needle)
            } else {
                value.ends_with(&needle)
            }))
        }
        ("replace", "(CC)Ljava/lang/String;") => {
            let from = args[0].as_int()? as u16;
            let to = args[1].as_int()? as u16;
            let replaced: Vec<_> = units
                .iter()
                .map(|unit| if *unit == from { to } else { *unit })
                .collect();
            return out(vm, utf16(&replaced)?);
        }
        ("codePointAt", "(I)I") => {
            let index = args[0].as_int()?;
            let first = *units.get(index as usize).ok_or_else(|| {
                exception(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    "codePointAt index",
                )
            })?;
            let point = if (0xd800..=0xdbff).contains(&first) {
                match units.get(index as usize + 1) {
                    Some(second) if (0xdc00..=0xdfff).contains(second) => {
                        0x10000 + ((first as u32 - 0xd800) << 10) + (*second as u32 - 0xdc00)
                    }
                    _ => first as u32,
                }
            } else {
                first as u32
            };
            Value::Int(point as i32)
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}
