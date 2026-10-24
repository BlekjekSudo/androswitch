//! Bounded, nonlocalized `String.format` subset for the portable VM.
//! Reference: Android API26 java.util.Formatter. Locale, floating-point,
//! dates and Formattable callbacks remain explicit unsupported contracts.
use crate::{err, Engine, Throwable, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

const LIMIT: usize = 64 << 10;
fn exception(kind: &str, detail: impl Into<String>) -> VmExit {
    Throwable::new(&format!("Ljava/util/{kind};"), detail).into()
}
fn budget() -> VmExit {
    err::vm_error(
        "FORMAT_BUDGET",
        "String.format: 64 KiB input/output/field, 4096 arguments",
    )
    .into()
}
fn unsupported(detail: &str) -> VmExit {
    err::vm_error("API_UNAVAILABLE", format!("String.format: {detail}")).into()
}
fn append(out: &mut String, text: &str) -> Result<(), VmExit> {
    if text.len() > LIMIT.saturating_sub(out.len()) {
        return Err(budget());
    }
    out.push_str(text);
    Ok(())
}
fn number(chars: &[char], cursor: &mut usize) -> Result<Option<usize>, VmExit> {
    let start = *cursor;
    let mut n = 0usize;
    while let Some(c) = chars.get(*cursor).filter(|c| c.is_ascii_digit()) {
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add(*c as usize - '0' as usize))
            .ok_or_else(budget)?;
        if n > LIMIT {
            return Err(budget());
        }
        *cursor += 1;
    }
    Ok((*cursor != start).then_some(n))
}
fn boxed(vm: &Engine, value: &Value) -> Result<Option<(String, Value)>, VmExit> {
    let Value::Obj(r) = value else {
        return Ok(None);
    };
    let class = vm.heap.class_of(*r)?;
    if matches!(
        class,
        "Ljava/lang/Byte;"
            | "Ljava/lang/Short;"
            | "Ljava/lang/Integer;"
            | "Ljava/lang/Long;"
            | "Ljava/lang/Character;"
            | "Ljava/lang/Boolean;"
            | "Ljava/lang/Float;"
            | "Ljava/lang/Double;"
    ) {
        return Ok(Some((class.to_string(), vm.heap.get_field(*r, "value")?)));
    }
    Ok(None)
}
fn text(vm: &mut Engine, value: &Value) -> Result<String, VmExit> {
    if matches!(value, Value::Null) {
        return Ok("null".into());
    }
    if let Value::Obj(r) = value {
        if vm
            .cp
            .is_subtype(vm.heap.class_of(*r)?, "Ljava/util/Formattable;")
        {
            return Err(unsupported("Formattable.formatTo callback"));
        }
    }
    if let Some((class, inner)) = boxed(vm, value)? {
        return Ok(match (class.as_str(), inner) {
            ("Ljava/lang/Boolean;", Value::Int(x)) => (x != 0).to_string(),
            ("Ljava/lang/Character;", Value::Int(x)) => char::from_u32(x as u32)
                .ok_or_else(|| unsupported("unpaired UTF-16 surrogate"))?
                .to_string(),
            (_, Value::Int(x)) => x.to_string(),
            (_, Value::Long(x)) => x.to_string(),
            // Java Float/Double formatting has different shortest-decimal rules.
            _ => return Err(unsupported("floating-point toString")),
        });
    }
    let Value::Obj(r) = value else {
        return Err(exception(
            "IllegalFormatConversionException",
            "Object[] contains non-reference value",
        ));
    };
    if let Ok(s) = vm.heap.as_str(*r) {
        if s.len() > LIMIT {
            return Err(budget());
        }
        return Ok(s.to_string());
    }
    let result = vm.invoke_instance(*r, "toString", "()Ljava/lang/String;", &[])?;
    match result {
        Value::Null => Ok("null".into()),
        Value::Obj(s) => {
            let s = vm.heap.as_str(s)?;
            if s.len() > LIMIT {
                return Err(budget());
            }
            Ok(s.to_string())
        }
        _ => Err(err::vm_error("VM_TYPE_ERROR", "toString did not return String").into()),
    }
}
fn integer(vm: &Engine, value: &Value, conversion: char) -> Result<(i64, u32), VmExit> {
    if let Some((class, value)) = boxed(vm, value)? {
        let bits = match class.as_str() {
            "Ljava/lang/Byte;" => 8,
            "Ljava/lang/Short;" => 16,
            "Ljava/lang/Integer;" => 32,
            "Ljava/lang/Long;" => 64,
            _ => {
                return Err(exception(
                    "IllegalFormatConversionException",
                    conversion.to_string(),
                ))
            }
        };
        return match value {
            Value::Int(n) => Ok((n as i64, bits)),
            Value::Long(n) => Ok((n, bits)),
            _ => Err(exception(
                "IllegalFormatConversionException",
                conversion.to_string(),
            )),
        };
    }
    Err(exception(
        "IllegalFormatConversionException",
        conversion.to_string(),
    ))
}
fn pad(body: &str, width: usize, left: bool) -> String {
    let count = body.encode_utf16().count();
    let spaces = " ".repeat(width.saturating_sub(count));
    if left {
        format!("{body}{spaces}")
    } else {
        format!("{spaces}{body}")
    }
}
fn truncate(mut body: String, precision: Option<usize>) -> Result<String, VmExit> {
    if let Some(precision) = precision {
        let mut count = 0;
        for (offset, c) in body.char_indices() {
            if count == precision {
                body.truncate(offset);
                return Ok(body);
            }
            count += c.len_utf16();
            if count > precision {
                return Err(unsupported("precision splits UTF-16 surrogate pair"));
            }
        }
    }
    Ok(body)
}

