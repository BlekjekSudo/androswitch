use as_testsupport::apkfix::{build_apk_sdk, ArscFix};
use std::{
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "AndroSwitch CLI test {} {}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
    for field in [
        "install",
        "dex_execution",
        "native_execution",
        "device_verified",
        "horizon_emulator_verified",
    ] {
        assert_eq!(report[field], false, "{field}");
    }
}
