//! Isolated portable VM; no std intrinsic implementation or native backend.
use rd_vm::{Engine, Value, VmConfig};
const PROBE: &str = "Lorg/androswitch/vm/AtomicFixture;";
fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-atomic-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn vm() -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    Engine::new(
        vec![rd_dex::Dex::parse(std::fs::read(root().join("dex/classes.dex")).unwrap()).unwrap()],
        VmConfig::default(),
    )
}
#[test]
fn sixty_two_jvm_cases_match_before_and_after_gc() {
    let mut vm = vm();
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.txt")).unwrap();
    assert_eq!(oracle.lines().count(), 62);
    for _ in 0..2 {
        for row in oracle.lines() {
            let parts: Vec<_> = row.split('\t').collect();
            let sig = if parts[0] != "text" {
                "(I)J"
            } else {
                "(I)Ljava/lang/String;"
            };
            let value = vm
                .invoke_static(
                    PROBE,
                    parts[0],
                    sig,
                    &[Value::Int(parts[1].parse().unwrap())],
                )
                .unwrap();
            let actual = if parts[0] != "text" {
                value.as_long().unwrap().to_string()
            } else {
                vm.heap
                    .as_str(value.as_ref().unwrap().unwrap())
                    .unwrap()
                    .into()
            };
            assert_eq!(actual, parts[2], "{row}");
        }
        vm.gc();
    }
}
#[test]
fn shared_identity_cas_and_collection_follow_heap_lifetime() {
    let mut vm = vm();
    let a = vm
        .invoke_static(
            PROBE,
            "make",
            "(Z)Ljava/util/concurrent/atomic/AtomicBoolean;",
            &[Value::Int(0)],
        )
        .unwrap();
    let object = a.as_ref().unwrap().unwrap();
    vm.statics
        .insert(("LRoot;".into(), "atomic".into()), a.clone());
    vm.gc();
    for (expected, update, result) in [(0, 1, 1), (0, 1, 0), (1, 0, 1), (1, 1, 0)] {
        assert_eq!(
            vm.invoke_static(
                PROBE,
                "swap",
                "(Ljava/util/concurrent/atomic/AtomicBoolean;ZZ)Z",
                &[a.clone(), Value::Int(expected), Value::Int(update)]
            )
            .unwrap(),
            Value::Int(result)
        );
    }
    assert_eq!(
        vm.invoke_instance(object, "get", "()Z", &[]).unwrap(),
        Value::Int(0)
    );
    assert!(
        vm.invoke_instance(object, "getOpaque", "()Z", &[]).is_err(),
        "newer API is not silently implemented"
    );
    vm.statics.clear();
    vm.gc();
    assert_eq!(vm.heap.class_of(object).unwrap(), rd_vm::gc::FREED_CLASS);
    assert!(vm.invoke_instance(object, "get", "()Z", &[]).is_err());
}
#[test]
fn numeric_slots_keep_identity_reject_bad_descriptors_and_follow_gc() {
    let mut vm = vm();
    for (class, get, set, cas, one, zero) in [
        (
            "Ljava/util/concurrent/atomic/AtomicInteger;",
            "()I",
            "(I)V",
            "(II)Z",
            Value::Int(1),
            Value::Int(0),
        ),
        (
            "Ljava/util/concurrent/atomic/AtomicLong;",
            "()J",
            "(J)V",
            "(JJ)Z",
            Value::Long(1),
            Value::Long(0),
        ),
    ] {
        let object = vm.new_instance(class).unwrap();
        vm.statics
            .insert(("LRoot;".into(), "numeric".into()), Value::Obj(object));
        vm.gc();
        assert_eq!(vm.invoke_instance(object, "get", get, &[]).unwrap(), zero);
        assert_eq!(
            vm.invoke_instance(object, "compareAndSet", cas, &[zero.clone(), one.clone()])
                .unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            vm.invoke_instance(object, "compareAndSet", cas, &[zero, one.clone()])
                .unwrap(),
            Value::Int(0)
        );
        assert!(vm
            .invoke_instance(object, "set", set, &[Value::Null])
            .is_err());
        assert_eq!(vm.invoke_instance(object, "get", get, &[]).unwrap(), one);
        assert!(vm.invoke_instance(object, "getOpaque", get, &[]).is_err());
        vm.statics.clear();
        vm.gc();
        assert_eq!(vm.heap.class_of(object).unwrap(), rd_vm::gc::FREED_CLASS);
        assert!(vm.invoke_instance(object, "get", get, &[]).is_err());
    }
}
