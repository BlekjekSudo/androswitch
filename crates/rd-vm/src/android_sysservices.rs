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
//!   explicit-account, password and user-data operations;
//! * `android.gesture` point/stroke/gesture geometry and the PCA oriented
//!   bounding box used by `GestureStroke`;
//! * the sensorless `android.hardware` profile: SensorManager orientation
//!   math, `Sensor` value getters and a ConsumerIrManager with no emitter.

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
const GESTURE: &str = "Landroid/gesture/Gesture;";
const GESTURE_POINT: &str = "Landroid/gesture/GesturePoint;";
const GESTURE_STROKE: &str = "Landroid/gesture/GestureStroke;";
const GESTURE_UTILS: &str = "Landroid/gesture/GestureUtils;";
const ORIENTED_BOUNDING_BOX: &str = "Landroid/gesture/OrientedBoundingBox;";
const RECT_F: &str = "Landroid/graphics/RectF;";
const SENSOR_MANAGER: &str = "Landroid/hardware/SensorManager;";
const SENSOR: &str = "Landroid/hardware/Sensor;";
const CONSUMER_IR_MANAGER: &str = "Landroid/hardware/ConsumerIrManager;";
const CARRIER_FREQUENCY_RANGE: &str = "Landroid/hardware/ConsumerIrManager$CarrierFrequencyRange;";

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

pub(crate) fn throw(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}
pub(crate) fn npe(what: &str) -> VmExit {
    throw("Ljava/lang/NullPointerException;", what.to_string())
}
pub(crate) fn iae(msg: impl Into<String>) -> VmExit {
    throw("Ljava/lang/IllegalArgumentException;", msg)
}