pub(crate) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != "Ljava/lang/String;" || name != "format" {
        return Ok(None);
    }
    let (format_index, array_index) = match sig {
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;" => (0, 1),
        "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;" => {
            (1, 2) // Explicit null Locale means no localization in Java.
        }
        _ => return Ok(None),
    };
    if args.len() != array_index + 1 {
        return Err(err::vm_error("INVALID_ARG", "String.format argument count").into());
    }
    if format_index == 1 && args[0].as_ref()?.is_some() {
        return Err(unsupported("Locale formatting"));
    }
    let pattern = args[format_index]
        .as_ref()?
        .ok_or_else(|| Throwable::new("Ljava/lang/NullPointerException;", "format"))?;
    let pattern = vm.heap.as_str(pattern)?;
    if pattern.len() > LIMIT {
        return Err(budget());
    }
    let pattern = pattern.to_string();
    let items = match args[array_index].as_ref()? {
        None => None, // Java treats a null varargs array as null at every index.
        Some(array) => {
            let (elements, kind) = vm.heap.as_array_elems(array)?;
            if kind != crate::heap::ElemKind::Obj {
                return Err(err::vm_error("VM_TYPE_ERROR", "format requires Object[]").into());
            }
            if elements.len() > 4096 {
                return Err(budget());
            }
            Some(elements.to_vec())
        }
    };
    let result = render(vm, &pattern, items.as_deref())?;
    Ok(Some(Value::Obj(vm.heap.alloc_string(result)?)))
}

