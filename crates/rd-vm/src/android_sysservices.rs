//! Real, bounded subsets for several Appendix C surfaces that were
//! previously `stub`/`not_implemented` in the registry.
//!
//! This module intentionally does **not** try to model complete Android
//! packages.  Each handler below implements the documented public subset used
//! to justify a `partial` registry status, with AOSP argument checks and
//! value semantics for that subset:
//!
//! * `android.text.format.Formatter` file-size / IPv4 formatting and
//!   `android.text.format.DateFormat.format` over the VM's deterministic
//!   en-US/UTC time model;
//! * `android.text.util.Rfc822Tokenizer` / `Rfc822Token` address parsing and
//!   quoting;
//! * the pure `android.view.animation.*Interpolator` easing functions;
//! * an in-memory `android.accounts.AccountManager` account database for the
//!   explicit-account, password and user-data operations.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const FORMATTER: &str = "Landroid/text/format/Formatter;";
const FORMATTER_BYTES_RESULT: &str = "Landroid/text/format/Formatter$BytesResult;";
const ANDROID_DATE_FORMAT: &str = "Landroid/text/format/DateFormat;";
const RFC822_TOKEN: &str = "Landroid/text/util/Rfc822Token;";
const RFC822_TOKENIZER: &str = "Landroid/text/util/Rfc822Tokenizer;";
const ACCOUNT: &str = "Landroid/accounts/Account;";
const ACCOUNT_MANAGER: &str = "Landroid/accounts/AccountManager;";

const FLAG_SHORTER: i32 = 1 << 0;
const FLAG_CALCULATE_ROUNDED: i32 = 1 << 1;
const FLAG_SI_UNITS: i32 = 1 << 2;
const FLAG_IEC_UNITS: i32 = 1 << 3;

const INTERPOLATORS: &[&str] = &[
    "Landroid/view/animation/LinearInterpolator;",
    "Landroid/view/animation/AccelerateInterpolator;",
    "Landroid/view/animation/DecelerateInterpolator;",
    "Landroid/view/animation/AccelerateDecelerateInterpolator;",
    "Landroid/view/animation/AnticipateInterpolator;",
    "Landroid/view/animation/OvershootInterpolator;",
    "Landroid/view/animation/AnticipateOvershootInterpolator;",
    "Landroid/view/animation/BounceInterpolator;",
    "Landroid/view/animation/CycleInterpolator;",
];

fn throw(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}
fn npe(what: &str) -> VmExit {
    throw("Ljava/lang/NullPointerException;", what.to_string())
}
fn iae(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/IllegalArgumentException;", msg)
}

fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}
fn opt_str(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}
fn charseq(vm: &mut Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    match v {
        Some(Value::Obj(r)) => {
            if let Ok(s) = vm.heap.as_str(*r) {
                Ok(Some(s.to_string()))
            } else {
                Ok(Some(crate::intrinsics::object_to_string(vm, *r)?))
            }
        }
        _ => Ok(None),
    }
}
fn req_charseq(vm: &mut Engine, args: &[Value], i: usize, what: &str) -> Result<String, VmExit> {
    charseq(vm, args.get(i))?.ok_or_else(|| npe(what))
}
fn oarg(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn iarg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
fn larg(args: &[Value], i: usize) -> i64 {
    args.get(i).and_then(|v| v.as_long().ok()).unwrap_or(0)
}
fn farg(args: &[Value], i: usize) -> f32 {
    args.get(i).and_then(|v| v.as_float().ok()).unwrap_or(0.0)
}
fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
fn get(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}
fn field_str(vm: &Engine, r: ObjRef, f: &str) -> Option<String> {
    opt_str(vm, Some(&get(vm, r, f)))
}
fn put_opt_str(vm: &mut Engine, r: ObjRef, f: &str, s: Option<String>) -> Result<(), VmExit> {
    let v = match s {
        Some(s) => jstr(vm, s)?,
        None => Value::Null,
    };
    put(vm, r, f, v)
}
fn alloc_obj(vm: &mut Engine, class: &str, fields: Vec<(String, Value)>) -> Result<ObjRef, VmExit> {
    vm.heap
        .alloc_instance(class.to_string(), fields)
        .map_err(crate::err::oom_throwable)
}
fn obj_array(vm: &mut Engine, desc: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
    crate::arrays::new_array(vm, desc, ElemKind::Obj, elems)
}
fn char_array(vm: &mut Engine, chars: &[u16]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[C",
        ElemKind::Int,
        chars.iter().map(|c| Value::Int(*c as i32)).collect(),
    )
}
fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}
fn bool_value(b: bool) -> Value {
    Value::Int(b as i32)
}

pub(crate) fn host_static(
    _vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    let v = match (owner, field) {
        (FORMATTER, "FLAG_SHORTER") => Value::Int(FLAG_SHORTER),
        (FORMATTER, "FLAG_CALCULATE_ROUNDED") => Value::Int(FLAG_CALCULATE_ROUNDED),
        (FORMATTER, "FLAG_SI_UNITS") => Value::Int(FLAG_SI_UNITS),
        (FORMATTER, "FLAG_IEC_UNITS") => Value::Int(FLAG_IEC_UNITS),
        (ANDROID_DATE_FORMAT, "QUOTE") => Value::Int('\'' as i32),
        (ANDROID_DATE_FORMAT, "AM_PM") => Value::Int('a' as i32),
        (ANDROID_DATE_FORMAT, "CAPITAL_AM_PM") => Value::Int('A' as i32),
        (ANDROID_DATE_FORMAT, "DATE") => Value::Int('d' as i32),
        (ANDROID_DATE_FORMAT, "DAY") => Value::Int('E' as i32),
        (ANDROID_DATE_FORMAT, "HOUR") => Value::Int('h' as i32),
        (ANDROID_DATE_FORMAT, "HOUR_OF_DAY") => Value::Int('k' as i32),
        (ANDROID_DATE_FORMAT, "MINUTE") => Value::Int('m' as i32),
        (ANDROID_DATE_FORMAT, "MONTH") => Value::Int('M' as i32),
        (ANDROID_DATE_FORMAT, "STANDALONE_MONTH") => Value::Int('L' as i32),
        (ANDROID_DATE_FORMAT, "SECONDS") => Value::Int('s' as i32),
        (ANDROID_DATE_FORMAT, "TIME_ZONE") => Value::Int('z' as i32),
        (ANDROID_DATE_FORMAT, "YEAR") => Value::Int('y' as i32),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        FORMATTER => formatter_static(vm, name, sig, args),
        ANDROID_DATE_FORMAT => android_date_format_static(vm, name, sig, args),
        RFC822_TOKENIZER => rfc822_tokenizer_static(vm, name, sig, args),
        RFC822_TOKEN => rfc822_token_static(vm, name, sig, args),
        ACCOUNT => account_static(vm, name, sig, args),
        ACCOUNT_MANAGER if name == "get" => account_manager_get(vm, args).map(Some),
        _ => Ok(None),
    }
}

