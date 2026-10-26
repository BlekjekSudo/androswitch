//! Golden DETECTION fixtures, never advertised as golden runtime flows.

use rd_apk::{Apk, RdError};
use rd_engine_registry::{
    AdapterProcess, Artifact, DetectionReport, DetectionState, EngineAdapter, EngineEvent,
    EngineSyscall, Registry, CANONICAL_IDS, ENGINE_COUNT, SCOPE,
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
        assert_eq!(
            info.registry_adapter_status,
            if rd_engine_registry::native_execution_available(&info.engine.id) {
                "AVAILABLE_BOUNDED"
            } else {
                "NOT_IMPLEMENTED"
            }
        );
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
        if rd_engine_registry::native_execution_available(&e.id) {
            let bad = adapter
                .parse(Artifact {
                    name: "bad.apk",
                    bytes: b"bad",
                })
                .unwrap_err();
            assert_ne!(
                bad.code, "NOT_IMPLEMENTED",
                "real adapter must validate the APK"
            );
            let mut process = AdapterProcess::Unavailable;
            typed(
                adapter
                    .event(
                        &mut process,
                        EngineEvent {
                            name: "launch",
                            payload: &[],
                        },
                    )
                    .unwrap_err(),
                "INVALID_PROCESS",
            );
            typed(
                adapter
                    .syscall(
                        &mut process,
                        EngineSyscall {
                            name: "open",
                            args: &[],
                        },
                    )
                    .unwrap_err(),
                "INVALID_PROCESS",
            );
            continue;
        }
        let mut process = AdapterProcess::Unavailable; // negative sentinel, never a parsed process
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
    let a = Apk::from_bytes(rd_testsupport::apkfix::build_large_inventory_apk(100_000)).unwrap();
    let registry = Registry::embedded().unwrap();
    typed(registry.detect(&a).unwrap_err(), "DETECTION_LIMIT");
    let diagnostic = serde_json::to_value(registry.detect_for_inspection(&a).unwrap()).unwrap();
    assert_eq!(diagnostic["analysis_status"], "not_analyzed");
    assert_eq!(diagnostic["execution_claimed"], false);
    assert_eq!(diagnostic["error"]["code"], "DETECTION_LIMIT");
    assert!(
        diagnostic.get("engines").is_none(),
        "must not fake negative findings for unscanned input"
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

#[test]
fn existing_native_routes_are_partial_but_registry_adapters_stay_unavailable() {
    let registry = Registry::embedded().unwrap();
    for id in [
        "c-ndk",
        "cpp-ndk",
        "rust-ndk",
        "objective-c-ndk",
        "zig-llvm",
    ] {
        let descriptor = registry.get(id).unwrap();
        assert_eq!(
            descriptor.runtime_status,
            rd_engine_registry::RuntimeStatus::PartialNative
        );
        assert_eq!(
            registry.info(descriptor).unwrap().registry_adapter_status,
            if rd_engine_registry::native_execution_available(id) {
                "AVAILABLE_BOUNDED"
            } else {
                "NOT_IMPLEMENTED"
            }
        );
        if rd_engine_registry::native_execution_available(id) {
            continue;
        }
        typed(
            registry
                .adapter(id)
                .unwrap()
                .parse(Artifact {
                    name: "not-run.so",
                    bytes: &[],
                })
                .unwrap_err(),
            "NOT_IMPLEMENTED",
        );
    }
    assert_eq!(
        registry.get("flutter").unwrap().runtime_status,
        rd_engine_registry::RuntimeStatus::NotImplemented
    );
}

// ── fixes dos gaps documentados pelo APK-Tester (engines reais) ──────────

/// Gap engine-16 (tauri): a build REAL do Tauri v2 mobile (cargo-ndk)
/// embarca `libtauri_android.so` + `libwry.so` e NÃO carrega
/// `assets/tauri.conf.json` nem `assets/www/` — a regra antiga nunca casava.
#[test]
fn tauri_real_cargo_ndk_build_is_a_signature() {
    // layout exato reportado pelo APK-Tester: libs em lib/<abi>/, sem assets
    let r = detect(&[
        "lib/x86_64/libtauri_android.so",
        "lib/x86_64/libwry.so",
        "classes.dex",
    ]);
    assert_eq!(state(&r, "tauri"), DetectionState::Signature);
    // cada lib isolada também basta (any = OU)
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libwry.so"]), "tauri"),
        DetectionState::Signature
    );
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libtauri_android.so"]), "tauri"),
        DetectionState::Signature
    );
    // e um .so aleatório continua NÃO disparando tauri
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libapp.so"]), "tauri"),
        DetectionState::NotDetected
    );
}

