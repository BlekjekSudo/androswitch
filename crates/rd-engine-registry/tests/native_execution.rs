#![cfg(feature = "native-execution")]
//! Reproducible integration gate over actual source-built APKs, never fake ELF.
use rd_engine_registry::{Artifact, EngineAdapter, EngineEvent, EngineSyscall, Registry};
use serde_json::json;
fn digest(tag: u32, left: i32, right: i32) -> i32 {
    let mut h = 2166136261u32 ^ tag;
    for byte in left.to_le_bytes().into_iter().chain(right.to_le_bytes()) {
        h = (h ^ u32::from(byte)).wrapping_mul(16777619);
    }
    (h & 0x7fff_ffff) as i32
}
fn typed(error: rd_apk::RdError, code: &str) {
    assert_eq!(error.code, code);
    assert!(!error.cause.is_empty());
    assert!(error.suggestion.is_some());
    assert!(!error.module_id.is_empty());
}
#[test]
fn only_three_explicit_native_adapters_are_available() {
    let registry = Registry::embedded().unwrap();
    let ids: Vec<_> = registry
        .engines()
        .iter()
        .filter(|d| rd_engine_registry::native_execution_available(&d.id))
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(ids, ["c-ndk", "cpp-ndk", "rust-ndk"]);
    let mut z = rd_testsupport::apkfix::ZipFix::new();
    z.add(
        "AndroidManifest.xml",
        rd_testsupport::apkfix::build_manifest_axml_sdk("org.test", "org.test.Main", 26, 34),
    );
    z.add(
        "lib/x86_64/probe.so",
        b"not ELF; metadata must never execute this".to_vec(),
    );
    let apk = rd_apk::Apk::from_bytes(z.build()).unwrap();
    assert!(!registry.detect(&apk).unwrap().execution_claimed);
}
#[test]
#[ignore = "requires golden/native-languages/build.py output; set RD_NATIVE_LANGUAGE_APKS"]
fn real_source_apks_parse_launch_invoke_syscall_and_fail_closed() {
    let root = std::path::PathBuf::from(
        std::env::var_os("RD_NATIVE_LANGUAGE_APKS")
            .expect("set RD_NATIVE_LANGUAGE_APKS to source-built APK directory"),
    );
    let registry = Registry::embedded().unwrap();
    rd_vm::set_app_stdout(rd_vm::HostOut::Buffer(std::sync::Arc::default()));
    for (id, file, tag) in [
        ("c-ndk", "native-c.apk", 1),
        ("cpp-ndk", "native-cpp.apk", 2),
        ("rust-ndk", "native-rust.apk", 3),
    ] {
        let bytes = std::fs::read(root.join(file)).unwrap();
        let adapter = registry.adapter(id).unwrap();
        let mut p = adapter
            .parse(Artifact {
                name: file,
                bytes: &bytes,
            })
            .unwrap();
        assert_eq!(p.report().unwrap()["execution_claimed"], false);
        typed(
            adapter
                .event(
                    &mut p,
                    EngineEvent {
                        name: "invoke",
                        payload: b"{}",
                    },
                )
                .unwrap_err(),
            "INVALID_STATE",
        );
        adapter
            .event(
                &mut p,
                EngineEvent {
                    name: "launch",
                    payload: &[],
                },
            )
            .unwrap();
        let report = p.report().unwrap();
        assert_eq!(report["execution_claimed"], true);
        assert!(report["native"]["instructions"].as_u64().unwrap() > 0);
        assert_eq!(report["native"]["missing_imports"], json!([]));
        let main = "Lorg/rustydroid/nativeworkbench/MainActivity;";
        for (left, right) in [(42, 9), (-17, 3), (i32::MAX, i32::MIN), (0, 0)] {
            let payload =
                serde_json::to_vec(&json!({"class":main,"method":"digest","args":[left,right]}))
                    .unwrap();
            adapter
                .event(
                    &mut p,
                    EngineEvent {
                        name: "invoke",
                        payload: &payload,
                    },
                )
                .unwrap();
            assert_eq!(p.report().unwrap()["last_result"], digest(tag, left, right));
        }
        let flavor =
            serde_json::to_vec(&json!({"class":main,"method":"nativeFlavor","args":[]})).unwrap();
        adapter
            .event(
                &mut p,
                EngineEvent {
                    name: "invoke",
                    payload: &flavor,
                },
            )
            .unwrap();
        assert_eq!(p.report().unwrap()["last_result"], tag);
        adapter
            .syscall(
                &mut p,
                EngineSyscall {
                    name: "advance_clock",
                    args: &[1000],
                },
            )
            .unwrap();
        let before = p.report().unwrap()["native"]["instructions"].clone();
        typed(
            adapter
                .event(
                    &mut p,
                    EngineEvent {
                        name: "unsupported",
                        payload: &[],
                    },
                )
                .unwrap_err(),
            "NOT_IMPLEMENTED",
        );
        typed(
            adapter
                .syscall(
                    &mut p,
                    EngineSyscall {
                        name: "open",
                        args: &[],
                    },
                )
                .unwrap_err(),
            "NOT_IMPLEMENTED",
        );
        typed(
            adapter
                .event(
                    &mut p,
                    EngineEvent {
                        name: "invoke",
                        payload: b"{\"unknown\":true}",
                    },
                )
                .unwrap_err(),
            "INVALID_FORMAT",
        );
        assert_eq!(p.report().unwrap()["native"]["instructions"], before);
        let wrong = registry
            .adapter(if id == "c-ndk" { "rust-ndk" } else { "c-ndk" })
            .unwrap();
        typed(
            wrong
                .event(
                    &mut p,
                    EngineEvent {
                        name: "launch",
                        payload: &[],
                    },
                )
                .unwrap_err(),
            "INVALID_PROCESS",
        );
        let mut apk = rd_apk::Apk::from_bytes(bytes.clone()).unwrap();
        apk.file_size_bytes = 256 * 1024 * 1024 + 1;
        typed(
            rd_engine_registry::NativeProcess::from_apk(id, apk).unwrap_err(),
            "APK_TOO_LARGE",
        );
        let mut apk = rd_apk::Apk::from_bytes(bytes.clone()).unwrap();
        apk.manifest.min_sdk = 25;
        typed(
            rd_engine_registry::NativeProcess::from_apk(id, apk).unwrap_err(),
            "UNSUPPORTED_API_FLOOR",
        );
        let mut apk = rd_apk::Apk::from_bytes(bytes).unwrap();
        apk.native_libs.clear();
        typed(
            rd_engine_registry::NativeProcess::from_apk(id, apk).unwrap_err(),
            "MISSING_ENTRY",
        );
    }
}
