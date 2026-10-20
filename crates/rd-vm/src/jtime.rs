//! `java.time` core: `LocalDate`, `LocalDateTime`, `Instant`, `ZoneId`,
//! `ZonedDateTime`, `YearMonth`, `Duration`, `ChronoUnit` and a
//! `DateTimeFormatter` pattern engine.
//!
//! Each value is an immutable host object of its exact JDK class with one
//! scalar field:
//! - `LocalDate`: `rd_day`, the epoch day;
//! - `LocalDateTime`, `Instant`, `ZonedDateTime`, `Duration`: `rd_ms`, in
//!   milliseconds;
//! - `YearMonth`: `rd_ym`, months since year 0;
//! - `DateTimeFormatter`: `rd_pat` and `rd_lang`.
//!
//! "Now" comes from the deterministic virtual wall clock
//! (`jlsystem::epoch_ms` + `HostState.clock_ms`), so runs are reproducible.
//!
//! **Deviation:** the device zone is UTC. `ZoneId.systemDefault()` and
//! `ZoneId.of(..)` both return UTC, so zoned and local values share epoch
//! milliseconds.
//!
//! Supported pattern letters: `y M d H h m s S a E` and `'quoted'` text.
//! Month and day names exist for `pt` (CLDR abbreviations such as "jan.")
//! and fall back to English for other locales.
//!
//! Calendar arithmetic is proleptic Gregorian (Hinnant's civil algorithms).

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

const LD: &str = "Ljava/time/LocalDate;";
const LDT: &str = "Ljava/time/LocalDateTime;";
const INST: &str = "Ljava/time/Instant;";
const ZONE: &str = "Ljava/time/ZoneId;";
const ZDT: &str = "Ljava/time/ZonedDateTime;";
const YM: &str = "Ljava/time/YearMonth;";
const DUR: &str = "Ljava/time/Duration;";
const FMT: &str = "Ljava/time/format/DateTimeFormatter;";
const UNIT: &str = "Ljava/time/temporal/ChronoUnit;";
const DAY_MS: i64 = 86_400_000;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn month_len(y: i64, m: i64) -> i64 {
    days_from_civil(
        if m == 12 { y + 1 } else { y },
        if m == 12 { 1 } else { m + 1 },
        1,
    ) - days_from_civil(y, m, 1)
}

fn now_ms(vm: &Engine) -> i64 {
    crate::jlsystem::epoch_ms().saturating_add(vm.fw.clock() as i64)
}

fn alloc(vm: &mut Engine, class: &str, fields: Vec<(&str, Value)>) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(
            class.to_string(),
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
        .map_err(crate::err::oom_throwable)?;
    Ok(Value::Obj(r))
}

fn long_field(vm: &Engine, r: ObjRef, f: &str) -> i64 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Long(v)) => v,
        Ok(Value::Int(v)) => v as i64,
        _ => 0,
    }
}

fn class_of(vm: &Engine, v: &Value) -> Option<(ObjRef, String)> {
    match v {
        Value::Obj(r) => vm.heap.class_of(*r).ok().map(|c| (*r, c.to_string())),
        _ => None,
    }
}

/// Epoch milliseconds of any supported temporal (dates at start of day).
fn temporal_ms(vm: &Engine, v: &Value) -> Option<i64> {
    let (r, c) = class_of(vm, v)?;
    match c.as_str() {
        LD => Some(long_field(vm, r, "rd_day") * DAY_MS),
        LDT | INST | ZDT | DUR => Some(long_field(vm, r, "rd_ms")),
        YM => {
            let ym = long_field(vm, r, "rd_ym");
            Some(days_from_civil(ym.div_euclid(12), ym.rem_euclid(12) + 1, 1) * DAY_MS)
        }
        _ => None,
    }
}

fn ld(vm: &mut Engine, day: i64) -> Result<Value, VmExit> {
    alloc(vm, LD, vec![("rd_day", Value::Long(day))])
}
fn ms_obj(vm: &mut Engine, class: &str, ms: i64) -> Result<Value, VmExit> {
    alloc(vm, class, vec![("rd_ms", Value::Long(ms))])
}
fn ym(vm: &mut Engine, months: i64) -> Result<Value, VmExit> {
    alloc(vm, YM, vec![("rd_ym", Value::Long(months))])
}
fn zone(vm: &mut Engine) -> Result<Value, VmExit> {
    let k = (ZONE.to_string(), "rd_utc".to_string());
    if let Some(v) = vm.statics.get(&k) {
        return Ok(v.clone());
    }
    let id = crate::intrinsics::alloc_string(vm, "UTC".into())?;
    let v = alloc(vm, ZONE, vec![("rd_id", Value::Obj(id))])?;
    vm.statics.insert(k, v.clone());
    Ok(v)
}

