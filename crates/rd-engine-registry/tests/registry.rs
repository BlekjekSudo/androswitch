//! Golden DETECTION fixtures, never advertised as golden runtime flows.

use rd_apk::{Apk, RdError};
use rd_engine_registry::{
    Artifact, DetectionReport, DetectionState, EngineAdapter, EngineEvent, EngineSyscall, Registry,
    CANONICAL_IDS, ENGINE_COUNT, SCOPE,
};
use rd_testsupport::apkfix::{build_manifest_axml_sdk, ZipFix};
use serde::Deserialize;

const CATALOG: &str = include_str!("../../../data/engines.toml");

fn apk_with_class(entries: &[&str], class: &str) -> Apk {
    let mut z = ZipFix::new();
    z.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk("com.example.engines", class, 26, 36),
    );
    for path in entries {
        // Deliberately not valid DEX/ELF/WASM. Detector must not load payload.
        z.add(path, b"not-executable-payload".to_vec());
    }
    Apk::from_bytes(z.build()).expect("APK inventory fixture")
}
fn apk(entries: &[&str]) -> Apk {
    apk_with_class(entries, "com.example.GoldenActivity")
}
fn detect(entries: &[&str]) -> DetectionReport {
    Registry::embedded().unwrap().detect(&apk(entries)).unwrap()
}
fn state(r: &DetectionReport, id: &str) -> DetectionState {
    r.engines.iter().find(|e| e.id == id).unwrap().state
}
fn typed(e: RdError, code: &str) {
    let json = serde_json::to_value(e).unwrap();
    assert_eq!(json["code"], code);
    assert!(json["cause"].is_string());
    assert!(json["suggestion"].is_string());
    assert!(json["module_id"].is_string());
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenSet {
    schema_version: u32,
    scope: String,
    cases: Vec<GoldenCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoldenCase {
    number: usize,
    id: String,
    entries: Vec<String>,
    manifest_class: String,
    expected_state: DetectionState,
}

#[test]
fn golden_signatures_cover_exactly_all_28_stable_ids() {
    let golden: GoldenSet =
        serde_json::from_str(include_str!("../../../tests/engines/signatures.json")).unwrap();
    assert_eq!(golden.schema_version, 1);
    assert_eq!(golden.scope, "artifact-signatures-not-runtime-goldens");
    assert_eq!(golden.cases.len(), ENGINE_COUNT);
    let registry = Registry::embedded().unwrap();
    for (i, case) in golden.cases.iter().enumerate() {
        assert_eq!(case.number, i + 1);
        assert_eq!(case.id, CANONICAL_IDS[i]);
        let paths: Vec<&str> = case.entries.iter().map(String::as_str).collect();
        let report = registry
            .detect(&apk_with_class(&paths, &case.manifest_class))
            .unwrap();
        assert_eq!(report.engines.len(), ENGINE_COUNT);
        assert_eq!(
            state(&report, &case.id),
            case.expected_state,
            "golden #{} {}",
            case.number,
            case.id
        );
        assert!(!report.execution_claimed);
        assert_eq!(report.artifact_validation, "not-performed");
    }
}

#[test]
fn schema_ids_modules_and_runtime_states_are_strict() {
    let good = Registry::from_toml(CATALOG).unwrap();
    assert_eq!(good.engines().len(), 28);
    assert_eq!(good.modules().len(), 12);
    assert_eq!(good.scope, SCOPE);
    for text in [
        CATALOG.replace("schema_version = 1", "schema_version = 2"),
        CATALOG.replace(
            "scope = \"registry-and-detection-only\"",
            "scope = \"all-runtimes-complete\"",
        ),
        CATALOG.replace("number = 28", "number = 29"),
        CATALOG.replace("id = \"wasm\"", "id = \"renamed-wasm\""),
        CATALOG.replace(
            "runtime_status = \"not_implemented\"",
            "runtime_status = \"complete\"",
        ),
        CATALOG.replacen(
            "runtime_status = \"not_implemented\"",
            "runtime_status = \"partial_dex\"",
            1,
        ),
        format!("unknown_field = true\n{CATALOG}"),
    ] {
        typed(
            Registry::from_toml(&text).unwrap_err(),
            "INVALID_ENGINE_CATALOG",
        );
    }
}

#[test]
fn unknown_cyclic_duplicate_or_self_dependencies_are_rejected() {
    for deps in [
        "[\"LANG-99\"]",
        "[\"LANG-01\"]",
        "[\"LANG-02\"]",
        "[\"LANG-03\", \"LANG-03\"]",
    ] {
        let text = CATALOG.replacen("depends_on = []", &format!("depends_on = {deps}"), 1);
        typed(
            Registry::from_toml(&text).unwrap_err(),
            "INVALID_ENGINE_CATALOG",
        );
    }
}

#[test]
fn detector_patterns_are_validated_before_scanning() {
    for predicate in [
        "regex:.*",
        "prefix:assets/www",
        "path:../evil.so",
        "native:foo/lib.so",
        "suffix:/evil",
        "metadata:",
    ] {
        let text = CATALOG.replacen(
            "all = [\"root-dex\"]",
            &format!("all = [\"{predicate}\"]"),
            1,
        );
        typed(
            Registry::from_toml(&text).unwrap_err(),
            "INVALID_ENGINE_CATALOG",
        );
    }
}

#[test]
fn catalog_size_is_bounded_before_parsing() {
    typed(
        Registry::from_toml(&" ".repeat(1024 * 1024 + 1)).unwrap_err(),
        "CATALOG_TOO_LARGE",
    );
}

#[test]
fn features_are_opt_in_and_transitive_but_never_runtime_claims() {
    let r = Registry::embedded().unwrap();
    for module in r.module_states() {
        if module.feature_enabled {
            for dep in &module.module.depends_on {
                assert!(
                    rd_engine_registry::feature_enabled(dep),
                    "{} requires {dep}",
                    module.module.id
                );
            }
        }
    }
    for info in r.engine_infos().unwrap() {
        assert_eq!(info.feature_enabled, info.disabled_modules.is_empty());
        assert_eq!(info.registry_adapter_status, "NOT_IMPLEMENTED");
    }
    assert_eq!(
        r.required_modules(r.get("tauri").unwrap()).unwrap(),
        ["LANG-01", "LANG-02", "LANG-03", "LANG-06"]
    );
    assert_eq!(
        r.required_modules(r.get("unity-il2cpp").unwrap()).unwrap(),
        ["LANG-01", "LANG-02", "LANG-07", "LANG-08"]
    );
    assert!(!rd_engine_registry::feature_enabled("LANG-99"));
}

#[test]
fn every_parse_event_and_syscall_returns_structured_not_implemented() {
    let r = Registry::embedded().unwrap();
    for e in r.engines() {
        let adapter = r.adapter(&e.id).unwrap();
        assert_eq!(adapter.descriptor().number, e.number);
        let mut process = (); // placeholder for testing failures, not a real process
        for error in [
            adapter
                .parse(Artifact {
                    name: "untrusted.so",
                    bytes: b"bad",
                })
                .unwrap_err(),
            adapter
                .event(
                    &mut process,
                    EngineEvent {
                        name: "resume",
                        payload: &[],
                    },
                )
                .unwrap_err(),
            adapter
                .syscall(
                    &mut process,
                    EngineSyscall {
                        name: "open",
                        args: &[1, 2],
                    },
                )
                .unwrap_err(),
        ] {
            assert_eq!(error.module_id, e.modules[0]);
            assert!(error.cause.contains(&e.id));
            typed(error, "NOT_IMPLEMENTED");
        }
    }
    typed(r.adapter("unregistered").unwrap_err(), "UNKNOWN_ENGINE");
}

#[test]
fn generic_dex_and_elf_do_not_attribute_source_language() {
    let r = detect(&["classes.dex", "lib/x86_64/libnative.so"]);
    for id in [
        "java",
        "kotlin",
        "jvm-dynamic",
        "generated-java",
        "c-ndk",
        "cpp-ndk",
        "rust-ndk",
        "objective-c-ndk",
        "zig-llvm",
        "haxe",
    ] {
        assert_eq!(state(&r, id), DetectionState::Candidate, "{id}");
    }
    assert!(r
        .diagnostics
        .iter()
        .any(|d| d.code == "AMBIGUOUS_SOURCE_LANGUAGE"));
    assert_eq!(state(&r, "flutter"), DetectionState::NotDetected);
}

#[test]
fn hybrid_apks_keep_multiple_independent_signatures() {
    let r = detect(&[
        "classes.dex",
        "lib/arm64-v8a/libflutter.so",
        "assets/flutter_assets/kernel_blob.bin",
        "lib/arm64-v8a/libreactnative.so",
        "assets/index.android.bundle",
        "assets/www/cordova.js",
        "assets/www/index.html",
        "assets/demo.wasm",
    ]);
    for id in ["flutter", "react-native", "cordova"] {
        assert_eq!(state(&r, id), DetectionState::Signature, "{id}");
    }
    assert_eq!(state(&r, "wasm"), DetectionState::Candidate);
}

#[test]
fn partial_markers_are_candidates_not_confident_framework_signatures() {
    let r = detect(&[
        "lib/arm64-v8a/libapp.so",
        "lib/arm64-v8a/libhermes.so",
        "assets/bin/Data/Managed/Assembly-CSharp.dll",
        "assets/index.android.bundle",
    ]);
    for id in ["flutter", "react-native", "unity-mono"] {
        assert_eq!(state(&r, id), DetectionState::Candidate, "{id}");
    }
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libflutter.so"]), "flutter"),
        DetectionState::NotDetected
    );
}

#[test]
fn dex_and_native_paths_must_be_canonical_android_inventory_paths() {
    let r = detect(&[
        "other.dex",
        "nested/classes.dex",
        "classes0.dex",
        "classes1.dex",
        "classes02.dex",
        "classesX.dex",
        "assets/libflutter.so",
        "lib/x86_64/subdir/libflutter.so",
        "lib/x86_64/libflutter.so.bak",
        "assets/www2/index.html",
        "assets/flutter_assets/", // dirs alone don't count
    ]);
    for id in ["java", "c-ndk", "flutter", "cordova"] {
        assert_eq!(state(&r, id), DetectionState::NotDetected, "{id}");
    }
    let multi = detect(&["classes.dex", "classes2.dex", "classes10.dex"]);
    let java = multi.engines.iter().find(|e| e.id == "java").unwrap();
    assert_eq!(java.evidence[0].matches[0].total_matches, 3);
}

#[test]
fn untrusted_paths_are_reported_and_never_used_as_evidence() {
    let r = detect(&[
        "../assets/e.wasm",
        "/assets/e.wasm",
        "assets\\e.wasm",
        "assets//e.wasm",
        "assets/./e.wasm",
        "assets/e\n.wasm",
    ]);
    assert_eq!(state(&r, "wasm"), DetectionState::NotDetected);
    let d = r
        .diagnostics
        .iter()
        .find(|d| d.code == "NON_CANONICAL_ARTIFACT_PATH")
        .unwrap();
    assert_eq!(d.total, 6);
    assert_eq!(d.samples.len(), 4);
}

#[test]
fn duplicate_names_are_counted_once_with_explicit_diagnostic() {
    let r = detect(&[
        "classes.dex",
        "classes.dex",
        "assets/e.wasm",
        "assets/e.wasm",
    ]);
    let wasm = r.engines.iter().find(|e| e.id == "wasm").unwrap();
    assert_eq!(wasm.evidence[0].matches[0].total_matches, 1);
    let d = r
        .diagnostics
        .iter()
        .find(|d| d.code == "DUPLICATE_ZIP_ENTRY")
        .unwrap();
    assert_eq!(d.total, 2);
}

#[test]
fn samples_are_bounded_and_reports_are_order_independent() {
    let names: Vec<String> = (0..50)
        .map(|i| format!("assets/modules/{i:03}.wasm"))
        .collect();
    let paths: Vec<&str> = names.iter().map(String::as_str).collect();
    let r = detect(&paths);
    let reverse: Vec<&str> = paths.into_iter().rev().collect();
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        serde_json::to_value(detect(&reverse)).unwrap()
    );
    let wasm = r.engines.iter().find(|e| e.id == "wasm").unwrap();
    let evidence = &wasm.evidence[0].matches[0];
    assert_eq!(evidence.total_matches, 50);
    assert_eq!(evidence.samples.len(), 4);
    assert_eq!(evidence.samples[0], "assets/modules/000.wasm");
}

