//! Static utilities of `java.lang.Integer/Long/Float/Double/Boolean/
//! Character/Math/StrictMath`: bit twiddling, comparisons, hashing, IEEE bit
//! conversions, radix parsing/printing, Unicode classification and
//! transcendental functions. Semantics follow the JDK:
//! - `floatToIntBits`/`doubleToLongBits` collapse NaN to the canonical bit
//!   pattern; the `Raw` variants keep it.
//! - `compare` orders -0.0 < 0.0 and puts NaN last.
//! - `parseX` throws NumberFormatException on bad input, empty strings and
//!   out-of-range values, and accepts a leading `+`/`-`.
//! - `Double.toString`/`Float.toString` use Java's format: "1.0", "1.0E10",
//!   "1.0E-5", "NaN", "Infinity".

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::value::Value;

fn nfe(s: &str) -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/NumberFormatException;",
        format!("For input string: \"{s}\""),
    ))
}

fn str_arg(vm: &Engine, v: Option<&Value>) -> Result<String, VmExit> {
    match v {
        Some(Value::Obj(r)) => Ok(vm.heap.as_str(*r)?.to_string()),
        _ => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NumberFormatException;",
            "Cannot parse null string: null",
        ))),
    }
}

fn parse_radix(s: &str, radix: i32, min: i128, max: i128) -> Result<i128, VmExit> {
    if !(2..=36).contains(&radix) || s.is_empty() {
        return Err(nfe(s));
    }
    let (neg, digits) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(nfe(s));
    }
    let mut acc: i128 = 0;
    for c in digits.chars() {
        let d = c.to_digit(radix as u32).ok_or_else(|| nfe(s))? as i128;
        acc = acc * radix as i128 + d;
        if acc > max + 1 {
            return Err(nfe(s));
        }
    }
    let v = if neg { -acc } else { acc };
    if v < min || v > max {
        return Err(nfe(s));
    }
    Ok(v)
}

fn to_radix(mut v: i128, radix: u32) -> String {
    let radix = if (2..=36).contains(&radix) { radix } else { 10 };
    if v == 0 {
        return "0".into();
    }
    let neg = v < 0;
    let mut out = Vec::new();
    while v != 0 {
        let d = (v % radix as i128).unsigned_abs() as u32;
        out.push(std::char::from_digit(d, radix).unwrap_or('0'));
        v /= radix as i128;
    }
    if neg {
        out.push('-');
    }
    out.iter().rev().collect()
}

/// Java `Double.toString` formatting of the shortest round-trip digits.
pub(crate) fn java_double_to_string(d: f64, is_float: bool) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if d == 0.0 {
        return if d.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // shortest digits from Rust's float printing (round-trip exact)
    let sci = if is_float {
        format!("{:e}", d as f32)
    } else {
        format!("{d:e}")
    };
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if neg { "-" } else { "" };
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        // plain notation
        let point = exp + 1; // digits before the decimal point
        let s = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{s}")
    } else {
        let frac = if digits.len() > 1 { &digits[1..] } else { "0" };
        format!("{sign}{}.{}E{}", &digits[..1], frac, exp)
    }
}

fn cmp_f64(a: f64, b: f64) -> i32 {
    // Double.compare: -0.0 < 0.0, NaN == NaN and greater than everything
    let ka = canonical_bits(a);
    let kb = canonical_bits(b);
    if a < b {
        -1
    } else if a > b {
        1
    } else if ka == kb {
        0
    } else if ka < kb {
        -1
    } else {
        1
    }
}

fn canonical_bits(d: f64) -> i64 {
    if d.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        d.to_bits() as i64
    }
}

fn canonical_fbits(f: f32) -> i32 {
    if f.is_nan() {
        0x7fc0_0000
    } else {
        f.to_bits() as i32
    }
}

fn java_whitespace(c: char) -> bool {
    // Character.isWhitespace: Unicode space separators except non-breaking
    // ones, plus \t \n VT \f \r FS GS RS US
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}'
    ) || (c.is_whitespace() && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}') && c != '\u{85}')
}

