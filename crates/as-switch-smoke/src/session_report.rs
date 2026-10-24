//! Reporting identity and ephemeral evidence ownership, shared by host tests and
//! the Horizon launcher. A stale report must never describe a different launch.
use alloc::{
    format,
    string::{String, ToString},
};
use as_platform::fs::FileSystem;
use serde_json::{json, Value};

pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub fn console_prefix(random_nonce: u64, monotonic_tick: u64) -> String {
    format!("console-{random_nonce:016x}-{monotonic_tick:016x}")
}
pub fn session_id(prefix: &str, serial: u64) -> Option<String> {
    let id = format!("{prefix}-{serial}");
    valid_session_id(&id).then_some(id)
}
pub fn valid_identity(value: &Value) -> bool {
    value["schema_version"] == 1
        && value["session_id"].as_str().is_some_and(valid_session_id)
        && value["package"]
            .as_str()
            .is_some_and(as_platform::vfs::valid_package)
        && value["revision"].as_u64().is_some_and(|n| n != 0)
}
pub fn identity_matches(report: &Value, request: &Value) -> bool {
    valid_identity(report)
        && valid_identity(request)
        && ["session_id", "package", "revision"]
            .iter()
            .all(|key| report[*key] == request[*key])
}
pub fn failure(request: &Value, message: &str) -> Value {
    json!({"schema_version":1,"status":"failed","error":message,
        "session_id":request["session_id"],"package":request["package"],
        "revision":request["revision"],
        "identity_status":if valid_identity(request){"known"}else{"missing_or_invalid"}})
}
/// Preserve the first execution failure when shutdown also fails. Cleanup
/// diagnostics cannot replace the application error that triggered shutdown.
pub fn cleanup_failure(primary: Option<&str>, cleanup: &str) -> String {
    match primary.filter(|primary| !primary.is_empty() && *primary != cleanup) {
        Some(primary) => format!("{primary}; cleanup: {cleanup}"),
        None => cleanup.to_string(),
    }
}
pub fn interrupted(request: &Value, report: Option<&Value>) -> Value {
    let matching = report.filter(|report| identity_matches(report, request));
    json!({"schema_version":1,"status":"interrupted",
        "session_id":request["session_id"],"package":request["package"],
        "revision":request["revision"],"request":request,
        "report_identity_match":matching.is_some(),"report":matching,
        "evidence":"pending marker found at launcher startup; current session-report may later change"})
}
pub fn reporting_error(expected: &Value, actual: Option<&Value>) -> Value {
    json!({"schema_version":1,"status":"reporting_error",
        "error":"SESSION_REPORT_IDENTITY_MISMATCH",
        "session_id":expected["session_id"],"package":expected["package"],
        "revision":expected["revision"],"expected":expected,"actual":actual})
}

