use rd_vm::{boxes, framework_host::HostMethodKind, Engine, Value, VmConfig, VmExit};
#[test]
fn string_virtual_and_interface_dispatch_reuse_existing_methods_for_80_jvm_rows() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-boxes-fixture");
    let dex = rd_dex::Dex::parse(std::fs::read(root.join("dex/classes.dex")).unwrap()).unwrap();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let oracle = std::fs::read_to_string(root.join("string-dispatch-oracle.tsv")).unwrap();
    assert_eq!(oracle.lines().count(), 80);
    for _ in 0..2 {
        for row in oracle.lines() {
            let (id, wanted) = row.split_once('\t').unwrap();
            let value = vm
                .invoke_static(
                    "Lorg/androswitch/vm/StringDispatchFixture;",
                    "observe",
                    "(I)J",
                    &[Value::Int(id.parse().unwrap())],
                )
                .unwrap_or_else(|e| panic!("{row}: {e}"));
            assert_eq!(
                value.as_long().unwrap(),
                wanted.parse::<i64>().unwrap(),
                "{row}"
            );
        }
        vm.gc();
    }
}
#[test]
fn integer_utilities_match_518_jvm_results_and_four_zero_divisor_exceptions() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-boxes-fixture");
    let dex = rd_dex::Dex::parse(std::fs::read(root.join("dex/classes.dex")).unwrap()).unwrap();
    let mut vm = Engine::new(vec![dex], VmConfig::default());
    let oracle = std::fs::read_to_string(root.join("bits-oracle.tsv")).unwrap();
    assert_eq!(oracle.lines().count(), 519);
    for _ in 0..2 {
        for row in oracle.lines() {
            let (id, wanted) = row.split_once('\t').unwrap();
            let value = if id == "zero" {
                vm.invoke_static("Lorg/androswitch/vm/BitFixture;", "zero", "()I", &[])
                    .unwrap()
                    .as_int()
                    .unwrap() as i64
            } else {
                vm.invoke_static(
                    "Lorg/androswitch/vm/BitFixture;",
                    "observe",
                    "(I)J",
                    &[Value::Int(id.parse().unwrap())],
                )
                .unwrap()
                .as_long()
                .unwrap()
            };
            assert_eq!(value, wanted.parse::<i64>().unwrap(), "{row}");
        }
        vm.gc();
    }
}
#[test]
fn integer_utility_metadata_and_malformed_arguments_are_explicit() {
    let mut vm = Engine::new(vec![], VmConfig::default());
    for (class, name, sig) in [
        ("Ljava/lang/Integer;", "highestOneBit", "(I)I"),
        ("Ljava/lang/Long;", "reverseBytes", "(J)J"),
        ("Ljava/lang/Long;", "divideUnsigned", "(JJ)J"),
    ] {
        assert_eq!(
            boxes::method_metadata(class, name, sig),
            Some(HostMethodKind::Static)
        );
        assert!(
            matches!(boxes::static_intrinsic(&mut vm,class,name,sig,&[]),Err(VmExit::Error(ref e)) if e.code=="VM_TYPE_ERROR")
        );
    }
    assert_eq!(
        boxes::method_metadata("Ljava/lang/Long;", "highestOneBit", "(I)I"),
        None
    );
    assert!(boxes::static_intrinsic(
        &mut vm,
        "Ljava/lang/Long;",
        "highestOneBit",
        "(J)J",
        &[Value::Int(1)]
    )
    .is_err());
}
