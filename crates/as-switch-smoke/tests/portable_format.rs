//! Explicitly test the no_std Engine; no upstream std-unifying dev dependency.
use rd_vm::{heap::ElemKind, Engine, Value, VmConfig, VmExit};
const PROBE: &str = "Lorg/androswitch/vm/FormatFixture;";
#[allow(clippy::assertions_on_constants)]
fn vm() -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-format-fixture");
    let dex = rd_dex::Dex::parse(
        std::fs::read(root.join("dex/classes.dex"))
            .expect("run crates/rd-vm/tests/fixtures/portable-format/build.ps1"),
    )
    .unwrap();
    Engine::new(vec![dex], VmConfig::default())
}
fn string(vm: &mut Engine, text: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(text.into()).unwrap())
}
fn boxed(vm: &mut Engine, class: &str, value: Value) -> Value {
    Value::Obj(
        vm.heap
            .alloc_instance(
                format!("Ljava/lang/{class};"),
                vec![("value".into(), value)],
            )
            .unwrap(),
    )
}
fn int(vm: &mut Engine, value: i32) -> Value {
    boxed(vm, "Integer", Value::Int(value))
}
fn array(vm: &mut Engine, items: Option<Vec<Value>>) -> Value {
    let Some(items) = items else {
        return Value::Null;
    };
    let r = vm
        .heap
        .alloc_array(ElemKind::Obj, items.len(), "Ljava/lang/Object;")
        .unwrap();
    *vm.heap.array_elems_mut(r).unwrap() = items;
    Value::Obj(r)
}
fn render(vm: &mut Engine, pattern: &str, items: Option<Vec<Value>>) -> Result<Value, VmExit> {
    let pattern = string(vm, pattern);
    let items = array(vm, items);
    vm.invoke_static(
        PROBE,
        "render",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        &[pattern, items],
    )
}
#[test]
fn unchanged_dex_formatter_matches_jvm_for_24_cases() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-format-fixture");
    let expected = std::fs::read_to_string(root.join("jvm-oracle.txt")).unwrap();
    let expected: Vec<_> = expected.lines().collect();
    assert_eq!(expected.len(), 24);
    let mut vm = vm();
    for round in 0..2 {
        for (index, expected) in expected.iter().enumerate() {
            let (pattern, items) = match index {
                0 => (
                    "Values: %d,%d,%d,%d,%d,%d",
                    Some(
                        [8, 8, 8, 0, 24, 0]
                            .into_iter()
                            .map(|n| int(&mut vm, n))
                            .collect(),
                    ),
                ),
                1 => (
                    "%+06d|%(7d|% d",
                    Some([42, -42, 42].into_iter().map(|n| int(&mut vm, n)).collect()),
                ),
                2 => (
                    "%#08x|%#06o|%X",
                    Some([42, 9, -1].into_iter().map(|n| int(&mut vm, n)).collect()),
                ),
                3 => (
                    "%x|%o|%x",
                    Some(vec![
                        boxed(&mut vm, "Byte", Value::Int(-1)),
                        boxed(&mut vm, "Short", Value::Int(-1)),
                        boxed(&mut vm, "Long", Value::Long(-1)),
                    ]),
                ),
                4 => (
                    "%2$s:%s:%<s:%1$d",
                    Some(vec![int(&mut vm, 7), string(&mut vm, "item")]),
                ),
                5 => (
                    "[%-8.3s]|%S|%.0s",
                    Some(
                        ["abcdef", "upper", "gone"]
                            .into_iter()
                            .map(|s| string(&mut vm, s))
                            .collect(),
                    ),
                ),
                6 => (
                    "%b:%B:%b:%b",
                    Some(vec![
                        Value::Null,
                        boxed(&mut vm, "Boolean", Value::Int(0)),
                        string(&mut vm, "object"),
                        int(&mut vm, 0),
                    ]),
                ),
                7 => (
                    "%c:%C:%c",
                    Some(vec![
                        boxed(&mut vm, "Character", Value::Int('z' as i32)),
                        boxed(&mut vm, "Character", Value::Int('q' as i32)),
                        int(&mut vm, 0x1f600),
                    ]),
                ),
                8 => ("%5%:%n%s", Some(vec![string(&mut vm, "line")])),
                9 => ("%05d:%X:%C:%s", Some(vec![Value::Null; 4])),
                10 => (
                    "%d",
                    Some(vec![boxed(&mut vm, "Long", Value::Long(i64::MIN))]),
                ),
                11 => ("%3$s:%s", None),
                12 => ("%d", Some(vec![string(&mut vm, "wrong")])),
                13 => ("%s:%s", Some(vec![string(&mut vm, "one")])),
                14 => ("%--5d", Some(vec![int(&mut vm, 1)])),
                15 => ("%-d", Some(vec![int(&mut vm, 1)])),
                16 => ("%#d", Some(vec![int(&mut vm, 1)])),
                17 => ("%.2d", Some(vec![int(&mut vm, 1)])),
                18 => ("%q", Some(vec![int(&mut vm, 1)])),
                19 => ("%<s", Some(vec![string(&mut vm, "one")])),
                20 => ("%+ d", Some(vec![int(&mut vm, 1)])),
                21 => ("%c", Some(vec![int(&mut vm, -1)])),
                22 => ("%5n", Some(vec![])),
                23 => ("%+n", Some(vec![])),
                _ => unreachable!(),
            };
            let actual = match render(&mut vm, pattern, items) {
                Ok(Value::Obj(r)) => format!(
                    "S{}",
                    vm.heap
                        .as_str(r)
                        .unwrap()
                        .replace('\\', "\\\\")
                        .replace('\n', "\\n")
                        .replace('\r', "\\r")
                ),
                Err(VmExit::Exception(error)) => format!(
                    "E{}",
                    error
                        .class
                        .trim_start_matches("Ljava/util/")
                        .trim_end_matches(';')
                ),
                other => panic!("case{index} unexpected {other:?}"),
            };
            assert_eq!(&actual, expected, "case{index} round{round} {pattern}");
        }
        vm.gc();
    }
}
#[test]
fn formatter_bounds_unsupported_contracts_and_dex_callback_gc() {
    let mut vm = vm();
    for pattern in ["%1000000000s", "%999999999999999999999999s", "%.70000s"] {
        assert!(
            matches!(render(&mut vm,pattern,Some(vec![Value::Null])),Err(VmExit::Error(e)) if e.code=="FORMAT_BUDGET")
        );
    }
    let big = string(&mut vm, &"x".repeat(40000));
    assert!(
        matches!(render(&mut vm,"%1$s%1$s",Some(vec![big])),Err(VmExit::Error(e)) if e.code=="FORMAT_BUDGET")
    );
    assert!(
        matches!(render(&mut vm,"%s",Some(vec![Value::Null;4097])),Err(VmExit::Error(e)) if e.code=="FORMAT_BUDGET")
    );
    for pattern in ["%f", "%tY", "%,d"] {
        let n = int(&mut vm, 1);
        assert!(
            matches!(render(&mut vm,pattern,Some(vec![n])),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
        );
    }
    let fancy = vm
        .heap
        .alloc_instance("Lorg/androswitch/vm/FormatFixture$Fancy;".into(), vec![])
        .unwrap();
    assert!(
        matches!(render(&mut vm,"%s",Some(vec![Value::Obj(fancy)])),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
    let result = render(&mut vm, "%b", Some(vec![Value::Obj(fancy)])).unwrap();
    assert_eq!(
        vm.heap.as_str(result.as_ref().unwrap().unwrap()).unwrap(),
        "true"
    );
    assert!(
        matches!(rd_vm::intrinsics::call_static_intrinsic(&mut vm,"Ljava/lang/String;","format","(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",&[]),Err(VmExit::Error(e)) if e.code=="INVALID_ARG")
    );
    assert!(
        matches!(vm.invoke_static(PROBE,"render","(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",&[Value::Null,Value::Null]),Err(VmExit::Exception(error)) if error.class=="Ljava/lang/NullPointerException;")
    );
    let value = vm
        .heap
        .alloc_instance("Lorg/androswitch/vm/FormatFixture$Named;".into(), vec![])
        .unwrap();
    let result = vm
        .invoke_static(
            PROBE,
            "callback",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            &[Value::Obj(value)],
        )
        .unwrap();
    assert_eq!(
        vm.heap.as_str(result.as_ref().unwrap().unwrap()).unwrap(),
        "<real DEX callback>"
    );
    let pattern = string(&mut vm, "%d");
    let n = int(&mut vm, 26);
    let args = array(&mut vm, Some(vec![n]));
    let result = vm
        .invoke_static(
            PROBE,
            "unlocalized",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
            &[pattern, args],
        )
        .unwrap();
    assert_eq!(
        vm.heap.as_str(result.as_ref().unwrap().unwrap()).unwrap(),
        "26"
    );
    for (method, expected) in [
        ("nullArray", "null:null"),
        ("literalNull", "null/null/false"),
    ] {
        let result = vm
            .invoke_static(PROBE, method, "()Ljava/lang/String;", &[])
            .unwrap();
        assert_eq!(
            vm.heap.as_str(result.as_ref().unwrap().unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn math_min_max_match_1024_jvm_observations_including_signed_zero_and_nan() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-format-fixture");
    let oracle = std::fs::read_to_string(root.join("math-oracle.tsv")).unwrap();
    assert_eq!(oracle.lines().count(), 1024);
    let mut vm = vm();
    for _ in 0..2 {
        for line in oracle.lines() {
            let row: Vec<_> = line.split('\t').collect();
            let number = |i: usize| row[i].parse::<i64>().unwrap();
            let (sig, a, b) = match row[0] {
                "ints" => (
                    "(III)I",
                    Value::Int(number(2) as i32),
                    Value::Int(number(3) as i32),
                ),
                "longs" => ("(IJJ)J", Value::Long(number(2)), Value::Long(number(3))),
                "floats" => (
                    "(IFF)F",
                    Value::Float(f32::from_bits(number(2) as u32)),
                    Value::Float(f32::from_bits(number(3) as u32)),
                ),
                "doubles" => (
                    "(IDD)D",
                    Value::Double(f64::from_bits(number(2) as u64)),
                    Value::Double(f64::from_bits(number(3) as u64)),
                ),
                _ => panic!("unknown JVM type"),
            };
            let result = vm
                .invoke_static(
                    "Lorg/androswitch/vm/MathFixture;",
                    row[0],
                    sig,
                    &[Value::Int(number(1) as i32), a, b],
                )
                .unwrap();
            let bits = match result {
                Value::Int(x) => i64::from(x),
                Value::Long(x) => x,
                Value::Float(x) => i64::from(x.to_bits() as i32),
                Value::Double(x) => x.to_bits() as i64,
                _ => panic!("invalid numeric result"),
            };
            assert_eq!(bits, number(4), "{line}");
        }
        vm.gc();
    }
    assert!(vm
        .invoke_static(
            "Ljava/lang/Math;",
            "max",
            "(II)J",
            &[Value::Int(1), Value::Int(2)]
        )
        .is_err());
}