fn handled_class(recv_class: &str, mref_class: &str) -> Option<&'static str> {
    const HANDLED: &[&str] = &[
        FORMATTER_BYTES_RESULT,
        RFC822_TOKEN,
        RFC822_TOKENIZER,
        ACCOUNT,
        ACCOUNT_MANAGER,
    ];
    if let Some(c) = HANDLED.iter().find(|c| **c == recv_class) {
        return Some(c);
    }
    if let Some(c) = INTERPOLATORS.iter().find(|c| **c == recv_class) {
        return Some(c);
    }
    if let Some(c) = HANDLED.iter().find(|c| **c == mref_class) {
        return Some(c);
    }
    INTERPOLATORS.iter().find(|c| **c == mref_class).copied()
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let Some(class) = handled_class(recv_class, mref_class) else {
        return Ok(None);
    };
    match class {
        FORMATTER_BYTES_RESULT => bytes_result_instance(vm, name, sig, recv, args),
        RFC822_TOKEN => rfc822_token_instance(vm, name, sig, recv, args),
        RFC822_TOKENIZER => rfc822_tokenizer_instance(vm, name, sig, args),
        ACCOUNT => account_instance(vm, name, sig, recv, args),
        ACCOUNT_MANAGER => account_manager_instance(vm, name, sig, recv, args),
        _ if INTERPOLATORS.contains(&class) => {
            interpolator_instance(vm, class, name, sig, recv, args)
        }
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// android.text.format.Formatter / DateFormat
// ---------------------------------------------------------------------------

fn rounded_bytes(size_bytes: i64, flags: i32) -> (f32, &'static str, usize, i64) {
    let unit = if flags & FLAG_IEC_UNITS != 0 {
        1024.0f32
    } else {
        1000.0f32
    };
    let negative = size_bytes < 0;
    // AOSP does `-sizeBytes` in long arithmetic before widening to float; the
    // Long.MIN_VALUE corner keeps the same wrapped magnitude in practice for
    // formatting, so use the absolute value as an i128 to avoid a Rust panic.
    let mut result = (size_bytes as i128).unsigned_abs() as f32;
    let mut units = "B";
    let mut mult: i64 = 1;
    if result > 900.0 {
        units = "kB";
        mult = unit as i64;
        result /= unit;
    }
    if result > 900.0 {
        units = "MB";
        mult *= unit as i64;
        result /= unit;
    }
    if result > 900.0 {
        units = "GB";
        mult *= unit as i64;
        result /= unit;
    }
    if result > 900.0 {
        units = "TB";
        mult *= unit as i64;
        result /= unit;
    }
    if result > 900.0 {
        units = "PB";
        mult *= unit as i64;
        result /= unit;
    }
    let (round_factor, round_digits) = if mult == 1 || result >= 100.0 {
        (1i64, 0usize)
    } else if result < 1.0 {
        (100i64, 2usize)
    } else if result < 10.0 {
        if flags & FLAG_SHORTER != 0 {
            (10i64, 1usize)
        } else {
            (100i64, 2usize)
        }
    } else if flags & FLAG_SHORTER != 0 {
        (1i64, 0usize)
    } else {
        (100i64, 2usize)
    };
    if negative {
        result = -result;
    }
    let rounded = if flags & FLAG_CALCULATE_ROUNDED == 0 {
        0
    } else {
        ((result * round_factor as f32).round() as i64) * mult / round_factor
    };
    (result, units, round_digits, rounded)
}

fn format_size(size_bytes: i64, flags: i32) -> String {
    let (value, units, digits, _) = rounded_bytes(size_bytes, flags);
    format!("{value:.digits$} {units}")
}

fn formatter_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("formatFileSize", "(Landroid/content/Context;J)Ljava/lang/String;") => {
            if oarg(args, 0).is_none() {
                jstr(vm, String::new())?
            } else {
                jstr(vm, format_size(larg(args, 1), FLAG_SI_UNITS))?
            }
        }
        ("formatFileSize", "(Landroid/content/Context;JI)Ljava/lang/String;") => {
            if oarg(args, 0).is_none() {
                jstr(vm, String::new())?
            } else {
                jstr(vm, format_size(larg(args, 1), iarg(args, 2)))?
            }
        }
        ("formatShortFileSize", "(Landroid/content/Context;J)Ljava/lang/String;") => {
            if oarg(args, 0).is_none() {
                jstr(vm, String::new())?
            } else {
                jstr(vm, format_size(larg(args, 1), FLAG_SI_UNITS | FLAG_SHORTER))?
            }
        }
        (
            "formatBytes",
            "(Landroid/content/res/Resources;JI)Landroid/text/format/Formatter$BytesResult;",
        ) => {
            if oarg(args, 0).is_none() {
                return Err(npe("Formatter.formatBytes resources == null"));
            }
            let (value, units, digits, rounded) = rounded_bytes(larg(args, 1), iarg(args, 2));
            let value = jstr(vm, format!("{value:.digits$}"))?;
            let units = jstr(vm, units.to_string())?;
            Value::Obj(alloc_obj(
                vm,
                FORMATTER_BYTES_RESULT,
                vec![
                    ("value".to_string(), value),
                    ("units".to_string(), units),
                    ("roundedBytes".to_string(), Value::Long(rounded)),
                ],
            )?)
        }
        ("formatIpAddress", "(I)Ljava/lang/String;") => {
            let ip = iarg(args, 0) as u32;
            jstr(
                vm,
                format!(
                    "{}.{}.{}.{}",
                    ip & 0xff,
                    (ip >> 8) & 0xff,
                    (ip >> 16) & 0xff,
                    (ip >> 24) & 0xff
                ),
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn bytes_result_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" && sig == "(Ljava/lang/String;Ljava/lang/String;J)V" {
        put(
            vm,
            recv,
            "value",
            args.first().cloned().unwrap_or(Value::Null),
        )?;
        put(
            vm,
            recv,
            "units",
            args.get(1).cloned().unwrap_or(Value::Null),
        )?;
        put(vm, recv, "roundedBytes", Value::Long(larg(args, 2)))?;
        return Ok(Some(Value::Null));
    }
    Ok(None)
}

fn temporal_ms_from_value(vm: &Engine, v: Option<&Value>) -> Option<i64> {
    let r = match v {
        Some(Value::Obj(r)) => *r,
        _ => return None,
    };
    let class = vm.heap.class_of(r).ok()?.to_string();
    let field = if class == "Ljava/util/Date;" {
        "rd_ms"
    } else if class == "Ljava/util/Calendar;" || class == "Ljava/util/GregorianCalendar;" {
        "rd$calendar_ms"
    } else {
        return None;
    };
    match get(vm, r, field) {
        Value::Long(ms) => Some(ms),
        Value::Int(ms) => Some(ms as i64),
        _ => Some(0),
    }
}

fn android_date_pattern(pattern: &str, ms: i64) -> String {
    // Map the Android-specific letters not already covered by jtime's
    // SimpleDateFormat-style engine, outside quoted literals.
    let hour0_11 = ms.div_euclid(3_600_000).rem_euclid(24) % 12;
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut quoted = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push_str("''");
                i += 2;
                continue;
            }
            quoted = !quoted;
            out.push(c);
            i += 1;
            continue;
        }
        if quoted || !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        match c {
            'A' => out.push_str(&"a".repeat(n)),
            'c' => out.push_str(&"E".repeat(n)),
            'k' => out.push_str(&"H".repeat(n)),
            'K' => out.push_str(&format!("'{:0width$}'", hour0_11, width = n)),
            'z' => out.push_str(if n >= 4 {
                "'Coordinated Universal Time'"
            } else {
                "'UTC'"
            }),
            _ => out.push_str(&c.to_string().repeat(n)),
        }
        i += n;
    }
    out
}

