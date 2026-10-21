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
//! - `DateTimeFormatter`: `rd_pat`, `rd_lang`, and an optional `rd_zone`.
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
const DATE: &str = "Ljava/util/Date;";
const TIME_ZONE: &str = "Ljava/util/TimeZone;";
const CALENDAR: &str = "Ljava/util/Calendar;";
const GREGORIAN_CALENDAR: &str = "Ljava/util/GregorianCalendar;";
const SIMPLE_DATE_FORMAT: &str = "Ljava/text/SimpleDateFormat;";
const DATE_FORMAT_SYMBOLS: &str = "Ljava/text/DateFormatSymbols;";
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
        DATE | LDT | INST | ZDT | DUR => Some(long_field(vm, r, "rd_ms")),
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

/// java.util.TimeZone is deliberately deterministic like java.time.ZoneId:
/// the emulator models UTC/GMT only and maps unknown IDs to GMT, matching the
/// JDK fallback instead of consulting the host timezone database.
fn java_util_timezone(vm: &mut Engine, requested: &str) -> Result<Value, VmExit> {
    let id = match requested {
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "GMT+00:00" | "GMT-00:00" => requested,
        _ => "GMT",
    };
    let id = crate::intrinsics::alloc_string(vm, id.to_string())?;
    alloc(
        vm,
        TIME_ZONE,
        vec![("rd_id", Value::Obj(id)), ("rd_offset", Value::Int(0))],
    )
}

fn calendar_ms(vm: &Engine, recv: ObjRef) -> i64 {
    match vm.heap.get_field(recv, "rd$calendar_ms") {
        Ok(Value::Long(ms)) => ms,
        Ok(Value::Int(ms)) => ms as i64,
        _ => 0,
    }
}

fn calendar_initialize(vm: &mut Engine, recv: ObjRef, args: &[Value]) -> Result<(), VmExit> {
    let now = now_ms(vm);
    set_field(vm, recv, "rd$calendar_ms", Value::Long(now))?;
    set_field(vm, recv, "rd$first_day_of_week", Value::Int(1))?; // US/Sunday
    set_field(vm, recv, "rd$minimal_days_in_first_week", Value::Int(1))?;
    set_field(vm, recv, "rd$lenient", Value::Int(1))?;
    let timezone = match args.first() {
        Some(Value::Obj(r))
            if vm
                .cp
                .is_subtype(vm.heap.class_of(*r).unwrap_or(""), TIME_ZONE) =>
        {
            Value::Obj(*r)
        }
        _ => java_util_timezone(vm, "UTC")?,
    };
    set_field(vm, recv, "rd$timezone", timezone)?;
    Ok(())
}

const CAL_ERA: i32 = 0;
const CAL_YEAR: i32 = 1;
const CAL_MONTH: i32 = 2;
const CAL_WEEK_OF_YEAR: i32 = 3;
const CAL_WEEK_OF_MONTH: i32 = 4;
const CAL_DAY_OF_MONTH: i32 = 5;
const CAL_DAY_OF_YEAR: i32 = 6;
const CAL_DAY_OF_WEEK: i32 = 7;
const CAL_DAY_OF_WEEK_IN_MONTH: i32 = 8;
const CAL_AM_PM: i32 = 9;
const CAL_HOUR: i32 = 10;
const CAL_HOUR_OF_DAY: i32 = 11;
const CAL_MINUTE: i32 = 12;
const CAL_SECOND: i32 = 13;
const CAL_MILLISECOND: i32 = 14;

fn calendar_get_field(ms: i64, field: i32) -> Option<i32> {
    let (year, month, day, hour, minute, second, milli, dow_monday) = fields(ms);
    let day_of_year = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
    Some(match field {
        CAL_ERA => 1, // Gregorian AD; BC dates are outside this UTC model.
        CAL_YEAR => year as i32,
        CAL_MONTH => (month - 1) as i32,
        CAL_WEEK_OF_YEAR => ((day_of_year - 1) / 7 + 1) as i32,
        CAL_WEEK_OF_MONTH => ((day - 1) / 7 + 1) as i32,
        CAL_DAY_OF_MONTH => day as i32,
        CAL_DAY_OF_YEAR => day_of_year as i32,
        CAL_DAY_OF_WEEK => (dow_monday.rem_euclid(7) + 1) as i32, // Sunday=1
        CAL_DAY_OF_WEEK_IN_MONTH => ((day - 1) / 7 + 1) as i32,
        CAL_AM_PM => (hour / 12) as i32,
        CAL_HOUR => (hour % 12) as i32,
        CAL_HOUR_OF_DAY => hour as i32,
        CAL_MINUTE => minute as i32,
        CAL_SECOND => second as i32,
        CAL_MILLISECOND => milli as i32,
        _ => return None,
    })
}

