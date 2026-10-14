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
        assert_eq!(engine["registry_adapter_status"], "NOT_IMPLEMENTED");
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