/// Clear only current-application snapshots. Process-wide memory/display traces
/// and historical interruption evidence retain their separate lifetime.
pub fn clear_current_evidence(fs: &dyn FileSystem) -> Result<(), as_platform::PlatformError> {
    for path in [
        "logs/capture-owner.json",
        "logs/teardown.log",
        "logs/native-compatibility.json",
        "logs/memory-admission.json",
        "logs/reporting-error.json",
        "logs/gles-frame-2.ppm",
        "logs/gles-frame-30.ppm",
        "logs/last-frame.ppm",
    ] {
        if fs.stat(path)?.is_some() {
            fs.remove(path, false)?;
        }
    }
    Ok(())
}
pub fn clear_process_evidence(fs: &dyn FileSystem) -> Result<(), as_platform::PlatformError> {
    for path in ["logs/display-error.json", "logs/display-handoffs.log"] {
        if fs.stat(path)?.is_some() {
            fs.remove(path, false)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::BTreeMap, string::ToString, vec, vec::Vec};
    use as_platform::{
        fs::{Entry, Metadata},
        PlatformError,
    };
    use core::cell::RefCell;
    #[test]
    fn shutdown_diagnostic_keeps_the_first_application_failure() {
        assert_eq!(
            cleanup_failure(Some("renderer API unavailable"), "activity stop failed"),
            "renderer API unavailable; cleanup: activity stop failed"
        );
        assert_eq!(
            cleanup_failure(None, "worker join failed"),
            "worker join failed"
        );
        assert_eq!(
            cleanup_failure(Some(""), "worker join failed"),
            "worker join failed"
        );
        assert_eq!(
            cleanup_failure(Some("worker join failed"), "worker join failed"),
            "worker join failed"
        );
    }
    #[test]
    fn console_ids_distinguish_boots_and_scripts_keep_their_exact_prefix() {
        let first = console_prefix(17, 1000);
        assert_ne!(first, console_prefix(18, 1000));
        assert_ne!(first, console_prefix(17, 1001));
        assert_ne!(session_id(&first, 1), session_id(&first, 2));
        assert!(valid_session_id(&session_id(&first, u64::MAX).unwrap()));
        assert_eq!(session_id("e2e-guid", 2).as_deref(), Some("e2e-guid-2"));
        assert!(session_id("invalid/identity", 1).is_none());
        assert!(session_id(&"x".repeat(128), 1).is_none());
    }
    #[test]
    fn repeated_console_serial_does_not_merge_different_packages_or_revisions() {
        let request = json!({"schema_version":1,"session_id":"console-2","package":"com.example.current","revision":1});
        let mut report = request.clone();
        report["status"] = "failed".into();
        assert!(identity_matches(&report, &request));
        report["package"] = "org.fixture.previous".into();
        assert!(!identity_matches(&report, &request));
        report["package"] = request["package"].clone();
        report["revision"] = 2.into();
        assert!(!identity_matches(&report, &request));
        assert!(!identity_matches(&Value::Null, &Value::Null));
        let diagnostic = reporting_error(&request, Some(&report));
        assert_eq!(diagnostic["status"], "reporting_error");
        assert_ne!(diagnostic["status"], "failed");
    }
    #[test]
    fn interrupted_snapshot_keeps_only_a_matching_report_and_is_independent() {
        let request = json!({"schema_version":1,"session_id":"console-2","package":"org.fixture.previous","revision":1});
        let mut report = failure(&request, "previous callback failed");
        let snapshot = interrupted(&request, Some(&report));
        assert_eq!(snapshot["report_identity_match"], true);
        report["error"] = "new current report".into();
        assert_eq!(snapshot["report"]["error"], "previous callback failed");
        report["package"] = "com.example.current".into();
        let unmatched = interrupted(&request, Some(&report));
        assert_eq!(unmatched["package"], "org.fixture.previous");
        assert!(unmatched["report"].is_null());
        assert_eq!(unmatched["report_identity_match"], false);
        assert_eq!(
            failure(&Value::Null, "failure")["identity_status"],
            "missing_or_invalid"
        );
    }
    #[derive(Default)]
    struct MemoryFiles(RefCell<BTreeMap<String, Vec<u8>>>);
    impl FileSystem for MemoryFiles {
        fn stat(&self, p: &str) -> Result<Option<Metadata>, PlatformError> {
            Ok(self.0.borrow().get(p).map(|bytes| Metadata {
                directory: false,
                length: bytes.len() as u64,
            }))
        }
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, PlatformError> {
            unreachable!()
        }
        fn write_at(&self, _: &str, _: u64, _: &[u8], _: bool) -> Result<usize, PlatformError> {
            unreachable!()
        }
        fn create_dir(&self, _: &str, _: bool) -> Result<bool, PlatformError> {
            unreachable!()
        }
        fn list(&self, _: &str) -> Result<Vec<Entry>, PlatformError> {
            unreachable!()
        }
        fn rename_new(&self, _: &str, _: &str) -> Result<(), PlatformError> {
            unreachable!()
        }
        fn remove(&self, p: &str, recursive: bool) -> Result<(), PlatformError> {
            assert!(!recursive);
            self.0.borrow_mut().remove(p);
            Ok(())
        }
    }
    #[test]
    fn current_snapshot_cleanup_preserves_history_and_process_traces() {
        let files = MemoryFiles::default();
        for path in [
            "logs/capture-owner.json",
            "logs/gles-frame-2.ppm",
            "logs/teardown.log",
            "logs/native-compatibility.json",
            "logs/memory-admission.json",
            "logs/memory.log",
            "logs/last-interrupted.json",
            "logs/display-handoffs.log",
            "logs/display-error.json",
            "userdata/com.example.current/save",
        ] {
            files.0.borrow_mut().insert(path.to_string(), vec![1]);
        }
        clear_current_evidence(&files).unwrap();
        for path in [
            "logs/capture-owner.json",
            "logs/gles-frame-2.ppm",
            "logs/teardown.log",
            "logs/native-compatibility.json",
            "logs/memory-admission.json",
        ] {
            assert!(!files.0.borrow().contains_key(path));
        }
        assert!(files.0.borrow().contains_key("logs/display-handoffs.log"));
        clear_process_evidence(&files).unwrap();
        assert!(!files.0.borrow().contains_key("logs/display-handoffs.log"));
        for path in [
            "logs/memory.log",
            "logs/last-interrupted.json",
            "userdata/com.example.current/save",
        ] {
            assert!(files.0.borrow().contains_key(path));
        }
    }
}