pub(crate) fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}
pub(crate) fn opt_str(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}
pub(crate) fn charseq(vm: &mut Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
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
pub(crate) fn oarg(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
pub(crate) fn iarg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
pub(crate) fn larg(args: &[Value], i: usize) -> i64 {
    args.get(i).and_then(|v| v.as_long().ok()).unwrap_or(0)
}
pub(crate) fn farg(args: &[Value], i: usize) -> f32 {
    args.get(i).and_then(|v| v.as_float().ok()).unwrap_or(0.0)
}
pub(crate) fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
pub(crate) fn get(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}
pub(crate) fn field_str(vm: &Engine, r: ObjRef, f: &str) -> Option<String> {
    opt_str(vm, Some(&get(vm, r, f)))
}
pub(crate) fn put_opt_str(
    vm: &mut Engine,
    r: ObjRef,
    f: &str,
    s: Option<String>,
) -> Result<(), VmExit> {
    let v = match s {
        Some(s) => jstr(vm, s)?,
        None => Value::Null,
    };
    put(vm, r, f, v)
}
pub(crate) fn alloc_obj(
    vm: &mut Engine,
    class: &str,
    fields: Vec<(String, Value)>,
) -> Result<ObjRef, VmExit> {
    vm.heap
        .alloc_instance(class.to_string(), fields)
        .map_err(crate::err::oom_throwable)
}
pub(crate) fn obj_array(vm: &mut Engine, desc: &str, elems: Vec<Value>) -> Result<Value, VmExit> {
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
pub(crate) fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}
pub(crate) fn bool_value(b: bool) -> Value {
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
        GESTURE_UTILS => gesture_utils_static(vm, name, sig, args),
        SENSOR_MANAGER => sensor_manager_static(vm, name, sig, args),
        SENSOR => sensor_static(vm, name, sig, args),
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
        GESTURE,
        GESTURE_POINT,
        GESTURE_STROKE,
        ORIENTED_BOUNDING_BOX,
        SENSOR_MANAGER,
        SENSOR,
        CONSUMER_IR_MANAGER,
        CARRIER_FREQUENCY_RANGE,
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
        GESTURE => gesture_instance(vm, name, sig, recv, args),
        GESTURE_POINT => gesture_point_instance(vm, name, sig, recv, args),
        GESTURE_STROKE => gesture_stroke_instance(vm, name, sig, recv, args),
        ORIENTED_BOUNDING_BOX => oriented_bounding_box_instance(vm, name, sig, recv, args),
        SENSOR_MANAGER => sensor_manager_instance(vm, name, sig, recv, args),
        SENSOR => sensor_instance(vm, name, sig, recv, args),
        CONSUMER_IR_MANAGER => consumer_ir_instance(vm, name, sig, recv, args),
        CARRIER_FREQUENCY_RANGE => carrier_frequency_range_instance(vm, name, sig, recv, args),
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

// ---------------------------------------------------------------------------
// Shared value helpers for the reconstructed system-service subsets
// ---------------------------------------------------------------------------

fn int_field(vm: &Engine, recv: ObjRef, field: &str, default: i32) -> i32 {
    match get(vm, recv, field) {
        Value::Int(v) => v,
        _ => default,
    }
}
fn long_field(vm: &Engine, recv: ObjRef, field: &str, default: i64) -> i64 {
    match get(vm, recv, field) {
        Value::Long(v) => v,
        Value::Int(v) => i64::from(v),
        _ => default,
    }
}
fn float_field(vm: &Engine, recv: ObjRef, field: &str) -> f32 {
    match get(vm, recv, field) {
        Value::Float(v) => v,
        Value::Int(v) => v as f32,
        _ => 0.0,
    }
}
#[allow(dead_code)]
fn double_field(vm: &Engine, recv: ObjRef, field: &str) -> f64 {
    match get(vm, recv, field) {
        Value::Double(v) => v,
        Value::Float(v) => f64::from(v),
        Value::Long(v) => v as f64,
        Value::Int(v) => f64::from(v),
        _ => 0.0,
    }
}
fn obj_field(vm: &Engine, recv: ObjRef, field: &str) -> Option<ObjRef> {
    match get(vm, recv, field) {
        Value::Obj(r) => Some(r),
        _ => None,
    }
}
#[allow(dead_code)]
fn darg(args: &[Value], i: usize) -> f64 {
    match args.get(i) {
        Some(Value::Double(v)) => *v,
        Some(Value::Float(v)) => f64::from(*v),
        Some(Value::Long(v)) => *v as f64,
        Some(Value::Int(v)) => f64::from(*v),
        _ => 0.0,
    }
}
pub(crate) fn new_list(vm: &mut Engine, elems: Vec<Value>) -> Result<ObjRef, VmExit> {
    let list = alloc_obj(vm, "Ljava/util/ArrayList;", Vec::new())?;
    vm.fw.objects.insert(list, HostObj::List { elems });
    Ok(list)
}
pub(crate) fn list_elems(vm: &Engine, list: Option<ObjRef>) -> Vec<Value> {
    let Some(list) = list else {
        return Vec::new();
    };
    match vm.fw.objects.get(&list) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => Vec::new(),
    }
}
fn float_array(vm: &mut Engine, values: &[f32]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[F",
        ElemKind::Float,
        values.iter().copied().map(Value::Float).collect(),
    )
}
fn long_array(vm: &mut Engine, values: &[i64]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[J",
        ElemKind::Long,
        values.iter().copied().map(Value::Long).collect(),
    )
}
#[allow(dead_code)]
fn int_array(vm: &mut Engine, values: &[i32]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[I",
        ElemKind::Int,
        values.iter().copied().map(Value::Int).collect(),
    )
}
#[allow(dead_code)]
fn byte_array(vm: &mut Engine, values: &[u8]) -> Result<Value, VmExit> {
    crate::arrays::new_array(
        vm,
        "[B",
        ElemKind::Int,
        values
            .iter()
            .map(|b| Value::Int(i32::from(*b as i8)))
            .collect(),
    )
}
fn array_values(vm: &Engine, array: Option<ObjRef>) -> Vec<Value> {
    let Some(array) = array else {
        return Vec::new();
    };
    vm.heap
        .as_array_elems(array)
        .map(|(elems, _)| elems.to_vec())
        .unwrap_or_default()
}
fn array_floats(vm: &Engine, array: Option<ObjRef>) -> Vec<f32> {
    array_values(vm, array)
        .into_iter()
        .map(|v| match v {
            Value::Float(f) => f,
            Value::Int(i) => i as f32,
            _ => 0.0,
        })
        .collect()
}
#[allow(dead_code)]
fn array_bytes(vm: &Engine, array: Option<ObjRef>) -> Vec<u8> {
    array_values(vm, array)
        .into_iter()
        .map(|v| match v {
            Value::Int(i) => i as u8,
            _ => 0,
        })
        .collect()
}
fn set_array_floats(vm: &mut Engine, array: ObjRef, values: &[f32]) -> Result<(), VmExit> {
    let elems = vm
        .heap
        .array_elems_mut(array)
        .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
    for (slot, value) in elems.iter_mut().zip(values.iter()) {
        *slot = Value::Float(*value);
    }
    Ok(())
}
fn aioobe(index: usize, len: usize) -> VmExit {
    throw(
        "Ljava/lang/ArrayIndexOutOfBoundsException;",
        format!("Index {index} out of bounds for length {len}"),
    )
}

// ---------------------------------------------------------------------------
// android.gesture
// ---------------------------------------------------------------------------

fn rectf_object(
    vm: &mut Engine,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Result<ObjRef, VmExit> {
    alloc_obj(
        vm,
        RECT_F,
        vec![
            ("left".to_string(), Value::Float(left)),
            ("top".to_string(), Value::Float(top)),
            ("right".to_string(), Value::Float(right)),
            ("bottom".to_string(), Value::Float(bottom)),
        ],
    )
}
fn rect_values(vm: &Engine, rect: ObjRef) -> [f32; 4] {
    [
        float_field(vm, rect, "left"),
        float_field(vm, rect, "top"),
        float_field(vm, rect, "right"),
        float_field(vm, rect, "bottom"),
    ]
}
fn put_rect_values(vm: &mut Engine, rect: ObjRef, values: [f32; 4]) -> Result<(), VmExit> {
    put(vm, rect, "left", Value::Float(values[0]))?;
    put(vm, rect, "top", Value::Float(values[1]))?;
    put(vm, rect, "right", Value::Float(values[2]))?;
    put(vm, rect, "bottom", Value::Float(values[3]))
}
fn rect_is_empty(values: [f32; 4]) -> bool {
    values[0] >= values[2] || values[1] >= values[3]
}
fn rect_union_point(values: &mut [f32; 4], x: f32, y: f32) {
    // AOSP RectF.union(float,float) deliberately has no isEmpty shortcut.
    if x < values[0] {
        values[0] = x;
    } else if x > values[2] {
        values[2] = x;
    }
    if y < values[1] {
        values[1] = y;
    } else if y > values[3] {
        values[3] = y;
    }
}
fn rect_union_rect(dst: &mut [f32; 4], src: [f32; 4]) {
    if rect_is_empty(src) {
        return;
    }
    if rect_is_empty(*dst) {
        *dst = src;
        return;
    }
    dst[0] = dst[0].min(src[0]);
    dst[1] = dst[1].min(src[1]);
    dst[2] = dst[2].max(src[2]);
    dst[3] = dst[3].max(src[3]);
}

fn gesture_point_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "(FFJ)V") => {
            put(vm, recv, "x", Value::Float(farg(args, 0)))?;
            put(vm, recv, "y", Value::Float(farg(args, 1)))?;
            put(vm, recv, "timestamp", Value::Long(larg(args, 2)))?;
            Ok(Some(Value::Null))
        }
        ("clone", "()Ljava/lang/Object;") => {
            let copy = alloc_obj(
                vm,
                GESTURE_POINT,
                vec![
                    ("x".to_string(), Value::Float(float_field(vm, recv, "x"))),
                    ("y".to_string(), Value::Float(float_field(vm, recv, "y"))),
                    (
                        "timestamp".to_string(),
                        Value::Long(long_field(vm, recv, "timestamp", 0)),
                    ),
                ],
            )?;
            Ok(Some(Value::Obj(copy)))
        }
        _ => Ok(None),
    }
}

