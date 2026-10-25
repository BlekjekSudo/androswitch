//! `java.math.BigDecimal`, `java.math.BigInteger` and `java.math.RoundingMode`.
//!
//! A number is an unscaled `i128` plus an `i32` scale (BigInteger has scale
//! 0), kept per object in `Engine::bignums`. Value = unscaled × 10^-scale.
//! This is exact for money and ordinary decimal arithmetic (38 significant
//! digits). Results outside the i128 range throw `ArithmeticException`, a
//! documented deviation from the arbitrary-precision JDK classes.
//!
//! JDK semantics that are kept:
//! - `add`/`subtract` use the larger scale and `multiply` the sum of scales.
//! - `divide(d)` is exact with preferred scale `this.scale - d.scale`, and
//!   throws "Non-terminating decimal expansion" otherwise.
//! - `divide(d, scale, mode)` and `setScale` round with all eight
//!   `RoundingMode`s; `UNNECESSARY` throws when rounding is needed.
//! - `equals` is scale-sensitive (2.0 ≠ 2.00); `compareTo` is not.
//! - `toString` uses scientific notation exactly when the JDK does (scale < 0
//!   or adjusted exponent < -6); `toPlainString` never does.
//! - `stripTrailingZeros` maps zero to `0` with scale 0 (JDK 8+), and
//!   `movePointLeft`/`movePointRight` never return a negative scale.
//!
//! Calls are keyed on the receiver (`Engine::bignums`), so `Object.equals`,
//! `Number.longValue` and `Comparable.compareTo` reach this model whatever
//! class the method ref names.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const BD: &str = "Ljava/math/BigDecimal;";
pub(crate) const BI: &str = "Ljava/math/BigInteger;";
pub(crate) const RM: &str = "Ljava/math/RoundingMode;";

const MODES: [&str; 8] = [
    "UP",
    "DOWN",
    "CEILING",
    "FLOOR",
    "HALF_UP",
    "HALF_DOWN",
    "HALF_EVEN",
    "UNNECESSARY",
];

fn arith(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/ArithmeticException;", msg))
}

fn overflow() -> VmExit {
    arith("BigDecimal overflow (RustyDroid: 128-bit unscaled value)")
}

fn pow10(n: u32) -> Result<i128, VmExit> {
    10i128.checked_pow(n).ok_or_else(overflow)
}

fn mul(a: i128, b: i128) -> Result<i128, VmExit> {
    a.checked_mul(b).ok_or_else(overflow)
}

/// `u × 10^k` (k ≥ 0).
fn upscale(u: i128, k: i64) -> Result<i128, VmExit> {
    if k <= 0 {
        return Ok(u);
    }
    let k = u32::try_from(k).map_err(|_| overflow())?;
    mul(u, pow10(k)?)
}

fn npe(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", msg))
}

fn nfe(msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/NumberFormatException;", msg))
}

fn iae(msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/IllegalArgumentException;", msg))
}

fn non_null_str(vm: &Engine, v: Option<&Value>, label: &str) -> Result<String, VmExit> {
    let Some(val @ Value::Obj(r)) = v else {
        return Err(npe(label));
    };
    if *r == 0 {
        return Err(npe(label));
    }
    crate::intrinsics::string_arg(vm, val)
}

/// `n / d` rounded with a JDK RoundingMode name.
fn div_round(n: i128, d: i128, mode: &str) -> Result<i128, VmExit> {
    if d == 0 {
        return Err(arith("Division by zero"));
    }
    let q = n.checked_div(d).ok_or_else(overflow)?;
    let r = n.checked_rem(d).ok_or_else(overflow)?;
    if r == 0 {
        return Ok(q);
    }
    let sign: i128 = if (n < 0) != (d < 0) { -1 } else { 1 };
    let twice = r.unsigned_abs().saturating_mul(2);
    let cmp = twice.cmp(&d.unsigned_abs());
    use std::cmp::Ordering::*;
    let inc = match mode {
        "UP" => true,
        "DOWN" => false,
        "CEILING" => sign > 0,
        "FLOOR" => sign < 0,
        "HALF_UP" => cmp != Less,
        "HALF_DOWN" => cmp == Greater,
        "UNNECESSARY" => return Err(arith("Rounding necessary")),
        // HALF_EVEN (also the default)
        _ => cmp == Greater || (cmp == Equal && q % 2 != 0),
    };
    if inc {
        q.checked_add(sign).ok_or_else(overflow)
    } else {
        Ok(q)
    }
}

