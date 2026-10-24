//! Run alone: upstream dev dependencies must not silently select the std VM.
use rd_vm::{Engine, Value, VmConfig, VmExit};
const PROBE: &str = "Lorg/androswitch/vm/ClassNamesFixture;";
fn fixture_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/portable-class-names-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn vm() -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "run -p as-switch-smoke in isolation");
    let bytes = std::fs::read(fixture_root().join("dex/classes.dex"))
        .expect("run crates/rd-vm/tests/fixtures/portable-class-names/build.ps1");
    Engine::new(
        vec![rd_dex::Dex::parse(bytes).unwrap()],
        VmConfig::default(),
    )
}
fn simple(vm: &mut Engine, descriptor: &str) -> Result<String, VmExit> {
    let class = vm.device_class_object(descriptor)?;
    let result = vm.invoke_static(
        PROBE,
        "simple",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        &[Value::Obj(class)],
    )?;
    Ok(vm.heap.as_str(result.as_ref()?.unwrap())?.into())
}
#[test]
fn simple_names_match_23_jvm_cases_and_survive_gc() {
    let oracle = std::fs::read_to_string(fixture_root().join("jvm-oracle.txt")).unwrap();
    let cases: Vec<_> = oracle
        .lines()
        .map(|line| line.split_once('\t').unwrap())
        .collect();
    assert_eq!(cases.len(), 23);
    let mut vm = vm();
    for _ in 0..2 {
        for (descriptor, expected) in &cases {
            assert_eq!(
                simple(&mut vm, descriptor).unwrap(),
                *expected,
                "{descriptor}"
            );
        }
        let value = vm
            .invoke_static(PROBE, "literalName", "()Ljava/lang/String;", &[])
            .unwrap();
        assert_eq!(
            vm.heap.as_str(value.as_ref().unwrap().unwrap()).unwrap(),
            "Deep"
        );
        vm.gc();
    }
}
#[test]
fn names_follow_actual_annotations_and_malformed_metadata_is_an_error() {
    let mut vm = vm();
    let nested = "Lorg/androswitch/vm/ClassNamesFixture$Member;";
    assert_eq!(simple(&mut vm, nested).unwrap(), "Member");
    let (index, definition) = vm.cp.find_class(nested).unwrap();
    let class_index = definition.class_idx;
    let definition = vm.cp.dexes[index]
        .class_defs
        .iter_mut()
        .find(|definition| definition.class_idx == class_index)
        .unwrap();
    let original = definition.annotations_off;
    definition.annotations_off = 0;
    // Without nesting metadata the binary short name retains its literal '$'.
    assert_eq!(simple(&mut vm, nested).unwrap(), "ClassNamesFixture$Member");
    vm.cp.dexes[index]
        .class_defs
        .iter_mut()
        .find(|definition| definition.class_idx == class_index)
        .unwrap()
        .annotations_off = u32::MAX;
    assert!(simple(&mut vm, nested).is_err());
    vm.cp.dexes[index]
        .class_defs
        .iter_mut()
        .find(|definition| definition.class_idx == class_index)
        .unwrap()
        .annotations_off = original;
    assert_eq!(simple(&mut vm, nested).unwrap(), "Member");
    assert!(vm.cp.class_simple_name("[V").is_err());
    assert!(vm.cp.class_simple_name("Lmissing/Class;").is_err());
}

#[test]
fn malformed_inner_annotation_is_not_silently_replaced_with_a_binary_guess() {
    use rd_dex::annotations::{
        parse_annotation_item, parse_annotation_set_item, read_encoded_value,
    };
    let mut vm = vm();
    let nested = "Lorg/androswitch/vm/ClassNamesFixture$Member;";
    let (dex_index, definition) = vm.cp.find_class(nested).unwrap();
    let class_index = definition.class_idx;
    let directory = definition.annotations_off;
    let dex = &vm.cp.dexes[dex_index];
    let set = rd_dex::read::u32_at(&dex.data, directory as usize).unwrap();
    let inner = parse_annotation_set_item(&dex.data, set)
        .unwrap()
        .into_iter()
        .find(|offset| {
            let item = parse_annotation_item(&dex.data, *offset).unwrap();
            dex.type_str(item.annotation.type_idx) == "Ldalvik/annotation/InnerClass;"
        })
        .unwrap();
    let mut reader = rd_dex::read::Reader::at(&dex.data, inner as usize).unwrap();
    reader.u8().unwrap();
    reader.uleb128().unwrap();
    let count = reader.uleb128().unwrap();
    let mut position = None;
    for _ in 0..count {
        let name = dex.string(reader.uleb128().unwrap());
        let start = reader.pos;
        read_encoded_value(&mut reader).unwrap();
        if name == "name" {
            position = Some(start);
        }
    }
    let position = position.unwrap();
    let original = vm.cp.dexes[dex_index].data[position];
    vm.cp.dexes[dex_index].data[position] = 0x1f; // Boolean, not String/null.
    assert!(matches!(simple(&mut vm, nested), Err(VmExit::Error(error))
        if error.code == "INVALID_CLASS_METADATA"));
    vm.cp.dexes[dex_index].data[position] = original;
    assert_eq!(simple(&mut vm, nested).unwrap(), "Member");

    // A duplicate actual system annotation is malformed even though either
    // copy alone contains a plausible short name.
    let data = &mut vm.cp.dexes[dex_index].data;
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let duplicate_set = data.len() as u32;
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&inner.to_le_bytes());
    data.extend_from_slice(&inner.to_le_bytes());
    let duplicate_directory = data.len() as u32;
    data.extend_from_slice(&duplicate_set.to_le_bytes());
    data.extend_from_slice(&[0; 12]);
    vm.cp.dexes[dex_index]
        .class_defs
        .iter_mut()
        .find(|definition| definition.class_idx == class_index)
        .unwrap()
        .annotations_off = duplicate_directory;
    assert!(matches!(simple(&mut vm, nested), Err(VmExit::Error(error))
        if error.code == "INVALID_CLASS_METADATA"));
}