fn gesture_centroid(points: &[f32]) -> Result<[f32; 2], VmExit> {
    let mut center_x = 0.0f32;
    let mut center_y = 0.0f32;
    let mut i = 0usize;
    while i < points.len() {
        center_x += points[i];
        i += 1;
        if i >= points.len() {
            return Err(aioobe(i, points.len()));
        }
        center_y += points[i];
        i += 1;
    }
    let count = points.len() as f32;
    Ok([2.0 * center_x / count, 2.0 * center_y / count])
}
fn gesture_covariance(points: &[f32]) -> [[f32; 2]; 2] {
    let mut c = [[0.0f32; 2]; 2];
    let mut i = 0usize;
    while i + 1 < points.len() {
        let x = points[i];
        let y = points[i + 1];
        c[0][0] += x * x;
        c[0][1] += x * y;
        c[1][0] = c[0][1];
        c[1][1] += y * y;
        i += 2;
    }
    let divisor = (points.len() / 2) as f32;
    for row in &mut c {
        for value in row {
            *value /= divisor;
        }
    }
    c
}
fn gesture_orientation(covariance: [[f32; 2]; 2]) -> [f32; 2] {
    let mut target = [0.0f32; 2];
    if covariance[0][1] == 0.0 || covariance[1][0] == 0.0 {
        target = [1.0, 0.0];
    }
    let a = -covariance[0][0] - covariance[1][1];
    let b = covariance[0][0] * covariance[1][1] - covariance[0][1] * covariance[1][0];
    let value = a / 2.0;
    let right_side = (f64::from(value).powi(2) - f64::from(b)).sqrt() as f32;
    let lambda1 = -value + right_side;
    let lambda2 = -value - right_side;
    if lambda1 == lambda2 {
        target = [0.0, 0.0];
    } else {
        let lambda = if lambda1 > lambda2 { lambda1 } else { lambda2 };
        target[0] = 1.0;
        // Intentionally divide by zero exactly as AOSP does when c01 == 0;
        // the earlier [1,0] assignment is overwritten in this branch.
        target[1] = (lambda - covariance[0][0]) / covariance[0][1];
    }
    target
}
fn gesture_obb_object(
    vm: &mut Engine,
    angle_degrees: f32,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
) -> Result<ObjRef, VmExit> {
    let ratio = width / height;
    let squareness = if ratio > 1.0 { 1.0 / ratio } else { ratio };
    alloc_obj(
        vm,
        ORIENTED_BOUNDING_BOX,
        vec![
            ("orientation".to_string(), Value::Float(angle_degrees)),
            ("centerX".to_string(), Value::Float(center_x)),
            ("centerY".to_string(), Value::Float(center_y)),
            ("width".to_string(), Value::Float(width)),
            ("height".to_string(), Value::Float(height)),
            ("squareness".to_string(), Value::Float(squareness)),
        ],
    )
}
fn compute_gesture_obb(vm: &mut Engine, original: &[f32]) -> Result<Value, VmExit> {
    let centroid = gesture_centroid(original)?;
    let mut points = original.to_vec();
    for pair in points.chunks_exact_mut(2) {
        pair[0] -= centroid[0];
        pair[1] -= centroid[1];
    }
    let target = gesture_orientation(gesture_covariance(&points));
    let angle = if target[0] == 0.0 && target[1] == 0.0 {
        (-core::f64::consts::PI / 2.0) as f32
    } else {
        let angle = f64::from(target[1]).atan2(f64::from(target[0])) as f32;
        let cos = f64::from(-angle).cos() as f32;
        let sin = f64::from(-angle).sin() as f32;
        for pair in points.chunks_exact_mut(2) {
            let (x, y) = (pair[0], pair[1]);
            pair[0] = x * cos - y * sin;
            pair[1] = x * sin + y * cos;
        }
        angle
    };
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    // Java Float.MIN_VALUE is the smallest positive subnormal, not -MAX.
    let mut max_x = f32::from_bits(1);
    let mut max_y = f32::from_bits(1);
    for pair in points.chunks_exact(2) {
        if pair[0] < min_x {
            min_x = pair[0];
        }
        if pair[0] > max_x {
            max_x = pair[0];
        }
        if pair[1] < min_y {
            min_y = pair[1];
        }
        if pair[1] > max_y {
            max_y = pair[1];
        }
    }
    Ok(Value::Obj(gesture_obb_object(
        vm,
        (f64::from(angle) * 180.0 / core::f64::consts::PI) as f32,
        centroid[0],
        centroid[1],
        max_x - min_x,
        max_y - min_y,
    )?))
}