/// Rescale `(u, s)` to `new_s`, rounding when digits are dropped.
fn rescale(u: i128, s: i32, new_s: i32, mode: &str) -> Result<i128, VmExit> {
    if new_s >= s {
        upscale(u, new_s as i64 - s as i64)
    } else {
        let k = u32::try_from(s as i64 - new_s as i64).map_err(|_| overflow())?;
        match 10i128.checked_pow(k) {
            Some(p) => div_round(u, p, mode),
            // dividing by more than 10^38: the quotient is 0 (± rounding)
            None => div_round(u.signum(), i128::MAX, mode),
        }
    }
}

fn align(a: (i128, i32), b: (i128, i32)) -> Result<(i128, i128, i32), VmExit> {
    let s = a.1.max(b.1);
    Ok((
        upscale(a.0, s as i64 - a.1 as i64)?,
        upscale(b.0, s as i64 - b.1 as i64)?,
        s,
    ))
}

fn digits(u: i128) -> i32 {
    let mut n = u.unsigned_abs();
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

fn strip_checked(mut u: i128, mut s: i32) -> Result<(i128, i32), VmExit> {
    if u == 0 {
        return Ok((0, 0));
    }
    while u % 10 == 0 {
        let next_s = s.checked_sub(1).ok_or_else(overflow)?;
        u /= 10;
        s = next_s;
    }
    Ok((u, s))
}

#[cfg(test)]
fn strip(u: i128, s: i32) -> (i128, i32) {
    strip_checked(u, s).unwrap_or((u, s))
}

fn plain_char_len(u: i128, s: i32) -> u64 {
    let d_len = digits(u) as u64;
    let body = if s <= 0 {
        d_len.saturating_add((-(s as i64)) as u64)
    } else {
        let su = s as u64;
        if d_len <= su {
            su.saturating_add(2)
        } else {
            d_len.saturating_add(1)
        }
    };
    body.saturating_add(u64::from(u < 0))
}

pub(crate) fn plain_string(u: i128, s: i32) -> String {
    let neg = u < 0;
    let mut d = u.unsigned_abs().to_string();
    if s <= 0 {
        let zeros = usize::try_from(-(s as i64)).unwrap_or(0);
        d.push_str(&"0".repeat(zeros));
    } else {
        let s = s as usize;
        if d.len() <= s {
            d = format!("{}{}", "0".repeat(s - d.len() + 1), d);
        }
        d.insert(d.len() - s, '.');
    }
    if neg {
        format!("-{d}")
    } else {
        d
    }
}

fn plain_string_checked(vm: &Engine, u: i128, s: i32) -> Result<String, VmExit> {
    let len = plain_char_len(u, s);
    let requested_u64 = len.saturating_mul(2).saturating_add(32);
    let requested = usize::try_from(requested_u64).unwrap_or(usize::MAX);
    if vm
        .heap
        .used()
        .checked_add(requested)
        .map_or(true, |n| n > vm.heap.budget())
    {
        return Err(crate::err::oom_throwable(crate::heap::OomError {
            requested,
            budget: vm.heap.budget(),
        }));
    }
    Ok(plain_string(u, s))
}

fn sci_string(u: i128, s: i32) -> String {
    let adjusted = -(s as i64) + (digits(u) as i64 - 1);
    if s >= 0 && adjusted >= -6 {
        return plain_string(u, s);
    }
    let neg = u < 0;
    let d = u.unsigned_abs().to_string();
    let mut m = d[..1].to_string();
    if d.len() > 1 {
        m.push('.');
        m.push_str(&d[1..]);
    }
    let e = if adjusted >= 0 {
        format!("E+{adjusted}")
    } else {
        format!("E{adjusted}")
    };
    format!("{}{m}{e}", if neg { "-" } else { "" })
}

fn eng_string(u: i128, s: i32) -> String {
    let adjusted = -(s as i64) + (digits(u) as i64 - 1);
    if s >= 0 && adjusted >= -6 {
        return plain_string(u, s);
    }
    if u == 0 {
        return sci_string(0, s);
    }
    let neg = u < 0;
    let d = u.unsigned_abs().to_string();
    let r = adjusted.rem_euclid(3) as usize;
    let int_digits = 1 + r;
    let eng_exp = adjusted - (r as i64);
    let m = if d.len() <= int_digits {
        format!("{d}{}", "0".repeat(int_digits - d.len()))
    } else {
        format!("{}.{}", &d[..int_digits], &d[int_digits..])
    };
    let e = if eng_exp >= 0 {
        format!("E+{eng_exp}")
    } else {
        format!("E{eng_exp}")
    };
    format!("{}{m}{e}", if neg { "-" } else { "" })
}

fn parse(text: &str) -> Option<(i128, i32)> {
    let t = text.trim();
    let (mant, exp) = match t.find(['e', 'E']) {
        Some(i) => (&t[..i], t[i + 1..].parse::<i64>().ok()?),
        None => (t, 0),
    };
    let (neg, body) = match mant.as_bytes().first()? {
        b'-' => (true, &mant[1..]),
        b'+' => (false, &mant[1..]),
        _ => (false, mant),
    };
    let (ip, fp) = match body.find('.') {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, ""),
    };
    if ip.is_empty() && fp.is_empty() {
        return None;
    }
    if !ip.bytes().chain(fp.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let all = format!("{ip}{fp}");
    let trimmed = all.trim_start_matches('0');
    let u: i128 = if trimmed.is_empty() {
        0
    } else {
        let mag: u128 = trimmed.parse().ok()?;
        if neg {
            if mag == (i128::MAX as u128) + 1 {
                i128::MIN
            } else if mag <= i128::MAX as u128 {
                -(mag as i128)
            } else {
                return None;
            }
        } else if mag <= i128::MAX as u128 {
            mag as i128
        } else {
            return None;
        }
    };
    let scale = i32::try_from(fp.len() as i64 - exp).ok()?;
    Some((u, scale))
}

fn new_num(vm: &mut Engine, class: &str, u: i128, s: i32) -> Result<Value, VmExit> {
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    vm.bignums.insert(r, (u, s));
    Ok(Value::Obj(r))
}

fn num_of(vm: &Engine, v: Option<&Value>) -> Result<(i128, i32), VmExit> {
    match v {
        Some(Value::Obj(r)) => vm.bignums.get(r).copied().ok_or_else(|| {
            VmExit::Exception(Throwable::new(
                "Ljava/lang/ClassCastException;",
                "not a BigDecimal/BigInteger",
            ))
        }),
        _ => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "BigDecimal/BigInteger argument is null",
        ))),
    }
}

