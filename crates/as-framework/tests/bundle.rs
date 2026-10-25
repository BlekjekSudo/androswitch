//! Actual DEX/framework calls; this test does not claim Parcel or Binder support.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{Value, VmExit};
use std::{path::PathBuf, rc::Rc};
const PROBE: &str = "Lorg/androswitch/contracts/BundleActivity;";
#[allow(clippy::assertions_on_constants)]
fn app(fixture: &str) -> App {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-bundle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/bundle-fixture/{fixture}.apk"));
    let record = pm
        .install(
            std::fs::read(artifact).expect("run fixtures/m9/bundle/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    app.launch().unwrap();
    app
}
fn invoke(app: &mut App, name: &str, signature: &str, args: &[Value]) -> Result<Value, VmExit> {
    app.vm.invoke_static(PROBE, name, signature, args)
}
fn call(app: &mut App, name: &str, signature: &str, args: &[Value]) -> Value {
    invoke(app, name, signature, args).unwrap()
}
fn key(value: &str) -> Value {
    Value::StrPlaceholder(value.into())
}
fn text(app: &App, value: &Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn put(app: &mut App, suffix: &str, descriptor: &str, key: Value, value: Value) {
    call(
        app,
        &format!("put{suffix}"),
        &format!("(Ljava/lang/String;{descriptor})V"),
        &[key, value],
    );
}
fn get(app: &mut App, suffix: &str, descriptor: &str, key: Value) -> Value {
    call(
        app,
        &format!("get{suffix}"),
        &format!("(Ljava/lang/String;){descriptor}"),
        &[key],
    )
}
fn fallback(app: &mut App, suffix: &str, descriptor: &str, key: Value, fallback: Value) -> Value {
    call(
        app,
        &format!("default{suffix}"),
        &format!("(Ljava/lang/String;{descriptor}){descriptor}"),
        &[key, fallback],
    )
}

#[test]
fn real_dex_preserves_primitive_bits_and_wide_values_in_both_packages() {
    for fixture in ["bundle", "renamed"] {
        let mut app = app(fixture);
        for (suffix, descriptor, value) in [
            ("Boolean", "Z", Value::Int(1)),
            ("Byte", "B", Value::Int(-128)),
            ("Char", "C", Value::Int(65535)),
            ("Short", "S", Value::Int(-32768)),
            ("Int", "I", Value::Int(i32::MIN)),
            ("Long", "J", Value::Long(i64::MIN)),
            ("Float", "F", Value::Float(-123.75)),
            ("Double", "D", Value::Double(1.234567890123456e120)),
        ] {
            put(&mut app, suffix, descriptor, key(suffix), value.clone());
            assert_eq!(get(&mut app, suffix, descriptor, key(suffix)), value);
        }
        for value in [i64::MAX, 0x1234_5678_9abc_def0, -1] {
            put(&mut app, "Long", "J", key("wide"), Value::Long(value));
            assert_eq!(get(&mut app, "Long", "J", key("wide")), Value::Long(value));
            assert_eq!(
                call(
                    &mut app,
                    "baseLong",
                    "(Ljava/lang/String;J)J",
                    &[key("wide"), Value::Long(7)]
                ),
                Value::Long(value)
            );
        }
        for bits in [0x8000_0000, 0x7f80_0000, 0x7fc0_1234] {
            put(
                &mut app,
                "Float",
                "F",
                key("bits"),
                Value::Float(f32::from_bits(bits)),
            );
            assert_eq!(
                get(&mut app, "Float", "F", key("bits"))
                    .as_float()
                    .unwrap()
                    .to_bits(),
                bits
            );
        }
        for bits in [
            0x8000_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0x7ff8_0000_0000_1234,
        ] {
            put(
                &mut app,
                "Double",
                "D",
                key("bits"),
                Value::Double(f64::from_bits(bits)),
            );
            assert_eq!(
                get(&mut app, "Double", "D", key("bits"))
                    .as_double()
                    .unwrap()
                    .to_bits(),
                bits
            );
        }
    }
}

#[test]
fn exact_types_defaults_null_keys_and_mutations_follow_bundle_contract() {
    let mut app = app("bundle");
    assert_eq!(call(&mut app, "empty", "()Z", &[]), Value::Int(1));
    put(&mut app, "Int", "I", key("value"), Value::Int(123));
    // All four of these use int register values, but Android stores distinct boxed types.
    for (suffix, descriptor) in [
        ("Boolean", "Z"),
        ("Byte", "B"),
        ("Char", "C"),
        ("Short", "S"),
    ] {
        assert_eq!(
            get(&mut app, suffix, descriptor, key("value")),
            Value::Int(0)
        );
    }
    assert_eq!(
        fallback(&mut app, "Long", "J", key("value"), Value::Long(-9)),
        Value::Long(-9)
    );
    assert_eq!(
        fallback(&mut app, "Int", "I", key("missing"), Value::Int(77)),
        Value::Int(77)
    );
    assert_eq!(
        get(&mut app, "Float", "F", key("missing")),
        Value::Float(0.)
    );
    assert_eq!(
        get(&mut app, "Double", "D", key("missing")),
        Value::Double(0.)
    );
    put(
        &mut app,
        "String",
        "Ljava/lang/String;",
        Value::Null,
        Value::Null,
    );
    assert_eq!(
        call(
            &mut app,
            "contains",
            "(Ljava/lang/String;)Z",
            &[Value::Null]
        ),
        Value::Int(1)
    );
    let value = fallback(
        &mut app,
        "String",
        "Ljava/lang/String;",
        Value::Null,
        key("fallback"),
    );
    assert_eq!(text(&app, &value), "fallback");
    assert_eq!(
        get(&mut app, "String", "Ljava/lang/String;", Value::Null),
        Value::Null
    );
    put(&mut app, "Long", "J", Value::Null, Value::Long(i64::MAX));
    assert_eq!(
        get(&mut app, "Long", "J", Value::Null),
        Value::Long(i64::MAX)
    );
    put(&mut app, "Boolean", "Z", key("value"), Value::Int(1));
    assert_eq!(get(&mut app, "Int", "I", key("value")), Value::Int(0));
    assert_eq!(call(&mut app, "count", "()I", &[]), Value::Int(2));
    call(&mut app, "remove", "(Ljava/lang/String;)V", &[Value::Null]);
    assert_eq!(
        call(
            &mut app,
            "contains",
            "(Ljava/lang/String;)Z",
            &[Value::Null]
        ),
        Value::Int(0)
    );
    call(&mut app, "clear", "()V", &[]);
    assert_eq!(call(&mut app, "empty", "()Z", &[]), Value::Int(1));
}

#[test]
fn references_survive_gc_and_intent_snapshots_preserve_shared_exact_types() {
    let mut app = app("bundle");
    let retained = app
        .vm
        .heap
        .alloc_string("only the Bundle owns this string".into())
        .unwrap();
    put(
        &mut app,
        "String",
        "Ljava/lang/String;",
        key("retained"),
        Value::Obj(retained),
    );
    app.vm.gc();
    assert_eq!(
        get(&mut app, "String", "Ljava/lang/String;", key("retained")),
        Value::Obj(retained)
    );
    call(
        &mut app,
        "remove",
        "(Ljava/lang/String;)V",
        &[key("retained")],
    );
    app.vm.gc();
    assert_eq!(
        app.vm.heap.class_of(retained).unwrap(),
        rd_vm::gc::FREED_CLASS
    );

    for (suffix, descriptor, value) in [
        ("Long", "J", Value::Long(0x1234_5678_9abc_def0)),
        ("Boolean", "Z", Value::Int(1)),
    ] {
        call(
            &mut app,
            &format!("extra{suffix}"),
            &format!("(Ljava/lang/String;{descriptor})V"),
            &[key(suffix), value.clone()],
        );
        assert_eq!(
            call(
                &mut app,
                &format!("readExtra{suffix}"),
                &format!("(Ljava/lang/String;{descriptor}){descriptor}"),
                &[key(suffix), value]
            ),
            if suffix == "Long" {
                Value::Long(0x1234_5678_9abc_def0)
            } else {
                Value::Int(1)
            }
        );
    }
    call(
        &mut app,
        "extraString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[key("String"), key("before")],
    );
    call(&mut app, "takeSnapshot", "()V", &[]);
    call(
        &mut app,
        "extraLong",
        "(Ljava/lang/String;J)V",
        &[key("Long"), Value::Long(7)],
    );
    call(
        &mut app,
        "extraString",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[key("String"), key("after")],
    );
    app.vm.gc();
    assert_eq!(
        call(
            &mut app,
            "snapshotLong",
            "(Ljava/lang/String;J)J",
            &[key("Long"), Value::Long(-1)]
        ),
        Value::Long(0x1234_5678_9abc_def0)
    );
    let value = call(
        &mut app,
        "snapshotString",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[key("String")],
    );
    assert_eq!(text(&app, &value), "before");
    assert_eq!(
        call(
            &mut app,
            "readExtraLong",
            "(Ljava/lang/String;J)J",
            &[key("Boolean"), Value::Long(-1)]
        ),
        Value::Long(-1)
    );
}

#[test]
fn bounded_store_rejects_new_entries_without_mutating_previous_values() {
    let mut app = app("bundle");
    for index in 0..1024 {
        put(
            &mut app,
            "Long",
            "J",
            key(&format!("key{index}")),
            Value::Long(index),
        );
    }
    let error = invoke(
        &mut app,
        "putLong",
        "(Ljava/lang/String;J)V",
        &[key("overflow"), Value::Long(999)],
    )
    .unwrap_err();
    assert!(matches!(error,VmExit::Error(error) if error.code=="BUNDLE_BUDGET"));
    assert_eq!(call(&mut app, "count", "()I", &[]), Value::Int(1024));
    assert_eq!(
        get(&mut app, "Long", "J", key("key1023")),
        Value::Long(1023)
    );
    put(&mut app, "Long", "J", key("key1023"), Value::Long(-9));
    assert_eq!(get(&mut app, "Long", "J", key("key1023")), Value::Long(-9));
}
#[test]
fn sdk_primitive_and_string_arrays_keep_identity_snapshot_aliases_and_gc_edges() {
    for fixture in ["bundle", "renamed"] {
        let mut app = app(fixture);
        assert_eq!(call(&mut app, "arrayProof", "()I", &[]), Value::Int(9));
        let array = get(
            &mut app,
            "StringArray",
            "[Ljava/lang/String;",
            key("String"),
        )
        .as_ref()
        .unwrap()
        .unwrap();
        app.vm.gc();
        let first = app.vm.heap.as_array_elems(array).unwrap().0[0].clone();
        assert_eq!(text(&app, &first), "changed");
        let bytes = get(&mut app, "ByteArray", "[B", key("Byte"));
        assert!(
            matches!(invoke(&mut app,"putIntArray","(Ljava/lang/String;[I)V",&[key("Int"), bytes]), Err(VmExit::Exception(e)) if e.class=="Ljava/lang/ClassCastException;")
        );
        let integers = get(&mut app, "IntArray", "[I", key("Int"))
            .as_ref()
            .unwrap()
            .unwrap();
        assert_eq!(app.vm.heap.class_of(integers).unwrap(), "[I");
        call(&mut app, "releaseArrays", "()V", &[]);
        app.vm.gc();
        assert_eq!(app.vm.heap.class_of(array).unwrap(), rd_vm::gc::FREED_CLASS);
    }
}