fn gesture_utils_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("computeOrientedBoundingBox", "([F)Landroid/gesture/OrientedBoundingBox;") => {
            let array = oarg(args, 0).ok_or_else(|| npe("originalPoints == null"))?;
            let points = array_floats(vm, Some(array));
            compute_gesture_obb(vm, &points)?
        }
        (
            "computeOrientedBoundingBox",
            "(Ljava/util/ArrayList;)Landroid/gesture/OrientedBoundingBox;",
        ) => {
            let list = oarg(args, 0).ok_or_else(|| npe("originalPoints == null"))?;
            let mut points = Vec::new();
            for value in list_elems(vm, Some(list)) {
                let Value::Obj(point) = value else {
                    return Err(npe("GesturePoint == null"));
                };
                points.push(float_field(vm, point, "x"));
                points.push(float_field(vm, point, "y"));
            }
            compute_gesture_obb(vm, &points)?
        }
        ("computeCentroid", "([F)[F") => {
            let array = oarg(args, 0).ok_or_else(|| npe("points == null"))?;
            let points = array_floats(vm, Some(array));
            float_array(vm, &gesture_centroid(&points)?)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn clone_gesture_stroke(vm: &mut Engine, recv: ObjRef) -> Result<ObjRef, VmExit> {
    let bbox = obj_field(vm, recv, "boundingBox")
        .ok_or_else(|| npe("GestureStroke.boundingBox == null"))?;
    let [left, top, right, bottom] = rect_values(vm, bbox);
    let points = array_floats(vm, obj_field(vm, recv, "points"));
    let times: Vec<i64> = array_values(vm, obj_field(vm, recv, "timestamps"))
        .into_iter()
        .map(|v| match v {
            Value::Long(v) => v,
            Value::Int(v) => i64::from(v),
            _ => 0,
        })
        .collect();
    let bbox = rectf_object(vm, left, top, right, bottom)?;
    let points = float_array(vm, &points)?;
    let timestamps = long_array(vm, &times)?;
    alloc_obj(
        vm,
        GESTURE_STROKE,
        vec![
            ("boundingBox".to_string(), Value::Obj(bbox)),
            (
                "length".to_string(),
                Value::Float(float_field(vm, recv, "length")),
            ),
            ("points".to_string(), points),
            ("timestamps".to_string(), timestamps),
            ("mCachedPath".to_string(), Value::Null),
        ],
    )
}

fn gesture_stroke_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Ljava/util/ArrayList;)V") => {
            let list = oarg(args, 0).ok_or_else(|| npe("points == null"))?;
            let mut points = Vec::new();
            let mut timestamps = Vec::new();
            let mut bbox: Option<[f32; 4]> = None;
            let mut length = 0.0f32;
            let mut previous: Option<(f32, f32)> = None;
            for value in list_elems(vm, Some(list)) {
                let Value::Obj(point) = value else {
                    return Err(npe("GesturePoint == null"));
                };
                let x = float_field(vm, point, "x");
                let y = float_field(vm, point, "y");
                points.push(x);
                points.push(y);
                timestamps.push(long_field(vm, point, "timestamp", 0));
                if let Some(values) = &mut bbox {
                    let (px, py) = previous.unwrap();
                    // Java widens Math.hypot(float,float) to double, then
                    // narrows on compound assignment to the float length.
                    length =
                        (f64::from(length) + f64::from(x - px).hypot(f64::from(y - py))) as f32;
                    rect_union_point(values, x, y);
                } else {
                    bbox = Some([x, y, x, y]);
                    length = 0.0;
                }
                previous = Some((x, y));
            }
            let bbox_value = match bbox {
                Some([l, t, r, b]) => Value::Obj(rectf_object(vm, l, t, r, b)?),
                None => Value::Null,
            };
            let points_value = float_array(vm, &points)?;
            let timestamps_value = long_array(vm, &timestamps)?;
            put(vm, recv, "boundingBox", bbox_value)?;
            put(vm, recv, "length", Value::Float(length))?;
            put(vm, recv, "points", points_value)?;
            put(vm, recv, "timestamps", timestamps_value)?;
            put(vm, recv, "mCachedPath", Value::Null)?;
            Value::Null
        }
        ("clone", "()Ljava/lang/Object;") => Value::Obj(clone_gesture_stroke(vm, recv)?),
        ("computeOrientedBoundingBox", "()Landroid/gesture/OrientedBoundingBox;") => {
            let points_ref =
                obj_field(vm, recv, "points").ok_or_else(|| npe("GestureStroke.points == null"))?;
            let points = array_floats(vm, Some(points_ref));
            compute_gesture_obb(vm, &points)?
        }
        ("clearPath", "()V") => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn next_gesture_id(vm: &mut Engine) -> i64 {
    let key = (GESTURE.to_string(), "rd$nextId".to_string());
    let next = match vm.statics.get(&key) {
        Some(Value::Long(v)) => *v,
        _ => crate::jlsystem::epoch_ms().wrapping_add(1),
    };
    vm.statics.insert(key, Value::Long(next.wrapping_add(1)));
    next
}
fn init_gesture(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    let bbox = rectf_object(vm, 0.0, 0.0, 0.0, 0.0)?;
    let strokes = new_list(vm, Vec::new())?;
    let id = next_gesture_id(vm);
    put(vm, recv, "mBoundingBox", Value::Obj(bbox))?;
    put(vm, recv, "mStrokes", Value::Obj(strokes))?;
    put(vm, recv, "mGestureID", Value::Long(id))
}
fn gesture_strokes_list(vm: &mut Engine, recv: ObjRef) -> Result<ObjRef, VmExit> {
    if let Some(list) = obj_field(vm, recv, "mStrokes") {
        vm.fw
            .objects
            .entry(list)
            .or_insert_with(|| HostObj::List { elems: Vec::new() });
        return Ok(list);
    }
    let list = new_list(vm, Vec::new())?;
    put(vm, recv, "mStrokes", Value::Obj(list))?;
    Ok(list)
}
fn gesture_bbox(vm: &mut Engine, recv: ObjRef) -> Result<ObjRef, VmExit> {
    if let Some(rect) = obj_field(vm, recv, "mBoundingBox") {
        return Ok(rect);
    }
    let rect = rectf_object(vm, 0.0, 0.0, 0.0, 0.0)?;
    put(vm, recv, "mBoundingBox", Value::Obj(rect))?;
    Ok(rect)
}

fn gesture_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "()V") => {
            init_gesture(vm, recv)?;
            Value::Null
        }
        ("clone", "()Ljava/lang/Object;") => {
            let strokes_list = gesture_strokes_list(vm, recv)?;
            let stroke_values = list_elems(vm, Some(strokes_list));
            let mut copied_strokes = Vec::new();
            for value in stroke_values {
                let Value::Obj(stroke) = value else {
                    return Err(npe("GestureStroke == null"));
                };
                copied_strokes.push(Value::Obj(clone_gesture_stroke(vm, stroke)?));
            }
            let src_bbox = gesture_bbox(vm, recv)?;
            let bbox = rect_values(vm, src_bbox);
            let copy = alloc_obj(vm, GESTURE, Vec::new())?;
            init_gesture(vm, copy)?;
            let copy_bbox = gesture_bbox(vm, copy)?;
            put_rect_values(vm, copy_bbox, bbox)?;
            let copy_strokes = gesture_strokes_list(vm, copy)?;
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&copy_strokes) {
                *elems = copied_strokes;
            }
            Value::Obj(copy)
        }
        ("getStrokes", "()Ljava/util/ArrayList;") => Value::Obj(gesture_strokes_list(vm, recv)?),
        ("getStrokesCount", "()I") => {
            let list = gesture_strokes_list(vm, recv)?;
            Value::Int(list_elems(vm, Some(list)).len() as i32)
        }
        ("addStroke", "(Landroid/gesture/GestureStroke;)V") => {
            let list = gesture_strokes_list(vm, recv)?;
            let stroke_value = args.first().cloned().unwrap_or(Value::Null);
            if let Some(HostObj::List { elems }) = vm.fw.objects.get_mut(&list) {
                elems.push(stroke_value.clone());
            }
            // AOSP appends first and only then dereferences stroke.boundingBox.
            let Value::Obj(stroke) = stroke_value else {
                return Err(npe("stroke == null"));
            };
            let src = obj_field(vm, stroke, "boundingBox")
                .ok_or_else(|| npe("stroke.boundingBox == null"))?;
            let src_values = rect_values(vm, src);
            let dst = gesture_bbox(vm, recv)?;
            let mut dst_values = rect_values(vm, dst);
            rect_union_rect(&mut dst_values, src_values);
            put_rect_values(vm, dst, dst_values)?;
            Value::Null
        }
        ("getLength", "()F") => {
            let list = gesture_strokes_list(vm, recv)?;
            let mut len: i32 = 0;
            for value in list_elems(vm, Some(list)) {
                let Value::Obj(stroke) = value else {
                    return Err(npe("GestureStroke == null"));
                };
                // Java `int += float` truncates after every stroke.
                len = (len as f32 + float_field(vm, stroke, "length")) as i32;
            }
            Value::Float(len as f32)
        }
        ("getBoundingBox", "()Landroid/graphics/RectF;") => Value::Obj(gesture_bbox(vm, recv)?),
        ("getID", "()J") => Value::Long(long_field(vm, recv, "mGestureID", 0)),
        ("setID", "(J)V") => {
            put(vm, recv, "mGestureID", Value::Long(larg(args, 0)))?;
            Value::Null
        }
        ("describeContents", "()I") => Value::Int(0),
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn oriented_bounding_box_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if (name, sig) != ("<init>", "(FFFFF)V") {
        return Ok(None);
    }
    let width = farg(args, 3);
    let height = farg(args, 4);
    let ratio = width / height;
    let squareness = if ratio > 1.0 { 1.0 / ratio } else { ratio };
    put(vm, recv, "orientation", Value::Float(farg(args, 0)))?;
    put(vm, recv, "centerX", Value::Float(farg(args, 1)))?;
    put(vm, recv, "centerY", Value::Float(farg(args, 2)))?;
    put(vm, recv, "width", Value::Float(width))?;
    put(vm, recv, "height", Value::Float(height))?;
    put(vm, recv, "squareness", Value::Float(squareness))?;
    Ok(Some(Value::Null))
}