fn mode_of(vm: &mut Engine, v: Option<&Value>) -> Result<String, VmExit> {
    match v {
        Some(Value::Obj(r)) if *r != 0 => {
            if let Ok(Value::Obj(n)) = vm.heap.get_field(*r, "name") {
                if let Ok(s) = vm.heap.as_str(n) {
                    return Ok(s.to_string());
                }
            }
            crate::intrinsics::object_to_string(vm, *r)
        }
        // legacy int constants (BigDecimal.ROUND_*)
        Some(Value::Int(i)) => {
            let Some(m) = usize::try_from(*i).ok().and_then(|idx| MODES.get(idx)) else {
                return Err(iae("Invalid rounding mode"));
            };
            Ok((*m).to_string())
        }
        _ => Err(npe("RoundingMode == null")),
    }
}

fn rounding_mode(vm: &mut Engine, name: &str) -> Result<Value, VmExit> {
    let key = (RM.to_string(), name.to_string());
    if let Some(v @ Value::Obj(_)) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let ord = MODES.iter().position(|m| *m == name).unwrap_or(6) as i32;
    let n = crate::intrinsics::alloc_string(vm, name.to_string())?;
    let r = vm.heap.alloc_instance(
        RM.to_string(),
        vec![
            ("name".to_string(), Value::Obj(n)),
            ("ordinal".to_string(), Value::Int(ord)),
        ],
    )?;
    vm.statics.insert(key, Value::Obj(r));
    Ok(Value::Obj(r))
}