fn str_arg(vm: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => vm.heap.as_str(*r).map(str::to_string).unwrap_or_default(),
        _ => String::new(),
    }
}
fn arg_l(args: &[Value], i: usize) -> i64 {
    match args.get(i) {
        Some(Value::Long(v)) => *v,
        Some(Value::Int(v)) => *v as i64,
        _ => 0,
    }
}

/// Broken-down fields: (year, month, day, hour, minute, second, milli, dow 1=Mon).
fn fields(ms: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let day = ms.div_euclid(DAY_MS);
    let rem = ms.rem_euclid(DAY_MS);
    let (y, m, d) = civil_from_days(day);
    let dow = (day + 3).rem_euclid(7) + 1;
    (
        y,
        m,
        d,
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000,
        dow,
    )
}

const PT_MON: [&str; 12] = [
    "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez.",
];
const PT_MON_FULL: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];
const EN_MON: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const EN_MON_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const PT_DOW: [&str; 7] = ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."];
const EN_DOW: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

pub(crate) fn format_pattern(pat: &str, lang: &str, ms: i64) -> String {
    let (y, mo, d, h, mi, s, milli, dow) = fields(ms);
    let pt = lang == "pt";
    let chars: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if chars.get(i + 1) == Some(&'\'') {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let num = |v: i64, w: usize| format!("{:0w$}", v, w = w);
        let mi0 = (mo - 1) as usize;
        match c {
            'y' | 'u' => out.push_str(&if n == 2 {
                num(y.rem_euclid(100), 2)
            } else {
                num(y, n)
            }),
            'M' | 'L' => out.push_str(&match n {
                1 | 2 => num(mo, n),
                3 => (if pt { PT_MON[mi0] } else { EN_MON[mi0] }).to_string(),
                _ => (if pt {
                    PT_MON_FULL[mi0]
                } else {
                    EN_MON_FULL[mi0]
                })
                .to_string(),
            }),
            'd' => out.push_str(&num(d, n)),
            'H' => out.push_str(&num(h, n)),
            'h' => out.push_str(&num(if h % 12 == 0 { 12 } else { h % 12 }, n)),
            'm' => out.push_str(&num(mi, n)),
            's' => out.push_str(&num(s, n)),
            'S' => out.push_str(&format!("{:03}", milli)[..n.min(3)]),
            'a' => out.push_str(if h < 12 { "AM" } else { "PM" }),
            'E' => out.push_str(if pt {
                PT_DOW[(dow - 1) as usize]
            } else {
                EN_DOW[(dow - 1) as usize]
            }),
            _ => {
                for _ in 0..n {
                    out.push(c)
                }
            }
        }
        i += n;
    }
    out
}

fn ld_hash(day: i64) -> i32 {
    let (y, m, d) = civil_from_days(day);
    let (y, m, d) = (y as i32, m as i32, d as i32);
    (y & (0xFFFF_F800u32 as i32)) ^ ((y << 11) + (m << 6) + d)
}

fn iso_ldt(ms: i64) -> String {
    let (y, m, d, h, mi, s, milli, _) = fields(ms);
    let mut out = format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}");
    if s != 0 || milli != 0 {
        out.push_str(&format!(":{s:02}"));
        if milli != 0 {
            out.push_str(&format!(".{milli:03}"));
        }
    }
    out
}

/// Materializes `ChronoUnit` enum constants on first `sget`.
pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    if owner != UNIT || unit_ms(name).is_none() {
        return Ok(None);
    }
    let n = crate::intrinsics::alloc_string(vm, name.to_string())?;
    Ok(Some(alloc(vm, UNIT, vec![("name", Value::Obj(n))])?))
}