/// Gap engine-22 (lua-engines/Defold): a build real do Defold produz
/// `libdmengine.so` (runtime dmengine do bob.jar); o registro esperava
/// `libdefold.so`, que nunca existiu na build oficial.
#[test]
fn defold_real_dmengine_library_is_a_signature() {
    // layout real reportado: lib/arm64-v8a/libdmengine.so + game.arcd/arci
    let r = detect(&[
        "lib/arm64-v8a/libdmengine.so",
        "assets/game.arcd",
        "assets/game.arci",
        "classes.dex",
    ]);
    assert_eq!(state(&r, "lua-engines"), DetectionState::Signature);
    // Solar2D (libcorona) continua casando — regra preservada
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libcorona.so"]), "lua-engines"),
        DetectionState::Signature
    );
    // lib aleatória não é engine Lua
    assert_eq!(
        state(&detect(&["lib/arm64-v8a/libengine.so"]), "lua-engines"),
        DetectionState::NotDetected
    );
}

/// Gap engine-32 (twa-pwa): um TWA real declara `asset_statements` como
/// `<meta-data>` do `<application>` (Digital Asset Links). O rd-apk agora
/// modela application-level meta-data e o detector a inventaria: a regra
/// signature ganhou `metadata:asset_statements`.
#[test]
fn twa_asset_statements_application_metadata_is_a_signature() {
    let registry = Registry::embedded().unwrap();
    let mut a = apk(&[]);
    a.manifest
        .application
        .meta_data
        .push(rd_apk::manifest::MetaData {
            name: Some("asset_statements".into()),
            resource: Some(0x7f_00_00_01),
            ..Default::default()
        });
    assert_eq!(
        state(&registry.detect(&a).unwrap(), "twa-pwa"),
        DetectionState::Signature
    );
    // metadata de componente continua válida (DEFAULT_URL no launcher)
    let mut b = apk_with_class(&[], "com.example.Main");
    b.manifest.application.components[0]
        .meta_data
        .push(rd_apk::manifest::MetaData {
            name: Some("android.support.customtabs.trusted.DEFAULT_URL".into()),
            value: Some("https://example.invalid/".into()),
            ..Default::default()
        });
    assert_eq!(
        state(&registry.detect(&b).unwrap(), "twa-pwa"),
        DetectionState::Signature
    );
    // sem marcadores → nada de TWA (ausência não é exclusão, apenas não-identificação)
    assert_eq!(state(&detect(&[]), "twa-pwa"), DetectionState::NotDetected);
}

/// Regressão do inventário: metadata de application E de componente são
/// somadas (nenhuma substitui a outra).
#[test]
fn metadata_inventory_merges_application_and_component_levels() {
    let registry = Registry::embedded().unwrap();
    let mut a = apk_with_class(&[], "com.example.Main");
    a.manifest
        .application
        .meta_data
        .push(rd_apk::manifest::MetaData {
            name: Some("asset_statements".into()),
            ..Default::default()
        });
    a.manifest.application.components[0]
        .meta_data
        .push(rd_apk::manifest::MetaData {
            name: Some("component.only.key".into()),
            ..Default::default()
        });
    let r = registry.detect(&a).unwrap();
    // ambas as chaves alimentam o mesmo inventário: TWA casa pela do
    // application; a do componente está lá para regras futuras.
    assert_eq!(state(&r, "twa-pwa"), DetectionState::Signature);
    let finding = r.engines.iter().find(|e| e.id == "twa-pwa").unwrap();
    let predicates: Vec<&str> = finding
        .evidence
        .iter()
        .flat_map(|ev| ev.matches.iter().map(|m| m.predicate.as_str()))
        .collect();
    assert!(predicates.contains(&"metadata:asset_statements"));
}