/// `sget` of BigDecimal/BigInteger ZERO/ONE/TEN and RoundingMode constants.
pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, field) {
        (BD | BI, "ZERO" | "ONE" | "TWO" | "TEN") => {
            let u = match field {
                "ZERO" => 0,
                "ONE" => 1,
                "TWO" => 2,
                _ => 10,
            };
            Ok(Some(new_num(vm, owner, u, 0)?))
        }
        (BD, f) if f.starts_with("ROUND_") => Ok(Some(Value::Int(
            MODES.iter().position(|m| *m == &f[6..]).unwrap_or(6) as i32,
        ))),
        (RM, f) if MODES.contains(&f) => Ok(Some(rounding_mode(vm, f)?)),
        _ => Ok(None),
    }
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let long = |i: usize| match args.get(i) {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(v)) => *v as i64,
        _ => 0,
    };
    Ok(Some(match (class, name, sig) {
        (BD, "valueOf", "(J)Ljava/math/BigDecimal;") => new_num(vm, BD, long(0) as i128, 0)?,
        (BD, "valueOf", "(JI)Ljava/math/BigDecimal;") => {
            let s = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
            new_num(vm, BD, long(0) as i128, s)?
        }
        (BD, "valueOf", "(D)Ljava/math/BigDecimal;") => {
            // Double.toString(d) canonical form, as the JDK does
            let d = args.first().and_then(|v| v.as_double().ok()).unwrap_or(0.0);
            let txt = crate::numerics::java_double_to_string(d, false);
            match parse(&txt) {
                Some((u, s)) => new_num(vm, BD, u, s)?,
                None => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NumberFormatException;",
                        format!("Infinite or NaN: {txt}"),
                    )))
                }
            }
        }
        (BI, "valueOf", "(J)Ljava/math/BigInteger;") => new_num(vm, BI, long(0) as i128, 0)?,
        (RM, "valueOf", "(Ljava/lang/String;)Ljava/math/RoundingMode;") => {
            let n = non_null_str(vm, args.first(), "Name is null")?;
            if !MODES.contains(&n.as_str()) {
                return Err(iae(format!("No enum constant java.math.RoundingMode.{n}")));
            }
            rounding_mode(vm, &n)?
        }
        (RM, "valueOf", "(I)Ljava/math/RoundingMode;") => {
            let ord = args.first().and_then(|v| v.as_int().ok()).unwrap_or(-1);
            let Some(m) = usize::try_from(ord).ok().and_then(|idx| MODES.get(idx)) else {
                return Err(iae("Invalid rounding mode"));
            };
            rounding_mode(vm, m)?
        }
        _ => return Ok(None),
    }))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // constructors (the receiver has no value yet)
    if name == "<init>" && (mref_class == BD || mref_class == BI) {
        let class = mref_class;
        let (u, s) = match sig {
            "(Ljava/lang/String;)V" | "(Ljava/lang/String;I)V" => {
                let t = non_null_str(vm, args.first(), "val == null")?;
                let parsed = if sig == "(Ljava/lang/String;I)V" {
                    // BigInteger(String, radix)
                    let radix_raw = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(10);
                    if !(2..=36).contains(&radix_raw) {
                        return Err(nfe("Radix out of range"));
                    }
                    i128::from_str_radix(t.trim(), radix_raw as u32)
                        .ok()
                        .map(|u| (u, 0))
                } else if class == BI {
                    t.trim().parse::<i128>().ok().map(|u| (u, 0))
                } else {
                    parse(&t)
                };
                parsed.ok_or_else(|| {
                    nfe(format!(
                        "Character array is missing \"exponent\" mark 'e' or 'E'. ({t})"
                    ))
                })?
            }
            "(I)V" => (
                args.first().and_then(|v| v.as_int().ok()).unwrap_or(0) as i128,
                0,
            ),
            "(J)V" => (
                match args.first() {
                    Some(Value::Long(l)) => *l as i128,
                    Some(Value::Int(i)) => *i as i128,
                    _ => 0,
                },
                0,
            ),
            "(Ljava/math/BigInteger;)V" => (num_of(vm, args.first())?.0, 0),
            "(Ljava/math/BigInteger;I)V" => (
                num_of(vm, args.first())?.0,
                args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0),
            ),
            "(D)V" => {
                // exact binary value of the double
                let d = args.first().and_then(|v| v.as_double().ok()).unwrap_or(0.0);
                if !d.is_finite() {
                    return Err(nfe("Infinite or NaN"));
                }
                let txt = format!("{d:.40}");
                let (u, s) = parse(&txt)
                    .or_else(|| parse(&crate::numerics::java_double_to_string(d, false)))
                    .unwrap_or((0, 0));
                strip_checked(u, s)?
            }
            _ => return Ok(None),
        };
        vm.bignums.insert(recv, (u, s));
        return Ok(Some(Value::Null));
    }
    let Some((u, s)) = vm.bignums.get(&recv).copied() else {
        return Ok(None);
    };
    let class = vm.heap.class_of(recv)?.to_string();
    let is_bi = class == BI;
    let int_arg = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let whole_checked = |u: i128, s: i32| -> Result<i128, VmExit> {
        if s <= 0 {
            upscale(u, -(s as i64))
        } else {
            Ok(match 10i128.checked_pow(s as u32) {
                Some(p) => u / p,
                None => 0,
            })
        }
    };
    let whole = |u: i128, s: i32| -> i128 { whole_checked(u, s).unwrap_or(0) };
    let v = match (name, sig) {
        ("add", _) | ("subtract", _) | ("multiply", _) | ("max", _) | ("min", _)
            if args.len() == 1 =>
        {
            let o = num_of(vm, args.first())?;
            let (a, b, sc) = align((u, s), o)?;
            let (ru, rs) = match name {
                "add" => (a.checked_add(b).ok_or_else(overflow)?, sc),
                "subtract" => (a.checked_sub(b).ok_or_else(overflow)?, sc),
                "multiply" => (mul(u, o.0)?, s.checked_add(o.1).ok_or_else(overflow)?),
                "max" => {
                    if a >= b {
                        (u, s)
                    } else {
                        o
                    }
                }
                _ => {
                    if a <= b {
                        (u, s)
                    } else {
                        o
                    }
                }
            };
            new_num(vm, &class, ru, rs)?
        }
        ("divide", _) => {
            let o = num_of(vm, args.first())?;
            if o.0 == 0 {
                return Err(arith(if u == 0 {
                    "Division undefined"
                } else {
                    "Division by zero"
                }));
            }
            if is_bi {
                new_num(vm, BI, u.checked_div(o.0).ok_or_else(overflow)?, 0)?
            } else if args.len() >= 2 {
                // divide(d, scale, mode) | divide(d, mode)
                let (scale, mode) = if args.len() == 3 {
                    (int_arg(1), mode_of(vm, args.get(2))?)
                } else {
                    (s, mode_of(vm, args.get(1))?)
                };
                let e = scale as i64 - s as i64 + o.1 as i64;
                let (n, d) = if e >= 0 {
                    (upscale(u, e)?, o.0)
                } else {
                    (u, upscale(o.0, -e)?)
                };
                new_num(vm, BD, div_round(n, d, &mode)?, scale)?
            } else {
                // exact quotient, preferred scale this.scale - divisor.scale
                let pref_i64 = s as i64 - o.1 as i64;
                let mut found = None;
                for extra in 0..=40i64 {
                    let sc_i64 = pref_i64.max(0) + extra;
                    let Ok(sc) = i32::try_from(sc_i64) else {
                        break;
                    };
                    let e = sc as i64 - s as i64 + o.1 as i64;
                    let (n, d) = if e >= 0 {
                        (upscale(u, e)?, o.0)
                    } else {
                        (u, upscale(o.0, -e)?)
                    };
                    if n.checked_rem(d) == Some(0) {
                        let q = n.checked_div(d).ok_or_else(overflow)?;
                        found = Some((q, sc));
                        break;
                    }
                }
                let Some((mut q, mut sc)) = found else {
                    return Err(arith(
                        "Non-terminating decimal expansion; no exact representable decimal result.",
                    ));
                };
                while (sc as i64) > pref_i64 && q % 10 == 0 && q != 0 {
                    q /= 10;
                    sc -= 1;
                }
                if q == 0 {
                    sc = i32::try_from(pref_i64).map_err(|_| overflow())?;
                }
                new_num(vm, BD, q, sc)?
            }
        }
        ("remainder", _) | ("mod", _) => {
            let o = num_of(vm, args.first())?;
            if o.0 == 0 {
                return Err(arith("Division by zero"));
            }
            if is_bi && name == "mod" && o.0 < 0 {
                return Err(arith("BigInteger: modulus not positive"));
            }
            let (a, b, sc) = align((u, s), o)?;
            let r = if name == "mod" {
                a.checked_rem_euclid(b).ok_or_else(overflow)?
            } else {
                a.checked_rem(b).ok_or_else(overflow)?
            };
            new_num(vm, &class, r, sc)?
        }
        ("negate", _) => new_num(vm, &class, u.checked_neg().ok_or_else(overflow)?, s)?,
        ("abs", _) => new_num(vm, &class, u.checked_abs().ok_or_else(overflow)?, s)?,
        ("pow", "(I)Ljava/math/BigDecimal;") | ("pow", "(I)Ljava/math/BigInteger;") => {
            let n = int_arg(0);
            if n < 0 {
                return Err(arith("Invalid operation"));
            }
            let ru = u.checked_pow(n as u32).ok_or_else(overflow)?;
            new_num(vm, &class, ru, s.checked_mul(n).ok_or_else(overflow)?)?
        }
        ("setScale", "(ILjava/math/RoundingMode;)Ljava/math/BigDecimal;")
        | ("setScale", "(II)Ljava/math/BigDecimal;")
        | ("setScale", "(I)Ljava/math/BigDecimal;") => {
            let ns = int_arg(0);
            let mode = if args.len() > 1 {
                mode_of(vm, args.get(1))?
            } else {
                "UNNECESSARY".to_string()
            };
            new_num(vm, BD, rescale(u, s, ns, &mode)?, ns)?
        }
        ("round", _) => {
            // round(MathContext): precision from the context's field, HALF_UP
            let Some(Value::Obj(mc)) = args.first() else {
                return Err(npe("mc == null"));
            };
            if *mc == 0 {
                return Err(npe("mc == null"));
            }
            let prec = match vm.heap.get_field(*mc, "precision") {
                Ok(Value::Int(p)) => p,
                _ => 0,
            };
            if prec <= 0 || digits(u) <= prec {
                new_num(vm, BD, u, s)?
            } else {
                let drop = digits(u) as i64 - prec as i64;
                let ns = i32::try_from(s as i64 - drop).map_err(|_| overflow())?;
                new_num(vm, BD, rescale(u, s, ns, "HALF_UP")?, ns)?
            }
        }
        ("movePointLeft", _) | ("movePointRight", _) | ("scaleByPowerOfTen", _) => {
            let n = int_arg(0);
            let ns = match name {
                "movePointLeft" => s.checked_add(n),
                _ => s.checked_sub(n),
            }
            .ok_or_else(overflow)?;
            if name != "scaleByPowerOfTen" && ns < 0 {
                new_num(vm, BD, rescale(u, ns, 0, "UNNECESSARY")?, 0)?
            } else {
                new_num(vm, BD, u, ns)?
            }
        }
        ("stripTrailingZeros", _) => {
            let (ru, rs) = strip_checked(u, s)?;
            new_num(vm, BD, ru, rs)?
        }
        ("signum", "()I") => Value::Int(u.signum() as i32),
        ("scale", "()I") => Value::Int(s),
        ("precision", "()I") => Value::Int(digits(u)),
        ("unscaledValue", _) => new_num(vm, BI, u, 0)?,
        ("toBigInteger", _) | ("toBigIntegerExact", _) => {
            let w = whole_checked(u, s)?;
            if name == "toBigIntegerExact" && upscale(w, s.max(0) as i64)? != u {
                return Err(arith("Rounding necessary"));
            }
            new_num(vm, BI, w, 0)?
        }
        ("compareTo", _) => {
            let o = num_of(vm, args.first())?;
            let (a, b, _) = align((u, s), o)?;
            Value::Int(a.cmp(&b) as i32)
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let same = match args.first() {
                Some(Value::Obj(o)) => {
                    vm.bignums.get(o) == Some(&(u, s)) && vm.heap.class_of(*o)? == class
                }
                _ => false,
            };
            Value::Int(same as i32)
        }
        ("hashCode", "()I") => {
            let h = (u as i64 ^ (u >> 64) as i64) as i32;
            Value::Int(h.wrapping_mul(31).wrapping_add(s))
        }
        ("toString", "()Ljava/lang/String;") => {
            let t = if is_bi {
                u.to_string()
            } else {
                sci_string(u, s)
            };
            Value::Obj(crate::intrinsics::alloc_string(vm, t)?)
        }
        ("toString", "(I)Ljava/lang/String;") if is_bi => {
            let raw = int_arg(0);
            let radix = if (2..=36).contains(&raw) {
                raw as u32
            } else {
                10
            };
            let mut n = u.unsigned_abs();
            let mut out = Vec::new();
            loop {
                out.push(std::char::from_digit((n % radix as u128) as u32, radix).unwrap_or('0'));
                n /= radix as u128;
                if n == 0 {
                    break;
                }
            }
            if u < 0 {
                out.push('-');
            }
            let t: String = out.into_iter().rev().collect();
            Value::Obj(crate::intrinsics::alloc_string(vm, t)?)
        }
        ("toPlainString", _) => {
            let t = plain_string_checked(vm, u, s)?;
            Value::Obj(crate::intrinsics::alloc_string(vm, t)?)
        }
        ("toEngineeringString", _) => {
            Value::Obj(crate::intrinsics::alloc_string(vm, eng_string(u, s))?)
        }
        ("doubleValue", "()D") => Value::Double(sci_string(u, s).parse::<f64>().unwrap_or(0.0)),
        ("floatValue", "()F") => Value::Float(sci_string(u, s).parse::<f32>().unwrap_or(0.0)),
        ("longValue", "()J") => Value::Long(whole(u, s) as i64),
        ("intValue", "()I") => Value::Int(whole(u, s) as i32),
        ("shortValue", "()S") => Value::Int(whole(u, s) as i16 as i32),
        ("byteValue", "()B") => Value::Int(whole(u, s) as i8 as i32),
        ("longValueExact", "()J") | ("intValueExact", "()I") => {
            let w = whole_checked(u, s)?;
            if upscale(w, s.max(0) as i64)? != u {
                return Err(arith("Rounding necessary"));
            }
            if name == "longValueExact" {
                Value::Long(i64::try_from(w).map_err(|_| arith("Overflow"))?)
            } else {
                Value::Int(i32::try_from(w).map_err(|_| arith("Overflow"))?)
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_text_and_rounding() {
        assert_eq!(parse("12.50"), Some((1250, 2)));
        assert_eq!(parse("-0.003"), Some((-3, 3)));
        assert_eq!(parse("1E+3"), Some((1, -3)));
        assert_eq!(parse("abc"), None);
        assert_eq!(plain_string(1250, 2), "12.50");
        assert_eq!(plain_string(-3, 3), "-0.003");
        assert_eq!(plain_string(1, -3), "1000");
        assert_eq!(sci_string(1, -3), "1E+3");
        assert_eq!(sci_string(1, 7), "1E-7");
        assert_eq!(sci_string(123, 2), "1.23");
        assert_eq!(div_round(25, 10, "HALF_EVEN").unwrap(), 2);
        assert_eq!(div_round(35, 10, "HALF_EVEN").unwrap(), 4);
        assert_eq!(div_round(25, 10, "HALF_UP").unwrap(), 3);
        assert_eq!(div_round(-25, 10, "HALF_UP").unwrap(), -3);
        assert_eq!(div_round(-21, 10, "FLOOR").unwrap(), -3);
        assert_eq!(div_round(21, 10, "CEILING").unwrap(), 3);
        assert!(div_round(21, 10, "UNNECESSARY").is_err());
        assert_eq!(rescale(12345, 3, 1, "HALF_EVEN").unwrap(), 123);
        assert_eq!(strip(1200, 2), (12, 0));
        assert_eq!(strip(0, 5), (0, 0));
    }
}
