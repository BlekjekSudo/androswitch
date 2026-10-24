//! Original ordinary SDK DEX input against JVM results, using the portable VM.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{
    framework_host::{FrameworkHost, HostMethodKind},
    ObjRef, Value, VmExit,
};
use std::{path::PathBuf, rc::Rc};

fn output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/reflection-fields-fixture")
}
fn namespace(variant: &str) -> &'static str {
    if variant == "renamed" {
        "net/example/fieldcontracts"
    } else {
        "org/androswitch/fields"
    }
}
fn cases(variant: &str) -> String {
    format!("L{}/FieldCases;", namespace(variant))
}
fn data(variant: &str) -> String {
    format!("L{}/FieldCases$Data;", namespace(variant))
}
#[allow(clippy::assertions_on_constants)]
fn app(variant: &str) -> (App, PathBuf) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-fields-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let record = pm
        .install(
            std::fs::read(output().join(format!("{variant}.apk")))
                .expect("run fixtures/m9/reflection-fields/build.ps1"),
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
    (app, root)
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn exception(result: Result<Value, VmExit>, class: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(ref error)) if error.class == class),
        "expected {class}, got {result:?}"
    );
}
fn lookup(app: &mut App, variant: &str, owner: &str, field: &str, inherited: bool) -> ObjRef {
    let class = app.vm.device_class_object(owner).unwrap();
    app.vm
        .invoke_static(
            &cases(variant),
            "lookup",
            "(Ljava/lang/Class;Ljava/lang/String;Z)Ljava/lang/reflect/Field;",
            &[
                Value::Obj(class),
                Value::StrPlaceholder(field.into()),
                Value::Int(i32::from(inherited)),
            ],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap()
}
fn invoke(
    app: &mut App,
    field: ObjRef,
    method: &str,
    signature: &str,
    value: Value,
) -> Result<Value, VmExit> {
    app.vm.invoke_instance(field, method, signature, &[value])
}
#[test]
fn sdk_getters_metadata_access_initialization_and_failures_match_jvm_in_two_namespaces() {
    for variant in ["reflection-fields", "renamed"] {
        let (mut app, root) = app(variant);
        let oracle =
            std::fs::read_to_string(output().join(variant).join("jvm-oracle.txt")).unwrap();
        assert_eq!(oracle.lines().count(), 119);
        for line in oracle.lines().take(54) {
            let (index, expected) = line.split_once('\t').unwrap();
            let value = app
                .vm
                .invoke_static(
                    &cases(variant),
                    "evaluate",
                    "(I)Ljava/lang/String;",
                    &[Value::Int(index.parse().unwrap())],
                )
                .unwrap_or_else(|error| panic!("{variant} case {index}: {error:?}"));
            assert_eq!(text(&app, value), expected, "{variant} case {index}");
        }
        for line in oracle.lines().skip(54).take(64) {
            let (case, expected) = line.split_once('\t').unwrap();
            let indices: Vec<_> = case
                .split(':')
                .skip(1)
                .map(|value| Value::Int(value.parse().unwrap()))
                .collect();
            let result = app
                .vm
                .invoke_static(
                    &cases(variant),
                    "widening",
                    "(II)Ljava/lang/String;",
                    &indices,
                )
                .unwrap();
            assert_eq!(text(&app, result), expected, "{variant} {case}");
        }
        let proof =
            std::fs::read_to_string(root.join("Download/androswitch-field-reflection-proof.txt"))
                .unwrap();
        assert!(proof.contains("status=PASS\n"));
        assert!(app.frame.pixels.iter().any(|pixel| *pixel != 0));
    }
}
#[test]
fn real_numeric_values_reference_identity_and_gc_are_preserved() {
    let variant = "renamed";
    let (mut app, _) = app(variant);
    let owner = data(variant);
    let object = app.vm.heap.alloc_instance(owner.clone(), vec![]).unwrap();
    app.vm
        .invoke_instance(object, "<init>", "()V", &[])
        .unwrap();
    let real = lookup(&mut app, variant, &owner, "real", false);
    assert_eq!(
        invoke(
            &mut app,
            real,
            "getFloat",
            "(Ljava/lang/Object;)F",
            Value::Obj(object)
        )
        .unwrap(),
        Value::Float(-2.5)
    );
    assert_eq!(
        invoke(
            &mut app,
            real,
            "getDouble",
            "(Ljava/lang/Object;)D",
            Value::Obj(object)
        )
        .unwrap(),
        Value::Double(-2.5)
    );
    let precise = lookup(&mut app, variant, &owner, "precise", false);
    assert_eq!(
        invoke(
            &mut app,
            precise,
            "getDouble",
            "(Ljava/lang/Object;)D",
            Value::Obj(object)
        )
        .unwrap(),
        Value::Double(9.25)
    );
    for (name, wrapper, expected) in [
        ("yes", "Ljava/lang/Boolean;", Value::Int(1)),
        ("tiny", "Ljava/lang/Byte;", Value::Int(-7)),
        ("letter", "Ljava/lang/Character;", Value::Int(65518)),
        ("small", "Ljava/lang/Short;", Value::Int(-123)),
        ("number", "Ljava/lang/Integer;", Value::Int(12345)),
        ("wide", "Ljava/lang/Long;", Value::Long(1234567890123)),
        ("real", "Ljava/lang/Float;", Value::Float(-2.5)),
        ("precise", "Ljava/lang/Double;", Value::Double(9.25)),
    ] {
        let field = lookup(&mut app, variant, &owner, name, false);
        let boxed = invoke(
            &mut app,
            field,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            Value::Obj(object),
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
        assert_eq!(app.vm.heap.class_of(boxed).unwrap(), wrapper);
        assert_eq!(app.vm.heap.get_field(boxed, "value").unwrap(), expected);
    }
    let payload = lookup(&mut app, variant, &owner, "payload", false);
    let original = app.vm.heap.get_field(object, "payload").unwrap();
    assert_eq!(
        invoke(
            &mut app,
            payload,
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            Value::Obj(object)
        )
        .unwrap(),
        original
    );
    // A live Field traces its actual declaring/type Class objects. A direct
    // host call can enter a DEX clinit that runs GC without a Java Field local.
    let target = format!("L{}/FieldCases$GcTarget;", namespace(variant));
    let field = lookup(&mut app, variant, &target, "payload", false);
    let value = invoke(
        &mut app,
        field,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        Value::Null,
    )
    .unwrap();
    assert_eq!(text(&app, value), "after-clinit-gc");
    let declared = app
        .vm
        .invoke_instance(field, "getDeclaringClass", "()Ljava/lang/Class;", &[])
        .unwrap();
    app.vm
        .statics
        .insert((cases(variant), "kept".into()), Value::Obj(field));
    app.vm.gc();
    assert_eq!(
        app.vm
            .invoke_instance(field, "getDeclaringClass", "()Ljava/lang/Class;", &[])
            .unwrap(),
        declared
    );
    for _ in 0..100 {
        let _ = lookup(&mut app, variant, &owner, "number", false);
    }
    assert!(app.vm.gc().last_freed_objects >= 100);
}
#[test]
fn unsupported_storage_platform_fields_forgery_and_writes_fail_explicitly() {
    let variant = "reflection-fields";
    let (mut app, _) = app(variant);
    assert!(
        matches!(app.vm.invoke_static(&cases(variant),"collision","()I",&[]),Err(VmExit::Error(ref error)) if error.code=="API_UNAVAILABLE")
    );
    assert!(
        matches!(app.vm.invoke_static(&cases(variant),"defaultBearingRead","()I",&[]),Err(VmExit::Error(ref error)) if error.code=="API_UNAVAILABLE")
    );
    for (owner, name) in [
        ("Ljava/lang/String;", "value"),
        ("Ljava/lang/Integer;", "MAX_VALUE"),
    ] {
        let class = app.vm.device_class_object(owner).unwrap();
        let result = app.vm.invoke_static(
            &cases(variant),
            "lookup",
            "(Ljava/lang/Class;Ljava/lang/String;Z)Ljava/lang/reflect/Field;",
            &[
                Value::Obj(class),
                Value::StrPlaceholder(name.into()),
                Value::Int(0),
            ],
        );
        assert!(
            matches!(result,Err(VmExit::Error(ref error)) if error.code=="API_UNAVAILABLE"),
            "{result:?}"
        );
    }
    let field = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/reflect/Field;".into(), vec![])
        .unwrap();
    assert!(
        matches!(invoke(&mut app,field,"getInt","(Ljava/lang/Object;)I",Value::Null),Err(VmExit::Error(ref error)) if error.code=="API_UNAVAILABLE")
    );
    let field = lookup(&mut app, variant, &data(variant), "constantNumber", false);
    let result = app.vm.invoke_instance(
        field,
        "setInt",
        "(Ljava/lang/Object;I)V",
        &[Value::Null, Value::Int(99)],
    );
    assert!(result.is_err());
    assert_eq!(
        invoke(
            &mut app,
            field,
            "getInt",
            "(Ljava/lang/Object;)I",
            Value::Null
        )
        .unwrap(),
        Value::Int(61)
    );
    assert_eq!(
        app.android.method_metadata(
            "Ljava/lang/reflect/Field;",
            "getInt",
            "(Ljava/lang/Object;)I"
        ),
        Some(HostMethodKind::Instance)
    );
    assert_eq!(
        app.android.method_metadata(
            "Ljava/lang/reflect/Field;",
            "setInt",
            "(Ljava/lang/Object;I)V"
        ),
        None
    );
}
#[test]
fn access_override_is_per_field_and_does_not_bypass_receiver_or_conversion_checks() {
    let variant = "reflection-fields";
    let (mut app, _) = app(variant);
    let first = lookup(&mut app, variant, &data(variant), "hidden", false);
    let second = lookup(&mut app, variant, &data(variant), "hidden", false);
    app.vm
        .invoke_instance(first, "setAccessible", "(Z)V", &[Value::Int(1)])
        .unwrap();
    assert_eq!(
        app.vm
            .invoke_instance(first, "isAccessible", "()Z", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        app.vm
            .invoke_instance(second, "isAccessible", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
    exception(
        invoke(
            &mut app,
            first,
            "getInt",
            "(Ljava/lang/Object;)I",
            Value::Null,
        ),
        "Ljava/lang/NullPointerException;",
    );
    let object = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    exception(
        invoke(
            &mut app,
            first,
            "getInt",
            "(Ljava/lang/Object;)I",
            Value::Obj(object),
        ),
        "Ljava/lang/IllegalArgumentException;",
    );
    // ART API26 initializes before access checking. Its clinit reenters the
    // service and changes this Field, so an early cached override is wrong.
    assert_eq!(
        app.vm
            .invoke_static(&cases(variant), "reentrantRead", "()I", &[])
            .unwrap(),
        Value::Int(79)
    );
}
#[test]
fn field_metadata_budget_is_reclaimed_after_unreachable_field_objects_are_collected() {
    let variant = "reflection-fields";
    let (mut app, _) = app(variant);
    let owner = data(variant);
    let class = app.vm.device_class_object(&owner).unwrap();
    let mut exhausted = false;
    for index in 0..4097 {
        let result = app.vm.invoke_static(
            &cases(variant),
            "lookup",
            "(Ljava/lang/Class;Ljava/lang/String;Z)Ljava/lang/reflect/Field;",
            &[
                Value::Obj(class),
                Value::StrPlaceholder("number".into()),
                Value::Int(0),
            ],
        );
        match result {
            Ok(field) => {
                app.vm
                    .statics
                    .insert(("LFieldBudgetRoots;".into(), index.to_string()), field);
            }
            Err(VmExit::Error(error)) if error.code == "REFLECTION_BUDGET" => {
                exhausted = true;
                break;
            }
            other => panic!("unexpected budget result {other:?}"),
        }
    }
    assert!(
        exhausted,
        "retained Fields must consume the bounded metadata budget"
    );
    app.vm
        .statics
        .retain(|(owner, _), _| owner != "LFieldBudgetRoots;");
    assert!(app.vm.gc().last_freed_objects >= 4090);
    let field = lookup(&mut app, variant, &owner, "number", false);
    assert_eq!(
        app.vm
            .invoke_instance(field, "getName", "()Ljava/lang/String;", &[])
            .map(|value| text(&app, value))
            .unwrap(),
        "number"
    );
}
#[test]
fn access_caller_is_the_real_native_dex_frame_and_unwinds_after_errors() {
    use rd_vm::{native_host::NativeHost, Engine};
    struct Backend;
    impl NativeHost for Backend {
        fn current_thread_id(&self) -> Option<u64> {
            Some(7)
        }
        fn roots(&self) -> Vec<ObjRef> {
            vec![]
        }
        fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
            unreachable!()
        }
        fn invoke(
            &self,
            vm: &mut Engine,
            class: &str,
            name: &str,
            sig: &str,
            is_static: bool,
            _: &[Value],
        ) -> Result<Value, VmExit> {
            assert!(is_static);
            assert_eq!(sig, "()I");
            assert!(class.ends_with("/FieldCases$NativeCaller;"));
            assert_eq!(vm.active_declaring_class().unwrap().as_deref(), Some(class));
            if name == "failNative" {
                return Err(rd_vm::Throwable::new(
                    "Ljava/lang/ArithmeticException;",
                    "fixture error",
                )
                .into());
            }
            assert_eq!(name, "readNative");
            let owner = vm.device_class_object(class)?;
            let field = vm
                .invoke_instance(
                    owner,
                    "getDeclaredField",
                    "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
                    &[Value::StrPlaceholder("number".into())],
                )?
                .as_ref()?
                .unwrap();
            vm.invoke_instance(field, "getInt", "(Ljava/lang/Object;)I", &[Value::Null])
        }
    }
    let (mut app, _) = app("renamed");
    app.vm.bind_native(Rc::new(Backend));
    let class = format!("L{}/FieldCases$NativeCaller;", namespace("renamed"));
    assert_eq!(
        app.vm
            .invoke_static(&class, "readNative", "()I", &[])
            .unwrap(),
        Value::Int(89)
    );
    assert_eq!(app.vm.active_declaring_class().unwrap(), None);
    exception(
        app.vm.invoke_static(&class, "failNative", "()I", &[]),
        "Ljava/lang/ArithmeticException;",
    );
    assert_eq!(app.vm.active_declaring_class().unwrap(), None);
}