fn unit_ms(name: &str) -> Option<i64> {
    Some(match name {
        "MILLIS" => 1,
        "SECONDS" => 1000,
        "MINUTES" => 60_000,
        "HOURS" => 3_600_000,
        "HALF_DAYS" => 43_200_000,
        "DAYS" => DAY_MS,
        "WEEKS" => 7 * DAY_MS,
        "MONTHS" | "YEARS" => -1,
        _ => return None,
    })
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !class.starts_with("Ljava/time/") {
        return Ok(None);
    }
    let v = match (class, name) {
        (LDT, "now") => ms_obj(vm, LDT, now_ms(vm))?,
        (LDT, "ofInstant") => ms_obj(vm, LDT, temporal_ms(vm, &args[0]).unwrap_or(0))?,
        (LDT, "of") if args.len() >= 5 => {
            let day = days_from_civil(arg_l(args, 0), arg_l(args, 1), arg_l(args, 2));
            let ms = day * DAY_MS
                + arg_l(args, 3) * 3_600_000
                + arg_l(args, 4) * 60_000
                + arg_l(args, 5) * 1000;
            ms_obj(vm, LDT, ms)?
        }
        (LD, "now") => ld(vm, now_ms(vm).div_euclid(DAY_MS))?,
        (LD, "ofEpochDay") => ld(vm, arg_l(args, 0))?,
        (LD, "of") if sig == "(III)Ljava/time/LocalDate;" => ld(
            vm,
            days_from_civil(arg_l(args, 0), arg_l(args, 1), arg_l(args, 2)),
        )?,
        (ZONE, "systemDefault" | "of") => zone(vm)?,
        (INST, "ofEpochMilli") => ms_obj(vm, INST, arg_l(args, 0))?,
        (INST, "ofEpochSecond") => ms_obj(vm, INST, arg_l(args, 0).saturating_mul(1000))?,
        (INST, "now") => ms_obj(vm, INST, now_ms(vm))?,
        (YM, "now") => {
            let (y, m, ..) = fields(now_ms(vm));
            ym(vm, y * 12 + m - 1)?
        }
        (YM, "of") => ym(vm, arg_l(args, 0) * 12 + arg_l(args, 1) - 1)?,
        (YM, "from") => {
            let ms = temporal_ms(vm, &args[0]).unwrap_or(0);
            let (y, m, ..) = fields(ms);
            ym(vm, y * 12 + m - 1)?
        }
        (DUR, "ofMillis") => ms_obj(vm, DUR, arg_l(args, 0))?,
        (DUR, "ofSeconds") => ms_obj(vm, DUR, arg_l(args, 0).saturating_mul(1000))?,
        (DUR, "ofMinutes") => ms_obj(vm, DUR, arg_l(args, 0).saturating_mul(60_000))?,
        (DUR, "ofHours") => ms_obj(vm, DUR, arg_l(args, 0).saturating_mul(3_600_000))?,
        (DUR, "ofDays") => ms_obj(vm, DUR, arg_l(args, 0).saturating_mul(DAY_MS))?,
        (FMT, "ofPattern") => {
            let pat = args.first().cloned().unwrap_or(Value::Null);
            let lang = match args.get(1) {
                Some(Value::Obj(l)) => {
                    let lv = vm.heap.get_field(*l, "rd_lang").unwrap_or(Value::Null);
                    str_arg(vm, &lv)
                }
                _ => std::env::var("RD_LOCALE")
                    .unwrap_or_else(|_| "en".into())
                    .chars()
                    .take(2)
                    .collect(),
            };
            let lang = crate::intrinsics::alloc_string(vm, lang)?;
            alloc(
                vm,
                FMT,
                vec![("rd_pat", pat), ("rd_lang", Value::Obj(lang))],
            )?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !recv_class.starts_with("Ljava/time/") {
        return Ok(None);
    }
    let this = Value::Obj(recv);
    let same = |vm: &Engine, other: &Value| {
        class_of(vm, other)
            .map(|(_, c)| c == recv_class)
            .unwrap_or(false)
    };
    // Shared comparisons on the scalar value.
    let my = temporal_ms(vm, &this);
    let cmp_other = args.first().and_then(|o| temporal_ms(vm, o));
    match name {
        "equals" if sig == "(Ljava/lang/Object;)Z" => {
            let eq = same(vm, &args[0]) && my == cmp_other;
            return Ok(Some(Value::Int(eq as i32)));
        }
        "isBefore" | "isAfter" | "isEqual" | "compareTo" if args.len() == 1 => {
            let (a, b) = (my.unwrap_or(0), cmp_other.unwrap_or(0));
            return Ok(Some(Value::Int(match name {
                "isBefore" => (a < b) as i32,
                "isAfter" => (a > b) as i32,
                "isEqual" => (a == b) as i32,
                _ => a.cmp(&b) as i32,
            })));
        }
        _ => {}
    }
    let v = match recv_class {
        LD => {
            let day = long_field(vm, recv, "rd_day");
            let (y, m, d) = civil_from_days(day);
            match name {
                "toEpochDay" => Value::Long(day),
                "getYear" => Value::Int(y as i32),
                "getMonthValue" => Value::Int(m as i32),
                "getDayOfMonth" => Value::Int(d as i32),
                "getDayOfYear" => Value::Int((day - days_from_civil(y, 1, 1) + 1) as i32),
                "lengthOfMonth" => Value::Int(month_len(y, m) as i32),
                "plusDays" => ld(vm, day + arg_l(args, 0))?,
                "minusDays" => ld(vm, day - arg_l(args, 0))?,
                "plusWeeks" => ld(vm, day + 7 * arg_l(args, 0))?,
                "minusWeeks" => ld(vm, day - 7 * arg_l(args, 0))?,
                "plusMonths" | "minusMonths" => {
                    let k = if name == "plusMonths" {
                        arg_l(args, 0)
                    } else {
                        -arg_l(args, 0)
                    };
                    let t = y * 12 + m - 1 + k;
                    let (ny, nm) = (t.div_euclid(12), t.rem_euclid(12) + 1);
                    ld(vm, days_from_civil(ny, nm, d.min(month_len(ny, nm))))?
                }
                "withDayOfMonth" => ld(vm, days_from_civil(y, m, arg_l(args, 0)))?,
                "atStartOfDay" => ms_obj(vm, LDT, day * DAY_MS)?,
                "hashCode" => Value::Int(ld_hash(day)),
                "toString" => Value::Obj(crate::intrinsics::alloc_string(
                    vm,
                    format!("{y:04}-{m:02}-{d:02}"),
                )?),
                "format" => format_with(vm, &args[0], day * DAY_MS)?,
                _ => return Ok(None),
            }
        }
        LDT | ZDT | INST => {
            let ms = long_field(vm, recv, "rd_ms");
            let (y, m, d, h, mi, s, ..) = fields(ms);
            match name {
                "toLocalDate" => ld(vm, ms.div_euclid(DAY_MS))?,
                "toLocalDateTime" => ms_obj(vm, LDT, ms)?,
                "toInstant" => ms_obj(vm, INST, ms)?,
                "atZone" => ms_obj(vm, ZDT, ms)?,
                "toEpochMilli" => Value::Long(ms),
                "getEpochSecond" => Value::Long(ms.div_euclid(1000)),
                "toEpochSecond" => Value::Long(ms.div_euclid(1000)),
                "getZone" => zone(vm)?,
                "getYear" => Value::Int(y as i32),
                "getMonthValue" => Value::Int(m as i32),
                "getDayOfMonth" => Value::Int(d as i32),
                "getHour" => Value::Int(h as i32),
                "getMinute" => Value::Int(mi as i32),
                "getSecond" => Value::Int(s as i32),
                "plusDays" => ms_obj(vm, recv_class, ms + arg_l(args, 0) * DAY_MS)?,
                "minusDays" => ms_obj(vm, recv_class, ms - arg_l(args, 0) * DAY_MS)?,
                "plusHours" => ms_obj(vm, recv_class, ms + arg_l(args, 0) * 3_600_000)?,
                "plusMinutes" => ms_obj(vm, recv_class, ms + arg_l(args, 0) * 60_000)?,
                "plusSeconds" => ms_obj(vm, recv_class, ms + arg_l(args, 0) * 1000)?,
                "plusMillis" => ms_obj(vm, recv_class, ms + arg_l(args, 0))?,
                "hashCode" => {
                    let day = ms.div_euclid(DAY_MS);
                    let nanos = ms.rem_euclid(DAY_MS) * 1_000_000;
                    if recv_class == INST {
                        Value::Int((ms ^ (ms >> 32)) as i32)
                    } else {
                        Value::Int(ld_hash(day) ^ ((nanos ^ (nanos >> 32)) as i32))
                    }
                }
                "toString" => {
                    let s = match recv_class {
                        INST => format!("{}Z", iso_ldt(ms)),
                        ZDT => format!("{}Z[UTC]", iso_ldt(ms)),
                        _ => iso_ldt(ms),
                    };
                    Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
                }
                "format" => format_with(vm, &args[0], ms)?,
                _ => return Ok(None),
            }
        }
        YM => {
            let t = long_field(vm, recv, "rd_ym");
            let (y, m) = (t.div_euclid(12), t.rem_euclid(12) + 1);
            match name {
                "plusMonths" => ym(vm, t + arg_l(args, 0))?,
                "minusMonths" => ym(vm, t - arg_l(args, 0))?,
                "plusYears" => ym(vm, t + 12 * arg_l(args, 0))?,
                "minusYears" => ym(vm, t - 12 * arg_l(args, 0))?,
                "getYear" => Value::Int(y as i32),
                "getMonthValue" => Value::Int(m as i32),
                "lengthOfMonth" => Value::Int(month_len(y, m) as i32),
                "atDay" => ld(vm, days_from_civil(y, m, arg_l(args, 0)))?,
                "atEndOfMonth" => ld(vm, days_from_civil(y, m, month_len(y, m)))?,
                "hashCode" => Value::Int((y as i32) ^ ((m as i32) << 27)),
                "toString" => Value::Obj(crate::intrinsics::alloc_string(
                    vm,
                    format!("{y:04}-{m:02}"),
                )?),
                "format" => format_with(vm, &args[0], days_from_civil(y, m, 1) * DAY_MS)?,
                _ => return Ok(None),
            }
        }
        DUR => {
            let ms = long_field(vm, recv, "rd_ms");
            match name {
                "toMillis" => Value::Long(ms),
                "getSeconds" | "toSeconds" => Value::Long(ms.div_euclid(1000)),
                "toMinutes" => Value::Long(ms / 60_000),
                "toHours" => Value::Long(ms / 3_600_000),
                "toDays" => Value::Long(ms / DAY_MS),
                "isNegative" => Value::Int((ms < 0) as i32),
                "isZero" => Value::Int((ms == 0) as i32),
                "hashCode" => Value::Int((ms ^ (ms >> 32)) as i32),
                _ => return Ok(None),
            }
        }
        FMT => match name {
            "format" => {
                let ms = temporal_ms(vm, &args[0]).unwrap_or(0);
                format_with(vm, &this, ms)?
            }
            "withLocale" => {
                let pat = vm.heap.get_field(recv, "rd_pat").unwrap_or(Value::Null);
                let lang = match args.first() {
                    Some(Value::Obj(l)) => {
                        let lv = vm.heap.get_field(*l, "rd_lang").unwrap_or(Value::Null);
                        str_arg(vm, &lv)
                    }
                    _ => String::new(),
                };
                let lang = crate::intrinsics::alloc_string(vm, lang)?;
                alloc(
                    vm,
                    FMT,
                    vec![("rd_pat", pat), ("rd_lang", Value::Obj(lang))],
                )?
            }
            _ => return Ok(None),
        },
        UNIT => match name {
            "between" => {
                let n = vm.heap.get_field(recv, "name").unwrap_or(Value::Null);
                let unit = str_arg(vm, &n);
                let a = temporal_ms(vm, &args[0]).unwrap_or(0);
                let b = temporal_ms(vm, &args[1]).unwrap_or(0);
                Value::Long(match unit_ms(&unit) {
                    Some(-1) => {
                        let (ya, ma, da, ..) = fields(a);
                        let (yb, mb, db, ..) = fields(b);
                        let mut months = (yb * 12 + mb) - (ya * 12 + ma);
                        if months > 0 && db < da {
                            months -= 1;
                        } else if months < 0 && db > da {
                            months += 1;
                        }
                        if unit == "YEARS" {
                            months / 12
                        } else {
                            months
                        }
                    }
                    Some(u) => (b - a) / u,
                    None => return Ok(None),
                })
            }
            "name" | "toString" => vm.heap.get_field(recv, "name").unwrap_or(Value::Null),
            _ => return Ok(None),
        },
        ZONE => match name {
            "getId" | "toString" => vm.heap.get_field(recv, "rd_id").unwrap_or(Value::Null),
            "hashCode" => Value::Int(0),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn format_with(vm: &mut Engine, fmt: &Value, ms: i64) -> Result<Value, VmExit> {
    let Value::Obj(f) = fmt else {
        return Err(
            crate::err::Throwable::new("Ljava/lang/NullPointerException;", "formatter").into(),
        );
    };
    let p = vm.heap.get_field(*f, "rd_pat").unwrap_or(Value::Null);
    let l = vm.heap.get_field(*f, "rd_lang").unwrap_or(Value::Null);
    let (pat, lang) = (str_arg(vm, &p), str_arg(vm, &l));
    let s = format_pattern(&pat, &lang, ms);
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_roundtrip_and_format() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(month_len(2024, 2), 29);
        let ms = days_from_civil(2026, 10, 19) * DAY_MS + 14 * 3_600_000 + 5 * 60_000;
        assert_eq!(
            format_pattern("dd/MM/yyyy '\u{e0}s' HH:mm", "pt", ms),
            "19/10/2026 às 14:05"
        );
        assert_eq!(format_pattern("MMM/yy", "pt", ms), "out./26");
        assert_eq!(
            format_pattern("yyyyMMdd-HHmmss", "en", ms),
            "20261019-140500"
        );
        assert_eq!(ld_hash(days_from_civil(2026, 10, 19)), {
            let (y, m, d) = (2026i32, 10i32, 19i32);
            (y & (0xFFFF_F800u32 as i32)) ^ ((y << 11) + (m << 6) + d)
        });
    }
}
