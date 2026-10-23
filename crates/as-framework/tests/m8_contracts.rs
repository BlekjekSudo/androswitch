use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use std::{path::PathBuf, rc::Rc};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/m8-tests/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn launch(pm: &as_package::PackageManager<Rc<HostFileSystem>>, package: &str) -> App {
    let apk = pm.load_apk(package, None).unwrap().1;
    let mut app = App::new(
        apk,
        pm.fs.clone(),
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    app.launch().unwrap();
    app
}
#[test]
#[ignore = "requires scripts/build-m8-fixture.ps1"]
fn dex_preferences_collections_gc_restart_and_renamed_package_share_contracts() {
    let root = root();
    let storage = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(storage);
    pm.initialize().unwrap();
    let artifacts =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m8/fixture");
    for (file, package) in [
        ("preferences.apk", "org.fixture.preferences"),
        ("renamed.apk", "net.example.renamednotes"),
    ] {
        std::fs::copy(artifacts.join(file), root.join("apks/import.apk")).unwrap();
        let record = pm.install_file("apks/import.apk", true, None).unwrap();
        assert_eq!(record.package, package);
        for count in 1..=2 {
            let mut app = launch(&pm, package);
            let label = app
                .android
                .views()
                .into_iter()
                .find(|v| v.text.starts_with("Prefs "))
                .unwrap()
                .text;
            assert!(label.contains(&format!("Prefs {count} /")), "{label}");
            assert!(label.ends_with("1200:2:survives:1"), "{label}");
            app.vm.gc();
            app.draw().unwrap();
            app.android.stop(&mut app.vm).unwrap();
        }
        let xml = std::fs::read_to_string(
            root.join(format!("userdata/{package}/shared_prefs/settings.xml")),
        )
        .unwrap();
        assert!(xml.contains("&amp;") && xml.contains("&lt;"));
        assert!(root.join(format!("userdata/{package}/shared_prefs/org.androswitch.persistence.PreferencesActivity.xml")).is_file());
    }
}
#[test]
#[ignore = "requires scripts/build-m8-fixture.ps1"]
fn dex_native_sqlite_helper_cursor_reflection_permissions_and_restart() {
    let root = root();
    let pm = as_package::PackageManager::new(Rc::new(HostFileSystem::new(&root).unwrap()));
    pm.initialize().unwrap();
    let artifacts =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local-artifacts/m8/fixture");
    let oracle = std::fs::read_to_string(artifacts.join("jvm-oracle.txt")).unwrap();
    let expected = oracle.lines().nth(1).unwrap();
    for (file, package) in [
        ("database.apk", "org.fixture.database"),
        ("database-renamed.apk", "net.example.renameddb"),
    ] {
        std::fs::copy(artifacts.join(file), root.join("apks/import.apk")).unwrap();
        pm.install_file("apks/import.apk", true, None).unwrap();
        for count in 1..=3 {
            let mut app = launch(&pm, package);
            let label = app
                .android
                .views()
                .into_iter()
                .find(|v| v.text.starts_with("Notes "))
                .unwrap()
                .text;
            assert_eq!(label, format!("Notes {count} / {expected}"));
            app.vm.gc();
            app.draw().unwrap();
            app.android.stop(&mut app.vm).unwrap();
        }
        let bytes =
            std::fs::read(root.join(format!("userdata/{package}/databases/notes.db"))).unwrap();
        assert!(bytes.starts_with(b"SQLite format 3\0"));
    }
}