fn compose_calendar_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    days_from_civil(year, month, day) * DAY_MS
        + hour * 3_600_000
        + minute * 60_000
        + second * 1000
        + milli
}

fn calendar_set_field(ms: i64, field: i32, value: i32) -> Option<i64> {
    let (year, month, day, _, _, _, _, dow_monday) = fields(ms);
    let value = i64::from(value);
    let day_epoch = ms.div_euclid(DAY_MS);
    let remainder = ms.rem_euclid(DAY_MS);
    let (hour, minute, second, milli) = (
        remainder / 3_600_000,
        remainder / 60_000 % 60,
        remainder / 1000 % 60,
        remainder % 1000,
    );
    Some(match field {
        CAL_ERA => ms,
        CAL_YEAR => compose_calendar_ms(
            value,
            month,
            day.min(month_len(value, month)),
            hour,
            minute,
            second,
            milli,
        ),
        CAL_MONTH => {
            let month_index = year * 12 + value;
            let y = month_index.div_euclid(12);
            let m = month_index.rem_euclid(12) + 1;
            compose_calendar_ms(y, m, day.min(month_len(y, m)), hour, minute, second, milli)
        }
        CAL_WEEK_OF_YEAR => {
            days_from_civil(year, 1, 1) * DAY_MS + (value - 1) * 7 * DAY_MS + remainder
        }
        CAL_WEEK_OF_MONTH => {
            days_from_civil(year, month, 1) * DAY_MS + (value - 1) * 7 * DAY_MS + remainder
        }
        CAL_DAY_OF_YEAR => days_from_civil(year, 1, 1) * DAY_MS + (value - 1) * DAY_MS + remainder,
        CAL_DAY_OF_MONTH => {
            days_from_civil(year, month, 1) * DAY_MS + (value - 1) * DAY_MS + remainder
        }
        CAL_DAY_OF_WEEK | CAL_DAY_OF_WEEK_IN_MONTH => {
            let current_dow = dow_monday.rem_euclid(7) + 1;
            day_epoch * DAY_MS + (value - current_dow) * DAY_MS + remainder
        }
        CAL_AM_PM => {
            let new_hour = value * 12 + hour.rem_euclid(12);
            day_epoch * DAY_MS + new_hour * 3_600_000 + minute * 60_000 + second * 1000 + milli
        }
        CAL_HOUR => {
            day_epoch * DAY_MS
                + (hour - hour.rem_euclid(12) + value) * 3_600_000
                + minute * 60_000
                + second * 1000
                + milli
        }
        CAL_HOUR_OF_DAY => {
            day_epoch * DAY_MS + value * 3_600_000 + minute * 60_000 + second * 1000 + milli
        }
        CAL_MINUTE => {
            day_epoch * DAY_MS + hour * 3_600_000 + value * 60_000 + second * 1000 + milli
        }
        CAL_SECOND => {
            day_epoch * DAY_MS + hour * 3_600_000 + minute * 60_000 + value * 1000 + milli
        }
        CAL_MILLISECOND => {
            day_epoch * DAY_MS + hour * 3_600_000 + minute * 60_000 + second * 1000 + value
        }
        _ => return None,
    })
}

fn calendar_roll_field(ms: i64, field: i32, amount: i32) -> Option<i64> {
    let (year, month, day, hour, minute, second, milli, dow_monday) = fields(ms);
    let amount = i64::from(amount);
    Some(match field {
        CAL_MILLISECOND => {
            let next = (milli + amount).rem_euclid(1000);
            ms + next - milli
        }
        CAL_SECOND => {
            let next = (second + amount).rem_euclid(60);
            ms + (next - second) * 1000
        }
        CAL_MINUTE => {
            let next = (minute + amount).rem_euclid(60);
            ms + (next - minute) * 60_000
        }
        CAL_HOUR | CAL_HOUR_OF_DAY => {
            let next = (hour + amount).rem_euclid(24);
            ms + (next - hour) * 3_600_000
        }
        CAL_DAY_OF_MONTH => {
            let len = month_len(year, month);
            let next = (day - 1 + amount).rem_euclid(len) + 1;
            compose_calendar_ms(year, month, next, hour, minute, second, milli)
        }
        CAL_DAY_OF_YEAR => {
            let len = days_from_civil(year + 1, 1, 1) - days_from_civil(year, 1, 1);
            let doy = days_from_civil(year, month, day) - days_from_civil(year, 1, 1);
            days_from_civil(year, 1, 1) * DAY_MS
                + (doy + amount).rem_euclid(len) * DAY_MS
                + ms.rem_euclid(DAY_MS)
        }
        CAL_MONTH => {
            let next_month = (month - 1 + amount).rem_euclid(12) + 1;
            compose_calendar_ms(
                year,
                next_month,
                day.min(month_len(year, next_month)),
                hour,
                minute,
                second,
                milli,
            )
        }
        CAL_YEAR => {
            let next_year = year + amount;
            compose_calendar_ms(
                next_year,
                month,
                day.min(month_len(next_year, month)),
                hour,
                minute,
                second,
                milli,
            )
        }
        CAL_DAY_OF_WEEK | CAL_DAY_OF_WEEK_IN_MONTH => {
            let _ = dow_monday;
            ms + amount * DAY_MS
        }
        _ => return None,
    })
}

