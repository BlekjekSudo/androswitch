use as_testsupport::apkfix::{build_apk_sdk, ArscFix};
use std::{
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "AndroSwitch CLI test {} {} {}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }
    fn apk(&self) -> PathBuf {
        let path = self.dir.join("example.apk");
        std::fs::write(
            &path,
            build_apk_sdk(
                "org.example.cli",
                "MainActivity",
                ArscFix::new().build(),
                vec![],
                21,
                35,
            ),
        )
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["example.apk", "invalid.apk"] {
            let _ = std::fs::remove_file(self.dir.join(name));
        }
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_asctl"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn json_inspection_has_one_parseable_result_even_with_spaces_in_path() {
    let fixture = Fixture::new();
    let path = fixture.apk();
    let output = run(&["inspect".as_ref(), path.as_os_str(), "--json".as_ref()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["manifest"]["package"], "org.example.cli");
    assert_eq!(report["preflight"]["sdk_allowed"], true);
    assert_eq!(report["preflight"]["execution_implemented"], false);
}

#[test]
fn missing_and_invalid_inputs_return_nonzero_with_structured_json() {
    let fixture = Fixture::new();
    for name in ["missing.apk", "invalid.apk"] {
        let path = fixture.dir.join(name);
        if name == "invalid.apk" {
            std::fs::write(&path, b"invalid").unwrap();
        }
        let output = run(&["inspect".as_ref(), path.as_os_str(), "--json".as_ref()]);
        assert_eq!(output.status.code(), Some(1));
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["status"], "error");
        assert_eq!(result["error"]["operation"], "inspect");
    }
}

#[test]
fn capabilities_do_not_turn_upstream_features_into_switch_claims() {
    let output = run(&["capabilities".as_ref(), "--json".as_ref()]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["apk_inspection"], true);
    assert_eq!(report["install"], true);
    for field in [
        "dex_execution",
        "native_execution",
        "device_verified",
        "horizon_emulator_verified",
    ] {
        assert_eq!(report[field], false, "{field}");
    }
}

#[test]
fn package_commands_persist_without_source_apk_and_uninstall_keeps_saves() {
    let fixture = Fixture::new();
    let apk = fixture.apk();
    let root = fixture.dir.join("runtime storage");
    let json = |args: &[&std::ffi::OsStr]| {
        let output = run(args);
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let installed = json(&[
        "install".as_ref(),
        apk.as_os_str(),
        "--root".as_ref(),
        root.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(installed["package"]["package"], "org.example.cli");
    assert_eq!(installed["package"]["signature_verified"], false);
    std::fs::remove_file(apk).unwrap();
    let listed = json(&[
        "list".as_ref(),
        "--root".as_ref(),
        root.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(listed["packages"][0], installed["package"]);
    let save = root.join("userdata/org.example.cli/files/save");
    std::fs::write(&save, b"retained").unwrap();
    json(&[
        "uninstall".as_ref(),
        "org.example.cli".as_ref(),
        "--root".as_ref(),
        root.as_os_str(),
        "--json".as_ref(),
    ]);
    let listed = json(&[
        "list".as_ref(),
        "--root".as_ref(),
        root.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(listed["packages"], serde_json::json!([]));
    assert_eq!(std::fs::read(save).unwrap(), b"retained");
    // This unique temporary directory was created by this fixture only.
    std::fs::remove_dir_all(root).unwrap();
}