fn java_identifier_start(c: char) -> bool {
    use std::sync::OnceLock;
    static CATEGORIES: OnceLock<regex::Regex> = OnceLock::new();
    CATEGORIES
        .get_or_init(|| {
            regex::Regex::new(r"\A[\p{L}\p{Nl}\p{Sc}\p{Pc}]\z")
                .expect("valid Java identifier-start Unicode categories")
        })
        .is_match(&c.to_string())
}

fn java_identifier_ignorable(c: char) -> bool {
    use std::sync::OnceLock;
    static FORMAT: OnceLock<regex::Regex> = OnceLock::new();
    let code = c as u32;
    matches!(code, 0x0000..=0x0008 | 0x000E..=0x001B | 0x007F..=0x009F)
        || FORMAT
            .get_or_init(|| regex::Regex::new(r"\A\p{Cf}\z").expect("valid Unicode Cf category"))
            .is_match(&c.to_string())
}

fn java_identifier_part(c: char) -> bool {
    use std::sync::OnceLock;
    static CATEGORIES: OnceLock<regex::Regex> = OnceLock::new();
    java_identifier_start(c)
        || java_identifier_ignorable(c)
        || CATEGORIES
            .get_or_init(|| {
                regex::Regex::new(r"\A[\p{Mn}\p{Mc}\p{Nd}]\z")
                    .expect("valid Java identifier-part Unicode categories")
            })
            .is_match(&c.to_string())
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let cls = match class
        .strip_prefix("Ljava/lang/")
        .and_then(|c| c.strip_suffix(';'))
    {
        Some(c) => c,
        None => return Ok(None),
    };
    let i = |k: usize| args.get(k).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let j = |k: usize| args.get(k).and_then(|v| v.as_long().ok()).unwrap_or(0);
    let f = |k: usize| args.get(k).and_then(|v| v.as_float().ok()).unwrap_or(0.0);
    let d = |k: usize| args.get(k).and_then(|v| v.as_double().ok()).unwrap_or(0.0);
    let ch = |k: usize| char::from_u32(i(k) as u32);
    let b = |x: bool| Value::Int(x as i32);
    let s = |vm: &mut Engine, x: String| -> Result<Value, VmExit> {
        Ok(Value::Obj(crate::intrinsics::alloc_string(vm, x)?))
    };
    let v = match (cls, name, sig) {
        // ── Integer ──
        ("Integer", "bitCount", "(I)I") => Value::Int(i(0).count_ones() as i32),
        ("Integer", "numberOfLeadingZeros", "(I)I") => Value::Int(i(0).leading_zeros() as i32),
        ("Integer", "numberOfTrailingZeros", "(I)I") => Value::Int(i(0).trailing_zeros() as i32),
        ("Integer", "highestOneBit", "(I)I") => {
            let x = i(0) as u32;
            Value::Int(if x == 0 {
                0
            } else {
                (1u32 << (31 - x.leading_zeros())) as i32
            })
        }
        ("Integer", "lowestOneBit", "(I)I") => Value::Int(i(0) & i(0).wrapping_neg()),
        ("Integer", "rotateLeft", "(II)I") => Value::Int(i(0).rotate_left((i(1) & 31) as u32)),
        ("Integer", "rotateRight", "(II)I") => Value::Int(i(0).rotate_right((i(1) & 31) as u32)),
        ("Integer", "reverse", "(I)I") => Value::Int(i(0).reverse_bits()),
        ("Integer", "reverseBytes", "(I)I") => Value::Int(i(0).swap_bytes()),
        ("Integer", "signum", "(I)I") => Value::Int(i(0).signum()),
        ("Integer", "hashCode", "(I)I") => Value::Int(i(0)),
        ("Integer", "compare", "(II)I") => Value::Int(i(0).cmp(&i(1)) as i32),
        ("Integer", "compareUnsigned", "(II)I") => {
            Value::Int((i(0) as u32).cmp(&(i(1) as u32)) as i32)
        }
        ("Integer", "divideUnsigned", "(II)I") | ("Integer", "remainderUnsigned", "(II)I") => {
            if i(1) == 0 {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ArithmeticException;",
                    "/ by zero",
                )));
            }
            let (a, c) = (i(0) as u32, i(1) as u32);
            Value::Int(if name == "divideUnsigned" {
                a / c
            } else {
                a % c
            } as i32)
        }
        ("Integer", "toUnsignedLong", "(I)J") => Value::Long(i(0) as u32 as i64),
        ("Integer", "max", "(II)I") => Value::Int(i(0).max(i(1))),
        ("Integer", "min", "(II)I") => Value::Int(i(0).min(i(1))),
        ("Integer", "sum", "(II)I") => Value::Int(i(0).wrapping_add(i(1))),
        ("Integer", "toHexString", "(I)Ljava/lang/String;") => s(vm, format!("{:x}", i(0) as u32))?,
        ("Integer", "toBinaryString", "(I)Ljava/lang/String;") => {
            s(vm, format!("{:b}", i(0) as u32))?
        }
        ("Integer", "toOctalString", "(I)Ljava/lang/String;") => {
            s(vm, format!("{:o}", i(0) as u32))?
        }
        ("Integer", "toString", "(II)Ljava/lang/String;") => {
            s(vm, to_radix(i(0) as i128, i(1) as u32))?
        }
        ("Integer", "toString", "(I)Ljava/lang/String;") => s(vm, i(0).to_string())?,
        ("Integer", "parseInt", "(Ljava/lang/String;I)I") => {
            let t = str_arg(vm, args.first())?;
            Value::Int(parse_radix(&t, i(1), i32::MIN as i128, i32::MAX as i128)? as i32)
        }
        ("Integer", "parseInt", "(Ljava/lang/String;)I") => {
            let t = str_arg(vm, args.first())?;
            Value::Int(parse_radix(&t, 10, i32::MIN as i128, i32::MAX as i128)? as i32)
        }
        ("Integer", "valueOf", "(Ljava/lang/String;)Ljava/lang/Integer;")
        | ("Integer", "valueOf", "(Ljava/lang/String;I)Ljava/lang/Integer;") => {
            let t = str_arg(vm, args.first())?;
            let radix = if args.len() > 1 { i(1) } else { 10 };
            let n = parse_radix(&t, radix, i32::MIN as i128, i32::MAX as i128)? as i32;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Integer;",
                Value::Int(n),
            )?)
        }
        // ── Long ──
        ("Long", "bitCount", "(J)I") => Value::Int(j(0).count_ones() as i32),
        ("Long", "numberOfLeadingZeros", "(J)I") => Value::Int(j(0).leading_zeros() as i32),
        ("Long", "numberOfTrailingZeros", "(J)I") => Value::Int(j(0).trailing_zeros() as i32),
        ("Long", "highestOneBit", "(J)J") => {
            let x = j(0) as u64;
            Value::Long(if x == 0 {
                0
            } else {
                (1u64 << (63 - x.leading_zeros())) as i64
            })
        }
        ("Long", "lowestOneBit", "(J)J") => Value::Long(j(0) & j(0).wrapping_neg()),
        ("Long", "rotateLeft", "(JI)J") => Value::Long(j(0).rotate_left((i(1) & 63) as u32)),
        ("Long", "rotateRight", "(JI)J") => Value::Long(j(0).rotate_right((i(1) & 63) as u32)),
        ("Long", "reverse", "(J)J") => Value::Long(j(0).reverse_bits()),
        ("Long", "signum", "(J)I") => Value::Int(j(0).signum() as i32),
        ("Long", "hashCode", "(J)I") => Value::Int((j(0) ^ ((j(0) as u64) >> 32) as i64) as i32),
        ("Long", "compare", "(JJ)I") => Value::Int(j(0).cmp(&j(1)) as i32),
        ("Long", "compareUnsigned", "(JJ)I") => {
            Value::Int((j(0) as u64).cmp(&(j(1) as u64)) as i32)
        }
        ("Long", "max", "(JJ)J") => Value::Long(j(0).max(j(1))),
        ("Long", "min", "(JJ)J") => Value::Long(j(0).min(j(1))),
        ("Long", "sum", "(JJ)J") => Value::Long(j(0).wrapping_add(j(1))),
        ("Long", "toString", "(J)Ljava/lang/String;") => s(vm, j(0).to_string())?,
        ("Long", "toString", "(JI)Ljava/lang/String;") => {
            s(vm, to_radix(j(0) as i128, i(1) as u32))?
        }
        ("Long", "toHexString", "(J)Ljava/lang/String;") => s(vm, format!("{:x}", j(0) as u64))?,
        ("Long", "toBinaryString", "(J)Ljava/lang/String;") => s(vm, format!("{:b}", j(0) as u64))?,
        ("Long", "parseLong", "(Ljava/lang/String;I)J") => {
            let t = str_arg(vm, args.first())?;
            Value::Long(parse_radix(&t, i(1), i64::MIN as i128, i64::MAX as i128)? as i64)
        }
        ("Long", "parseLong", "(Ljava/lang/String;)J") => {
            let t = str_arg(vm, args.first())?;
            Value::Long(parse_radix(&t, 10, i64::MIN as i128, i64::MAX as i128)? as i64)
        }
        ("Long", "valueOf", "(Ljava/lang/String;)Ljava/lang/Long;") => {
            let t = str_arg(vm, args.first())?;
            let n = parse_radix(&t, 10, i64::MIN as i128, i64::MAX as i128)? as i64;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Long;",
                Value::Long(n),
            )?)
        }
        // ── Float ──
        ("Float", "floatToIntBits", "(F)I") => Value::Int(canonical_fbits(f(0))),
        ("Float", "floatToRawIntBits", "(F)I") => Value::Int(f(0).to_bits() as i32),
        ("Float", "intBitsToFloat", "(I)F") => Value::Float(f32::from_bits(i(0) as u32)),
        ("Float", "isNaN", "(F)Z") => b(f(0).is_nan()),
        ("Float", "isInfinite", "(F)Z") => b(f(0).is_infinite()),
        ("Float", "isFinite", "(F)Z") => b(f(0).is_finite()),
        ("Float", "hashCode", "(F)I") => Value::Int(canonical_fbits(f(0))),
        ("Float", "compare", "(FF)I") => Value::Int(cmp_f64(f(0) as f64, f(1) as f64)),
        ("Float", "max", "(FF)F") => Value::Float(if f(0).is_nan() || f(1).is_nan() {
            f32::NAN
        } else {
            f(0).max(f(1))
        }),
        ("Float", "min", "(FF)F") => Value::Float(if f(0).is_nan() || f(1).is_nan() {
            f32::NAN
        } else {
            f(0).min(f(1))
        }),
        ("Float", "toString", "(F)Ljava/lang/String;") => {
            s(vm, java_double_to_string(f(0) as f64, true))?
        }
        ("Float", "parseFloat", "(Ljava/lang/String;)F") => {
            let t = str_arg(vm, args.first())?;
            Value::Float(parse_float(&t)? as f32)
        }
        ("Float", "valueOf", "(Ljava/lang/String;)Ljava/lang/Float;") => {
            let t = str_arg(vm, args.first())?;
            let x = parse_float(&t)? as f32;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Float;",
                Value::Float(x),
            )?)
        }
        // ── Double ──
        ("Double", "doubleToLongBits", "(D)J") => Value::Long(canonical_bits(d(0))),
        ("Double", "doubleToRawLongBits", "(D)J") => Value::Long(d(0).to_bits() as i64),
        ("Double", "longBitsToDouble", "(J)D") => Value::Double(f64::from_bits(j(0) as u64)),
        ("Double", "isNaN", "(D)Z") => b(d(0).is_nan()),
        ("Double", "isInfinite", "(D)Z") => b(d(0).is_infinite()),
        ("Double", "isFinite", "(D)Z") => b(d(0).is_finite()),
        ("Double", "hashCode", "(D)I") => {
            let x = canonical_bits(d(0));
            Value::Int((x ^ ((x as u64) >> 32) as i64) as i32)
        }
        ("Double", "compare", "(DD)I") => Value::Int(cmp_f64(d(0), d(1))),
        ("Double", "toString", "(D)Ljava/lang/String;") => {
            s(vm, java_double_to_string(d(0), false))?
        }
        ("Double", "parseDouble", "(Ljava/lang/String;)D") => {
            let t = str_arg(vm, args.first())?;
            Value::Double(parse_float(&t)?)
        }
        ("Double", "valueOf", "(Ljava/lang/String;)Ljava/lang/Double;") => {
            let t = str_arg(vm, args.first())?;
            let x = parse_float(&t)?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Double;",
                Value::Double(x),
            )?)
        }
        // ── Boolean ──
        ("Boolean", "parseBoolean", "(Ljava/lang/String;)Z") => match args.first() {
            Some(Value::Obj(r)) => b(vm.heap.as_str(*r)?.eq_ignore_ascii_case("true")),
            _ => b(false),
        },
        ("Boolean", "hashCode", "(Z)I") => Value::Int(if i(0) != 0 { 1231 } else { 1237 }),
        ("Boolean", "compare", "(ZZ)I") => Value::Int((i(0) != 0).cmp(&(i(1) != 0)) as i32),
        ("Boolean", "toString", "(Z)Ljava/lang/String;") => s(vm, (i(0) != 0).to_string())?,
        // ── Character ──
        ("Character", "charCount", "(I)I") => Value::Int(if i(0) >= 0x10000 { 2 } else { 1 }),
        ("Character", "isJavaIdentifierStart", "(C)Z" | "(I)Z") => {
            b(ch(0).is_some_and(java_identifier_start))
        }
        ("Character", "isJavaIdentifierPart", "(C)Z" | "(I)Z") => {
            b(ch(0).is_some_and(java_identifier_part))
        }
        ("Character", "isIdentifierIgnorable", "(C)Z" | "(I)Z") => {
            b(ch(0).is_some_and(java_identifier_ignorable))
        }
        ("Character", "isDigit", _) => b(ch(0).is_some_and(|c| {
            c.is_numeric() && c.is_ascii_digit() || ('\u{660}'..='\u{669}').contains(&c)
        })),
        ("Character", "isLetter", _) => b(ch(0).is_some_and(char::is_alphabetic)),
        ("Character", "isLetterOrDigit", _) => b(ch(0).is_some_and(char::is_alphanumeric)),
        ("Character", "isUpperCase", _) => b(ch(0).is_some_and(char::is_uppercase)),
        ("Character", "isLowerCase", _) => b(ch(0).is_some_and(char::is_lowercase)),
        ("Character", "isWhitespace", _) => b(ch(0).is_some_and(java_whitespace)),
        ("Character", "isSpaceChar", _) => b(ch(0)
            .is_some_and(|c| c == ' ' || c == '\u{A0}' || (c.is_whitespace() && !c.is_control()))),
        ("Character", "isISOControl", _) => b(ch(0).is_some_and(char::is_control)),
        ("Character", "isHighSurrogate", "(C)Z") => {
            b((0xD800..=0xDBFF).contains(&(i(0) as u16 as i32)))
        }
        ("Character", "isLowSurrogate", "(C)Z") => {
            b((0xDC00..=0xDFFF).contains(&(i(0) as u16 as i32)))
        }
        ("Character", "isSurrogate", "(C)Z") => {
            b((0xD800..=0xDFFF).contains(&(i(0) as u16 as i32)))
        }
        ("Character", "isSurrogatePair", "(CC)Z") => b((0xD800..=0xDBFF)
            .contains(&(i(0) as u16 as i32))
            && (0xDC00..=0xDFFF).contains(&(i(1) as u16 as i32))),
        ("Character", "toCodePoint", "(CC)I") => {
            let hi = i(0) as u16 as i32;
            let lo = i(1) as u16 as i32;
            Value::Int(((hi - 0xD800) << 10) + (lo - 0xDC00) + 0x10000)
        }
        ("Character", "toLowerCase", "(C)C") | ("Character", "toLowerCase", "(I)I") => {
            Value::Int(ch(0).map_or(i(0), |c| c.to_lowercase().next().map_or(i(0), |x| x as i32)))
        }
        ("Character", "toUpperCase", "(C)C") | ("Character", "toUpperCase", "(I)I") => {
            Value::Int(ch(0).map_or(i(0), |c| {
                let mut u = c.to_uppercase();
                match (u.next(), u.next()) {
                    (Some(x), None) => x as i32,
                    _ => i(0), // multi-char uppercase (ß) keeps the char, as in Java
                }
            }))
        }
        ("Character", "digit", "(CI)I") | ("Character", "digit", "(II)I") => {
            let radix = i(1);
            let cp = if sig == "(CI)I" {
                i(0) as u16 as u32
            } else {
                i(0) as u32
            };
            let d_out = if (2..=36).contains(&radix) {
                char::from_u32(cp)
                    .and_then(|c| c.to_digit(radix as u32))
                    .map_or(-1, |x| x as i32)
            } else {
                -1
            };
            Value::Int(d_out)
        }
        ("Character", "getType", "(I)I") | ("Character", "getType", "(C)I") => {
            Value::Int(char_type(ch(0)))
        }
        ("Character", "getDirectionality", "(I)B") | ("Character", "getDirectionality", "(C)B") => {
            // L=0, R=1, AL=2, EN=3, WS=12, ON=13; rough but stable
            let c = ch(0).unwrap_or(' ');
            Value::Int(match c as u32 {
                0x0590..=0x05FF => 1,
                0x0600..=0x06FF | 0x0750..=0x077F => 2,
                _ if c.is_ascii_digit() => 3,
                _ if c.is_whitespace() => 12,
                _ if c.is_alphabetic() => 0,
                _ => 13,
            })
        }
        ("Character", "codePointAt", "(Ljava/lang/CharSequence;I)I")
        | ("Character", "codePointBefore", "(Ljava/lang/CharSequence;I)I") => {
            let r = match args.first() {
                Some(Value::Obj(r)) => *r,
                None | Some(Value::Null) | Some(Value::Int(0)) => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "seq == null",
                    )));
                }
                _ => return Ok(None),
            };
            let text: Vec<u16> = match vm.heap.as_str(r) {
                Ok(t) => t.encode_utf16().collect(),
                Err(_) => crate::intrinsics::object_to_string(vm, r)?
                    .encode_utf16()
                    .collect(),
            };
            let idx = i(1);
            let mut p = if name == "codePointAt" {
                if idx < 0 || (idx as usize) >= text.len() {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/StringIndexOutOfBoundsException;",
                        format!("index {idx}"),
                    )));
                }
                idx as usize
            } else {
                if idx < 1 || (idx as usize) > text.len() {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/StringIndexOutOfBoundsException;",
                        format!("index {idx}"),
                    )));
                }
                (idx as usize) - 1
            };
            if name == "codePointBefore"
                && p > 0
                && (0xDC00..=0xDFFF).contains(&text[p])
                && (0xD800..=0xDBFF).contains(&text[p - 1])
            {
                p -= 1;
            }
            let cp = char::decode_utf16(text[p..].iter().copied())
                .next()
                .and_then(|r| r.ok())
                .map_or(text[p] as i32, |c| c as i32);
            Value::Int(cp)
        }
        ("Character", "toChars", "(I[CI)I") => {
            let cp = i(0);
            if !(0..=0x10_FFFF).contains(&cp) {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("Not a valid Unicode code point: 0x{cp:X}"),
                )));
            }
            let (units, unit_len): ([u16; 2], usize) = if cp < 0x10000 {
                ([cp as u16, 0], 1)
            } else {
                let u = (cp - 0x10000) as u32;
                (
                    [0xD800 + ((u >> 10) as u16), 0xDC00 + ((u & 0x3FF) as u16)],
                    2,
                )
            };
            let arr = match args.get(1) {
                Some(Value::Obj(arr)) => *arr,
                None | Some(Value::Null) | Some(Value::Int(0)) => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "dst == null",
                    )));
                }
                _ => return Ok(None),
            };
            let desc = vm
                .heap
                .class_of(arr)
                .map_err(|e| VmExit::from(crate::err::vm_error("VM_TYPE_ERROR", e)))?;
            if desc != "[C" {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    format!("Character.toChars expected [C, got {desc}"),
                )
                .into());
            }
            let dst_idx = i(2);
            let elems = vm
                .heap
                .array_elems_mut(arr)
                .map_err(|e| VmExit::from(crate::err::vm_error("VM_TYPE_ERROR", e)))?;
            if dst_idx < 0
                || (dst_idx as usize) > elems.len()
                || unit_len > elems.len() - (dst_idx as usize)
            {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("dstIndex={dst_idx} length={}", elems.len()),
                )));
            }
            let at = dst_idx as usize;
            for (k, u) in units[..unit_len].iter().enumerate() {
                elems[at + k] = Value::Int(*u as i32);
            }
            Value::Int(unit_len as i32)
        }
        // ── Math / StrictMath ──
        ("Math" | "StrictMath", op, "(D)D") => {
            let x = d(0);
            Value::Double(match op {
                "sin" => x.sin(),
                "cos" => x.cos(),
                "tan" => x.tan(),
                "asin" => x.asin(),
                "acos" => x.acos(),
                "atan" => x.atan(),
                "sinh" => x.sinh(),
                "cosh" => x.cosh(),
                "tanh" => x.tanh(),
                "exp" => x.exp(),
                "expm1" => x.exp_m1(),
                "log" => x.ln(),
                "log10" => x.log10(),
                "log1p" => x.ln_1p(),
                "cbrt" => x.cbrt(),
                "sqrt" => x.sqrt(),
                "rint" => {
                    // round half to even
                    let r = x.round();
                    if (x - x.trunc()).abs() == 0.5 {
                        2.0 * (x / 2.0).round()
                    } else {
                        r
                    }
                }
                "toDegrees" => x.to_degrees(),
                "toRadians" => x.to_radians(),
                "signum" => {
                    if x.is_nan() || x == 0.0 {
                        x
                    } else {
                        x.signum()
                    }
                }
                "ulp" => {
                    if x.is_nan() {
                        f64::NAN
                    } else if x.is_infinite() {
                        f64::INFINITY
                    } else {
                        let ax = x.abs();
                        if ax == f64::MAX {
                            f64::from_bits(0x7ca0_0000_0000_0000)
                        } else {
                            let next = f64::from_bits(ax.to_bits() + 1);
                            next - ax
                        }
                    }
                }
                _ => return Ok(None),
            })
        }
        ("Math" | "StrictMath", "signum", "(F)F") => {
            let x = f(0);
            Value::Float(if x.is_nan() || x == 0.0 {
                x
            } else {
                x.signum()
            })
        }
        ("Math" | "StrictMath", "ulp", "(F)F") => {
            let x = f(0);
            Value::Float(if x.is_nan() {
                f32::NAN
            } else if x.is_infinite() {
                f32::INFINITY
            } else {
                let ax = x.abs();
                if ax == f32::MAX {
                    f32::from_bits(0x7380_0000)
                } else {
                    let next = f32::from_bits(ax.to_bits() + 1);
                    next - ax
                }
            })
        }
        ("Math" | "StrictMath", "copySign", "(DD)D") => Value::Double(d(0).copysign(d(1))),
        ("Math" | "StrictMath", "copySign", "(FF)F") => Value::Float(f(0).copysign(f(1))),
        ("Math" | "StrictMath", "atan2", "(DD)D") => Value::Double(d(0).atan2(d(1))),
        ("Math" | "StrictMath", "hypot", "(DD)D") => Value::Double(d(0).hypot(d(1))),
        ("Math" | "StrictMath", "pow", "(DD)D") => Value::Double(d(0).powf(d(1))),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

