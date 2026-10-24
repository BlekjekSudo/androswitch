//! Run this package in isolation so upstream dev-deps cannot enable rd-vm/std.
use rd_vm::{Engine, Throwable, Value, VmConfig, VmExit};

const ENUM: &str = "Ljava/lang/Enum;";
const PROBE: &str = "Lorg/androswitch/vm/EnumFixture;";
#[allow(clippy::assertions_on_constants)]
fn vm() -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-enum-fixture");
    let dex = rd_dex::Dex::parse(
        std::fs::read(root.join("dex/classes.dex"))
            .expect("run crates/rd-vm/tests/fixtures/portable-enum/build.ps1"),
    )
    .unwrap();
    Engine::new(vec![dex], VmConfig::default())
}
fn exception(error: VmExit, expected: &str) {
    assert!(matches!(error, VmExit::Exception(Throwable { class, .. }) if class == expected));
}
#[test]
fn enum_state_is_budgeted_gc_traced_and_uninitialized_state_is_an_error() {
    let mut vm = vm();
    let class = "Lorg/androswitch/vm/EnumFixture$Renamed;";
    let value = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
    vm.attach_host_state(class, value).unwrap();
    let error = vm
        .invoke_instance(value, "ordinal", "()I", &[])
        .unwrap_err();
    assert!(matches!(error,VmExit::Error(error) if error.code == "ENUM_STATE_UNINITIALIZED"));
    let name = vm.heap.alloc_string("REAL_NAME".into()).unwrap();
    rd_vm::intrinsics::call_instance_intrinsic_mref(
        &mut vm,
        class,
        ENUM,
        "<init>",
        "(Ljava/lang/String;I)V",
        value,
        &[Value::Obj(name), Value::Int(7)],
    )
    .unwrap();
    vm.statics
        .insert(("LRoot;".into(), "constant".into()), Value::Obj(value));
    vm.gc();
    assert_eq!(
        vm.invoke_instance(value, "name", "()Ljava/lang/String;", &[])
            .unwrap(),
        Value::Obj(name)
    );
    assert_eq!(vm.heap.as_str(name).unwrap(), "REAL_NAME");
    assert_eq!(
        vm.invoke_instance(value, "ordinal", "()I", &[]).unwrap(),
        Value::Int(7)
    );
    exception(
        vm.invoke_instance(value, "clone", "()Ljava/lang/Object;", &[])
            .unwrap_err(),
        "Ljava/lang/CloneNotSupportedException;",
    );
    vm.statics.clear();
    vm.gc();
    assert_eq!(vm.heap.class_of(value).unwrap(), rd_vm::gc::FREED_CLASS);
    assert!(
        vm.heap.as_str(name).is_err(),
        "enum name must be collectable when no enum root remains"
    );
}
#[test]
fn unchanged_java_enum_dex_matches_jvm_and_survives_gc() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-enum-fixture");
    let mut vm = vm();
    let oracle = std::fs::read_to_string(root.join("jvm-oracle.txt")).unwrap();
    let expected: Vec<_> = oracle.lines().collect();
    let integer_methods = [
        "ordinals",
        "comparisons",
        "identities",
        "arrayCopies",
        "fieldShadow",
        "missing",
        "aliasIsNotAConstant",
        "differentTypes",
        "nullArgs",
    ];
    let string_methods = ["defaultName", "overrideName", "superName", "renamedName"];
    assert_eq!(expected.len(), integer_methods.len() + string_methods.len());
    for _ in 0..2 {
        for (index, method) in integer_methods.iter().enumerate() {
            let actual = vm
                .invoke_static(PROBE, method, "()I", &[])
                .unwrap_or_else(|error| panic!("{method}: {error}"));
            assert_eq!(
                actual.as_int().unwrap().to_string(),
                expected[index],
                "{method}"
            );
        }
        for (index, method) in string_methods.iter().enumerate() {
            let actual = vm
                .invoke_static(PROBE, method, "()Ljava/lang/String;", &[])
                .unwrap_or_else(|error| panic!("{method}: {error}"));
            assert_eq!(
                vm.heap.as_str(actual.as_ref().unwrap().unwrap()).unwrap(),
                expected[integer_methods.len() + index],
                "{method}"
            );
        }
        vm.gc();
    }
}
#[test]
fn value_of_does_not_inspect_nonconstant_enum_typed_static_fields() {
    let mut vm = vm();
    let class = "Lorg/androswitch/vm/EnumFixture$Alpha;";
    vm.ensure_initialized(class).unwrap();
    // Fault injection: a nonconstant field can hold a reference whose Enum
    // constructor did not complete. valueOf must inspect only ACC_ENUM fields.
    let uninitialized = vm.heap.alloc_instance(class.into(), vec![]).unwrap();
    vm.statics.insert(
        (class.into(), "AAAPOISON".into()),
        Value::Obj(uninitialized),
    );
    assert_eq!(
        vm.invoke_static(PROBE, "missing", "()I", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.invoke_static(PROBE, "identities", "()I", &[]).unwrap(),
        Value::Int(1)
    );
}
