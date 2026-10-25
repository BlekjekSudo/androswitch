//! RustyDroid numeric parsers extracted from numerics.rs for both VM builds.
//! Decimal/radix ASCII parsing is shared. Unported Unicode digits and Java
//! hexadecimal floating literals produce named capability errors.
use crate::{Engine, Throwable, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
};

fn nfe(s: &str) -> VmExit {
    Throwable::new(
        "Ljava/lang/NumberFormatException;",
        format!("For input string: \"{s}\""),
    )
    .into()
}

pub(crate) fn parse_radix(s: &str, radix: i32, min: i128, max: i128) -> Result<i128, VmExit> {
    if !(2..=36).contains(&radix) || s.is_empty() {
        return Err(nfe(s));
    }
    if !s.is_ascii() {
        return Err(
            crate::err::vm_error("API_UNAVAILABLE", "numeric parsing: Unicode digits").into(),
        );
    }
    let (neg, digits) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(nfe(s));
    }
    let mut acc = 0i128;
    for c in digits.chars() {
        let d = c.to_digit(radix as u32).ok_or_else(|| nfe(s))? as i128;
        acc = acc
            .checked_mul(radix as i128)
            .and_then(|n| n.checked_add(d))
            .ok_or_else(|| nfe(s))?;
        if acc > max + 1 {
            return Err(nfe(s));
        }
    }
    let value = if neg { -acc } else { acc };
    if value < min || value > max {
        return Err(nfe(s));
    }
    Ok(value)
}

pub(crate) fn parse_float(t: &str) -> Result<f64, VmExit> {
    parse_floating(t, f64::NAN, f64::INFINITY, f64::NEG_INFINITY)
}

pub(crate) fn parse_float32(t: &str) -> Result<f32, VmExit> {
    parse_floating(t, f32::NAN, f32::INFINITY, f32::NEG_INFINITY)
}

fn parse_floating<T: core::str::FromStr>(
    t: &str,
    nan: T,
    infinity: T,
    negative_infinity: T,
) -> Result<T, VmExit> {
    let s = t.trim_matches(|c: char| c <= ' ');
    match s {
        "NaN" | "+NaN" | "-NaN" => return Ok(nan),
        "Infinity" | "+Infinity" => return Ok(infinity),
        "-Infinity" => return Ok(negative_infinity),
        _ => {}
    }
    let core = s.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(s);
    let unsigned = core.trim_start_matches(['+', '-']);
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        return Err(crate::err::vm_error(
            "API_UNAVAILABLE",
            "numeric parsing: hexadecimal floating literals",
        )
        .into());
    }
    if core.is_empty() || core.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return Err(nfe(t));
    }
    core.parse::<T>().map_err(|_| nfe(t))
}

/// Get a real String reference, preserving Java's numeric null-string error.
pub(crate) fn string_argument(vm: &Engine, value: &Value) -> Result<String, VmExit> {
    match value.as_ref()? {
        Some(r) => Ok(vm.heap.as_str(r)?.to_string()),
        None => Err(nfe("null")),
    }
}
