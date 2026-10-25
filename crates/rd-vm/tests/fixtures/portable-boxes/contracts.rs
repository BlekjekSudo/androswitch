use rd_vm::{boxes, Engine, Value, VmConfig};

fn fixture() -> Engine {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-boxes-fixture");
    let dex = rd_dex::Dex::parse(
        std::fs::read(root.join("dex/classes.dex")).expect("run portable-boxes/build.ps1"),
    )
    .unwrap();
    Engine::new(vec![dex], VmConfig::default())
}

#[test]
fn inherited_wrapper_family_matches_86_jvm_observations_after_gc() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-boxes-fixture");
    let expected = std::fs::read_to_string(root.join("oracle.tsv")).unwrap();
    assert_eq!(expected.lines().count(), 86);
    let mut vm = fixture();
    for _ in 0..2 {
        for row in expected.lines() {
            let parts: Vec<_> = row.split('\t').collect();
            let id = parts[1].parse().unwrap();
            let (name, sig) = if parts[0] == "text" {
                ("text", "(I)Ljava/lang/String;")
            } else {
                ("observe", "(I)J")
            };
            let result = vm
                .invoke_static(
                    "Lorg/androswitch/vm/BoxFixture;",
                    name,
                    sig,
                    &[Value::Int(id)],
                )
                .unwrap_or_else(|e| panic!("{row}: {e:?}"));
            let actual = if parts[0] == "text" {
                vm.heap
                    .as_str(result.as_ref().unwrap().unwrap())
                    .unwrap()
                    .to_string()
            } else {
                result.as_long().unwrap().to_string()
            };
            assert_eq!(actual, parts[2], "{row}");
        }
        vm.gc();
    }
}

#[test]
fn wrapper_cache_roots_and_metadata_are_shared_and_descriptor_exact() {
    use rd_vm::framework_host::HostMethodKind::{Constructor, Instance, Static};
    let mut vm = fixture();
    for class in ["Byte", "Short", "Character", "Integer", "Long"] {
        let class = format!("Ljava/lang/{class};");
        let value = if class.contains("Long;") {
            Value::Long(42)
        } else {
            Value::Int(42)
        };
        let first = boxes::box_value(&mut vm, &class, value.clone()).unwrap();
        vm.gc();
        assert_eq!(boxes::box_value(&mut vm, &class, value).unwrap(), first);
        assert_eq!(vm.heap.class_of(first).unwrap(), class);
    }
    assert_eq!(
        boxes::method_metadata("Ljava/lang/Double;", "<init>", "(D)V"),
        Some(Constructor)
    );
    assert_eq!(
        boxes::method_metadata("Ljava/lang/Double;", "hashCode", "()I"),
        Some(Instance)
    );
    assert_eq!(
        boxes::method_metadata("Ljava/lang/Double;", "hashCode", "(D)I"),
        Some(Static)
    );
    for (class, name, sig) in [
        ("Ljava/lang/Integer;", "intValue", "()J"),
        ("Ljava/lang/Character;", "<init>", "(Ljava/lang/String;)V"),
        ("Ljava/lang/Void;", "valueOf", "(I)Ljava/lang/Void;"),
        ("Ljava/lang/Boolean;", "logicalAnd", "(II)I"),
        ("Ljava/lang/Float;", "isFinite", "(D)Z"),
    ] {
        assert_eq!(boxes::method_metadata(class, name, sig), None);
    }
    assert_eq!(
        boxes::static_field_descriptor("Ljava/lang/Integer;", "MAX_VALUE"),
        Some("I")
    );
    assert_eq!(
        boxes::static_field_descriptor("Ljava/lang/Integer;", "imaginary"),
        None
    );
    assert!(
        boxes::static_intrinsic(&mut vm, "Ljava/lang/Boolean;", "logicalAnd", "(II)I", &[])
            .unwrap()
            .is_none()
    );
}