// ---------------------------------------------------------------------------
// android.hardware: sensorless SensorManager, Sensor values, Consumer IR
// ---------------------------------------------------------------------------

fn empty_list(vm: &mut Engine) -> Result<Value, VmExit> {
    // The existing Collections intrinsic returns the immutable host list,
    // so mutation attempts follow the normal UOE path.
    vm.invoke_static(
        "Ljava/util/Collections;",
        "emptyList",
        "()Ljava/util/List;",
        &[],
    )
}
fn required_floats(vm: &Engine, args: &[Value], i: usize, what: &str) -> Result<Vec<f32>, VmExit> {
    let array = oarg(args, i).ok_or_else(|| npe(what))?;
    Ok(array_floats(vm, Some(array)))
}
fn float_at(values: &[f32], index: usize) -> Result<f32, VmExit> {
    values
        .get(index)
        .copied()
        .ok_or_else(|| aioobe(index, values.len()))
}
fn sensor_manager_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("getRotationMatrix", "([F[F[F[F)Z") => {
            let gravity = required_floats(vm, args, 2, "gravity == null")?;
            let (mut ax, mut ay, mut az) = (
                float_at(&gravity, 0)?,
                float_at(&gravity, 1)?,
                float_at(&gravity, 2)?,
            );
            let norm_sq_a = ax * ax + ay * ay + az * az;
            let free_fall_squared = 0.01f32 * 9.81f32 * 9.81f32;
            if norm_sq_a < free_fall_squared {
                bool_value(false)
            } else {
                let geomagnetic = required_floats(vm, args, 3, "geomagnetic == null")?;
                let (ex, ey, ez) = (
                    float_at(&geomagnetic, 0)?,
                    float_at(&geomagnetic, 1)?,
                    float_at(&geomagnetic, 2)?,
                );
                let mut hx = ey * az - ez * ay;
                let mut hy = ez * ax - ex * az;
                let mut hz = ex * ay - ey * ax;
                let norm_h = (f64::from(hx * hx + hy * hy + hz * hz)).sqrt() as f32;
                if norm_h < 0.1 {
                    bool_value(false)
                } else {
                    let inv_h = 1.0f32 / norm_h;
                    hx *= inv_h;
                    hy *= inv_h;
                    hz *= inv_h;
                    let inv_a = 1.0f32 / ((f64::from(ax * ax + ay * ay + az * az)).sqrt() as f32);
                    ax *= inv_a;
                    ay *= inv_a;
                    az *= inv_a;
                    let mx = ay * hz - az * hy;
                    let my = az * hx - ax * hz;
                    let mz = ax * hy - ay * hx;
                    if let Some(r) = oarg(args, 0) {
                        let mut out = array_floats(vm, Some(r));
                        if out.len() == 9 {
                            out.copy_from_slice(&[hx, hy, hz, mx, my, mz, ax, ay, az]);
                            set_array_floats(vm, r, &out)?;
                        } else if out.len() == 16 {
                            out.copy_from_slice(&[
                                hx, hy, hz, 0.0, mx, my, mz, 0.0, ax, ay, az, 0.0, 0.0, 0.0, 0.0,
                                1.0,
                            ]);
                            set_array_floats(vm, r, &out)?;
                        }
                    }
                    if let Some(i_ref) = oarg(args, 1) {
                        let inv_e =
                            1.0f32 / ((f64::from(ex * ex + ey * ey + ez * ez)).sqrt() as f32);
                        let c = (ex * mx + ey * my + ez * mz) * inv_e;
                        let s = (ex * ax + ey * ay + ez * az) * inv_e;
                        let mut out = array_floats(vm, Some(i_ref));
                        if out.len() == 9 {
                            out.copy_from_slice(&[1.0, 0.0, 0.0, 0.0, c, s, 0.0, -s, c]);
                            set_array_floats(vm, i_ref, &out)?;
                        } else if out.len() == 16 {
                            out.copy_from_slice(&[
                                1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0,
                                1.0,
                            ]);
                            set_array_floats(vm, i_ref, &out)?;
                        }
                    }
                    bool_value(true)
                }
            }
        }
        ("getInclination", "([F)F") => {
            let values = required_floats(vm, args, 0, "I == null")?;
            let result = if values.len() == 9 {
                f64::from(float_at(&values, 5)?).atan2(f64::from(float_at(&values, 4)?))
            } else {
                f64::from(float_at(&values, 6)?).atan2(f64::from(float_at(&values, 5)?))
            };
            Value::Float(result as f32)
        }
        ("remapCoordinateSystem", "([FII[F)Z") => {
            let in_ref = oarg(args, 0).ok_or_else(|| npe("inR == null"))?;
            let out_ref = oarg(args, 3).ok_or_else(|| npe("outR == null"))?;
            let input = array_floats(vm, Some(in_ref));
            let existing = array_floats(vm, Some(out_ref));
            // AOSP's same-array path uses a fixed float[16] temporary, so a
            // 9-element in-place remap fails the length check and returns false.
            if (in_ref == out_ref && input.len() != 16) || input.len() != existing.len() {
                bool_value(false)
            } else {
                let x_axis = iarg(args, 1);
                let y_axis = iarg(args, 2);
                if (x_axis & 0x7c) != 0
                    || (y_axis & 0x7c) != 0
                    || (x_axis & 0x3) == 0
                    || (y_axis & 0x3) == 0
                    || (x_axis & 0x3) == (y_axis & 0x3)
                {
                    bool_value(false)
                } else {
                    let mut z_axis = x_axis ^ y_axis;
                    let x = (x_axis & 0x3) - 1;
                    let y = (y_axis & 0x3) - 1;
                    let z = (z_axis & 0x3) - 1;
                    let axis_y = (z + 1) % 3;
                    let axis_z = (z + 2) % 3;
                    if ((x ^ axis_y) | (y ^ axis_z)) != 0 {
                        z_axis ^= 0x80;
                    }
                    let sx = x_axis >= 0x80;
                    let sy = y_axis >= 0x80;
                    let sz = z_axis >= 0x80;
                    let row_len = if input.len() == 16 { 4 } else { 3 };
                    let mut out = existing;
                    for j in 0..3usize {
                        let offset = j * row_len;
                        for i in 0..3i32 {
                            let index = offset + i as usize;
                            if index >= out.len() {
                                return Err(aioobe(index, out.len()));
                            }
                            if x == i {
                                out[index] = if sx { -input[offset] } else { input[offset] };
                            }
                            if y == i {
                                out[index] = if sy {
                                    -input[offset + 1]
                                } else {
                                    input[offset + 1]
                                };
                            }
                            if z == i {
                                out[index] = if sz {
                                    -input[offset + 2]
                                } else {
                                    input[offset + 2]
                                };
                            }
                        }
                    }
                    if out.len() == 16 {
                        for index in [3, 7, 11, 12, 13, 14] {
                            out[index] = 0.0;
                        }
                        out[15] = 1.0;
                    }
                    set_array_floats(vm, out_ref, &out)?;
                    bool_value(true)
                }
            }
        }
        ("getOrientation", "([F[F)[F") => {
            let r_ref = oarg(args, 0).ok_or_else(|| npe("R == null"))?;
            let values_ref = oarg(args, 1).ok_or_else(|| npe("values == null"))?;
            let r = array_floats(vm, Some(r_ref));
            let mut out = array_floats(vm, Some(values_ref));
            let computed = if r.len() == 9 {
                [
                    f64::from(float_at(&r, 1)?).atan2(f64::from(float_at(&r, 4)?)) as f32,
                    f64::from(-float_at(&r, 7)?).asin() as f32,
                    f64::from(-float_at(&r, 6)?).atan2(f64::from(float_at(&r, 8)?)) as f32,
                ]
            } else {
                [
                    f64::from(float_at(&r, 1)?).atan2(f64::from(float_at(&r, 5)?)) as f32,
                    f64::from(-float_at(&r, 9)?).asin() as f32,
                    f64::from(-float_at(&r, 8)?).atan2(f64::from(float_at(&r, 10)?)) as f32,
                ]
            };
            if out.len() < 3 {
                return Err(aioobe(out.len(), out.len()));
            }
            out[..3].copy_from_slice(&computed);
            set_array_floats(vm, values_ref, &out)?;
            Value::Obj(values_ref)
        }
        ("getAltitude", "(FF)F") => {
            let coef = 1.0f32 / 5.255f32;
            Value::Float(
                44330.0f32
                    * (1.0f32
                        - (f64::from(farg(args, 1) / farg(args, 0)).powf(f64::from(coef)) as f32)),
            )
        }
        ("getAngleChange", "([F[F[F)V") => {
            fn unpack(values: &[f32]) -> [f32; 9] {
                let mut out = [0.0f32; 9];
                if values.len() == 9 {
                    out.copy_from_slice(values);
                } else if values.len() == 16 {
                    out.copy_from_slice(&[
                        values[0], values[1], values[2], values[4], values[5], values[6],
                        values[8], values[9], values[10],
                    ]);
                }
                out
            }
            let out_ref = oarg(args, 0).ok_or_else(|| npe("angleChange == null"))?;
            let r = unpack(&required_floats(vm, args, 1, "R == null")?);
            let p = unpack(&required_floats(vm, args, 2, "prevR == null")?);
            let rd1 = p[0] * r[1] + p[3] * r[4] + p[6] * r[7];
            let rd4 = p[1] * r[1] + p[4] * r[4] + p[7] * r[7];
            let rd6 = p[2] * r[0] + p[5] * r[3] + p[8] * r[6];
            let rd7 = p[2] * r[1] + p[5] * r[4] + p[8] * r[7];
            let rd8 = p[2] * r[2] + p[5] * r[5] + p[8] * r[8];
            let mut out = array_floats(vm, Some(out_ref));
            if out.len() < 3 {
                return Err(aioobe(out.len(), out.len()));
            }
            out[0] = f64::from(rd1).atan2(f64::from(rd4)) as f32;
            out[1] = f64::from(-rd7).asin() as f32;
            out[2] = f64::from(-rd6).atan2(f64::from(rd8)) as f32;
            set_array_floats(vm, out_ref, &out)?;
            Value::Null
        }
        ("getRotationMatrixFromVector", "([F[F)V") => {
            let r_ref = oarg(args, 0).ok_or_else(|| npe("R == null"))?;
            let rv = required_floats(vm, args, 1, "rotationVector == null")?;
            let q1 = float_at(&rv, 0)?;
            let q2 = float_at(&rv, 1)?;
            let q3 = float_at(&rv, 2)?;
            let q0 = if rv.len() >= 4 {
                rv[3]
            } else {
                let value = 1.0f32 - q1 * q1 - q2 * q2 - q3 * q3;
                if value > 0.0 {
                    f64::from(value).sqrt() as f32
                } else {
                    0.0
                }
            };
            let sq_q1 = 2.0f32 * q1 * q1;
            let sq_q2 = 2.0f32 * q2 * q2;
            let sq_q3 = 2.0f32 * q3 * q3;
            let q1_q2 = 2.0f32 * q1 * q2;
            let q3_q0 = 2.0f32 * q3 * q0;
            let q1_q3 = 2.0f32 * q1 * q3;
            let q2_q0 = 2.0f32 * q2 * q0;
            let q2_q3 = 2.0f32 * q2 * q3;
            let q1_q0 = 2.0f32 * q1 * q0;
            let mut out = array_floats(vm, Some(r_ref));
            if out.len() == 9 {
                out.copy_from_slice(&[
                    1.0 - sq_q2 - sq_q3,
                    q1_q2 - q3_q0,
                    q1_q3 + q2_q0,
                    q1_q2 + q3_q0,
                    1.0 - sq_q1 - sq_q3,
                    q2_q3 - q1_q0,
                    q1_q3 - q2_q0,
                    q2_q3 + q1_q0,
                    1.0 - sq_q1 - sq_q2,
                ]);
                set_array_floats(vm, r_ref, &out)?;
            } else if out.len() == 16 {
                out.copy_from_slice(&[
                    1.0 - sq_q2 - sq_q3,
                    q1_q2 - q3_q0,
                    q1_q3 + q2_q0,
                    0.0,
                    q1_q2 + q3_q0,
                    1.0 - sq_q1 - sq_q3,
                    q2_q3 - q1_q0,
                    0.0,
                    q1_q3 - q2_q0,
                    q2_q3 + q1_q0,
                    1.0 - sq_q1 - sq_q2,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                ]);
                set_array_floats(vm, r_ref, &out)?;
            }
            Value::Null
        }
        ("getQuaternionFromVector", "([F[F)V") => {
            let q_ref = oarg(args, 0).ok_or_else(|| npe("Q == null"))?;
            let rv = required_floats(vm, args, 1, "rv == null")?;
            let mut out = array_floats(vm, Some(q_ref));
            if out.is_empty() {
                return Err(aioobe(0, 0));
            }
            out[0] = if rv.len() >= 4 {
                rv[3]
            } else {
                let value = 1.0f32
                    - float_at(&rv, 0)? * float_at(&rv, 0)?
                    - float_at(&rv, 1)? * float_at(&rv, 1)?
                    - float_at(&rv, 2)? * float_at(&rv, 2)?;
                if value > 0.0 {
                    f64::from(value).sqrt() as f32
                } else {
                    0.0
                }
            };
            for i in 0..3 {
                if i + 1 >= out.len() {
                    // AOSP writes Q[0] before failing on a short Q.
                    set_array_floats(vm, q_ref, &out)?;
                    return Err(aioobe(i + 1, out.len()));
                }
                out[i + 1] = float_at(&rv, i)?;
            }
            set_array_floats(vm, q_ref, &out)?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn sensor_reporting_mode(vm: &Engine, sensor: ObjRef) -> i32 {
    (int_field(vm, sensor, "mFlags", 0) & 0xe) >> 1
}
fn sensor_manager_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "()V") | ("<init>", "(Landroid/content/Context;Landroid/os/Looper;)V") => {
            Value::Null
        }
        ("getSensorList", "(I)Ljava/util/List;")
        | ("getDynamicSensorList", "(I)Ljava/util/List;")
        | ("getFullSensorList", "()Ljava/util/List;")
        | ("getFullDynamicSensorList", "()Ljava/util/List;") => empty_list(vm)?,
        ("getDefaultSensor", "(I)Landroid/hardware/Sensor;")
        | ("getDefaultSensor", "(IZ)Landroid/hardware/Sensor;")
        | ("getSensorByHandle", "(I)Landroid/hardware/Sensor;") => Value::Null,
        ("isDynamicSensorDiscoverySupported", "()Z") => bool_value(false),
        (
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;I)Z",
        )
        | (
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;II)Z",
        )
        | (
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;ILandroid/os/Handler;)Z",
        )
        | (
            "registerListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;IILandroid/os/Handler;)Z",
        ) => bool_value(false),
        ("unregisterListener", "(Landroid/hardware/SensorEventListener;)V")
        | (
            "unregisterListener",
            "(Landroid/hardware/SensorEventListener;Landroid/hardware/Sensor;)V",
        ) => Value::Null,
        ("flush", "(Landroid/hardware/SensorEventListener;)Z") => {
            if oarg(args, 0).is_none() {
                return Err(iae("listener cannot be null"));
            }
            bool_value(false)
        }
        (
            "requestTriggerSensor",
            "(Landroid/hardware/TriggerEventListener;Landroid/hardware/Sensor;)Z",
        ) => {
            let sensor = oarg(args, 1).ok_or_else(|| iae("sensor cannot be null"))?;
            if oarg(args, 0).is_none() {
                return Err(iae("listener cannot be null"));
            }
            let _ = sensor_reporting_mode(vm, sensor);
            bool_value(false)
        }
        (
            "cancelTriggerSensor",
            "(Landroid/hardware/TriggerEventListener;Landroid/hardware/Sensor;)Z",
        ) => bool_value(false),
        (
            "registerDynamicSensorCallback",
            "(Landroid/hardware/SensorManager$DynamicSensorCallback;)V",
        )
        | (
            "registerDynamicSensorCallback",
            "(Landroid/hardware/SensorManager$DynamicSensorCallback;Landroid/os/Handler;)V",
        ) => {
            if oarg(args, 0).is_none() {
                return Err(iae("callback cannot be null"));
            }
            Value::Null
        }
        (
            "unregisterDynamicSensorCallback",
            "(Landroid/hardware/SensorManager$DynamicSensorCallback;)V",
        ) => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(v))
}

