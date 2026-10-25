//! Public CLI contracts for Appendices D (`rd jetpack`) and E (`rd gms`).

use std::path::PathBuf;
use std::process::{Command, Output};

struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rd-jetpack-gms-{tag}-{}-{}",
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
fn embedded_appendix_d_check_works_outside_the_repository() {
    let t = Temp::new("jetpack-check");
    let v = json(&t.run(&["jetpack", "check", "--json"]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"], "data/jetpack.toml");
    assert_eq!(v["categories"], 6);
    assert!(v["libraries"].as_u64() >= Some(44), "{v}");
    assert_eq!(v["execution_claimed"], false);
    assert!(
        v["by_status"]["not_implemented"].as_u64() > v["by_status"]["supported"].as_u64(),
        "the registry must not pretend Jetpack is done: {v}"
    );
}

#[test]
fn jetpack_show_classifies_by_longest_prefix() {
    let t = Temp::new("jetpack-show");
    let live = json(&t.run(&[
        "jetpack",
        "show",
        "Landroidx/lifecycle/MutableLiveData;",
        "--json",
    ]));
    assert_eq!(live["library"]["id"], "androidx-lifecycle-livedata");
    assert_eq!(live["classification"]["category"], "architecture");

    let room = json(&t.run(&["jetpack", "show", "androidx.room.Room", "--json"]));
    assert_eq!(room["library"]["id"], "androidx-room");
    assert_eq!(room["library"]["status"], "partial");
    assert_eq!(room["classification"]["strategy"], "host-shim");

    // Gson is Appendix D third-party, never GMS/Firebase.
    let gson = json(&t.run(&["jetpack", "show", "com.google.gson.Gson", "--json"]));
    assert_eq!(gson["library"]["id"], "gson");
}

#[test]
fn jetpack_unknown_query_is_a_typed_error_not_silent_success() {
    let t = Temp::new("jetpack-unknown");
    let out = t.run(&["jetpack", "show", "androidx.future.NewLib", "--json"]);
    assert!(!out.status.success(), "unknown type must fail typed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("code") || stderr.contains("cause"),
        "{stderr}"
    );
}

#[test]
fn embedded_appendix_e_check_enforces_honest_mode() {
    let t = Temp::new("gms-check");
    let v = json(&t.run(&["gms", "check", "--json"]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"], "data/gms.toml");
    assert_eq!(v["mode"], "no-gms-honest");
    assert_eq!(v["network"], "none");
    assert_eq!(v["categories"], 3);
    assert!(v["services"].as_u64() >= Some(18), "{v}");
    assert_eq!(v["execution_claimed"], false);
}

#[test]
fn gms_show_classifies_by_longest_prefix_and_keeps_boundaries() {
    let t = Temp::new("gms-show");
    let analytics = json(&t.run(&[
        "gms",
        "show",
        "Lcom/google/firebase/analytics/FirebaseAnalytics;",
        "--json",
    ]));
    assert_eq!(analytics["service"]["id"], "firebase-analytics");
    assert_eq!(analytics["service"]["network"], "none");
    assert_eq!(analytics["classification"]["network"], "none");

    let tasks = json(&t.run(&["gms", "show", "com.google.android.gms.tasks.Task", "--json"]));
    assert_eq!(tasks["service"]["id"], "gms-tasks");

    let billing = json(&t.run(&[
        "gms",
        "show",
        "com.android.billingclient.api.BillingClient",
        "--json",
    ]));
    assert_eq!(billing["service"]["id"], "gms-billing");

    // Gson is not GMS: a typed failure, not a fake match.
    let out = t.run(&["gms", "show", "com.google.gson.Gson", "--json"]);
    assert!(!out.status.success());
}

#[test]
fn list_filters_and_invalid_status_are_typed() {
    let t = Temp::new("filters");
    let arch = json(&t.run(&["jetpack", "list", "--category", "architecture", "--json"]));
    let libs = arch["libraries"].as_array().unwrap();
    assert!(
        !libs.is_empty() && libs.iter().all(|l| l["category"] == "architecture"),
        "{arch}"
    );

    let bad = t.run(&["jetpack", "list", "--status", "done", "--json"]);
    assert!(!bad.status.success());
    let missing = t.run(&["gms", "list", "--category", "nope", "--json"]);
    assert!(!missing.status.success());
}

#[test]
fn reports_render_from_the_same_data() {
    let t = Temp::new("reports");
    let out = t.run(&["jetpack", "report"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("data/jetpack.toml"), "{text}");
    assert!(text.contains("recyclerview"), "{text}");

    let out = t.run(&["gms", "report"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("sem GMS"), "{text}");
    assert!(text.contains("data/gms.toml"), "{text}");
}

#[test]
fn explicit_data_dir_requires_the_new_tomls() {
    // --data pointing at a dir without jetpack.toml must NOT silently fall
    // back to the embedded registry (same contract as Appendix C, #38/#51).
    let t = Temp::new("data-require");
    let empty = t.0.join("empty-data");
    std::fs::create_dir_all(&empty).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .current_dir(&t.0)
        .env_remove("RD_DATA_DIR")
        .args([
            "--data",
            empty.to_str().unwrap(),
            "jetpack",
            "check",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("jetpack.toml"), "{stderr}");
}
