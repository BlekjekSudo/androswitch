//! Public CLI contracts for Appendix C (`rd api-coverage`).

use std::path::PathBuf;
use std::process::{Command, Output};

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rd-api-coverage-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rd"))
            .current_dir(&self.0)
            .env_remove("RD_DATA_DIR")
            .args(args)
            .output()
            .unwrap()
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

#[test]
fn embedded_appendix_c_check_works_outside_the_repository() {
    let t = Temp::new("check");
    let v = json(&t.run(&["api-coverage", "check", "--json"]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"], "data/api-coverage.toml");
    assert_eq!(v["domains"], 17);
    assert!(v["surfaces"].as_u64() >= Some(100), "{v}");
    assert_eq!(v["execution_claimed"], false);
    assert!(
        v["by_status"]["not_implemented"].as_u64() > v["by_status"]["supported"].as_u64(),
        "the registry must not pretend the whole Android surface is done: {v}"
    );
}

#[test]
fn show_classifies_descriptors_and_methods_by_longest_prefix() {
    let t = Temp::new("show");
    let build = json(&t.run(&[
        "api-coverage",
        "show",
        "Landroid/os/Build$VERSION;",
        "--json",
    ]));
    assert_eq!(build["surface"]["id"], "android-os-build");
    assert_eq!(build["classification"]["status"], "supported");
    assert_eq!(build["classification"]["domain"], "os-ipc");

    let method = json(&t.run(&[
        "api-coverage",
        "show",
        "android.app.Activity.setContentView",
        "--json",
    ]));
    assert_eq!(method["match"], "longest-prefix");
    assert_eq!(method["surface"]["prefix"], "android.app");
    assert_eq!(method["classification"]["domain"], "app-lifecycle");
    assert_eq!(method["classification"]["status"], "partial");
    assert_eq!(method["execution_claimed"], false);
}

#[test]
fn list_filters_by_domain_and_status() {
    let t = Temp::new("list");
    let v = json(&t.run(&[
        "api-coverage",
        "list",
        "--domain",
        "database",
        "--status",
        "partial",
        "--json",
    ]));
    let surfaces = v["surfaces"].as_array().unwrap();
    assert!(!surfaces.is_empty(), "{v}");
    assert!(surfaces
        .iter()
        .all(|s| s["domain"] == "database" && s["status"] == "partial"));
    assert!(surfaces
        .iter()
        .any(|s| s["prefix"] == "android.database.sqlite"));
}

#[test]
fn explicit_data_dir_never_falls_back_silently() {
    let t = Temp::new("data");
    std::fs::create_dir_all(t.0.join("empty")).unwrap();
    let out = t.run(&["--data", "empty", "api-coverage", "check", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stdout.is_empty(),
        "errors do not emit a success artifact"
    );
    let e: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(e["code"], "IO_ERROR");
    assert!(e["cause"].as_str().unwrap().contains("api-coverage.toml"));

    // The same explicit door succeeds with the real versioned table copied in.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/api-coverage.toml");
    std::fs::create_dir_all(t.0.join("custom")).unwrap();
    std::fs::copy(source, t.0.join("custom/api-coverage.toml")).unwrap();
    let v = json(&t.run(&["--data", "custom", "api-coverage", "check", "--json"]));
    assert_eq!(v["domains"], 17);
}

#[test]
fn report_writes_the_generated_markdown_artifact() {
    let t = Temp::new("report");
    let out = t.run(&["api-coverage", "report", "--out", "coverage.md"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = std::fs::read_to_string(t.0.join("coverage.md")).unwrap();
    assert!(md.starts_with("# Android API coverage (Apêndice C)"));
    assert!(md.contains("`supported` significa modelo host com estado"));
    assert!(md.contains("android.database.sqlite"));
}