fn calendar_add_field(ms: i64, field: i32, amount: i32) -> Option<i64> {
    let amount = i64::from(amount);
    let (year, month, day, hour, minute, second, milli, _) = fields(ms);
    Some(match field {
        CAL_MILLISECOND => ms + amount,
        CAL_SECOND => ms + amount * 1000,
        CAL_MINUTE => ms + amount * 60_000,
        CAL_HOUR | CAL_HOUR_OF_DAY => ms + amount * 3_600_000,
        CAL_AM_PM => ms + amount * 12 * 3_600_000,
        CAL_WEEK_OF_MONTH | CAL_WEEK_OF_YEAR => ms + amount * 7 * DAY_MS,
        CAL_DAY_OF_WEEK | CAL_DAY_OF_WEEK_IN_MONTH | CAL_DAY_OF_MONTH | CAL_DAY_OF_YEAR => {
            ms + amount * DAY_MS
        }
        CAL_MONTH => {
            let month_index = year * 12 + month - 1 + amount;
            let y = month_index.div_euclid(12);
            let m = month_index.rem_euclid(12) + 1;
            compose_calendar_ms(y, m, day.min(month_len(y, m)), hour, minute, second, milli)
        }
        CAL_YEAR => {
            let y = year + amount;
            compose_calendar_ms(
                y,
                month,
                day.min(month_len(y, month)),
                hour,
                minute,
                second,
                milli,
            )
        }
        _ => return None,
    })
}

fn string_array(vm: &mut Engine, items: &[&str]) -> Result<Value, VmExit> {
    let mut values = Vec::new();
    for item in items {
        values.push(Value::Obj(crate::intrinsics::alloc_string(
            vm,
            (*item).to_string(),
        )?));
    }
    crate::arrays::new_array(
        vm,
        "[Ljava/lang/String;",
        crate::heap::ElemKind::Obj,
        values,
    )
}

/// java.text.DateFormatSymbols: deterministic en-US symbols (the virtual
/// device locale model, same as SimpleDateFormat). Arrays follow the JDK
/// layout: months 13 slots (last empty), weekdays 8 slots (first empty,
/// Sunday first), short weekdays likewise, eras [BC, AD], ampm [AM, PM].
fn date_format_symbols(vm: &mut Engine) -> Result<Value, VmExit> {
    alloc(vm, DATE_FORMAT_SYMBOLS, Vec::new())
}

fn date_format_symbols_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        return Ok(Some(Value::Null));
    }
    let weekdays_full: [&str; 8] = [
        "",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let weekdays_short: [&str; 8] = ["", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months_full: [&str; 13] = [
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
        "",
    ];
    let months_short: [&str; 13] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", "",
    ];
    let value = match (name, sig) {
        ("getAmPmStrings", "()[Ljava/lang/String;") => string_array(vm, &["AM", "PM"])?,
        ("getWeekdays", "()[Ljava/lang/String;") => string_array(vm, &weekdays_full)?,
        ("getShortWeekdays", "()[Ljava/lang/String;") => string_array(vm, &weekdays_short)?,
        ("getMonths", "()[Ljava/lang/String;") => string_array(vm, &months_full)?,
        ("getShortMonths", "()[Ljava/lang/String;") => string_array(vm, &months_short)?,
        ("getEras", "()[Ljava/lang/String;") => string_array(vm, &["BC", "AD"])?,
        ("getZoneStrings", "()[[Ljava/lang/String;") => crate::arrays::new_array(
            vm,
            "[[Ljava/lang/String;",
            crate::heap::ElemKind::Obj,
            Vec::new(),
        )?,
        ("clone", "()Ljava/lang/Object;") => alloc(vm, DATE_FORMAT_SYMBOLS, Vec::new())?,
        ("hashCode", "()I") => Value::Int(recv as i32),
        ("equals", "(Ljava/lang/Object;)Z") => {
            Value::Int(matches!(_args.first(), Some(Value::Obj(o)) if *o == recv) as i32)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn timezone_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("getTimeZone", "(Ljava/lang/String;)Ljava/util/TimeZone;") => {
            let id = match args.first() {
                Some(Value::Obj(r)) => vm.heap.as_str(*r).map(str::to_string).map_err(|e| {
                    crate::err::vm_error("INVALID_ARG", format!("TimeZone id: {e}"))
                })?,
                Some(Value::Null) => {
                    return Err(crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "TimeZone.getTimeZone(id == null)",
                    )
                    .into());
                }
                _ => "GMT".into(),
            };
            Ok(Some(java_util_timezone(vm, &id)?))
        }
        ("getDefault", "()Ljava/util/TimeZone;") => Ok(Some(java_util_timezone(vm, "UTC")?)),
        _ => Ok(None),
    }
}