fn date_format_order(pattern: &str) -> Result<[u16; 3], VmExit> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = [0u16; 3];
    let mut result_index = 0usize;
    let mut saw_day = false;
    let mut saw_month = false;
    let mut saw_year = false;
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            'd' if !saw_day => {
                result[result_index] = 'd' as u16;
                result_index += 1;
                saw_day = true;
            }
            'L' | 'M' if !saw_month => {
                result[result_index] = 'M' as u16;
                result_index += 1;
                saw_month = true;
            }
            'y' if !saw_year => {
                result[result_index] = 'y' as u16;
                result_index += 1;
                saw_year = true;
            }
            'd' | 'L' | 'M' | 'y' | 'G' => {}
            '\'' => {
                if i + 1 < chars.len() && chars[i + 1] == '\'' {
                    i += 1;
                } else {
                    let Some(end) = chars.iter().skip(i + 1).position(|c| *c == '\'') else {
                        return Err(iae(format!("Bad quoting in {pattern}")));
                    };
                    i += end + 2;
                    continue;
                }
            }
            c if c.is_ascii_alphabetic() => {
                return Err(iae(format!("Bad pattern character '{c}' in {pattern}")));
            }
            _ => {}
        }
        i += 1;
    }
    Ok(result)
}

fn has_designator(pattern: Option<&str>, designator: char) -> bool {
    let Some(pattern) = pattern else {
        return false;
    };
    let mut quoted = false;
    for c in pattern.chars() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && c == designator {
            return true;
        }
    }
    false
}

