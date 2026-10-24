//! The real no_std Engine executes unchanged SDK getBytes call sites.
//! OpenJDK differential cases and the API 26 ART replacement divergence are
//! deliberately separate evidence. No Android-reference execution is implied.
use rd_vm::{Engine, Value, VmConfig, VmExit};
use std::path::PathBuf;
const CLASS: &str = "Lorg/androswitch/vm/EncodingFixture;";
const NAMED: &str = "(Ljava/lang/String;Ljava/lang/String;)[B";
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/portable-encoding-fixture")
}
#[allow(clippy::assertions_on_constants)]
fn vm_with_budget(budget: usize) -> Engine {
    assert!(rd_vm::PORTABLE_CORE, "test isolated -p as-switch-smoke");
    let dex = rd_dex::Dex::parse(
        std::fs::read(root().join("dex/classes.dex"))
            .expect("run crates/rd-vm/tests/fixtures/portable-encoding/build.ps1"),
    )
    .unwrap();
    Engine::new(
        vec![dex],
        VmConfig {
            heap_budget: budget,
            ..VmConfig::default()
        },
    )
}
fn vm() -> Engine {
    vm_with_budget(1024 * 1024)
}
fn string(vm: &mut Engine, value: &str) -> Value {
    Value::Obj(vm.heap.alloc_string(value.into()).unwrap())
}
fn bytes(vm: &Engine, value: Value) -> Vec<u8> {
    let reference = value.as_ref().unwrap().unwrap();
    assert_eq!(vm.heap.class_of(reference).unwrap(), "[B");
    vm.heap
        .as_array_elems(reference)
        .unwrap()
        .0
        .iter()
        .map(|item| {
            let n = item.as_int().unwrap();
            assert!((-128..=127).contains(&n), "byte[] uses signed Java bytes");
            n as u8
        })
        .collect()
}
fn encode(vm: &mut Engine, text: &str, charset: Option<&str>) -> Result<Value, VmExit> {
    let text = string(vm, text);
    let name = charset.map(|name| string(vm, name)).unwrap_or(Value::Null);
    vm.invoke_static(CLASS, "encode", NAMED, &[text, name])
}
fn hex(raw: &[u8]) -> String {
    raw.iter().map(|value| format!("{value:02x}")).collect()
}
fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn checksum(raw: &[u8]) -> i32 {
    raw.iter().fold(17i32, |value, byte| {
        value.wrapping_mul(31).wrapping_add(i32::from(*byte))
    })
}
#[test]
fn unchanged_dex_matches_31_jvm_cases_and_documents_two_android26_divergences() {
    let oracle = std::fs::read_to_string(root().join("jvm-oracle.tsv")).unwrap();
    assert_eq!(oracle.lines().count(), 33);
    let mut vm = vm();
    for round in 0..2 {
        for (index, row) in oracle.lines().enumerate() {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 4);
            assert_eq!(fields[0].parse::<usize>().unwrap(), index);
            let input = String::from_utf8(unhex(fields[1])).unwrap();
            let charset = if fields[2] == "null" {
                None
            } else {
                Some(String::from_utf8(unhex(fields[2])).unwrap())
            };
            let result = if charset.as_deref() == Some("DEFAULT") {
                let input = string(&mut vm, &input);
                vm.invoke_static(CLASS, "encodeDefault", "(Ljava/lang/String;)[B", &[input])
            } else {
                encode(&mut vm, &input, charset.as_deref())
            };
            let actual = match result {
                Ok(value) => format!("B{}", hex(&bytes(&vm, value))),
                Err(VmExit::Exception(error)) => format!("E{}", error.name_without_l()),
                other => panic!("case{index} unexpected {other:?}"),
            };
            if index < 31 {
                assert_eq!(actual, fields[3], "JVM case{index} round{round}");
            } else {
                assert_eq!(
                    fields[3], "B413f5a",
                    "OpenJDK replaces the surrogate pair once"
                );
                assert_eq!(actual, "B413f3f5a", "API26 ART replaces each UTF-16 unit");
            }
        }
        vm.gc();
    }
}
#[test]
fn unsupported_charset_is_a_catchable_io_exception_but_known_missing_encoders_are_explicit() {
    let mut vm = vm();
    let text = string(&mut vm, "payload");
    let name = string(&mut vm, "AndroSwitch-Unknown-Charset");
    assert_eq!(
        vm.invoke_static(
            CLASS,
            "catchIOException",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[text, name]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        1
    );
    for charset in [
        "Shift_JIS",
        "windows-1252",
        "UTF-32",
        "UnicodeBig",
        "x-UTF-16LE-BOM",
        "JavaUnicode2",
        "utf 8",
        "u*T@f08",
        "UTF-8,version=1",
        "x-Shift_JIS",
    ] {
        let input = string(&mut vm, "payload");
        let charset = string(&mut vm, charset);
        let before = vm.heap.used();
        assert!(
            matches!(vm.invoke_static(CLASS, "encode", NAMED, &[input, charset]),
            Err(VmExit::Error(error)) if error.code == "API_UNAVAILABLE")
        );
        assert_eq!(
            vm.heap.used(),
            before,
            "missing encoder does not allocate output"
        );
    }
    for charset in [
        "definitely-no-such-charset",
        "",
        ":invalid?",
        &"x".repeat(60),
        &"x".repeat(4096),
        "UTF-8bogus",
        "Shift_JISbogus",
    ] {
        assert!(matches!(encode(&mut vm, "payload", Some(charset)),
            Err(VmExit::Exception(error)) if error.class == "Ljava/io/UnsupportedEncodingException;"));
    }
}
#[test]
fn documented_android_alias_groups_keep_bom_and_endianness_distinct() {
    let mut vm = vm();
    for (alias, expected) in [
        ("ibm-5304", "41c3a9"),
        ("windows-65001", "41c3a9"),
        ("UTF_8", "41c3a9"),
        ("ibm-1204", "feff004100e9"),
        ("ISO-10646-UCS-2", "feff004100e9"),
        ("ibm-61956", "004100e9"),
        ("cp1201", "004100e9"),
        ("windows-1200", "4100e900"),
        ("UTF16_LittleEndian", "4100e900"),
        ("ISO_8859-1:1987", "41e9"),
        ("csISOLatin1", "41e9"),
        ("ANSI_X3.4-1968", "413f"),
        ("ibm-367", "413f"),
    ] {
        let result = encode(&mut vm, "Aé", Some(alias)).unwrap();
        assert_eq!(hex(&bytes(&vm, result)), expected, "alias {alias}");
    }
}
#[test]
fn byte_payload_survives_dex_gc_is_fresh_and_reaches_shared_vfs_unchanged() {
    let mut vm = vm();
    let text = "data:\0é中😀\n";
    for charset in [
        "UTF-8",
        "UTF-16",
        "UTF-16BE",
        "UTF-16LE",
        "ISO-8859-1",
        "US-ASCII",
    ] {
        let expected = encode(&mut vm, text, Some(charset)).unwrap();
        let expected = bytes(&vm, expected);
        let input = string(&mut vm, text);
        let name = string(&mut vm, charset);
        let result = vm
            .invoke_static(CLASS, "afterGc", NAMED, &[input, name])
            .unwrap();
        assert_eq!(bytes(&vm, result), expected);
        let input = string(&mut vm, text);
        let name = string(&mut vm, charset);
        assert_eq!(
            vm.invoke_static(
                CLASS,
                "separateArrays",
                "(Ljava/lang/String;Ljava/lang/String;)I",
                &[input, name]
            )
            .unwrap()
            .as_int()
            .unwrap(),
            checksum(&expected)
        );
    }
    let directory =
        std::env::temp_dir().join(format!("androswitch-encoding-{}", std::process::id()));
    let host = as_platform::host_fs::HostFileSystem::new(&directory).unwrap();
    let vfs = as_platform::vfs::Vfs::new(host, "org.fixture.encoding").unwrap();
    as_platform::vfs::GuestFileSystem::mkdirs(&vfs, "/sdcard/Download").unwrap();
    vm.bind_storage("org.fixture.encoding", Box::new(vfs))
        .unwrap();
    let input = string(&mut vm, text);
    let name = string(&mut vm, "UTF-8");
    let path = string(&mut vm, "/sdcard/Download/encoding.bin");
    assert_eq!(
        vm.invoke_static(
            CLASS,
            "writePayload",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[input, name, path]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        checksum(text.as_bytes())
    );
    let mut actual = vec![0; text.len() + 8];
    let count = vm
        .guest_filesystem()
        .unwrap()
        .read_at("/storage/emulated/0/Download/encoding.bin", 0, &mut actual)
        .unwrap();
    assert_eq!(&actual[..count], text.as_bytes());
    assert_eq!(
        std::fs::read(directory.join("Download/encoding.bin")).unwrap(),
        text.as_bytes()
    );
    std::fs::remove_file(directory.join("Download/encoding.bin")).unwrap();
}
#[test]
fn real_value_array_charge_is_checked_before_host_reserve_and_catchable_as_oom() {
    let mut vm = vm_with_budget(4096);
    let input = string(&mut vm, &"é".repeat(150));
    let name = string(&mut vm, "UTF-16");
    let before = vm.heap.used();
    let before_objects = vm.heap.len();
    assert!(matches!(rd_vm::intrinsics::call_instance_intrinsic_mref(
        &mut vm, "Ljava/lang/String;", "Ljava/lang/String;", "getBytes",
        "(Ljava/lang/String;)[B", input.as_ref().unwrap().unwrap(), std::slice::from_ref(&name)),
        Err(VmExit::Exception(error)) if error.class == "Ljava/lang/OutOfMemoryError;"));
    assert_eq!(
        vm.heap.used(),
        before,
        "budget rejection creates no managed byte[]"
    );
    assert_eq!(vm.heap.len(), before_objects);
    assert_eq!(
        vm.invoke_static(
            CLASS,
            "catchAllocation",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            &[input, name]
        )
        .unwrap()
        .as_int()
        .unwrap(),
        1
    );
    // Exception materialization can allocate its own throwable/message; no byte
    // array backing allocation is admitted. A successful short encode still works.
    assert!(vm.heap.used() >= before);
    let result = encode(&mut vm, "A", Some("UTF-8")).unwrap();
    assert_eq!(bytes(&vm, result), b"A");

    // Exact budget boundary uses the actual Value size on this target, not a
    // logical four-byte Int slot. Only the resulting managed Vec is allocated.
    let mut probe = vm_with_budget(4096);
    string(&mut probe, "é");
    string(&mut probe, "UTF-16");
    let boundary = probe.heap.used() + 4 * std::mem::size_of::<Value>() + 16;
    for (budget, allowed) in [(boundary, true), (boundary - 1, false)] {
        let mut vm = vm_with_budget(budget);
        let input = string(&mut vm, "é");
        let name = string(&mut vm, "UTF-16");
        let before = vm.heap.used();
        let actual = rd_vm::intrinsics::call_instance_intrinsic_mref(
            &mut vm,
            "Ljava/lang/String;",
            "Ljava/lang/String;",
            "getBytes",
            "(Ljava/lang/String;)[B",
            input.as_ref().unwrap().unwrap(),
            &[name],
        );
        if allowed {
            assert_eq!(bytes(&vm, actual.unwrap().unwrap()), [0xfe, 0xff, 0, 0xe9]);
            assert_eq!(vm.heap.used(), budget);
        } else {
            assert!(matches!(actual, Err(VmExit::Exception(error))
                if error.class == "Ljava/lang/OutOfMemoryError;"));
            assert_eq!(vm.heap.used(), before);
        }
    }
}
