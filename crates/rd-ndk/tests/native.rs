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
/// Issue #400: Unicorn 2.1.4 mis-executes VEX/EVEX (AVX) code (`floor(5.5)+0.25`
/// gave 0.25 on CI). The worker must fail closed on such encodings rather than
/// return a wrong value. The x86-64 baseline build stays the positive control.
#[test]
fn avx_encoded_guest_code_is_rejected_fail_closed_400() {
    let call_floating = |bytes: Vec<u8>| {
        let mut r = Runtime::new(Abi::X86_64).unwrap();
        let mut available = BTreeMap::new();
        available.insert("libfixture.so".into(), bytes);
        let init = r.load("libfixture.so", &available).unwrap();
        for a in init {
            r.call(a, &[], 10000, |_, _| {
                panic!("initializer should not call JNI")
            })
            .unwrap();
        }
        let f = r.symbol("Java_test_Native_floating").unwrap();
        r.call(
            f,
            &[
                Argument::pointer(0),
                Argument::pointer(0),
                Argument {
                    kind: 'F',
                    bits: 5.5f32.to_bits() as u64,
                },
                Argument {
                    kind: 'D',
                    bits: 0.25f64.to_bits(),
                },
            ],
            100000,
            |_, _| Ok(Return::default()),
        )
    };
    let baseline = call_floating(fixture(Abi::X86_64)).expect("baseline x86-64 must run");
    assert_eq!(f64::from_bits(baseline.fp_bits.unwrap()), 5.75);
    let avx_path = format!(
        "{}/../../golden/native/artifacts/x86_64-avx2.so",
        env!("CARGO_MANIFEST_DIR")
    );
    let avx =
        std::fs::read(&avx_path).expect("run bash golden/native/build.sh before native tests");
    let e = call_floating(avx).expect_err("VEX/EVEX code must not run silently");
    assert_eq!(e.code, "NOT_IMPLEMENTED", "{e}");
    assert!(e.cause.contains("VEX"), "{}", e.cause);
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
