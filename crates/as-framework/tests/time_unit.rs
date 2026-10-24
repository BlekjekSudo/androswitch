//! Actual JVM vectors exercised through ordinary portable DEX methods.
#![cfg(feature = "host")]
use as_framework::App;
use as_platform::{host_fs::HostFileSystem, Clock};
use rd_vm::{framework_host::HostMethodKind, Value, VmExit};
use std::{path::PathBuf, rc::Rc};

const CASES: &str = "Lorg/androswitch/timeunits/TimeUnitCases;";
const UNIT: &str = "Ljava/util/concurrent/TimeUnit;";
struct Time;
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        0
    }
}
struct Test {
    app: App,
    root: PathBuf,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(variant: &str, launch: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-time-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/time-unit-fixture/{variant}.apk"));
        let record = pm
            .install(
                std::fs::read(apk).expect("run fixtures/m9/time-unit/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let mut app = App::new(
            pm.load_apk(&record.package, None).unwrap().1,
            fs,
            Rc::new(Time),
            960,
            540,
            None,
        )
        .unwrap();
        if launch {
            app.launch().unwrap();
        }
        Self { app, root }
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.app.vm.invoke_static(CASES, name, sig, args).unwrap()
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/time-unit-fixture/{name}")),
    )
    .unwrap()
}

#[test]
fn all_units_truncation_saturation_and_enum_contracts_match_1995_jvm_rows() {
    let oracle = fixture("oracle.tsv");
    assert_eq!(oracle.lines().count(), 1995);
    for variant in ["time-unit", "renamed"] {
        let mut test = Test::new(variant, false);
        for (row, line) in oracle.lines().enumerate() {
            let fields: Vec<_> = line.split('\t').collect();
            let int = |index: usize| fields[index].parse::<i32>().unwrap();
            let actual = match fields[0] {
                "C" | "T" => test
                    .call(
                        if fields[0] == "C" {
                            "convertCase"
                        } else {
                            "directCase"
                        },
                        "(IIJ)J",
                        &[
                            Value::Int(int(1)),
                            Value::Int(int(2)),
                            Value::Long(fields[3].parse().unwrap()),
                        ],
                    )
                    .as_long()
                    .unwrap()
                    .to_string(),
                "P" => test
                    .call(
                        "compare",
                        "(II)I",
                        &[Value::Int(int(1)), Value::Int(int(2))],
                    )
                    .as_int()
                    .unwrap()
                    .to_string(),
                "N" => {
                    let name = test.call("name", "(I)Ljava/lang/String;", &[Value::Int(int(1))]);
                    test.app
                        .vm
                        .heap
                        .as_str(name.as_ref().unwrap().unwrap())
                        .unwrap()
                        .to_string()
                }
                "O" | "I" => test
                    .call(
                        if fields[0] == "O" {
                            "ordinal"
                        } else {
                            "identity"
                        },
                        "(I)I",
                        &[Value::Int(int(1))],
                    )
                    .as_int()
                    .unwrap()
                    .to_string(),
                "A" => test
                    .call("freshArray", "()I", &[])
                    .as_int()
                    .unwrap()
                    .to_string(),
                "E" => test
                    .call("error", "(I)I", &[Value::Int(int(1))])
                    .as_int()
                    .unwrap()
                    .to_string(),
                _ => panic!("unknown JVM oracle row {line}"),
            };
            assert_eq!(
                actual,
                *fields.last().unwrap(),
                "{variant} JVM row {row}: {line}"
            );
            if row.is_multiple_of(64) {
                test.app.vm.gc();
            }
        }
    }
}

#[test]
fn canonical_constants_remain_gc_roots_without_dex_statics_and_catalog_is_exact() {
    let mut test = Test::new("time-unit", false);
    let host = test.app.vm.framework_host().unwrap();
    let names = [
        "NANOSECONDS",
        "MICROSECONDS",
        "MILLISECONDS",
        "SECONDS",
        "MINUTES",
        "HOURS",
        "DAYS",
    ];
    let mut references = Vec::new();
    for name in names {
        let reference = host
            .static_field(&mut test.app.vm, UNIT, name, UNIT)
            .unwrap()
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        assert!(!references.contains(&reference));
        references.push(reference);
    }
    test.app.vm.gc();
    for (index, reference) in references.iter().copied().enumerate() {
        let name = test
            .app
            .vm
            .invoke_instance(reference, "name", "()Ljava/lang/String;", &[])
            .unwrap();
        assert_eq!(
            test.app
                .vm
                .heap
                .as_str(name.as_ref().unwrap().unwrap())
                .unwrap(),
            names[index]
        );
        assert_eq!(
            host.static_field(&mut test.app.vm, UNIT, names[index], UNIT)
                .unwrap(),
            Some(Value::Obj(reference))
        );
    }
    assert!(host
        .static_field(&mut test.app.vm, UNIT, "WEEKS", UNIT)
        .unwrap()
        .is_none());
    assert!(host
        .static_field(&mut test.app.vm, UNIT, "SECONDS", "I")
        .unwrap()
        .is_none());
    assert_eq!(
        host.method_metadata(UNIT, "values", "()[Ljava/util/concurrent/TimeUnit;"),
        Some(HostMethodKind::Static)
    );
    assert_eq!(
        host.method_metadata(
            UNIT,
            "valueOf",
            "(Ljava/lang/String;)Ljava/util/concurrent/TimeUnit;"
        ),
        Some(HostMethodKind::Static)
    );
    for method in [
        "toNanos",
        "toMicros",
        "toMillis",
        "toSeconds",
        "toMinutes",
        "toHours",
        "toDays",
    ] {
        assert_eq!(
            host.method_metadata(UNIT, method, "(J)J"),
            Some(HostMethodKind::Instance)
        );
    }
    assert_eq!(
        host.method_metadata(UNIT, "convert", "(JLjava/util/concurrent/TimeUnit;)J"),
        Some(HostMethodKind::Instance)
    );
    assert!(host.method_metadata(UNIT, "sleep", "(J)V").is_none());
    assert!(host
        .method_metadata(UNIT, "convert", "(Ljava/time/Duration;)J")
        .is_none());
    assert!(host.method_metadata(UNIT, "toNanos", "(I)J").is_none());
    let forged = test
        .app
        .vm
        .heap
        .alloc_instance(UNIT.into(), vec![])
        .unwrap();
    assert!(matches!(
        host.call(&mut test.app.vm, UNIT, "toNanos", "(J)J", forged, &[Value::Long(1)]),
        Err(VmExit::Error(error)) if error.code == "TIME_UNIT_STATE_UNINITIALIZED"
    ));
    // The generic Enum discovery path has no host constant registry. It must
    // identify that missing capability, not claim an existing constant is absent.
    let class = test.app.vm.device_class_object(UNIT).unwrap();
    let name = test.app.vm.heap.alloc_string("SECONDS".into()).unwrap();
    assert!(matches!(
        test.app.vm.invoke_static(
            "Ljava/lang/Enum;",
            "valueOf",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;",
            &[Value::Obj(class), Value::Obj(name)],
        ),
        Err(VmExit::Error(error)) if error.code == "API_UNAVAILABLE"
            && error.cause.contains("host enum constant registry")
    ));
}

#[test]
fn normal_sdk_activity_writes_exact_jvm_numeric_proof_for_both_manifest_identities() {
    let expected = fixture("proof.txt");
    assert!(expected.ends_with("TIME_UNIT_PROOF_PASS\n"));
    for variant in ["time-unit", "renamed"] {
        let mut test = Test::new(variant, true);
        let vfs = test.app.vm.guest_filesystem().unwrap();
        let path = "/sdcard/Download/androswitch-timeunit-proof.txt";
        assert_eq!(
            vfs.stat(path).unwrap().unwrap().length,
            expected.len() as u64
        );
        let mut bytes = vec![0; expected.len() + 1];
        let read = vfs.read_at(path, 0, &mut bytes).unwrap();
        assert_eq!(&bytes[..read], expected.as_bytes());
        test.app.vm.gc();
        assert_eq!(test.call("freshArray", "()I", &[]), Value::Int(1));
    }
}
