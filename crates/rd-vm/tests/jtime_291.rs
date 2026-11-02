//! Regression suite for Issue #291 (`crates/rd-vm/src/jtime.rs` stabilization):
//! 1. `Date.equals` type symmetry + `NullPointerException` on `null` comparisons.
//! 2. Direct `rd_day`/`rd_ym` comparisons & checked `i64` arithmetic (`LocalDate.MAX`).
//! 3. `Instant.ofEpochSecond(sec, nanoAdj)` and 7-arg `LocalDateTime.of(..., nano)`.
//! 4. Strict calendar range validation (`DateTimeException` on invalid dates/fields).
//! 5. `DateTimeFormatter.ofPattern` NPE on `null` & deterministic VM `Locale.getDefault()`.
//! 6. `format_pattern` (`G`, `z`, `Z`, `X`, `SSSSSS` padding), `Calendar.set(DAY_OF_WEEK_IN_MONTH)`,
//!    and `Calendar.roll(HOUR)` 12-hour preservation of `AM_PM`.

mod common;

use common::*;
use rd_vm::value::Value;
use rd_vm::{Engine, ObjRef, VmConfig, VmExit};

const DATE: &str = "Ljava/util/Date;";
const CALENDAR: &str = "Ljava/util/Calendar;";
const SIMPLE_DATE_FORMAT: &str = "Ljava/text/SimpleDateFormat;";
const LOCALE: &str = "Ljava/util/Locale;";
const LD: &str = "Ljava/time/LocalDate;";
const LDT: &str = "Ljava/time/LocalDateTime;";
const INST: &str = "Ljava/time/Instant;";
const YM: &str = "Ljava/time/YearMonth;";
const MD: &str = "Ljava/time/MonthDay;";
const DUR: &str = "Ljava/time/Duration;";
const FMT: &str = "Ljava/time/format/DateTimeFormatter;";
const UNIT: &str = "Ljava/time/temporal/ChronoUnit;";

fn new_vm() -> Engine {
    Engine::new(Vec::new(), VmConfig::default())
}

fn obj(v: Value) -> ObjRef {
    let Value::Obj(r) = v else {
        panic!("expected Obj, got {v:?}");
    };
    r
}

fn jstr(vm: &mut Engine, s: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(s.to_string()).unwrap())
}

fn as_str(vm: &Engine, v: Value) -> String {
    vm.heap.as_str(obj(v)).unwrap().to_string()
}

fn call_s(vm: &mut Engine, cls: &str, m: &str, sig: &str, args: &[Value]) -> Value {
    vm.invoke_static(cls, m, sig, args).unwrap()
}

fn call_m(vm: &mut Engine, recv: ObjRef, m: &str, sig: &str, args: &[Value]) -> Value {
    vm.invoke_instance(recv, m, sig, args).unwrap()
}

fn new_date(vm: &mut Engine, ms: i64) -> ObjRef {
    let d = vm.heap.alloc_instance(DATE.into(), Vec::new()).unwrap();
    call_m(vm, d, "<init>", "(J)V", &[Value::Long(ms)]);
    d
}

fn assert_throws(res: Result<Value, VmExit>, expected_desc: &str) {
    match res {
        Err(VmExit::Exception(t)) => assert_eq!(
            t.class, expected_desc,
            "expected {expected_desc}, got {} ({:?})",
            t.class, t.message
        ),
        other => panic!("expected exception {expected_desc}, got {other:?}"),
    }
}