#[test]
fn unknown_abis_do_not_claim_native_loader_support() {
    let r = detect(&["lib/not-an-android-abi/libnative.so"]);
    assert_eq!(state(&r, "c-ndk"), DetectionState::Candidate);
    assert!(r.diagnostics.iter().any(|d| d.code == "UNKNOWN_ABI"));
}

#[test]
fn twa_is_detectable_without_local_web_assets_but_absence_is_not_exclusion() {
    let registry = Registry::embedded().unwrap();
    let mut a = apk_with_class(
        &[],
        "com.google.androidbrowserhelper.trusted.LauncherActivity",
    );
    let r = registry.detect(&a).unwrap();
    assert_eq!(state(&r, "twa-pwa"), DetectionState::Signature);
    a.manifest.application.components[0].class_name = "com.example.Main".into();
    a.manifest.application.components[0]
        .meta_data
        .push(rd_apk::manifest::MetaData {
            name: Some("android.support.customtabs.trusted.DEFAULT_URL".into()),
            value: Some("https://example.invalid/not-fetched".into()),
            ..Default::default()
        });
    assert_eq!(
        state(&registry.detect(&a).unwrap(), "twa-pwa"),
        DetectionState::Signature
    );
    assert_eq!(state(&detect(&[]), "twa-pwa"), DetectionState::NotDetected);
}