fn timezone_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let value = match (name, sig) {
        ("getID", "()Ljava/lang/String;") | ("toString", "()Ljava/lang/String;") => {
            vm.heap.get_field(recv, "rd_id").unwrap_or(Value::Null)
        }
        ("setID", "(Ljava/lang/String;)V") => {
            let Some(Value::Obj(id)) = args.first() else {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "TimeZone.setID(id == null)",
                )
                .into());
            };
            set_field(vm, recv, "rd_id", Value::Obj(*id))?;
            Value::Null
        }
        ("getRawOffset", "()I") | ("getOffset", "(J)I") => vm
            .heap
            .get_field(recv, "rd_offset")
            .unwrap_or(Value::Int(0)),
        ("getOffset", "(IIIIII)I") => Value::Int(0),
        ("setRawOffset", "(I)V") => {
            let offset = args.first().map(Value::as_int).transpose()?.unwrap_or(0);
            set_field(vm, recv, "rd_offset", Value::Int(offset))?;
            Value::Null
        }
        ("getDSTSavings", "()I") => Value::Int(0),
        ("useDaylightTime", "()Z") | ("observesDaylightTime", "()Z") => Value::Int(0),
        ("inDaylightTime", "(Ljava/util/Date;)Z") => Value::Int(0),
        ("hasSameRules", "(Ljava/util/TimeZone;)Z") => {
            let this = vm
                .heap
                .get_field(recv, "rd_offset")
                .unwrap_or(Value::Int(0));
            let same = match args.first() {
                Some(Value::Obj(other)) => {
                    vm.heap
                        .get_field(*other, "rd_offset")
                        .unwrap_or(Value::Int(0))
                        == this
                }
                _ => false,
            };
            Value::Int(same as i32)
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let equal = match args.first() {
                Some(Value::Obj(other)) if *other == recv => true,
                Some(Value::Obj(other)) => {
                    let a = vm.heap.get_field(recv, "rd_id").unwrap_or(Value::Null);
                    let b = vm.heap.get_field(*other, "rd_id").unwrap_or(Value::Null);
                    a == b
                }
                _ => false,
            };
            Value::Int(equal as i32)
        }
        ("clone", "()Ljava/lang/Object;") => {
            let id = vm.heap.get_field(recv, "rd_id").unwrap_or(Value::Null);
            let offset = vm
                .heap
                .get_field(recv, "rd_offset")
                .unwrap_or(Value::Int(0));
            alloc(vm, TIME_ZONE, vec![("rd_id", id), ("rd_offset", offset)])?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

fn calendar_instance(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.cp.is_subtype(recv_class, CALENDAR) {
        return Ok(None);
    }
    if name == "<init>" {
        // Only intercept the platform superclass constructors. A DEX subclass
        // (for example Logback's RollingCalendar(String)) must still execute
        // its own constructor body and initialize its guest fields.
        if matches!(sig, "()V" | "(Ljava/util/TimeZone;Ljava/util/Locale;)V") {
            calendar_initialize(vm, recv, args)?;
            return Ok(Some(Value::Null));
        }
        return Ok(None);
    }
    let value = match (name, sig) {
        ("getTimeInMillis", "()J") => Value::Long(calendar_ms(vm, recv)),
        ("setTimeInMillis", "(J)V") => {
            set_field(vm, recv, "rd$calendar_ms", Value::Long(arg_l(args, 0)))?;
            Value::Null
        }
        ("getTime", "()Ljava/util/Date;") => ms_obj(vm, DATE, calendar_ms(vm, recv))?,
        ("setTime", "(Ljava/util/Date;)V") => {
            let Some(ms) = args.first().and_then(|v| temporal_ms(vm, v)) else {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "Calendar.setTime requires a Date",
                )
                .into());
            };
            set_field(vm, recv, "rd$calendar_ms", Value::Long(ms))?;
            Value::Null
        }
        ("get", "(I)I") => {
            let field = args.first().map(Value::as_int).transpose()?.unwrap_or(-1);
            let Some(value) = calendar_get_field(calendar_ms(vm, recv), field) else {
                return Ok(None);
            };
            Value::Int(value)
        }
        ("set", "(II)V") => {
            let field = args.first().map(Value::as_int).transpose()?.unwrap_or(-1);
            let field_value = args.get(1).map(Value::as_int).transpose()?.unwrap_or(0);
            let Some(ms) = calendar_set_field(calendar_ms(vm, recv), field, field_value) else {
                return Ok(None);
            };
            set_field(vm, recv, "rd$calendar_ms", Value::Long(ms))?;
            Value::Null
        }
        ("add", "(II)V") | ("roll", "(II)V") => {
            let field = args.first().map(Value::as_int).transpose()?.unwrap_or(-1);
            let amount = args.get(1).map(Value::as_int).transpose()?.unwrap_or(0);
            let ms = calendar_ms(vm, recv);
            let changed = if name == "add" {
                calendar_add_field(ms, field, amount)
            } else {
                calendar_roll_field(ms, field, amount)
            };
            let Some(changed) = changed else {
                return Ok(None);
            };
            set_field(vm, recv, "rd$calendar_ms", Value::Long(changed))?;
            Value::Null
        }
        ("getFirstDayOfWeek", "()I") => vm
            .heap
            .get_field(recv, "rd$first_day_of_week")
            .unwrap_or(Value::Int(1)),
        ("setFirstDayOfWeek", "(I)V") => {
            let value = args.first().map(Value::as_int).transpose()?.unwrap_or(1);
            set_field(vm, recv, "rd$first_day_of_week", Value::Int(value))?;
            Value::Null
        }
        ("getMinimalDaysInFirstWeek", "()I") => vm
            .heap
            .get_field(recv, "rd$minimal_days_in_first_week")
            .unwrap_or(Value::Int(1)),
        ("setMinimalDaysInFirstWeek", "(I)V") => {
            let value = args.first().map(Value::as_int).transpose()?.unwrap_or(1);
            set_field(vm, recv, "rd$minimal_days_in_first_week", Value::Int(value))?;
            Value::Null
        }
        ("getTimeZone", "()Ljava/util/TimeZone;") => {
            if let Ok(value) = vm.heap.get_field(recv, "rd$timezone") {
                value
            } else {
                let timezone = java_util_timezone(vm, "UTC")?;
                set_field(vm, recv, "rd$timezone", timezone.clone())?;
                timezone
            }
        }
        ("setTimeZone", "(Ljava/util/TimeZone;)V") => {
            let Some(Value::Obj(timezone)) = args.first() else {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Calendar.setTimeZone(zone == null)",
                )
                .into());
            };
            set_field(vm, recv, "rd$timezone", Value::Obj(*timezone))?;
            Value::Null
        }
        ("isLenient", "()Z") => vm
            .heap
            .get_field(recv, "rd$lenient")
            .unwrap_or(Value::Int(1)),
        ("setLenient", "(Z)V") => {
            let value = args.first().map(Value::as_int).transpose()?.unwrap_or(1);
            set_field(vm, recv, "rd$lenient", Value::Int((value != 0) as i32))?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
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

fn set_field(vm: &mut Engine, recv: ObjRef, field: &str, value: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(recv, field, value))?;
    Ok(())
}