const SENSOR_VALUE_LENGTHS: &[i32] = &[
    0, 3, 3, 3, 3, 1, 1, 1, 1, 3, 3, 5, 1, 1, 6, 4, 6, 1, 1, 1, 5, 1, 1, 1, 1, 1, 1, 1, 16, 1, 1,
    1, 2, 16, 1, 6, 1, 6, 6, 6, 9, 9, 2,
];
fn sensor_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if (name, sig) != ("getMaxLengthValuesArray", "(Landroid/hardware/Sensor;I)I") {
        return Ok(None);
    }
    let sensor = oarg(args, 0).ok_or_else(|| npe("sensor == null"))?;
    let sensor_type = int_field(vm, sensor, "mType", 0);
    if sensor_type == 11 && iarg(args, 1) <= 17 {
        return Ok(Some(Value::Int(3)));
    }
    if sensor_type < 0 {
        return Err(aioobe(sensor_type as usize, SENSOR_VALUE_LENGTHS.len()));
    }
    if sensor_type as usize >= SENSOR_VALUE_LENGTHS.len() {
        return Ok(Some(Value::Int(16)));
    }
    Ok(Some(Value::Int(SENSOR_VALUE_LENGTHS[sensor_type as usize])))
}

fn sensor_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let flags = int_field(vm, recv, "mFlags", 0);
    let v = match (name, sig) {
        ("<init>", "()V") => Value::Null,
        ("getName", "()Ljava/lang/String;") => get(vm, recv, "mName"),
        ("getVendor", "()Ljava/lang/String;") => get(vm, recv, "mVendor"),
        ("getType", "()I") => Value::Int(int_field(vm, recv, "mType", 0)),
        ("getVersion", "()I") => Value::Int(int_field(vm, recv, "mVersion", 0)),
        ("getMaximumRange", "()F") => Value::Float(float_field(vm, recv, "mMaxRange")),
        ("getResolution", "()F") => Value::Float(float_field(vm, recv, "mResolution")),
        ("getPower", "()F") => Value::Float(float_field(vm, recv, "mPower")),
        ("getMinDelay", "()I") => Value::Int(int_field(vm, recv, "mMinDelay", 0)),
        ("getFifoReservedEventCount", "()I") => {
            Value::Int(int_field(vm, recv, "mFifoReservedEventCount", 0))
        }
        ("getFifoMaxEventCount", "()I") => Value::Int(int_field(vm, recv, "mFifoMaxEventCount", 0)),
        ("getStringType", "()Ljava/lang/String;") => get(vm, recv, "mStringType"),
        ("getUuid", "()Ljava/util/UUID;") => get(vm, recv, "mUuid"),
        ("getId", "()I") => Value::Int(int_field(vm, recv, "mId", 0)),
        ("getRequiredPermission", "()Ljava/lang/String;") => get(vm, recv, "mRequiredPermission"),
        ("getHandle", "()I") => Value::Int(int_field(vm, recv, "mHandle", 0)),
        ("getMaxDelay", "()I") => Value::Int(int_field(vm, recv, "mMaxDelay", 0)),
        ("getReportingMode", "()I") => Value::Int((flags & 0xe) >> 1),
        ("getHighestDirectReportRateLevel", "()I") => Value::Int(((flags & 0x380) >> 7).min(3)),
        ("isDirectChannelTypeSupported", "(I)Z") => bool_value(match iarg(args, 0) {
            1 => flags & (1 << 10) > 0,
            2 => flags & (1 << 11) > 0,
            _ => false,
        }),
        ("isWakeUpSensor", "()Z") => bool_value(flags & 1 != 0),
        ("isDynamicSensor", "()Z") => bool_value(flags & 0x20 != 0),
        ("isAdditionalInfoSupported", "()Z") => bool_value(flags & 0x40 != 0),
        ("isDataInjectionSupported", "()Z") => bool_value(((flags & 0x10) >> 4) != 0),
        ("setRange", "(FF)V") => {
            put(vm, recv, "mMaxRange", Value::Float(farg(args, 0)))?;
            put(vm, recv, "mResolution", Value::Float(farg(args, 1)))?;
            Value::Null
        }
        ("toString", "()Ljava/lang/String;") => {
            let name_value = get(vm, recv, "mName");
            let vendor_value = get(vm, recv, "mVendor");
            let rendered = format!(
                "{{Sensor name=\"{}\", vendor=\"{}\", version={}, type={}, maxRange={}, resolution={}, power={}, minDelay={}}}",
                opt_str(vm, Some(&name_value)).as_deref().unwrap_or("null"),
                opt_str(vm, Some(&vendor_value)).as_deref().unwrap_or("null"),
                int_field(vm, recv, "mVersion", 0),
                int_field(vm, recv, "mType", 0),
                crate::repr::java_float(float_field(vm, recv, "mMaxRange")),
                crate::repr::java_float(float_field(vm, recv, "mResolution")),
                crate::repr::java_float(float_field(vm, recv, "mPower")),
                int_field(vm, recv, "mMinDelay", 0),
            );
            jstr(vm, rendered)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn consumer_ir_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/content/Context;)V") => {
            let context = oarg(args, 0).ok_or_else(|| npe("context == null"))?;
            let package = vm
                .invoke_instance(context, "getPackageName", "()Ljava/lang/String;", &[])
                .unwrap_or(Value::Null);
            put(vm, recv, "mPackageName", package)?;
            put(vm, recv, "mService", Value::Null)?;
            Value::Null
        }
        ("hasIrEmitter", "()Z") => bool_value(false),
        ("transmit", "(I[I)V") => Value::Null,
        (
            "getCarrierFrequencies",
            "()[Landroid/hardware/ConsumerIrManager$CarrierFrequencyRange;",
        ) => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn carrier_frequency_range_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let v = match (name, sig) {
        ("<init>", "(Landroid/hardware/ConsumerIrManager;II)V") => {
            put(
                vm,
                recv,
                "this$0",
                args.first().cloned().unwrap_or(Value::Null),
            )?;
            put(vm, recv, "mMinFrequency", Value::Int(iarg(args, 1)))?;
            put(vm, recv, "mMaxFrequency", Value::Int(iarg(args, 2)))?;
            Value::Null
        }
        ("<init>", "(II)V") => {
            put(vm, recv, "mMinFrequency", Value::Int(iarg(args, 0)))?;
            put(vm, recv, "mMaxFrequency", Value::Int(iarg(args, 1)))?;
            Value::Null
        }
        ("getMinFrequency", "()I") => Value::Int(int_field(vm, recv, "mMinFrequency", 0)),
        ("getMaxFrequency", "()I") => Value::Int(int_field(vm, recv, "mMaxFrequency", 0)),
        _ => return Ok(None),
    };
    Ok(Some(v))
}