fn render(vm: &mut Engine, pattern: &str, args: Option<&[Value]>) -> Result<String, VmExit> {
    let chars: Vec<_> = pattern.chars().collect();
    let mut at = 0;
    let mut next = 0;
    let mut previous = None;
    let mut out = String::new();
    while at < chars.len() {
        if chars[at] != '%' {
            let mut utf8 = [0; 4];
            append(&mut out, chars[at].encode_utf8(&mut utf8))?;
            at += 1;
            continue;
        }
        at += 1;
        let start = at;
        let candidate = number(&chars, &mut at)?;
        let explicit =
            if let Some(candidate) = candidate.filter(|_| chars.get(at) == Some(&'$')) {
                at += 1;
                Some(candidate.checked_sub(1).ok_or_else(|| {
                    exception("MissingFormatArgumentException", "argument index0")
                })?)
            } else {
                at = start;
                None
            };
        let mut flags = String::new();
        while let Some(c) = chars.get(at).filter(|c| "-#+ 0,(<".contains(**c)) {
            if flags.contains(*c) {
                return Err(exception("DuplicateFormatFlagsException", c.to_string()));
            }
            flags.push(*c);
            at += 1;
        }
        let width = number(&chars, &mut at)?;
        let precision = if chars.get(at) == Some(&'.') {
            at += 1;
            Some(
                number(&chars, &mut at)?
                    .ok_or_else(|| exception("UnknownFormatConversionException", "."))?,
            )
        } else {
            None
        };
        let conversion = *chars
            .get(at)
            .ok_or_else(|| exception("UnknownFormatConversionException", "%"))?;
        at += 1;
        if !"sSbBcCdoxX%n".contains(conversion) {
            return Err(if "hHfeEgGaAtT".contains(conversion) {
                unsupported("conversion requires additional Formatter contract")
            } else {
                exception("UnknownFormatConversionException", conversion.to_string())
            });
        }
        if (flags.contains('-') && flags.contains('0'))
            || (flags.contains('+') && flags.contains(' '))
        {
            return Err(exception("IllegalFormatFlagsException", flags));
        }
        if (flags.contains('-') || flags.contains('0')) && width.is_none() {
            return Err(exception(
                "MissingFormatWidthException",
                conversion.to_string(),
            ));
        }
        if conversion == 'n' {
            if !flags.is_empty() {
                return Err(exception("IllegalFormatFlagsException", flags));
            }
            if width.is_some() {
                return Err(exception("IllegalFormatWidthException", "newline width"));
            }
            if precision.is_some() {
                return Err(exception(
                    "IllegalFormatPrecisionException",
                    "newline precision",
                ));
            }
            append(&mut out, "\n")?;
            continue;
        }
        let allowed = match conversion {
            'd' => "-+ 0,(<",
            'o' | 'x' | 'X' => "-#0<",
            _ => "-<",
        };
        if flags.chars().any(|c| !allowed.contains(c)) {
            return Err(exception("FormatFlagsConversionMismatchException", flags));
        }
        if flags.contains(',') {
            return Err(unsupported("locale-dependent grouping"));
        }
        if precision.is_some() && !"sSbB".contains(conversion) {
            return Err(exception("IllegalFormatPrecisionException", "precision"));
        }
        if conversion == '%' {
            if flags.contains('<') {
                return Err(exception("IllegalFormatFlagsException", flags));
            }
            append(&mut out, &pad("%", width.unwrap_or(0), flags.contains('-')))?;
            continue;
        }
        let index = if flags.contains('<') {
            previous
                .ok_or_else(|| exception("MissingFormatArgumentException", "relative argument"))?
        } else if let Some(index) = explicit {
            index
        } else {
            let index = next;
            next += 1;
            index
        };
        previous = Some(index);
        let value = match args {
            None => Value::Null,
            Some(args) => args.get(index).cloned().ok_or_else(|| {
                exception("MissingFormatArgumentException", conversion.to_string())
            })?,
        };
        // Dalvik const/4 0 is a null reference at this Object[] use site.
        let value = match value.as_ref()? {
            Some(object) => Value::Obj(object),
            None => Value::Null,
        };
        let width = width.unwrap_or(0);
        let body = if "sSbB".contains(conversion) {
            let value = if "sS".contains(conversion) {
                text(vm, &value)?
            } else {
                let boolean = match &value {
                    Value::Null => false,
                    _ => {
                        !matches!(boxed(vm,&value)?,Some((class,Value::Int(0))) if class=="Ljava/lang/Boolean;")
                    }
                };
                boolean.to_string()
            };
            let value = truncate(value, precision)?;
            let value = if conversion.is_ascii_uppercase() {
                if !value.is_ascii() {
                    return Err(unsupported("locale-dependent uppercase"));
                }
                value.to_ascii_uppercase()
            } else {
                value
            };
            pad(&value, width, flags.contains('-'))
        } else if matches!(value, Value::Null) {
            pad(
                if conversion.is_ascii_uppercase() {
                    "NULL"
                } else {
                    "null"
                },
                width,
                flags.contains('-'),
            )
        } else if "cC".contains(conversion) {
            let Some((class, inner)) = boxed(vm, &value)? else {
                return Err(exception(
                    "IllegalFormatConversionException",
                    conversion.to_string(),
                ));
            };
            if !matches!(
                class.as_str(),
                "Ljava/lang/Character;"
                    | "Ljava/lang/Byte;"
                    | "Ljava/lang/Short;"
                    | "Ljava/lang/Integer;"
            ) {
                return Err(exception(
                    "IllegalFormatConversionException",
                    conversion.to_string(),
                ));
            }
            let n = inner.as_int()?;
            if !(0..=0x10ffff).contains(&n) {
                return Err(exception("IllegalFormatCodePointException", n.to_string()));
            }
            let c =
                char::from_u32(n as u32).ok_or_else(|| unsupported("unpaired UTF-16 surrogate"))?;
            let body = if conversion == 'C' {
                if !c.is_ascii() {
                    return Err(unsupported("locale-dependent uppercase"));
                }
                c.to_ascii_uppercase().to_string()
            } else {
                c.to_string()
            };
            pad(&body, width, flags.contains('-'))
        } else {
            let (number, bits) = integer(vm, &value, conversion)?;
            let (prefix, digits, suffix) = if conversion == 'd' {
                let (prefix, suffix) = if number < 0 {
                    if flags.contains('(') {
                        ("(", ")")
                    } else {
                        ("-", "")
                    }
                } else if flags.contains('+') {
                    ("+", "")
                } else if flags.contains(' ') {
                    (" ", "")
                } else {
                    ("", "")
                };
                (prefix, number.unsigned_abs().to_string(), suffix)
            } else {
                let unsigned = if bits == 64 {
                    number as u64
                } else {
                    number as u64 & ((1u64 << bits) - 1)
                };
                let digits = match conversion {
                    'o' => format!("{unsigned:o}"),
                    'x' => format!("{unsigned:x}"),
                    _ => format!("{unsigned:X}"),
                };
                let prefix = if flags.contains('#') {
                    match conversion {
                        'o' => "0",
                        'x' => "0x",
                        _ => "0X",
                    }
                } else {
                    ""
                };
                (prefix, digits, "")
            };
            let body = if flags.contains('0') {
                format!(
                    "{prefix}{}{digits}{suffix}",
                    "0".repeat(width.saturating_sub(prefix.len() + digits.len() + suffix.len()))
                )
            } else {
                format!("{prefix}{digits}{suffix}")
            };
            pad(&body, width, flags.contains('-'))
        };
        append(&mut out, &body)?;
    }
    Ok(out)
}