fn simple_date_format_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let pattern = match args.first() {
            Some(Value::Obj(r)) => vm.heap.as_str(*r).map(str::to_string).map_err(|e| {
                crate::err::vm_error("INVALID_ARG", format!("SimpleDateFormat pattern: {e}"))
            })?,
            Some(Value::Null) => {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "SimpleDateFormat pattern == null",
                )
                .into());
            }
            _ => "yyyy.MM.dd G 'at' HH:mm:ss z".to_string(),
        };
        let language = match args.get(1) {
            Some(Value::Obj(locale)) => {
                let lang = vm.heap.get_field(*locale, "rd_lang").unwrap_or(Value::Null);
                let mut tag = str_arg(vm, &lang);
                if tag.is_empty() {
                    tag = crate::intrinsics::object_to_string(vm, *locale).unwrap_or_default();
                }
                tag.split(['_', '-'])
                    .next()
                    .unwrap_or("en")
                    .to_ascii_lowercase()
            }
            _ => "en".to_string(),
        };
        let pattern = crate::intrinsics::alloc_string(vm, pattern)?;
        let language = crate::intrinsics::alloc_string(vm, language)?;
        set_field(vm, recv, "rd_pattern", Value::Obj(pattern))?;
        set_field(vm, recv, "rd_language", Value::Obj(language))?;
        set_field(vm, recv, "rd_lenient", Value::Int(1))?;
        return Ok(Some(Value::Null));
    }

    let pattern = vm
        .heap
        .get_field(recv, "rd_pattern")
        .ok()
        .map(|v| str_arg(vm, &v))
        .unwrap_or_default();
    let language = vm
        .heap
        .get_field(recv, "rd_language")
        .ok()
        .map(|v| str_arg(vm, &v))
        .unwrap_or_else(|| "en".to_string());
    match (name, sig) {
        ("format", "(Ljava/util/Date;)Ljava/lang/String;")
        | ("format", "(Ljava/lang/Object;)Ljava/lang/String;") => {
            let ms = args.first().and_then(|value| temporal_ms(vm, value)).ok_or_else(|| {
                crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "SimpleDateFormat.format requires a Date",
                )
            })?;
            Ok(Some(Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format_pattern(&pattern, &language, ms),
            )?)))
        }
        (
            "format",
            "(Ljava/util/Date;Ljava/lang/StringBuffer;Ljava/text/FieldPosition;)Ljava/lang/StringBuffer;",
        )
        | (
            "format",
            "(Ljava/lang/Object;Ljava/lang/StringBuffer;Ljava/text/FieldPosition;)Ljava/lang/StringBuffer;",
        ) => {
            let ms = args.first().and_then(|value| temporal_ms(vm, value)).ok_or_else(|| {
                crate::err::Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "SimpleDateFormat.format requires a Date",
                )
            })?;
            let Some(Value::Obj(buffer)) = args.get(1) else {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "SimpleDateFormat output buffer == null",
                )
                .into());
            };
            let text = crate::intrinsics::alloc_string(vm, format_pattern(&pattern, &language, ms))?;
            vm.invoke_instance(
                *buffer,
                "append",
                "(Ljava/lang/String;)Ljava/lang/StringBuffer;",
                &[Value::Obj(text)],
            )?;
            Ok(Some(Value::Obj(*buffer)))
        }
        ("toPattern", "()Ljava/lang/String;") => Ok(Some(Value::Obj(
            crate::intrinsics::alloc_string(vm, pattern)?,
        ))),
        ("applyPattern", "(Ljava/lang/String;)V") => {
            let Some(Value::Obj(value)) = args.first() else {
                return Err(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "SimpleDateFormat pattern == null",
                )
                .into());
            };
            let pattern = vm.heap.as_str(*value).map(str::to_string).map_err(|e| {
                crate::err::vm_error("INVALID_ARG", format!("SimpleDateFormat pattern: {e}"))
            })?;
            let pattern = crate::intrinsics::alloc_string(vm, pattern)?;
            set_field(vm, recv, "rd_pattern", Value::Obj(pattern))?;
            Ok(Some(Value::Null))
        }
        ("setTimeZone", "(Ljava/util/TimeZone;)V")
        | ("setLenient", "(Z)V")
        | ("set2DigitYearStart", "(Ljava/util/Date;)V") => Ok(Some(Value::Null)),
        ("isLenient", "()Z") => Ok(Some(
            vm.heap.get_field(recv, "rd_lenient").unwrap_or(Value::Int(1)),
        )),
        ("hashCode", "()I") => Ok(Some(Value::Int(pattern.bytes().fold(0, |h, b| {
            h.wrapping_mul(31).wrapping_add(b as i32)
        })))),
        _ => Ok(None),
    }
}

