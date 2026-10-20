//! Public CLI contracts for all Appendix G registry routes.

use rd_testsupport::apkfix::{build_manifest_axml_sdk, ZipFix};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rd-engine-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rd"));
        c.current_dir(&self.0).env_remove("RD_DATA_DIR");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn apk(&self, min_sdk: i32) {
        let mut z = ZipFix::new();
        z.add(
            "AndroidManifest.xml",
            build_manifest_axml_sdk("com.example.hybrid", "com.example.Main", min_sdk, 36),
        );
        for path in [
            "classes.dex",
            "lib/arm64-v8a/libflutter.so",
            "assets/flutter_assets/AssetManifest.bin",
            "assets/modules/demo.wasm",
        ] {
            z.add(path, b"not-executable".to_vec());
        }
        std::fs::write(self.0.join("fixture.apk"), z.build()).unwrap();
    }
    fn data(&self, text: &str) {
        std::fs::create_dir_all(self.0.join("custom")).unwrap();
        std::fs::write(self.0.join("custom/engines.toml"), text).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn json(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout contains only the JSON artifact")
}
fn error(out: &Output, code: i32, kind: &str) -> serde_json::Value {
    assert_eq!(out.status.code(), Some(code));
    assert!(
        out.stdout.is_empty(),
        "an error cannot emit a successful artifact"
    );
    let e: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(e["code"], kind);
    assert!(e["cause"].is_string());
    assert!(e["module_id"].is_string());
    e
}

#[test]
fn embedded_catalog_is_complete_even_when_run_outside_the_repository() {
    let t = Temp::new("list");
    let v = json(&t.run(&["engine", "list", "--json"]));
    assert_eq!(v["registered_engines"], 28);
    assert_eq!(v["modules"].as_array().unwrap().len(), 12);
    assert_eq!(v["engines"].as_array().unwrap().len(), 28);
    assert_eq!(v["scope"], "registry-and-detection-only");
    assert_eq!(v["execution_claimed"], false);
    for engine in v["engines"].as_array().unwrap() {
        assert_eq!(
            engine["registry_adapter_status"],
            if rd_engine_registry::native_execution_available(engine["id"].as_str().unwrap()) {
                "AVAILABLE_BOUNDED"
            } else {
                "NOT_IMPLEMENTED"
            }
        );
        assert!(engine["feature_enabled"].is_boolean());
    }
    let out = t.run(&["engine", "list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("detecção NÃO é execução"));
}

#[test]
fn detect_and_inspect_json_share_the_exact_report_without_executing_payloads() {
    let t = Temp::new("detect");
    t.apk(26);
    let detected = json(&t.run(&["engine", "detect", "fixture.apk", "--json"]));
    let inspected = json(&t.run(&["inspect", "fixture.apk", "--json"]));
    assert_eq!(inspected["engines"], detected);
    assert_eq!(detected["artifact_validation"], "not-performed");
    assert_eq!(detected["execution_claimed"], false);
    let flutter = detected["engines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "flutter")
        .unwrap();
    assert_eq!(flutter["state"], "signature");
    assert_eq!(flutter["runtime_status"], "not_implemented");
    let out = t.run(&["engine", "detect", "fixture.apk"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("payload NÃO validado"));
}

#[test]
fn detection_is_allowed_below_floor_since_it_does_not_execute_the_apk() {
    let t = Temp::new("belowfloor");
    t.apk(23);
    let v = json(&t.run(&["engine", "detect", "fixture.apk", "--json"]));
    assert_eq!(v["execution_claimed"], false);
    assert_eq!(v["registered_engines"], 28);
}

#[test]
fn check_and_invalid_ids_keep_the_shared_exit_code_error_contract() {
    let t = Temp::new("check");
    let e = error(
        &t.run(&["engine", "check", "flutter", "--json"]),
        2,
        "NOT_IMPLEMENTED",
    );
    assert_eq!(e["module_id"], "LANG-04");
    assert!(e["suggestion"].is_string());
    error(
        &t.run(&["engine", "check", "no-such-engine"]),
        1,
        "UNKNOWN_ENGINE",
    );
    let bad_usage = t.run(&["engine", "detect"]);
    assert_eq!(bad_usage.status.code(), Some(64));
}

#[test]
fn explicit_data_and_env_never_silently_fall_back_to_embedded_catalog() {
    let t = Temp::new("missing-data");
    std::fs::create_dir_all(t.0.join("empty")).unwrap();
    let e = error(
        &t.run(&["--data", "empty", "engine", "list", "--json"]),
        1,
        "IO_ERROR",
    );
    assert!(e["cause"].as_str().unwrap().contains("engines.toml"));
    let out = t
        .command()
        .env("RD_DATA_DIR", "empty")
        .args(["engine", "list", "--json"])
        .output()
        .unwrap();
    error(&out, 1, "IO_ERROR");
    t.data("invalid catalog = [");
    error(
        &t.run(&["--data", "custom", "engine", "list", "--json"]),
        1,
        "INVALID_ENGINE_CATALOG",
    );
}

#[test]
fn explicit_catalog_wins_over_env_and_is_used_by_detection() {
    let t = Temp::new("override");
    t.apk(26);
    t.data(
        &include_str!("../../../data/engines.toml")
            .replace("name = \"Java\"", "name = \"Java override\""),
    );
    let out = t
        .command()
        .env("RD_DATA_DIR", "missing")
        .args(["--data", "custom", "engine", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(json(&out)["engines"][0]["name"], "Java override");
    let v = json(&t.run(&[
        "--data",
        "custom",
        "engine",
        "detect",
        "fixture.apk",
        "--json",
    ]));
    assert_eq!(v["engines"][0]["name"], "Java override");
}

#[test]
fn corrupt_or_missing_apks_are_typed_errors_not_detection_success() {
    let t = Temp::new("bad-apk");
    error(
        &t.run(&["engine", "detect", "missing.apk", "--json"]),
        1,
        "IO_ERROR",
    );
    std::fs::write(t.0.join("bad.apk"), b"not a zip").unwrap();
    error(
        &t.run(&["engine", "detect", "bad.apk", "--json"]),
        1,
        "PARSE_ERROR",
    );
}

#[test]
fn optional_engine_limit_does_not_break_basic_inspect() {
    let temp = Temp::new("large-inventory");
    std::fs::write(
        temp.0.join("large.apk"),
        rd_testsupport::apkfix::build_large_inventory_apk(100_000),
    )
    .unwrap();
    let inspected = json(&temp.run(&["inspect", "large.apk", "--json"]));
    assert_eq!(inspected["package"], "com.example.inventory");
    assert_eq!(inspected["engines"]["analysis_status"], "not_analyzed");
    assert_eq!(inspected["engines"]["error"]["code"], "DETECTION_LIMIT");
    assert_eq!(inspected["engines"]["execution_claimed"], false);
    error(
        &temp.run(&["engine", "detect", "large.apk", "--json"]),
        1,
        "DETECTION_LIMIT",
    );
}

#[test]
fn engine_run_preserves_availability_and_file_error_precedence() {
    let t = Temp::new("run-unavailable");
    // LANG/feature selection must not turn an unavailable adapter into IO_ERROR.
    error(
        &t.run(&["engine", "run", "wasm", "missing.apk", "--json"]),
        2,
        "NOT_IMPLEMENTED",
    );
    for id in ["c-ndk", "cpp-ndk", "rust-ndk"] {
        // Use actual availability: Cargo may unify dependency features from
        // another workspace package even without the CLI's forwarding flag.
        let (exit, code) = if rd_engine_registry::native_execution_available(id) {
            (1, "IO_ERROR")
        } else {
            (2, "NOT_IMPLEMENTED")
        };
        error(
            &t.run(&["engine", "run", id, "missing.apk", "--json"]),
            exit,
            code,
        );
    }
}

#[cfg(feature = "native-engines")]
#[test]
#[ignore = "requires source-built APKs and Unicorn; set RD_NATIVE_LANGUAGE_APKS"]
fn source_native_cli_run_keeps_stdout_json_and_captures_guest_logs() {
    let t = Temp::new("run-native-json");
    let root = std::fs::canonicalize(
        std::env::var_os("RD_NATIVE_LANGUAGE_APKS")
            .expect("set RD_NATIVE_LANGUAGE_APKS to source-built APK directory"),
    )
    .expect("source-built APK directory must exist");
    for (id, file, language, tag) in [
        ("c-ndk", "native-c.apk", "C", 1),
        ("cpp-ndk", "native-cpp.apk", "C++", 2),
        ("rust-ndk", "native-rust.apk", "Rust", 3),
    ] {
        let out = t
            .command()
            .args(["engine", "run", id])
            .arg(root.join(file))
            .arg("--json")
            .output()
            .unwrap();
        let report = json(&out);
        assert!(
            out.stderr.is_empty(),
            "guest logs must be captured in the JSON report: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(report["engine"], id);
        assert_eq!(report["launched"], true);
        assert_eq!(report["execution_claimed"], true);
        assert!(report["native"]["instructions"].as_u64().unwrap() > 0);
        assert_eq!(report["native"]["missing_imports"], serde_json::json!([]));
        assert!(report["logs"]
            .as_str()
            .unwrap()
            .contains(&format!("Native {language} flavor={tag} boot=")));
        assert!(report["ui"]
            .as_str()
            .unwrap()
            .contains(&format!("{language} native workbench")));
    }
}