pub(crate) fn parse_float(t: &str) -> Result<f64, VmExit> {
    let s = t.trim_matches(|c: char| c <= ' ');
    match s {
        "NaN" | "+NaN" | "-NaN" => return Ok(f64::NAN),
        "Infinity" | "+Infinity" => return Ok(f64::INFINITY),
        "-Infinity" => return Ok(f64::NEG_INFINITY),
        _ => {}
    }
    let core = s.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(s);
    if core.is_empty()
        || matches!(
            core,
            "NaN" | "+NaN" | "-NaN" | "Infinity" | "+Infinity" | "-Infinity"
        )
        || core.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return Err(nfe(t));
    }
    core.parse::<f64>().map_err(|_| nfe(t))
}

fn char_type(c: Option<char>) -> i32 {
    // java.lang.Character general categories (subset that apps branch on)
    let Some(c) = c else { return 0 };
    if c.is_uppercase() {
        1 // UPPERCASE_LETTER
    } else if c.is_lowercase() {
        2 // LOWERCASE_LETTER
    } else if c.is_alphabetic() {
        5 // OTHER_LETTER
    } else if c.is_ascii_digit() || c.is_numeric() {
        9 // DECIMAL_DIGIT_NUMBER
    } else if c == ' ' || c == '\u{A0}' {
        12 // SPACE_SEPARATOR
    } else if c.is_control() {
        15 // CONTROL
    } else if (0xD800..=0xDFFF).contains(&(c as u32)) {
        19 // SURROGATE
    } else if "!\"#%&'*,./:;?@\\".contains(c) {
        24 // OTHER_PUNCTUATION
    } else if "+<=>|~".contains(c) {
        25 // MATH_SYMBOL
    } else if c == '$' {
        26 // CURRENCY_SYMBOL
    } else if "([{".contains(c) {
        21 // START_PUNCTUATION
    } else if ")]}".contains(c) {
        22 // END_PUNCTUATION
    } else if c == '-' {
        20 // DASH_PUNCTUATION
    } else if c == '_' {
        23 // CONNECTOR_PUNCTUATION
    } else if "^`".contains(c) {
        27 // MODIFIER_SYMBOL
    } else {
        28 // OTHER_SYMBOL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_double_formatting() {
        assert_eq!(java_double_to_string(1.0, false), "1.0");
        assert_eq!(java_double_to_string(0.1, false), "0.1");
        assert_eq!(java_double_to_string(1e7, false), "1.0E7");
        assert_eq!(java_double_to_string(1.5e-4, false), "1.5E-4");
        assert_eq!(java_double_to_string(123.456, false), "123.456");
        assert_eq!(java_double_to_string(0.001, false), "0.001");
        assert_eq!(java_double_to_string(-2.5, false), "-2.5");
        assert_eq!(java_double_to_string(100.0, false), "100.0");
        assert_eq!(java_double_to_string(0.1f32 as f64, true), "0.1");
    }

    #[test]
    fn radix() {
        assert_eq!(
            parse_radix("-ff", 16, i32::MIN as i128, i32::MAX as i128).ok(),
            Some(-255)
        );
        assert!(parse_radix("2147483648", 10, i32::MIN as i128, i32::MAX as i128).is_err());
        assert!(parse_radix("", 10, 0, 10).is_err());
        assert_eq!(to_radix(-255, 16), "-ff");
    }

    #[test]
    fn compare_orders_zero_and_nan() {
        assert_eq!(cmp_f64(-0.0, 0.0), -1);
        assert_eq!(cmp_f64(f64::NAN, f64::INFINITY), 1);
        assert_eq!(cmp_f64(f64::NAN, f64::NAN), 0);
    }

    #[test]
    fn java_identifier_start_and_part_follow_java_categories() {
        fn call(name: &str, sig: &str, code_point: char) -> Value {
            let mut vm = Engine::new(vec![], crate::engine::VmConfig::default());
            static_intrinsic(
                &mut vm,
                "Ljava/lang/Character;",
                name,
                sig,
                &[Value::Int(code_point as i32)],
            )
            .unwrap()
            .expect("Character intrinsic")
        }

        for c in ['A', 'π', 'Ⅰ', '$', '_', '€'] {
            assert_eq!(
                call("isJavaIdentifierStart", "(C)Z", c),
                Value::Int(1),
                "{c}"
            );
        }
        for c in ['1', '-', '\u{0301}', '\n'] {
            assert_eq!(
                call("isJavaIdentifierStart", "(C)Z", c),
                Value::Int(0),
                "{c}"
            );
        }
        for c in ['A', '7', '\u{0301}', '\u{200C}', '\u{0001}', '$'] {
            assert_eq!(
                call("isJavaIdentifierPart", "(I)Z", c),
                Value::Int(1),
                "{c}"
            );
        }
        assert_eq!(call("isJavaIdentifierPart", "(I)Z", '\n'), Value::Int(0));
        assert_eq!(
            call("isIdentifierIgnorable", "(C)Z", '\u{200C}'),
            Value::Int(1)
        );
        assert_eq!(
            call("isIdentifierIgnorable", "(C)Z", '\u{0301}'),
            Value::Int(0)
        );
    }
}
