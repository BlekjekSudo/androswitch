#![cfg(feature = "native")]
use rd_ndk::{
    elf::{Abi, Elf},
    runtime::{Argument, Return, Runtime},
};
use std::collections::BTreeMap;
fn fixture(abi: Abi) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../golden/native/artifacts/{}.so",
        env!("CARGO_MANIFEST_DIR"),
        abi.android_name()
    ))
    .expect("run bash golden/native/build.sh before native tests")
}
#[test]
fn three_abis_load_relocate_initialize_and_execute() {
    for abi in [Abi::X86_64, Abi::Arm64, Abi::Arm] {
        let b = fixture(abi);
        let elf = Elf::parse(&b).unwrap();
        assert_eq!(elf.abi, abi);
        let mut r = Runtime::new(abi).unwrap();
        let mut available = BTreeMap::new();
        available.insert("libfixture.so".into(), b);
        let init = r.load("libfixture.so", &available).unwrap();
        for a in init {
            r.call(a, &[], 10000, |_, _| {
                panic!("initializer should not call JNI")
            })
            .unwrap();
        }
        let add = r.symbol("Java_test_Native_add").unwrap();
        let value = r
            .call(
                add,
                &[
                    Argument::pointer(0),
                    Argument::pointer(0),
                    Argument {
                        kind: 'I',
                        bits: 10,
                    },
                    Argument {
                        kind: 'I',
                        bits: 25,
                    },
                ],
                10000,
                |_, _| Ok(Return::default()),
            )
            .unwrap();
        assert_eq!(value.bits as i32, 42, "{abi:?}");
    }
}
#[test]
fn guest_infinite_loop_is_fuel_bounded() {
    for abi in [Abi::X86_64, Abi::Arm64, Abi::Arm] {
        let mut r = Runtime::new(abi).unwrap();
        let mut available = BTreeMap::new();
        available.insert("libfixture.so".into(), fixture(abi));
        r.load("libfixture.so", &available).unwrap();
        let spin = r.symbol("Java_test_Native_spin").unwrap();
        let e = r
            .call(
                spin,
                &[Argument::pointer(0), Argument::pointer(0)],
                100,
                |_, _| Ok(Return::default()),
            )
            .unwrap_err();
        assert_eq!(e.code, "NATIVE_FUEL", "{abi:?}: {e}");
    }
}
#[test]
fn parser_all_truncated_prefixes_fail_cleanly() {
    let b = fixture(Abi::X86_64);
    for n in 0..b.len() {
        let _ = Elf::parse(&b[..n]);
    }
}