fn date_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("<init>", "()V") => {
            let now = now_ms(vm);
            set_field(vm, recv, "rd_ms", Value::Long(now))?;
            Ok(Some(Value::Null))
        }
        ("<init>", "(J)V") => {
            set_field(vm, recv, "rd_ms", Value::Long(arg_l(args, 0)))?;
            Ok(Some(Value::Null))
        }
        ("getTime", "()J") => Ok(Some(Value::Long(long_field(vm, recv, "rd_ms")))),
        ("setTime", "(J)V") => {
            set_field(vm, recv, "rd_ms", Value::Long(arg_l(args, 0)))?;
            Ok(Some(Value::Null))
        }
        ("before", "(Ljava/util/Date;)Z") | ("after", "(Ljava/util/Date;)Z") => {
            let other = args
                .first()
                .and_then(|value| temporal_ms(vm, value))
                .unwrap_or(0);
            let this = long_field(vm, recv, "rd_ms");
            Ok(Some(Value::Int(if name == "before" {
                (this < other) as i32
            } else {
                (this > other) as i32
            })))
        }
        ("compareTo", "(Ljava/util/Date;)I") => {
            let other = args
                .first()
                .and_then(|value| temporal_ms(vm, value))
                .unwrap_or(0);
            let ordering = long_field(vm, recv, "rd_ms").cmp(&other);
            Ok(Some(Value::Int(match ordering {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })))
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let this = long_field(vm, recv, "rd_ms");
            let other = args.first().and_then(|value| temporal_ms(vm, value));
            Ok(Some(Value::Int((other == Some(this)) as i32)))
        }
        ("hashCode", "()I") => {
            let ms = long_field(vm, recv, "rd_ms");
            Ok(Some(Value::Int((ms ^ (ms >> 32)) as i32)))
        }
        ("toInstant", "()Ljava/time/Instant;") => {
            Ok(Some(ms_obj(vm, INST, long_field(vm, recv, "rd_ms"))?))
        }
        ("toString", "()Ljava/lang/String;") => {
            let ms = long_field(vm, recv, "rd_ms");
            let text = format!(
                "{} UTC",
                format_pattern("EEE MMM dd HH:mm:ss yyyy", "en", ms)
            );
            Ok(Some(Value::Obj(crate::intrinsics::alloc_string(vm, text)?)))
        }
        _ => Ok(None),
    }
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
    if class == TIME_ZONE {
        return timezone_static(vm, name, sig, args);
    }
    if class == DATE_FORMAT_SYMBOLS && name == "getInstance" {
        return Ok(Some(date_format_symbols(vm)?));
    }
    if class == CALENDAR && name == "getInstance" {
        let calendar = alloc(vm, GREGORIAN_CALENDAR, Vec::new())?;
        let Value::Obj(calendar_ref) = calendar else {
            unreachable!("Calendar.getInstance allocation returns an object")
        };
        calendar_initialize(vm, calendar_ref, args)?;
        return Ok(Some(calendar));
    }
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
    if recv_class == DATE {
        return date_intrinsic(vm, name, sig, recv, args);
    }
    if recv_class == SIMPLE_DATE_FORMAT {
        return simple_date_format_intrinsic(vm, name, sig, recv, args);
    }
    if recv_class == DATE_FORMAT_SYMBOLS {
        return date_format_symbols_instance(vm, name, sig, recv, args);
    }
    if recv_class == TIME_ZONE {
        return timezone_instance(vm, name, sig, recv, args);
    }
    if vm.cp.is_subtype(recv_class, CALENDAR) {
        return calendar_instance(vm, recv_class, name, sig, recv, args);
    }
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
                let zone = vm.heap.get_field(recv, "rd_zone").unwrap_or(Value::Null);
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
                    vec![
                        ("rd_pat", pat),
                        ("rd_lang", Value::Obj(lang)),
                        ("rd_zone", zone),
                    ],
                )?
            }
            "withZone" if sig == "(Ljava/time/ZoneId;)Ljava/time/format/DateTimeFormatter;" => {
                let Some(Value::Obj(zone)) = args.first() else {
                    return Err(crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "DateTimeFormatter.withZone(zone == null)",
                    )
                    .into());
                };
                let pat = vm.heap.get_field(recv, "rd_pat").unwrap_or(Value::Null);
                let lang = vm.heap.get_field(recv, "rd_lang").unwrap_or(Value::Null);
                alloc(
                    vm,
                    FMT,
                    vec![
                        ("rd_pat", pat),
                        ("rd_lang", lang),
                        ("rd_zone", Value::Obj(*zone)),
                    ],
                )?
            }
            "getZone" if sig == "()Ljava/time/ZoneId;" => {
                vm.heap.get_field(recv, "rd_zone").unwrap_or(Value::Null)
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
    use crate::engine::VmConfig;

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

    #[test]
    fn date_format_symbols_en_us_arrays() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let symbols = static_intrinsic(
            &mut vm,
            DATE_FORMAT_SYMBOLS,
            "getInstance",
            "()Ljava/text/DateFormatSymbols;",
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(symbols) = symbols else {
            panic!("getInstance returned non-object")
        };
        let months = date_format_symbols_instance(
            &mut vm,
            "getMonths",
            "()[Ljava/lang/String;",
            symbols,
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(months) = months else {
            panic!("getMonths returned non-array")
        };
        let (elems, _) = vm.heap.as_array_elems(months).unwrap();
        let elems = elems.to_vec();
        assert_eq!(elems.len(), 13);
        let first = match elems.first() {
            Some(Value::Obj(r)) => *r,
            _ => panic!("month not string"),
        };
        assert_eq!(vm.heap.as_str(first).unwrap(), "January");
        let ampm = date_format_symbols_instance(
            &mut vm,
            "getAmPmStrings",
            "()[Ljava/lang/String;",
            symbols,
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(ampm) = ampm else {
            panic!("getAmPmStrings returned non-array")
        };
        assert_eq!(vm.heap.as_array_elems(ampm).unwrap().0.len(), 2);
    }

    #[test]
    fn date_time_formatter_with_zone_preserves_formatter_and_zone() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let pattern = crate::intrinsics::alloc_string(&mut vm, "yyyy-MM-dd".into()).unwrap();
        let formatter = static_intrinsic(
            &mut vm,
            FMT,
            "ofPattern",
            "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
            &[Value::Obj(pattern)],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(formatter) = formatter else {
            panic!("DateTimeFormatter.ofPattern returned non-object");
        };
        let zone_name = crate::intrinsics::alloc_string(&mut vm, "UTC".into()).unwrap();
        let zone = vm
            .heap
            .alloc_instance(
                ZONE.to_string(),
                vec![("rd_id".into(), Value::Obj(zone_name))],
            )
            .unwrap();
        let zoned_formatter = instance_intrinsic(
            &mut vm,
            FMT,
            "withZone",
            "(Ljava/time/ZoneId;)Ljava/time/format/DateTimeFormatter;",
            formatter,
            &[Value::Obj(zone)],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(zoned_formatter) = zoned_formatter else {
            panic!("DateTimeFormatter.withZone returned non-object");
        };
        assert_ne!(formatter, zoned_formatter);
        assert_eq!(
            instance_intrinsic(
                &mut vm,
                FMT,
                "getZone",
                "()Ljava/time/ZoneId;",
                zoned_formatter,
                &[],
            )
            .unwrap(),
            Some(Value::Obj(zone))
        );
        let epoch = ms_obj(&mut vm, INST, 0).unwrap();
        let formatted = instance_intrinsic(
            &mut vm,
            FMT,
            "format",
            "(Ljava/time/temporal/TemporalAccessor;)Ljava/lang/String;",
            zoned_formatter,
            &[epoch],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(formatted) = formatted else {
            panic!("DateTimeFormatter.format returned non-string");
        };
        assert_eq!(vm.heap.as_str(formatted).unwrap(), "1970-01-01");
    }

    #[test]
    fn simple_date_format_formats_date_with_locale_constructor() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let sdf = vm
            .heap
            .alloc_instance(SIMPLE_DATE_FORMAT.to_string(), Vec::new())
            .unwrap();
        let date = vm
            .heap
            .alloc_instance(DATE.to_string(), Vec::new())
            .unwrap();
        let pattern =
            crate::intrinsics::alloc_string(&mut vm, "yyyy-MM-dd HH:mm:ss,SSS".to_string())
                .unwrap();
        let lang = crate::intrinsics::alloc_string(&mut vm, "en".to_string()).unwrap();
        let locale = vm
            .heap
            .alloc_instance(
                "Ljava/util/Locale;".to_string(),
                vec![("rd_lang".to_string(), Value::Obj(lang))],
            )
            .unwrap();
        simple_date_format_intrinsic(
            &mut vm,
            "<init>",
            "(Ljava/lang/String;Ljava/util/Locale;)V",
            sdf,
            &[Value::Obj(pattern), Value::Obj(locale)],
        )
        .unwrap();
        date_intrinsic(&mut vm, "<init>", "(J)V", date, &[Value::Long(0)]).unwrap();
        let formatted = simple_date_format_intrinsic(
            &mut vm,
            "format",
            "(Ljava/util/Date;)Ljava/lang/String;",
            sdf,
            &[Value::Obj(date)],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(formatted) = formatted else {
            panic!("SimpleDateFormat.format returned non-string");
        };
        assert_eq!(
            vm.heap.as_str(formatted).unwrap(),
            "1970-01-01 00:00:00,000"
        );
    }

    #[test]
    fn java_util_timezone_and_calendar_support_utc_rollover() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        let id = crate::intrinsics::alloc_string(&mut vm, "GMT".into()).unwrap();
        let timezone = static_intrinsic(
            &mut vm,
            TIME_ZONE,
            "getTimeZone",
            "(Ljava/lang/String;)Ljava/util/TimeZone;",
            &[Value::Obj(id)],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(timezone) = timezone else {
            panic!("TimeZone.getTimeZone returned non-object");
        };
        assert_eq!(vm.heap.class_of(timezone).unwrap(), TIME_ZONE);
        assert_eq!(
            str_arg(&vm, &vm.heap.get_field(timezone, "rd_id").unwrap()),
            "GMT"
        );

        let calendar = static_intrinsic(
            &mut vm,
            CALENDAR,
            "getInstance",
            "()Ljava/util/Calendar;",
            &[],
        )
        .unwrap()
        .unwrap();
        let Value::Obj(calendar) = calendar else {
            panic!("Calendar.getInstance returned non-object");
        };
        assert_eq!(vm.heap.class_of(calendar).unwrap(), GREGORIAN_CALENDAR);
        let instant = 1_700_000_000_987_i64;
        instance_intrinsic(
            &mut vm,
            GREGORIAN_CALENDAR,
            "setTimeInMillis",
            "(J)V",
            calendar,
            &[Value::Long(instant)],
        )
        .unwrap();
        assert_eq!(
            instance_intrinsic(
                &mut vm,
                GREGORIAN_CALENDAR,
                "get",
                "(I)I",
                calendar,
                &[Value::Int(CAL_MILLISECOND)],
            )
            .unwrap(),
            Some(Value::Int(987))
        );
        instance_intrinsic(
            &mut vm,
            GREGORIAN_CALENDAR,
            "roll",
            "(II)V",
            calendar,
            &[Value::Int(CAL_MILLISECOND), Value::Int(-987)],
        )
        .unwrap();
        instance_intrinsic(
            &mut vm,
            GREGORIAN_CALENDAR,
            "add",
            "(II)V",
            calendar,
            &[Value::Int(CAL_SECOND), Value::Int(1)],
        )
        .unwrap();
        assert_eq!(
            instance_intrinsic(
                &mut vm,
                GREGORIAN_CALENDAR,
                "getTimeInMillis",
                "()J",
                calendar,
                &[],
            )
            .unwrap(),
            Some(Value::Long(instant - 987 + 1000))
        );
    }
}
