#![cfg(feature = "host")]
use as_launcher::{Key, Shell, Tab};
use as_platform::{
    fs::{self, FileSystem},
    host_fs::HostFileSystem,
};
use std::{
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[test]
#[ignore = "requires scripts/build-m5-fixture.ps1"]
fn console_install_launch_update_failure_remove_and_clear_data() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/launcher-tests/{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let mut shell = Shell::new(fs.clone()).unwrap();
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local-artifacts/m5/fixture/java-ui.apk"),
    )
    .unwrap();
    fs::write_all(fs.as_ref(), "apks/a.apk", &bytes).unwrap();
    shell.key(Key::Refresh);
    shell.key(Key::Right);
    assert_eq!(shell.snapshot().tab, Tab::Apks);
    shell.key(Key::Accept);
    assert_eq!(shell.snapshot().installed.len(), 1);
    assert_eq!(shell.snapshot().tab, Tab::Installed);
    let first = shell.key(Key::Accept).unwrap();
    assert_eq!(first.package, "org.fixture.uiproof");
    fs::write_all(
        fs.as_ref(),
        "userdata/org.fixture.uiproof/files/save",
        b"persistent",
    )
    .unwrap();
    shell.key(Key::Right);
    shell.key(Key::Accept);
    let revision = shell.snapshot().installed[0].revision;
    assert_eq!(revision, first.revision);
    assert!(shell.snapshot().message.contains("Already installed"));
    assert_eq!(shell.key(Key::Accept).unwrap().revision, revision);
    fs::write_all(fs.as_ref(), "apks/a.apk", b"broken").unwrap();
    shell.key(Key::Right);
    shell.key(Key::Accept);
    assert_eq!(shell.snapshot().installed[0].revision, revision);
    assert!(shell.snapshot().message.contains("INVALID_APK"));
    shell.key(Key::Left);
    shell.key(Key::Remove);
    shell.key(Key::Back);
    assert_eq!(shell.snapshot().installed.len(), 1);
    shell.key(Key::Remove);
    shell.key(Key::Accept);
    assert!(shell.snapshot().installed.is_empty());
    assert_eq!(
        fs::read_all(fs.as_ref(), "userdata/org.fixture.uiproof/files/save", 32).unwrap(),
        b"persistent"
    );
    fs::write_all(fs.as_ref(), "apks/a.apk", &bytes).unwrap();
    shell.key(Key::Right);
    shell.key(Key::Accept);
    shell.key(Key::ClearData);
    shell.key(Key::Accept);
    assert!(fs
        .stat("userdata/org.fixture.uiproof/files/save")
        .unwrap()
        .is_none());
    fs.remove("apks/a.apk", false).unwrap();
    shell.key(Key::Refresh);
    assert!(shell.key(Key::Accept).is_some());
}