#[test]
fn date_equals_symmetry_and_null_npe_across_temporals() {
    let mut vm = new_vm();
    let (d1, d1_copy, d2, d0) = (
        new_date(&mut vm, 1_000),
        new_date(&mut vm, 1_000),
        new_date(&mut vm, 2_000),
        new_date(&mut vm, 0),
    );
    let inst = obj(call_s(
        &mut vm,
        INST,
        "ofEpochMilli",
        "(J)Ljava/time/Instant;",
        &[Value::Long(1_000)],
    ));
    let dur0 = obj(call_s(
        &mut vm,
        DUR,
        "ofMillis",
        "(J)Ljava/time/Duration;",
        &[Value::Long(0)],
    ));
    let ld0 = obj(call_s(
        &mut vm,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(1970), Value::Int(1), Value::Int(1)],
    ));
    let ym0 = obj(call_s(
        &mut vm,
        YM,
        "of",
        "(II)Ljava/time/YearMonth;",
        &[Value::Int(1970), Value::Int(1)],
    ));

    for non_date in [
        Value::Obj(inst),
        Value::Obj(dur0),
        Value::Obj(ld0),
        Value::Obj(ym0),
        Value::Null,
    ] {
        let base = if matches!(&non_date, Value::Obj(r) if *r == inst) {
            d1
        } else {
            d0
        };
        assert_eq!(
            call_m(
                &mut vm,
                base,
                "equals",
                "(Ljava/lang/Object;)Z",
                &[non_date]
            ),
            Value::Int(0)
        );
    }
    assert_eq!(
        call_m(
            &mut vm,
            inst,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(d1)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_m(
            &mut vm,
            d1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(d1_copy)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_m(
            &mut vm,
            d1,
            "equals",
            "(Ljava/lang/Object;)Z",
            &[Value::Obj(d2)]
        ),
        Value::Int(0)
    );

    for (m, sig) in [
        ("before", "(Ljava/util/Date;)Z"),
        ("after", "(Ljava/util/Date;)Z"),
        ("compareTo", "(Ljava/util/Date;)I"),
        ("compareTo", "(Ljava/lang/Object;)I"),
    ] {
        assert_throws(
            vm.invoke_instance(d1, m, sig, &[Value::Null]),
            "Ljava/lang/NullPointerException;",
        );
    }
    assert_throws(
        vm.invoke_instance(
            d1,
            "compareTo",
            "(Ljava/lang/Object;)I",
            &[Value::Obj(inst)],
        ),
        "Ljava/lang/ClassCastException;",
    );

    let md = obj(call_s(
        &mut vm,
        MD,
        "of",
        "(II)Ljava/time/MonthDay;",
        &[Value::Int(5), Value::Int(10)],
    ));
    for (recv, m, sig) in [
        (md, "compareTo", "(Ljava/time/MonthDay;)I"),
        (md, "isBefore", "(Ljava/time/MonthDay;)Z"),
        (md, "isAfter", "(Ljava/time/MonthDay;)Z"),
        (ld0, "isBefore", "(Ljava/time/chrono/ChronoLocalDate;)Z"),
        (ld0, "isAfter", "(Ljava/time/chrono/ChronoLocalDate;)Z"),
        (ld0, "isEqual", "(Ljava/time/chrono/ChronoLocalDate;)Z"),
        (ld0, "compareTo", "(Ljava/time/chrono/ChronoLocalDate;)I"),
        (inst, "compareTo", "(Ljava/time/Instant;)I"),
        (inst, "isBefore", "(Ljava/time/Instant;)Z"),
        (ym0, "compareTo", "(Ljava/time/YearMonth;)I"),
        (dur0, "compareTo", "(Ljava/time/Duration;)I"),
    ] {
        assert_throws(
            vm.invoke_instance(recv, m, sig, &[Value::Null]),
            "Ljava/lang/NullPointerException;",
        );
    }
}

#[test]
fn local_date_max_min_comparisons_and_checked_overflow() {
    let mut b = DexBuilder::new();
    let max_f = b.field_idx(LD, LD, "MAX");
    let min_f = b.field_idx(LD, LD, "MIN");
    let days_f = b.field_idx(UNIT, UNIT, "DAYS");
    let between_p = b.proto_idx(
        "J",
        vec![
            "Ljava/time/temporal/Temporal;".into(),
            "Ljava/time/temporal/Temporal;".into(),
        ],
    );
    let between_m = b.method_idx(UNIT, between_p, "between");
    let cls = b.class("LTest291;", "Ljava/lang/Object;");
    let mut units = Vec::new();
    units.extend(op21c(0x62, 0, days_f));
    units.extend(op21c(0x62, 1, min_f));
    units.extend(op21c(0x62, 2, max_f));
    units.extend(op35c(0x6e, 3, between_m, [0, 1, 2, 0, 0]));
    units.extend(op11x(0x0b, 3));
    units.extend(op11x(0x10, 3));
    let code = b.code(5, 0, 3, units);
    b.direct(
        cls,
        "spanDays",
        "J",
        vec![],
        ACC_PUBLIC | ACC_STATIC,
        Some(code),
    );
    let dex = rd_dex::Dex::parse(b.finish()).unwrap();
    let mut vm = Engine::new(vec![dex], VmConfig::default());

    assert_eq!(
        call_s(&mut vm, "LTest291;", "spanDays", "()J", &[]),
        Value::Long(730_484_999_633)
    );

    let ld_max = obj(call_s(
        &mut vm,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(999_999_999), Value::Int(12), Value::Int(31)],
    ));
    let ld_min = obj(call_s(
        &mut vm,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(-999_999_999), Value::Int(1), Value::Int(1)],
    ));
    let ld_2026 = obj(call_s(
        &mut vm,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(2026), Value::Int(1), Value::Int(1)],
    ));

    assert_eq!(
        call_m(
            &mut vm,
            ld_max,
            "isAfter",
            "(Ljava/time/chrono/ChronoLocalDate;)Z",
            &[Value::Obj(ld_2026)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call_m(
            &mut vm,
            ld_max,
            "isBefore",
            "(Ljava/time/chrono/ChronoLocalDate;)Z",
            &[Value::Obj(ld_2026)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call_m(
            &mut vm,
            ld_min,
            "isBefore",
            "(Ljava/time/chrono/ChronoLocalDate;)Z",
            &[Value::Obj(ld_2026)]
        ),
        Value::Int(1)
    );
    let max_str = call_m(&mut vm, ld_max, "toString", "()Ljava/lang/String;", &[]);
    assert_eq!(as_str(&vm, max_str), "+999999999-12-31");

    let pat = jstr(&mut vm, "yyyy-MM-dd");
    let fmt = obj(call_s(
        &mut vm,
        FMT,
        "ofPattern",
        "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
        &[pat],
    ));
    let max_fmt = call_m(
        &mut vm,
        ld_max,
        "format",
        "(Ljava/time/format/DateTimeFormatter;)Ljava/lang/String;",
        &[Value::Obj(fmt)],
    );
    assert_eq!(as_str(&vm, max_fmt), "999999999-12-31");

    assert_throws(
        vm.invoke_instance(
            ld_max,
            "plusDays",
            "(J)Ljava/time/LocalDate;",
            &[Value::Long(1)],
        ),
        "Ljava/time/DateTimeException;",
    );
    assert_throws(
        vm.invoke_instance(
            ld_2026,
            "plusDays",
            "(J)Ljava/time/LocalDate;",
            &[Value::Long(i64::MAX)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_throws(
        vm.invoke_instance(ld_max, "atStartOfDay", "()Ljava/time/LocalDateTime;", &[]),
        "Ljava/lang/ArithmeticException;",
    );

    let inst_max = obj(call_s(
        &mut vm,
        INST,
        "ofEpochMilli",
        "(J)Ljava/time/Instant;",
        &[Value::Long(i64::MAX)],
    ));
    assert_throws(
        vm.invoke_instance(
            inst_max,
            "plusMillis",
            "(J)Ljava/time/Instant;",
            &[Value::Long(1)],
        ),
        "Ljava/lang/ArithmeticException;",
    );
    assert_throws(
        vm.invoke_static(
            DUR,
            "ofDays",
            "(J)Ljava/time/Duration;",
            &[Value::Long(i64::MAX)],
        ),
        "Ljava/lang/ArithmeticException;",
    );

    let a_far = call_s(
        &mut vm,
        INST,
        "ofEpochMilli",
        "(J)Ljava/time/Instant;",
        &[Value::Long(-5_000_000_000_000_000_000)],
    );
    let b_far = call_s(
        &mut vm,
        INST,
        "ofEpochMilli",
        "(J)Ljava/time/Instant;",
        &[Value::Long(5_000_000_000_000_000_000)],
    );
    let sec_name = jstr(&mut vm, "SECONDS");
    let sec_unit = vm
        .heap
        .alloc_instance(UNIT.into(), vec![("name".into(), sec_name)])
        .unwrap();
    assert_eq!(
        call_m(
            &mut vm,
            sec_unit,
            "between",
            "(Ljava/time/temporal/Temporal;Ljava/time/temporal/Temporal;)J",
            &[a_far.clone(), b_far.clone()]
        ),
        Value::Long(10_000_000_000_000_000)
    );
    let ms_name = jstr(&mut vm, "MILLIS");
    let ms_unit = vm
        .heap
        .alloc_instance(UNIT.into(), vec![("name".into(), ms_name)])
        .unwrap();
    assert_throws(
        vm.invoke_instance(
            ms_unit,
            "between",
            "(Ljava/time/temporal/Temporal;Ljava/time/temporal/Temporal;)J",
            &[a_far, b_far],
        ),
        "Ljava/lang/ArithmeticException;",
    );
}

#[test]
fn instant_nano_adjustment_and_ldt_7_arg_nanos_are_preserved() {
    let mut vm = new_vm();
    for (sec, nanos, expected_ms) in [
        (10_i64, 500_000_000_i64, 10_500_i64),
        (10, -500_000_000, 9_500),
        (10, 2_250_000_000, 12_250),
    ] {
        let inst = obj(call_s(
            &mut vm,
            INST,
            "ofEpochSecond",
            "(JJ)Ljava/time/Instant;",
            &[Value::Long(sec), Value::Long(nanos)],
        ));
        assert_eq!(
            call_m(&mut vm, inst, "toEpochMilli", "()J", &[]),
            Value::Long(expected_ms)
        );
    }

    let ldt = obj(call_s(
        &mut vm,
        LDT,
        "of",
        "(IIIIIII)Ljava/time/LocalDateTime;",
        &[
            Value::Int(1970),
            Value::Int(1),
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
            Value::Int(10),
            Value::Int(750_000_000),
        ],
    ));
    let ldt_inst = obj(call_m(
        &mut vm,
        ldt,
        "toInstant",
        "()Ljava/time/Instant;",
        &[],
    ));
    assert_eq!(
        call_m(&mut vm, ldt_inst, "toEpochMilli", "()J", &[]),
        Value::Long(10_750)
    );
}

#[test]
fn calendar_bounds_validation_throws_date_time_exception() {
    let mut vm = new_vm();
    assert!(vm
        .invoke_static(
            LD,
            "of",
            "(III)Ljava/time/LocalDate;",
            &[Value::Int(2024), Value::Int(2), Value::Int(29)]
        )
        .is_ok());

    for (y, m, d) in [
        (2025, 2, 29),
        (2026, 13, 40),
        (2026, 0, 10),
        (2026, 4, 31),
        (2026, 1, 0),
        (1_000_000_000, 1, 1),
    ] {
        assert_throws(
            vm.invoke_static(
                LD,
                "of",
                "(III)Ljava/time/LocalDate;",
                &[Value::Int(y), Value::Int(m), Value::Int(d)],
            ),
            "Ljava/time/DateTimeException;",
        );
    }

    let apr15 = obj(call_s(
        &mut vm,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(2026), Value::Int(4), Value::Int(15)],
    ));
    assert_throws(
        vm.invoke_instance(
            apr15,
            "withDayOfMonth",
            "(I)Ljava/time/LocalDate;",
            &[Value::Int(31)],
        ),
        "Ljava/time/DateTimeException;",
    );
    assert_throws(
        vm.invoke_static(
            YM,
            "of",
            "(II)Ljava/time/YearMonth;",
            &[Value::Int(2026), Value::Int(13)],
        ),
        "Ljava/time/DateTimeException;",
    );

    let feb2025 = obj(call_s(
        &mut vm,
        YM,
        "of",
        "(II)Ljava/time/YearMonth;",
        &[Value::Int(2025), Value::Int(2)],
    ));
    assert_throws(
        vm.invoke_instance(
            feb2025,
            "atDay",
            "(I)Ljava/time/LocalDate;",
            &[Value::Int(29)],
        ),
        "Ljava/time/DateTimeException;",
    );

    assert_throws(
        vm.invoke_static(
            LDT,
            "of",
            "(IIIII)Ljava/time/LocalDateTime;",
            &[
                Value::Int(2026),
                Value::Int(1),
                Value::Int(1),
                Value::Int(24),
                Value::Int(0),
            ],
        ),
        "Ljava/time/DateTimeException;",
    );
    assert_throws(
        vm.invoke_static(
            LDT,
            "of",
            "(IIIIIII)Ljava/time/LocalDateTime;",
            &[
                Value::Int(2026),
                Value::Int(1),
                Value::Int(1),
                Value::Int(12),
                Value::Int(0),
                Value::Int(0),
                Value::Int(1_000_000_000),
            ],
        ),
        "Ljava/time/DateTimeException;",
    );
}

#[test]
fn date_time_formatter_null_npe_and_vm_locale_determinism() {
    let mut vm_en = new_vm();
    assert_throws(
        vm_en.invoke_static(
            FMT,
            "ofPattern",
            "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let pat_en = jstr(&mut vm_en, "MMM/yy");
    assert_throws(
        vm_en.invoke_static(
            FMT,
            "ofPattern",
            "(Ljava/lang/String;Ljava/util/Locale;)Ljava/time/format/DateTimeFormatter;",
            &[pat_en.clone(), Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let fmt_en = obj(call_s(
        &mut vm_en,
        FMT,
        "ofPattern",
        "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
        &[pat_en],
    ));
    assert_throws(
        vm_en.invoke_instance(
            fmt_en,
            "withLocale",
            "(Ljava/util/Locale;)Ljava/time/format/DateTimeFormatter;",
            &[Value::Null],
        ),
        "Ljava/lang/NullPointerException;",
    );
    let oct_en = call_s(
        &mut vm_en,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(2026), Value::Int(10), Value::Int(19)],
    );
    let out_en = call_m(
        &mut vm_en,
        fmt_en,
        "format",
        "(Ljava/time/temporal/TemporalAccessor;)Ljava/lang/String;",
        &[oct_en],
    );
    assert_eq!(as_str(&vm_en, out_en), "Oct/26");

    let mut vm_pt = new_vm();
    let tag = jstr(&mut vm_pt, "pt-BR");
    let pt_loc = call_s(
        &mut vm_pt,
        LOCALE,
        "forLanguageTag",
        "(Ljava/lang/String;)Ljava/util/Locale;",
        &[tag],
    );
    call_s(
        &mut vm_pt,
        LOCALE,
        "setDefault",
        "(Ljava/util/Locale;)V",
        &[pt_loc],
    );
    let pat_pt = jstr(&mut vm_pt, "MMM/yy");
    let fmt_pt = obj(call_s(
        &mut vm_pt,
        FMT,
        "ofPattern",
        "(Ljava/lang/String;)Ljava/time/format/DateTimeFormatter;",
        &[pat_pt],
    ));
    let oct_pt = call_s(
        &mut vm_pt,
        LD,
        "of",
        "(III)Ljava/time/LocalDate;",
        &[Value::Int(2026), Value::Int(10), Value::Int(19)],
    );
    let out_pt = call_m(
        &mut vm_pt,
        fmt_pt,
        "format",
        "(Ljava/time/temporal/TemporalAccessor;)Ljava/lang/String;",
        &[oct_pt],
    );
    assert_eq!(as_str(&vm_pt, out_pt), "out./26");
}

#[test]
fn format_pattern_tokens_and_calendar_dow_in_month_and_hour_roll() {
    let mut vm = new_vm();
    let sdf_default = vm
        .heap
        .alloc_instance(SIMPLE_DATE_FORMAT.into(), Vec::new())
        .unwrap();
    call_m(&mut vm, sdf_default, "<init>", "()V", &[]);
    let d0 = new_date(&mut vm, 0);
    let out_d0 = call_m(
        &mut vm,
        sdf_default,
        "format",
        "(Ljava/util/Date;)Ljava/lang/String;",
        &[Value::Obj(d0)],
    );
    assert_eq!(as_str(&vm, out_d0), "1970.01.01 AD at 00:00:00 UTC");

    let d_450 = new_date(&mut vm, 450);
    let sdf_ext = vm
        .heap
        .alloc_instance(SIMPLE_DATE_FORMAT.into(), Vec::new())
        .unwrap();
    let p_ext = jstr(&mut vm, "HH:mm:ss.SSSSSS Z XXX");
    call_m(
        &mut vm,
        sdf_ext,
        "<init>",
        "(Ljava/lang/String;)V",
        &[p_ext],
    );
    let out_ext = call_m(
        &mut vm,
        sdf_ext,
        "format",
        "(Ljava/util/Date;)Ljava/lang/String;",
        &[Value::Obj(d_450)],
    );
    assert_eq!(as_str(&vm, out_ext), "00:00:00.450000 +0000 Z");

    // 2026-10-01 is Thursday (DAY_OF_WEEK == 5); Thursdays in Oct 2026: 1, 8, 15, 22, 29.
    let cal = obj(call_s(
        &mut vm,
        CALENDAR,
        "getInstance",
        "()Ljava/util/Calendar;",
        &[],
    ));
    let oct_1_1030_am = 20_727_i64 * 86_400_000 + 10 * 3_600_000 + 30 * 60_000;
    call_m(
        &mut vm,
        cal,
        "setTimeInMillis",
        "(J)V",
        &[Value::Long(oct_1_1030_am)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(7)]),
        Value::Int(5)
    );
    call_m(
        &mut vm,
        cal,
        "set",
        "(II)V",
        &[Value::Int(8), Value::Int(3)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(5)]),
        Value::Int(15)
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(8)]),
        Value::Int(3)
    );
    call_m(
        &mut vm,
        cal,
        "set",
        "(II)V",
        &[Value::Int(8), Value::Int(-1)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(5)]),
        Value::Int(29)
    );

    // Calendar.roll(HOUR, 3) wraps 0..11 preserving AM_PM; HOUR_OF_DAY wraps 0..23.
    call_m(
        &mut vm,
        cal,
        "setTimeInMillis",
        "(J)V",
        &[Value::Long(oct_1_1030_am)],
    );
    call_m(
        &mut vm,
        cal,
        "roll",
        "(II)V",
        &[Value::Int(10), Value::Int(3)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(9)]),
        Value::Int(0)
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(10)]),
        Value::Int(1)
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(11)]),
        Value::Int(1)
    );

    let oct_1_1030_pm = oct_1_1030_am + 12 * 3_600_000;
    call_m(
        &mut vm,
        cal,
        "setTimeInMillis",
        "(J)V",
        &[Value::Long(oct_1_1030_pm)],
    );
    call_m(
        &mut vm,
        cal,
        "roll",
        "(II)V",
        &[Value::Int(10), Value::Int(3)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(9)]),
        Value::Int(1)
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(11)]),
        Value::Int(13)
    );

    call_m(
        &mut vm,
        cal,
        "setTimeInMillis",
        "(J)V",
        &[Value::Long(oct_1_1030_pm)],
    );
    call_m(
        &mut vm,
        cal,
        "roll",
        "(II)V",
        &[Value::Int(11), Value::Int(3)],
    );
    assert_eq!(
        call_m(&mut vm, cal, "get", "(I)I", &[Value::Int(11)]),
        Value::Int(1)
    );
}
