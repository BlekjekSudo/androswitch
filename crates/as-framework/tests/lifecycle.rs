use as_framework::App;
use as_platform::{host_fs::HostFileSystem, Clock};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Default)]
struct TestClock(Cell<u64>);
impl Clock for TestClock {
    fn monotonic_micros(&self) -> u64 {
        self.0.get() * 1000
    }
}
#[allow(clippy::assertions_on_constants)] // Deliberately diagnose Cargo feature unification at runtime.
fn make(language: &str) -> (App, Rc<TestClock>, PathBuf) {
    assert!(
        rd_vm::PORTABLE_CORE,
        "run -p as-framework in isolation to avoid rd-vm/std feature unification"
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/m5-tests/{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let storage = Rc::new(HostFileSystem::new(&root).unwrap());
    let bytes = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../local-artifacts/m5/fixture/{language}-ui.apk"
    )))
    .expect("run scripts/build-m5-fixture.ps1");
    let pm = as_package::PackageManager::new(storage.clone());
    let installed = pm.install(bytes, true, None).unwrap();
    let clock = Rc::new(TestClock::default());
    let app = App::new(
        pm.load_apk(&installed.package, None).unwrap().1,
        storage,
        clock.clone(),
        640,
        360,
        None,
    )
    .unwrap();
    (app, clock, root)
}
fn has(app: &App, text: &str) -> bool {
    app.android.views().iter().any(|v| v.text == text)
}
fn click(app: &mut App, x: f32, y: f32) {
    app.android.touch(&mut app.vm, 0, x, y).unwrap();
    app.android.touch(&mut app.vm, 1, x, y).unwrap();
    app.draw().unwrap();
}
#[test]
#[ignore = "requires built APK fixtures"]
fn dex_xml_lifecycle_handler_canvas_gc_and_restart_share_one_state_graph() {
    let (mut app, clock, root) = make("java");
    app.launch().unwrap();
    assert!(has(&app, "Count 0 / app 1"));
    click(&mut app, 100., 64.);
    assert!(has(&app, "Count 1 / app 1"));
    clock.0.set(1199);
    app.draw().unwrap();
    assert!(has(&app, "Count 1 / app 1"));
    clock.0.set(1200);
    app.draw().unwrap();
    assert!(has(&app, "Ready 1 / app 1"));
    let hash = app.frame.checksum();
    app.vm.gc();
    app.draw().unwrap();
    assert_eq!(app.frame.checksum(), hash);
    click(&mut app, 100., 110.);
    assert!(has(&app, "Second activity / count 1"));
    assert!(!has(&app, "Ready 1 / app 1"));
    app.android.back(&mut app.vm).unwrap();
    app.draw().unwrap();
    assert!(has(&app, "Ready 1 / app 1"));
    let events = app.android.lifecycle();
    let last = events
        .iter()
        .rev()
        .take(7)
        .rev()
        .map(|e| e.event.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        last,
        [
            "onStop",
            "onPause",
            "onRestart",
            "onStart",
            "onResume",
            "onStop",
            "onDestroy"
        ]
    );
    app.vm.gc();
    app.draw().unwrap();
    assert_eq!(app.frame.checksum(), hash);
    assert_eq!(app.frame.pixels[170 * 640 + 40], 0xff2ad6a1);
    click(&mut app, 60., 190.);
    assert_eq!(app.frame.pixels[170 * 640 + 40], 0xffe47664);
    let apk = app.android.apk.clone();
    drop(app);
    let mut restarted = App::new(
        apk,
        Rc::new(HostFileSystem::new(root).unwrap()),
        Rc::new(TestClock::default()),
        640,
        360,
        None,
    )
    .unwrap();
    restarted.launch().unwrap();
    assert!(has(&restarted, "Count 1 / app 1"));
}
#[test]
#[ignore = "requires built APK fixtures"]
fn kotlin_listener_survives_collection_and_uses_same_framework() {
    let (mut app, _, _) = make("kotlin");
    app.launch().unwrap();
    assert!(has(&app, "Kotlin ready"));
    app.android.key(&mut app.vm, 23).unwrap();
    app.vm.gc();
    app.android.key(&mut app.vm, 23).unwrap();
    app.draw().unwrap();
    assert!(has(&app, "Kotlin tapped 2"));
}
#[test]
#[ignore = "requires built APK fixtures"]
fn unavailable_framework_method_reports_exact_descriptor() {
    let (mut app, _, _) = make("java");
    app.launch().unwrap();
    let activity = app.android.views().iter().find_map(|v| v.listener).unwrap();
    let e = app
        .vm
        .invoke_instance(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[rd_vm::Value::StrPlaceholder("camera".into())],
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("getSystemService"), "{e}");
    assert!(e.contains("NOT_FOUND"), "{e}");
}

#[test]
#[ignore = "requires built APK fixtures"]
fn abort_startup_cancels_handler_without_delivering_lifecycle_callbacks() {
    let (mut app, clock, _) = make("java");
    app.launch().unwrap();
    let events = app.android.lifecycle().len();
    app.android.abort_startup().unwrap();
    clock.0.set(2000);
    app.draw().unwrap();
    assert!(has(&app, "Count 0 / app 1"));
    assert_eq!(app.android.lifecycle().len(), events);
}

#[test]
#[ignore = "requires scripts/build-large-apk-fixture.py"]
fn large_file_backed_apk_launches_and_shares_streamed_assets_after_restart() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/m7-large-tests/{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let storage = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(storage.clone());
    pm.initialize().unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local-artifacts/m7-improvements/fixture/large-ui.apk"),
        root.join("apks/large.apk"),
    )
    .unwrap();
    let record = pm.install_file("apks/large.apk", true, None).unwrap();
    assert!(record.apk_bytes > 64 << 20);
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        storage.clone(),
        Rc::new(TestClock::default()),
        640,
        360,
        None,
    )
    .unwrap();
    assert!(app.android.apk.zip.is_file_backed());
    app.launch().unwrap();
    assert!(has(&app, "Count 0 / app 1"));
    click(&mut app, 100., 64.);
    let mut asset = app
        .android
        .apk
        .zip
        .stream("assets/big-payload.bin")
        .unwrap();
    assert!(asset.len() > 64 << 20);
    asset.seek(65520).unwrap();
    let mut actual = [0; 128];
    assert_eq!(asset.read(&mut actual).unwrap(), actual.len());
    let mut block = vec![0xa5; 65536];
    let label = b"AndroSwitch archive streaming fixture\n";
    block[..label.len()].copy_from_slice(label);
    let expected = (65520..65648)
        .map(|i| block[i % block.len()])
        .collect::<Vec<_>>();
    assert_eq!(actual.as_slice(), expected);
    app.android.stop(&mut app.vm).unwrap();
    drop(app);
    let mut next = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        storage,
        Rc::new(TestClock::default()),
        640,
        360,
        None,
    )
    .unwrap();
    next.launch().unwrap();
    assert!(has(&next, "Count 1 / app 1"));
    assert_eq!(
        pm.install_file("apks/large.apk", true, None)
            .unwrap()
            .revision,
        record.revision
    );
}
