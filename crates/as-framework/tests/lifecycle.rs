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
    make_with_package_source(language, false)
}
#[allow(clippy::assertions_on_constants)]
fn make_with_package_source(language: &str, bind_source: bool) -> (App, Rc<TestClock>, PathBuf) {
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
    let app = App::new_with_options(
        pm.load_apk(&installed.package, None).unwrap().1,
        storage,
        clock.clone(),
        640,
        360,
        None,
        as_framework::AppOptions {
            source_apk: bind_source.then(|| {
                format!(
                    "installed/{}/{:020}/package.apk",
                    installed.package, installed.revision
                )
            }),
            ..Default::default()
        },
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
fn unavailable_service_is_null_and_unsupported_overload_reports_exact_descriptor() {
    let (mut app, _, _) = make("java");
    app.launch().unwrap();
    let activity = app.android.views().iter().find_map(|v| v.listener).unwrap();
    let missing = app
        .vm
        .invoke_instance(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[rd_vm::Value::StrPlaceholder("camera".into())],
        )
        .unwrap();
    assert!(matches!(missing, rd_vm::Value::Null));
    let service_class = app.vm.device_class_object("Ljava/lang/String;").unwrap();
    let e = app
        .vm
        .invoke_instance(
            activity,
            "getSystemService",
            "(Ljava/lang/Class;)Ljava/lang/Object;",
            &[rd_vm::Value::Obj(service_class)],
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
#[ignore = "requires scripts/build-m5-fixture.ps1"]
fn package_context_directories_and_class_loader_use_real_storage_and_vm_metadata() {
    use rd_vm::Value;
    let (mut app, _, root) = make("java");
    app.launch().unwrap();
    let activity = app.android.activity_object().unwrap();
    let package = app.android.apk.manifest.package.clone();
    for (method, directory) in [
        ("getFilesDir", "files"),
        ("getCacheDir", "cache"),
        ("getCodeCacheDir", "code_cache"),
        ("getNoBackupFilesDir", "no_backup"),
    ] {
        let file = app
            .vm
            .invoke_instance(activity, method, "()Ljava/io/File;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        let path = app
            .vm
            .invoke_instance(file, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        assert_eq!(
            app.vm.heap.as_str(path).unwrap(),
            format!("/data/user/0/{package}/{directory}")
        );
        assert!(root
            .join(format!("userdata/{package}/{directory}"))
            .is_dir());
    }
    let directory = app
        .vm
        .invoke_instance(
            activity,
            "getDir",
            "(Ljava/lang/String;I)Ljava/io/File;",
            &[
                Value::StrPlaceholder("shared_contract".into()),
                Value::Int(0),
            ],
        )
        .unwrap();
    assert!(directory.as_ref().unwrap().is_some());
    assert!(root
        .join(format!("userdata/{package}/app_shared_contract"))
        .is_dir());
    let error = app
        .vm
        .invoke_instance(
            activity,
            "getDir",
            "(Ljava/lang/String;I)Ljava/io/File;",
            &[Value::StrPlaceholder("../escape".into()), Value::Int(0)],
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("IllegalArgumentException"), "{error}");
    let loader = app
        .vm
        .invoke_instance(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let class = app
        .vm
        .invoke_instance(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[Value::StrPlaceholder(app.entry.clone())],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let same_loader = app
        .vm
        .invoke_instance(class, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(same_loader, loader);
    let string_class = app
        .vm
        .invoke_instance(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[Value::StrPlaceholder("java.lang.String".into())],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let boot = app
        .vm
        .invoke_instance(
            string_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_ne!(boot, loader);
    assert_eq!(
        app.vm.heap.class_of(boot).unwrap(),
        "Ljava/lang/BootClassLoader;"
    );
    assert_eq!(
        app.vm
            .invoke_instance(
                boot,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[Value::StrPlaceholder("java.lang.String".into())],
            )
            .unwrap(),
        Value::Obj(string_class)
    );
    let boot_error = app
        .vm
        .invoke_instance(
            boot,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[Value::StrPlaceholder(app.entry.clone())],
        )
        .unwrap_err()
        .to_string();
    assert!(
        boot_error.contains("ClassNotFoundException"),
        "{boot_error}"
    );
    for (descriptor, expected) in [
        ("I".to_string(), Value::Null),
        ("[I".to_string(), Value::Obj(boot)),
        (
            format!("[L{};", app.entry.replace('.', "/")),
            Value::Obj(loader),
        ),
    ] {
        let array_or_primitive = app.vm.device_class_object(&descriptor).unwrap();
        assert_eq!(
            app.vm
                .invoke_instance(
                    array_or_primitive,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[]
                )
                .unwrap(),
            expected
        );
    }
    let error = app
        .vm
        .invoke_instance(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[Value::StrPlaceholder("org.example.DoesNotExist".into())],
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("ClassNotFoundException"), "{error}");
    app.vm.gc();
    let loader_after_gc = app
        .vm
        .invoke_instance(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(loader, loader_after_gc);
    assert_eq!(
        app.vm.heap.class_of(boot).unwrap(),
        "Ljava/lang/BootClassLoader;"
    );
    app.android.stop(&mut app.vm).unwrap();
}

#[test]
#[ignore = "requires scripts/build-m5-fixture.ps1"]
fn package_application_info_requires_bound_installation_instead_of_fabricating_source_path() {
    let (mut app, _, _) = make("java");
    app.launch().unwrap();
    let activity = app.android.activity_object().unwrap();
    let error = app
        .vm
        .invoke_instance(
            activity,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            &[],
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("API_UNAVAILABLE") && error.contains("source path"),
        "{error}"
    );
}

#[test]
#[ignore = "requires scripts/build-m5-fixture.ps1"]
fn package_application_info_source_alias_opens_the_installed_apk_read_only_and_survives_gc() {
    use rd_vm::Value;
    let (mut app, _, _) = make_with_package_source("java", true);
    app.launch().unwrap();
    let activity = app.android.activity_object().unwrap();
    let info = app
        .vm
        .invoke_instance(
            activity,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let package = app.android.apk.manifest.package.clone();
    for (field, expected) in [
        ("packageName", package.clone()),
        ("sourceDir", format!("/data/app/{package}/base.apk")),
        ("publicSourceDir", format!("/data/app/{package}/base.apk")),
        ("dataDir", format!("/data/user/0/{package}")),
    ] {
        let object = app
            .vm
            .heap
            .get_field(info, field)
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        assert_eq!(app.vm.heap.as_str(object).unwrap(), expected);
    }
    assert!(
        matches!(app.vm.heap.get_field(info, "targetSdkVersion").unwrap(), Value::Int(target) if target == app.android.apk.manifest.target_sdk as i32)
    );
    let source = app.vm.heap.get_field(info, "sourceDir").unwrap();
    let path = app
        .vm
        .heap
        .as_str(source.as_ref().unwrap().unwrap())
        .unwrap()
        .to_string();
    let filesystem = app.vm.guest_filesystem().unwrap();
    assert_eq!(
        filesystem.stat(&path).unwrap().unwrap().length,
        app.android.apk.file_size_bytes
    );
    let mut bytes = [0; 4];
    assert_eq!(filesystem.read_at(&path, 0, &mut bytes).unwrap(), 4);
    assert_eq!(&bytes, b"PK\x03\x04");
    let error = filesystem.write_at(&path, 0, b"corrupt", true).unwrap_err();
    assert_eq!(error.code, "READ_ONLY_FILESYSTEM");
    let stream = app
        .vm
        .heap
        .alloc_instance("Ljava/io/FileInputStream;".into(), vec![])
        .unwrap();
    app.vm
        .invoke_instance(
            stream,
            "<init>",
            "(Ljava/lang/String;)V",
            std::slice::from_ref(&source),
        )
        .unwrap();
    for byte in bytes {
        assert!(
            matches!(app.vm.invoke_instance(stream, "read", "()I", &[]).unwrap(), Value::Int(actual) if actual == byte as i32)
        );
    }
    app.vm.invoke_instance(stream, "close", "()V", &[]).unwrap();
    let output = app
        .vm
        .heap
        .alloc_instance("Ljava/io/FileOutputStream;".into(), vec![])
        .unwrap();
    let error = app
        .vm
        .invoke_instance(output, "<init>", "(Ljava/lang/String;)V", &[source])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("IOException") && error.contains("READ_ONLY_FILESYSTEM"),
        "{error}"
    );
    let mut after_writes = [0; 4];
    app.vm
        .guest_filesystem()
        .unwrap()
        .read_at(&path, 0, &mut after_writes)
        .unwrap();
    assert_eq!(&after_writes, b"PK\x03\x04");
    app.vm.gc();
    let after_gc = app
        .vm
        .invoke_instance(
            activity,
            "getApplicationInfo",
            "()Landroid/content/pm/ApplicationInfo;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_eq!(info, after_gc);
    app.android.stop(&mut app.vm).unwrap();
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