#[test]
fn entry_limit_returns_error_instead_of_an_unbounded_report() {
    let mut z = ZipFix::new();
    z.add(
        "AndroidManifest.xml",
        build_manifest_axml_sdk("com.example.large", "com.example.Main", 26, 36),
    );
    // ZIP64 fixture helper supports only <=65535 EOCD entries; duplicate
    // entries are still physical entries. Build the small header then patch
    // the CD count via ZIP64, using a fully canonical ZIP64 locator.
    for i in 0..100_000 {
        z.add(&format!("assets/{i}"), Vec::new());
    }
    let mut bytes = z.build();
    let eocd_off = bytes.len() - 22;
    let cd_size =
        u32::from_le_bytes(bytes[eocd_off + 12..eocd_off + 16].try_into().unwrap()) as u64;
    let cd_off = u32::from_le_bytes(bytes[eocd_off + 16..eocd_off + 20].try_into().unwrap()) as u64;
    let mut eocd = bytes.split_off(eocd_off);
    let zip64_off = bytes.len() as u64;
    bytes.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
    bytes.extend_from_slice(&44u64.to_le_bytes());
    bytes.extend_from_slice(&[45, 0, 45, 0]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&100_001u64.to_le_bytes());
    bytes.extend_from_slice(&100_001u64.to_le_bytes());
    bytes.extend_from_slice(&cd_size.to_le_bytes());
    bytes.extend_from_slice(&cd_off.to_le_bytes());
    bytes.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&zip64_off.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    eocd[8..12].copy_from_slice(&[0xff; 4]);
    bytes.extend_from_slice(&eocd);
    let a = Apk::from_bytes(bytes).unwrap();
    typed(
        Registry::embedded().unwrap().detect(&a).unwrap_err(),
        "DETECTION_LIMIT",
    );
}

#[test]
#[ignore = "requires RD_TEST_APK pointing to a downloaded OSS APK; no silent fixture skip"]
fn real_oss_apk_inventory() {
    let path = std::env::var_os("RD_TEST_APK").expect("set RD_TEST_APK to a real OSS APK");
    let a = Apk::open(std::path::Path::new(&path)).expect("real APK parses");
    let report = Registry::embedded().unwrap().detect(&a).unwrap();
    assert!(!a.manifest.package.is_empty());
    assert_eq!(report.engines.len(), 28);
    assert!(!report.execution_claimed);
    assert_eq!(report.artifact_validation, "not-performed");
    assert!(report.entries_scanned > 0);
    if a.dex_files.iter().any(|p| p == "classes.dex") {
        assert_eq!(state(&report, "java"), DetectionState::Candidate);
    }
    println!(
        "OSS APK: {}; {} ZIP entries; 28 registry rows; detection only",
        a.manifest.package, a.zip_entry_count
    );
}