fn android_date_format_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("format", "(Ljava/lang/CharSequence;J)Ljava/lang/CharSequence;") => {
            let pattern = req_charseq(vm, args, 0, "DateFormat.format inFormat == null")?;
            let ms = larg(args, 1);
            jstr(
                vm,
                crate::jtime::format_pattern(&android_date_pattern(&pattern, ms), "en", ms),
            )?
        }
        ("format", "(Ljava/lang/CharSequence;Ljava/util/Date;)Ljava/lang/CharSequence;")
        | ("format", "(Ljava/lang/CharSequence;Ljava/util/Calendar;)Ljava/lang/CharSequence;") => {
            let pattern = req_charseq(vm, args, 0, "DateFormat.format inFormat == null")?;
            let ms = temporal_ms_from_value(vm, args.get(1))
                .ok_or_else(|| npe("DateFormat.format date == null"))?;
            jstr(
                vm,
                crate::jtime::format_pattern(&android_date_pattern(&pattern, ms), "en", ms),
            )?
        }
        ("getDateFormatOrder", "(Ljava/lang/String;)[C") => {
            let pattern = opt_str(vm, args.first())
                .ok_or_else(|| npe("DateFormat.getDateFormatOrder pattern == null"))?;
            char_array(vm, &date_format_order(&pattern)?)?
        }
        ("getDateFormatOrder", "(Landroid/content/Context;)[C") => {
            if oarg(args, 0).is_none() {
                return Err(npe("DateFormat.getDateFormatOrder context == null"));
            }
            // The VM's deterministic locale is en-US (M/d/yy).
            char_array(vm, &date_format_order("M/d/yy")?)?
        }
        ("is24HourFormat", "(Landroid/content/Context;)Z")
        | ("is24HourFormat", "(Landroid/content/Context;I)Z") => {
            if oarg(args, 0).is_none() {
                return Err(npe("DateFormat.is24HourFormat context == null"));
            }
            bool_value(false)
        }
        ("getTimeFormatString", "(Landroid/content/Context;)Ljava/lang/String;")
        | ("getTimeFormatString", "(Landroid/content/Context;I)Ljava/lang/String;") => {
            if oarg(args, 0).is_none() {
                return Err(npe("DateFormat.getTimeFormatString context == null"));
            }
            jstr(vm, "h:mm a".to_string())?
        }
        ("hasSeconds", "(Ljava/lang/CharSequence;)Z") => {
            let pattern = charseq(vm, args.first())?;
            bool_value(has_designator(pattern.as_deref(), 's'))
        }
        ("hasDesignator", "(Ljava/lang/CharSequence;C)Z") => {
            let pattern = charseq(vm, args.first())?;
            let designator = char::from_u32(iarg(args, 1) as u32).unwrap_or('\0');
            bool_value(has_designator(pattern.as_deref(), designator))
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

// ---------------------------------------------------------------------------
// android.text.util.Rfc822Token / Rfc822Tokenizer
// ---------------------------------------------------------------------------

fn rfc822_token_obj(
    vm: &mut Engine,
    name: Option<String>,
    address: Option<String>,
    comment: Option<String>,
) -> Result<ObjRef, VmExit> {
    let name = match name {
        Some(s) => jstr(vm, s)?,
        None => Value::Null,
    };
    let address = match address {
        Some(s) => jstr(vm, s)?,
        None => Value::Null,
    };
    let comment = match comment {
        Some(s) => jstr(vm, s)?,
        None => Value::Null,
    };
    alloc_obj(
        vm,
        RFC822_TOKEN,
        vec![
            ("mName".to_string(), name),
            ("mAddress".to_string(), address),
            ("mComment".to_string(), comment),
        ],
    )
}

fn quote_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
fn quote_name_if_necessary(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ') {
        name.to_string()
    } else {
        format!("\"{}\"", quote_name(name))
    }
}
fn quote_comment(comment: &str) -> String {
    let mut out = String::new();
    for c in comment.chars() {
        if c == '(' || c == ')' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn rfc822_token_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("quoteNameIfNecessary", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let s = opt_str(vm, args.first())
                .ok_or_else(|| npe("Rfc822Token.quoteNameIfNecessary name == null"))?;
            jstr(vm, quote_name_if_necessary(&s))?
        }
        ("quoteName", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let s = opt_str(vm, args.first())
                .ok_or_else(|| npe("Rfc822Token.quoteName name == null"))?;
            jstr(vm, quote_name(&s))?
        }
        ("quoteComment", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let s = opt_str(vm, args.first())
                .ok_or_else(|| npe("Rfc822Token.quoteComment comment == null"))?;
            jstr(vm, quote_comment(&s))?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn rfc822_token_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V") => {
            put(
                vm,
                recv,
                "mName",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "mAddress",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "mComment",
                args.get(2).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("getAddress", "()Ljava/lang/String;") => get(vm, recv, "mAddress"),
        ("getComment", "()Ljava/lang/String;") => get(vm, recv, "mComment"),
        ("setName", "(Ljava/lang/String;)V") => {
            put(
                vm,
                recv,
                "mName",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setAddress", "(Ljava/lang/String;)V") => {
            put(
                vm,
                recv,
                "mAddress",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("setComment", "(Ljava/lang/String;)V") => {
            put(
                vm,
                recv,
                "mComment",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("toString", "()Ljava/lang/String;") => {
            let mut out = String::new();
            if let Some(name) = field_str(vm, recv, "mName") {
                if !name.is_empty() {
                    out.push_str(&quote_name_if_necessary(&name));
                    out.push(' ');
                }
            }
            if let Some(comment) = field_str(vm, recv, "mComment") {
                if !comment.is_empty() {
                    out.push('(');
                    out.push_str(&quote_comment(&comment));
                    out.push_str(") ");
                }
            }
            if let Some(address) = field_str(vm, recv, "mAddress") {
                if !address.is_empty() {
                    out.push('<');
                    out.push_str(&address);
                    out.push('>');
                }
            }
            jstr(vm, out)?
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let other = oarg(args, 0);
            let eq = match other {
                Some(other) if other == recv => true,
                Some(other) => {
                    vm.heap
                        .class_of(other)
                        .ok()
                        .map(|c| c.to_string())
                        .as_deref()
                        == Some(RFC822_TOKEN)
                        && field_str(vm, recv, "mName") == field_str(vm, other, "mName")
                        && field_str(vm, recv, "mAddress") == field_str(vm, other, "mAddress")
                        && field_str(vm, recv, "mComment") == field_str(vm, other, "mComment")
                }
                None => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => {
            let mut result: i32 = 17;
            if let Some(s) = field_str(vm, recv, "mName") {
                result = result.wrapping_mul(31).wrapping_add(java_string_hash(&s));
            }
            if let Some(s) = field_str(vm, recv, "mAddress") {
                result = result.wrapping_mul(31).wrapping_add(java_string_hash(&s));
            }
            if let Some(s) = field_str(vm, recv, "mComment") {
                result = result.wrapping_mul(31).wrapping_add(java_string_hash(&s));
            }
            Value::Int(result)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn crunch(name: &mut Vec<u16>) {
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == 0 {
            let remove = i == 0
                || i + 1 == name.len()
                || name[i - 1] == b' ' as u16
                || name[i - 1] == 0
                || name[i + 1] == b' ' as u16
                || name[i + 1] == 0;
            if remove {
                name.remove(i);
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    for c in name.iter_mut() {
        if *c == 0 {
            *c = b' ' as u16;
        }
    }
}

#[derive(Clone)]
struct Rfc822Parts {
    name: Option<String>,
    address: Option<String>,
    comment: Option<String>,
}

fn parse_rfc822(text: &str) -> Vec<Rfc822Parts> {
    let text: Vec<u16> = text.encode_utf16().collect();
    let mut out = Vec::new();
    let mut name: Vec<u16> = Vec::new();
    let mut address: Vec<u16> = Vec::new();
    let mut comment: Vec<u16> = Vec::new();
    let mut i = 0usize;
    let cursor = text.len();
    let push_current = |name: &mut Vec<u16>,
                        address: &mut Vec<u16>,
                        comment: &mut Vec<u16>,
                        out: &mut Vec<Rfc822Parts>| {
        crunch(name);
        if !address.is_empty() {
            out.push(Rfc822Parts {
                name: Some(String::from_utf16_lossy(name)),
                address: Some(String::from_utf16_lossy(address)),
                comment: Some(String::from_utf16_lossy(comment)),
            });
        } else if !name.is_empty() {
            out.push(Rfc822Parts {
                name: None,
                address: Some(String::from_utf16_lossy(name)),
                comment: Some(String::from_utf16_lossy(comment)),
            });
        }
        name.clear();
        address.clear();
        comment.clear();
    };

    while i < cursor {
        let c = text[i];
        if c == b',' as u16 || c == b';' as u16 {
            i += 1;
            while i < cursor && text[i] == b' ' as u16 {
                i += 1;
            }
            push_current(&mut name, &mut address, &mut comment, &mut out);
        } else if c == b'"' as u16 {
            i += 1;
            while i < cursor {
                let c = text[i];
                if c == b'"' as u16 {
                    i += 1;
                    break;
                } else if c == b'\\' as u16 {
                    if i + 1 < cursor {
                        name.push(text[i + 1]);
                    }
                    i += 2;
                } else {
                    name.push(c);
                    i += 1;
                }
            }
        } else if c == b'(' as u16 {
            let mut level = 1i32;
            i += 1;
            while i < cursor && level > 0 {
                let c = text[i];
                if c == b')' as u16 {
                    if level > 1 {
                        comment.push(c);
                    }
                    level -= 1;
                    i += 1;
                } else if c == b'(' as u16 {
                    comment.push(c);
                    level += 1;
                    i += 1;
                } else if c == b'\\' as u16 {
                    if i + 1 < cursor {
                        comment.push(text[i + 1]);
                    }
                    i += 2;
                } else {
                    comment.push(c);
                    i += 1;
                }
            }
        } else if c == b'<' as u16 {
            i += 1;
            while i < cursor {
                let c = text[i];
                if c == b'>' as u16 {
                    i += 1;
                    break;
                } else {
                    address.push(c);
                    i += 1;
                }
            }
        } else if c == b' ' as u16 {
            name.push(0);
            i += 1;
        } else {
            name.push(c);
            i += 1;
        }
    }
    push_current(&mut name, &mut address, &mut comment, &mut out);
    out
}

fn find_token_end_u16(text: &[u16], cursor: usize) -> usize {
    let len = text.len();
    let mut i = cursor;
    while i < len {
        let c = text[i];
        if c == b',' as u16 || c == b';' as u16 {
            return i;
        } else if c == b'"' as u16 {
            i += 1;
            while i < len {
                let c = text[i];
                if c == b'"' as u16 {
                    i += 1;
                    break;
                } else if c == b'\\' as u16 && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if c == b'(' as u16 {
            let mut level = 1i32;
            i += 1;
            while i < len && level > 0 {
                let c = text[i];
                if c == b')' as u16 {
                    level -= 1;
                    i += 1;
                } else if c == b'(' as u16 {
                    level += 1;
                    i += 1;
                } else if c == b'\\' as u16 && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if c == b'<' as u16 {
            i += 1;
            while i < len {
                let c = text[i];
                if c == b'>' as u16 {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    i
}

fn find_token_start_u16(text: &[u16], cursor: usize) -> usize {
    let mut best = 0usize;
    let mut i = 0usize;
    while i < cursor {
        i = find_token_end_u16(text, i);
        if i < cursor {
            i += 1;
            while i < cursor && text[i] == b' ' as u16 {
                i += 1;
            }
            if i < cursor {
                best = i;
            }
        }
    }
    best
}

fn rfc822_tokenizer_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("tokenize", "(Ljava/lang/CharSequence;)[Landroid/text/util/Rfc822Token;") => {
            let text = req_charseq(vm, args, 0, "Rfc822Tokenizer.tokenize text == null")?;
            let mut elems = Vec::new();
            for part in parse_rfc822(&text) {
                elems.push(Value::Obj(rfc822_token_obj(
                    vm,
                    part.name,
                    part.address,
                    part.comment,
                )?));
            }
            obj_array(vm, "[Landroid/text/util/Rfc822Token;", elems)?
        }
        ("tokenize", "(Ljava/lang/CharSequence;Ljava/util/Collection;)V") => {
            let text = req_charseq(vm, args, 0, "Rfc822Tokenizer.tokenize text == null")?;
            let parts = parse_rfc822(&text);
            let Some(out) = oarg(args, 1) else {
                if parts.is_empty() {
                    return Ok(Some(Value::Null));
                }
                return Err(npe("Rfc822Tokenizer.tokenize out == null"));
            };
            let mut token_refs = Vec::new();
            for part in parts {
                token_refs.push(rfc822_token_obj(vm, part.name, part.address, part.comment)?);
            }
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&out) {
                elems.extend(token_refs.into_iter().map(Value::Obj));
            } else {
                for token in token_refs {
                    let _ = vm.invoke_instance(
                        out,
                        "add",
                        "(Ljava/lang/Object;)Z",
                        &[Value::Obj(token)],
                    )?;
                }
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn rfc822_tokenizer_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("findTokenStart", "(Ljava/lang/CharSequence;I)I") => {
            let text = req_charseq(vm, args, 0, "Rfc822Tokenizer.findTokenStart text == null")?;
            let units: Vec<u16> = text.encode_utf16().collect();
            Value::Int(find_token_start_u16(&units, iarg(args, 1).max(0) as usize) as i32)
        }
        ("findTokenEnd", "(Ljava/lang/CharSequence;I)I") => {
            let text = req_charseq(vm, args, 0, "Rfc822Tokenizer.findTokenEnd text == null")?;
            let units: Vec<u16> = text.encode_utf16().collect();
            Value::Int(find_token_end_u16(&units, iarg(args, 1).max(0) as usize) as i32)
        }
        ("terminateToken", "(Ljava/lang/CharSequence;)Ljava/lang/CharSequence;") => {
            let text = charseq(vm, args.first())?;
            jstr(
                vm,
                format!("{}, ", text.unwrap_or_else(|| "null".to_string())),
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

// ---------------------------------------------------------------------------
// android.view.animation interpolators
// ---------------------------------------------------------------------------

fn ffield(vm: &Engine, r: ObjRef, f: &str, default: f32) -> f32 {
    match get(vm, r, f) {
        Value::Float(v) => v,
        Value::Int(v) => v as f32,
        _ => default,
    }
}

fn interpolator_instance(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        // AttributeSet constructors use the same defaults in the host model;
        // there is no styled attribute array behind them in these tests.
        let first_float = if sig.contains('F') {
            farg(args, 0)
        } else {
            0.0
        };
        match class {
            "Landroid/view/animation/AccelerateInterpolator;"
            | "Landroid/view/animation/DecelerateInterpolator;" => {
                let factor = if sig.contains('F') { first_float } else { 1.0 };
                put(vm, recv, "mFactor", Value::Float(factor))?;
                put(vm, recv, "mDoubleFactor", Value::Float(2.0 * factor))?;
            }
            "Landroid/view/animation/AnticipateInterpolator;"
            | "Landroid/view/animation/OvershootInterpolator;" => {
                let tension = if sig.contains('F') { first_float } else { 2.0 };
                put(vm, recv, "mTension", Value::Float(tension))?;
            }
            "Landroid/view/animation/AnticipateOvershootInterpolator;" => {
                let tension = if sig == "(F)V" {
                    first_float * 1.5
                } else if sig == "(FF)V" {
                    first_float * farg(args, 1)
                } else {
                    2.0 * 1.5
                };
                put(vm, recv, "mTension", Value::Float(tension))?;
            }
            "Landroid/view/animation/CycleInterpolator;" => {
                if !sig.contains('F') && !sig.contains("Landroid/util/AttributeSet;") {
                    return Ok(None);
                }
                let cycles = if sig.contains('F') { first_float } else { 1.0 };
                put(vm, recv, "mCycles", Value::Float(cycles))?;
            }
            _ => {}
        }
        return Ok(Some(Value::Null));
    }
    if name != "getInterpolation" || sig != "(F)F" {
        return Ok(None);
    }
    let input = farg(args, 0);
    let out = match class {
        "Landroid/view/animation/LinearInterpolator;" => input,
        "Landroid/view/animation/AccelerateInterpolator;" => {
            let factor = ffield(vm, recv, "mFactor", 1.0);
            if factor == 1.0 {
                input * input
            } else {
                (input as f64).powf((2.0 * factor) as f64) as f32
            }
        }
        "Landroid/view/animation/DecelerateInterpolator;" => {
            let factor = ffield(vm, recv, "mFactor", 1.0);
            if factor == 1.0 {
                1.0 - (1.0 - input) * (1.0 - input)
            } else {
                1.0 - ((1.0 - input) as f64).powf((2.0 * factor) as f64) as f32
            }
        }
        "Landroid/view/animation/AccelerateDecelerateInterpolator;" => {
            (((input as f64 + 1.0) * std::f64::consts::PI).cos() / 2.0) as f32 + 0.5
        }
        "Landroid/view/animation/AnticipateInterpolator;" => {
            let tension = ffield(vm, recv, "mTension", 2.0);
            input * input * ((tension + 1.0) * input - tension)
        }
        "Landroid/view/animation/OvershootInterpolator;" => {
            let tension = ffield(vm, recv, "mTension", 2.0);
            let t = input - 1.0;
            t * t * ((tension + 1.0) * t + tension) + 1.0
        }
        "Landroid/view/animation/AnticipateOvershootInterpolator;" => {
            let tension = ffield(vm, recv, "mTension", 3.0);
            let a = |t: f32, s: f32| t * t * ((s + 1.0) * t - s);
            let o = |t: f32, s: f32| t * t * ((s + 1.0) * t + s);
            if input < 0.5 {
                0.5 * a(input * 2.0, tension)
            } else {
                0.5 * (o(input * 2.0 - 2.0, tension) + 2.0)
            }
        }
        "Landroid/view/animation/BounceInterpolator;" => {
            let bounce = |t: f32| t * t * 8.0;
            // AOSP first scales t by 1.1226.
            let t = input * 1.1226;
            if t < 0.3535 {
                bounce(t)
            } else if t < 0.7408 {
                bounce(t - 0.54719) + 0.7
            } else if t < 0.9644 {
                bounce(t - 0.8526) + 0.9
            } else {
                bounce(t - 1.0435) + 0.95
            }
        }
        "Landroid/view/animation/CycleInterpolator;" => {
            let cycles = ffield(vm, recv, "mCycles", 1.0);
            (2.0 * cycles as f64 * std::f64::consts::PI * input as f64).sin() as f32
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Float(out)))
}

// ---------------------------------------------------------------------------
// android.accounts.Account / AccountManager
// ---------------------------------------------------------------------------

fn account_name_type(vm: &Engine, account: ObjRef) -> (String, String) {
    (
        field_str(vm, account, "name").unwrap_or_default(),
        field_str(vm, account, "type").unwrap_or_default(),
    )
}

fn account_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "toSafeName" && sig == "(Ljava/lang/String;C)Ljava/lang/String;" {
        let input =
            opt_str(vm, args.first()).ok_or_else(|| npe("Account.toSafeName name == null"))?;
        let replacement = char::from_u32(iarg(args, 1) as u32).unwrap_or('x');
        let safe: String = input
            .chars()
            .map(|c| if c.is_alphanumeric() { replacement } else { c })
            .collect();
        return Ok(Some(jstr(vm, safe)?));
    }
    Ok(None)
}

fn account_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;)V") => {
            let name = opt_str(vm, args.first());
            let ty = opt_str(vm, args.get(1));
            if name.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
                return Err(iae(format!(
                    "the name must not be empty: {}",
                    name.as_deref().unwrap_or("null")
                )));
            }
            if ty.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
                return Err(iae(format!(
                    "the type must not be empty: {}",
                    ty.as_deref().unwrap_or("null")
                )));
            }
            put(
                vm,
                recv,
                "name",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "type",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "accessId", Value::Null)?;
            Value::Null
        }
        ("<init>", "(Landroid/accounts/Account;Ljava/lang/String;)V") => {
            let other = oarg(args, 0).ok_or_else(|| npe("Account other == null"))?;
            put(vm, recv, "name", get(vm, other, "name"))?;
            put(vm, recv, "type", get(vm, other, "type"))?;
            put(
                vm,
                recv,
                "accessId",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("<init>", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V") => {
            let name = opt_str(vm, args.first());
            let ty = opt_str(vm, args.get(1));
            if name.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
                return Err(iae(format!(
                    "the name must not be empty: {}",
                    name.as_deref().unwrap_or("null")
                )));
            }
            if ty.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
                return Err(iae(format!(
                    "the type must not be empty: {}",
                    ty.as_deref().unwrap_or("null")
                )));
            }
            put(
                vm,
                recv,
                "name",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "type",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            put(
                vm,
                recv,
                "accessId",
                args.get(2).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let eq = match oarg(args, 0) {
                Some(other) if other == recv => true,
                Some(other) => {
                    let other_class = vm.heap.class_of(other).ok().map(|c| c.to_string());
                    other_class.as_deref() == Some(ACCOUNT)
                        && account_name_type(vm, recv) == account_name_type(vm, other)
                }
                None => false,
            };
            bool_value(eq)
        }
        ("hashCode", "()I") => {
            let (name, ty) = account_name_type(vm, recv);
            let mut result: i32 = 17;
            result = result
                .wrapping_mul(31)
                .wrapping_add(java_string_hash(&name));
            result = result.wrapping_mul(31).wrapping_add(java_string_hash(&ty));
            Value::Int(result)
        }
        ("toString", "()Ljava/lang/String;") => {
            let (name, ty) = account_name_type(vm, recv);
            jstr(vm, format!("Account {{name={name}, type={ty}}}"))?
        }
        ("toSafeString", "()Ljava/lang/String;") => {
            let (name, ty) = account_name_type(vm, recv);
            let safe: String = name
                .chars()
                .map(|c| if c.is_alphanumeric() { 'x' } else { c })
                .collect();
            jstr(vm, format!("Account {{name={safe}, type={ty}}}"))?
        }
        ("describeContents", "()I") => Value::Int(0),
        ("getAccessId", "()Ljava/lang/String;") => get(vm, recv, "accessId"),
        ("writeToParcel", "(Landroid/os/Parcel;I)V") => {
            let dest = oarg(args, 0).ok_or_else(|| npe("Account.writeToParcel dest == null"))?;
            let name = get(vm, recv, "name");
            let ty = get(vm, recv, "type");
            let access_id = get(vm, recv, "accessId");
            vm.invoke_instance(dest, "writeString", "(Ljava/lang/String;)V", &[name])?;
            vm.invoke_instance(dest, "writeString", "(Ljava/lang/String;)V", &[ty])?;
            vm.invoke_instance(dest, "writeString", "(Ljava/lang/String;)V", &[access_id])?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn account_manager_get(vm: &mut Engine, args: &[Value]) -> Result<Value, VmExit> {
    if oarg(args, 0).is_none() {
        return Err(iae("context is null"));
    }
    let key = (ACCOUNT_MANAGER.to_string(), "rd_instance".to_string());
    if let Some(v) = vm.statics.get(&key) {
        return Ok(v.clone());
    }
    let manager = alloc_obj(vm, ACCOUNT_MANAGER, Vec::new())?;
    let v = Value::Obj(manager);
    vm.statics.insert(key, v.clone());
    Ok(v)
}

fn accounts_list(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let key = (ACCOUNT_MANAGER.to_string(), "rd_accounts".to_string());
    if let Some(Value::Obj(list)) = vm.statics.get(&key) {
        return Ok(*list);
    }
    let list = alloc_obj(vm, "Ljava/util/ArrayList;", Vec::new())?;
    vm.fw
        .objects
        .insert(list, HostObj::List { elems: Vec::new() });
    vm.statics.insert(key, Value::Obj(list));
    Ok(list)
}

fn account_entries(vm: &mut Engine) -> Result<Vec<ObjRef>, VmExit> {
    let list = accounts_list(vm)?;
    let entries = match vm.fw.objects.get(&list) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => Vec::new(),
    };
    Ok(entries
        .into_iter()
        .filter_map(|v| match v {
            Value::Obj(r) => Some(r),
            _ => None,
        })
        .collect())
}

fn find_account(vm: &mut Engine, account: ObjRef) -> Result<Option<ObjRef>, VmExit> {
    let wanted = account_name_type(vm, account);
    for entry in account_entries(vm)? {
        if account_name_type(vm, entry) == wanted {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

fn require_account(args: &[Value], i: usize) -> Result<ObjRef, VmExit> {
    oarg(args, i).ok_or_else(|| iae("account is null"))
}

fn user_data_field(key: &str) -> String {
    format!("rd$userData.{key}")
}
fn auth_token_field(auth_token_type: &str) -> String {
    format!("rd$authToken.{auth_token_type}")
}

fn accounts_array(vm: &mut Engine, ty: Option<&str>) -> Result<Value, VmExit> {
    let mut elems = Vec::new();
    for entry in account_entries(vm)? {
        let (_, entry_type) = account_name_type(vm, entry);
        if ty.map(|t| t == entry_type).unwrap_or(true) {
            elems.push(Value::Obj(entry));
        }
    }
    obj_array(vm, "[Landroid/accounts/Account;", elems)
}

fn account_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        return Ok(Some(Value::Null));
    }
    let v = match (name, sig) {
        ("getAccounts", "()[Landroid/accounts/Account;") => accounts_array(vm, None)?,
        ("getAccountsAsUser", "(I)[Landroid/accounts/Account;") => accounts_array(vm, None)?,
        ("getAccountsByType", "(Ljava/lang/String;)[Landroid/accounts/Account;") => {
            let ty = opt_str(vm, args.first());
            accounts_array(vm, ty.as_deref())?
        }
        (
            "getAccountsByTypeAsUser",
            "(Ljava/lang/String;Landroid/os/UserHandle;)[Landroid/accounts/Account;",
        ) => {
            let ty = opt_str(vm, args.first());
            accounts_array(vm, ty.as_deref())?
        }
        ("getAccountsForPackage", "(Ljava/lang/String;I)[Landroid/accounts/Account;") => {
            accounts_array(vm, None)?
        }
        (
            "getAccountsByTypeForPackage",
            "(Ljava/lang/String;Ljava/lang/String;)[Landroid/accounts/Account;",
        ) => {
            let ty = opt_str(vm, args.first());
            accounts_array(vm, ty.as_deref())?
        }
        (
            "addAccountExplicitly",
            "(Landroid/accounts/Account;Ljava/lang/String;Landroid/os/Bundle;)Z",
        ) => {
            let account = require_account(args, 0)?;
            if find_account(vm, account)?.is_some() {
                bool_value(false)
            } else {
                put(
                    vm,
                    account,
                    "rd$password",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
                if let Some(userdata) = oarg(args, 2) {
                    let entries = match vm.fw.objects.get(&userdata) {
                        Some(HostObj::Bundle { map }) => map.clone(),
                        _ => Vec::new(),
                    };
                    for (k, value) in entries {
                        if let Some(s) = opt_str(vm, Some(&value)) {
                            let field = user_data_field(&k);
                            put_opt_str(vm, account, &field, Some(s))?;
                        }
                    }
                }
                let list = accounts_list(vm)?;
                if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
                    elems.push(Value::Obj(account));
                }
                bool_value(true)
            }
        }
        ("removeAccountExplicitly", "(Landroid/accounts/Account;)Z") => {
            let account = require_account(args, 0)?;
            let wanted = account_name_type(vm, account);
            let list = accounts_list(vm)?;
            let current = match vm.fw.objects.get(&list) {
                Some(HostObj::List { elems }) => elems.clone(),
                _ => Vec::new(),
            };
            let mut kept = Vec::with_capacity(current.len());
            let mut removed = false;
            for value in current {
                let drop = match &value {
                    Value::Obj(r) => account_name_type(vm, *r) == wanted,
                    _ => false,
                };
                if drop {
                    removed = true;
                } else {
                    kept.push(value);
                }
            }
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
                *elems = kept;
            }
            bool_value(removed)
        }
        ("getPassword", "(Landroid/accounts/Account;)Ljava/lang/String;") => {
            let account = require_account(args, 0)?;
            match find_account(vm, account)? {
                Some(entry) => get(vm, entry, "rd$password"),
                None => Value::Null,
            }
        }
        ("setPassword", "(Landroid/accounts/Account;Ljava/lang/String;)V") => {
            let account = require_account(args, 0)?;
            if let Some(entry) = find_account(vm, account)? {
                put(
                    vm,
                    entry,
                    "rd$password",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
            }
            Value::Null
        }
        ("clearPassword", "(Landroid/accounts/Account;)V") => {
            let account = require_account(args, 0)?;
            if let Some(entry) = find_account(vm, account)? {
                put(vm, entry, "rd$password", Value::Null)?;
            }
            Value::Null
        }
        ("getUserData", "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;") => {
            let Some(account) = oarg(args, 0) else {
                return Ok(Some(Value::Null));
            };
            let Some(key) = opt_str(vm, args.get(1)) else {
                return Ok(Some(Value::Null));
            };
            match find_account(vm, account)? {
                Some(entry) => get(vm, entry, &user_data_field(&key)),
                None => Value::Null,
            }
        }
        ("setUserData", "(Landroid/accounts/Account;Ljava/lang/String;Ljava/lang/String;)V") => {
            let account = require_account(args, 0)?;
            let key = opt_str(vm, args.get(1)).ok_or_else(|| iae("key is null"))?;
            if let Some(entry) = find_account(vm, account)? {
                let field = user_data_field(&key);
                put(
                    vm,
                    entry,
                    &field,
                    args.get(2).cloned().unwrap_or(Value::Null),
                )?;
            }
            Value::Null
        }
        ("setAuthToken", "(Landroid/accounts/Account;Ljava/lang/String;Ljava/lang/String;)V") => {
            let account = require_account(args, 0)?;
            let token_type =
                opt_str(vm, args.get(1)).ok_or_else(|| iae("authTokenType is null"))?;
            if let Some(entry) = find_account(vm, account)? {
                let field = auth_token_field(&token_type);
                put(
                    vm,
                    entry,
                    &field,
                    args.get(2).cloned().unwrap_or(Value::Null),
                )?;
            }
            Value::Null
        }
        ("peekAuthToken", "(Landroid/accounts/Account;Ljava/lang/String;)Ljava/lang/String;") => {
            let account = require_account(args, 0)?;
            let token_type =
                opt_str(vm, args.get(1)).ok_or_else(|| iae("authTokenType is null"))?;
            match find_account(vm, account)? {
                Some(entry) => get(vm, entry, &auth_token_field(&token_type)),
                None => Value::Null,
            }
        }
        _ => return Ok(None),
    };
    let _ = recv;
    Ok(Some(v))
}
