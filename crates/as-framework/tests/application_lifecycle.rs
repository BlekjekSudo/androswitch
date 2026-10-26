//! Actual Application/Activity DEX super calls, ordered observers and GC lifetime.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{cell::Cell, path::PathBuf, rc::Rc, thread::ThreadId};
const PROBE: &str = "Lorg/androswitch/contracts/LifecycleProbe;";
const APPLICATION: &str = "Lorg/androswitch/contracts/ObserverApplication;";
const OBSERVER: &str = "Landroid/app/Application$ActivityLifecycleCallbacks;";
const OBSERVER_ARG: &str = "(Landroid/app/Application$ActivityLifecycleCallbacks;)V";
struct Hooks {
    owner: ThreadId,
    collections: Cell<usize>,
    captured: Cell<Option<(ObjRef, ObjRef)>>,
}
impl NativeHost for Hooks {
    fn current_thread_id(&self) -> Option<u64> {
        (std::thread::current().id() == self.owner).then_some(1)
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error("API_UNAVAILABLE", "fixture has no native library loader").into())
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        is_static: bool,
        args: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(class, PROBE);
        assert!(is_static);
        match (name, sig) {
            ("collect", "()V") => {
                self.collections.set(self.collections.get() + 1);
                vm.gc();
            }
            ("capture", "(Ljava/lang/Object;Ljava/lang/Object;)V") => {
                self.captured.set(Some((
                    args[0].as_ref()?.unwrap(),
                    args[1].as_ref()?.unwrap(),
                )));
            }
            _ => panic!("unexpected fixture hook"),
        }
        Ok(Value::Null)
    }
}
#[allow(clippy::assertions_on_constants)]
fn app(fixture: &str, mode: i32) -> (App, Rc<Hooks>) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-application-observers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/application-lifecycle-fixture/{fixture}.apk"
    ));
    let record = pm
        .install(
            std::fs::read(file).expect("run fixtures/m9/application-lifecycle/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&record.package, None).unwrap().1,
        fs,
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    let hooks = Rc::new(Hooks {
        owner: std::thread::current().id(),
        collections: Cell::new(0),
        captured: Cell::new(None),
    });
    app.vm.bind_native(hooks.clone());
    app.vm
        .invoke_static(PROBE, "configure", "(I)V", &[Value::Int(mode)])
        .unwrap();
    (app, hooks)
}
fn field(app: &App, name: &str) -> Value {
    app.vm.statics[&(PROBE.into(), name.into())].clone()
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn log(app: &App) -> String {
    text(app, field(app, "log"))
}
fn call(app: &mut App, name: &str) -> Result<Value, VmExit> {
    app.vm.invoke_static(PROBE, name, "()V", &[])
}
fn register(app: &mut App, callback: Value) -> Result<Value, VmExit> {
    let application = field(app, "app").as_ref().unwrap().unwrap();
    app.vm.invoke_instance(
        application,
        "registerActivityLifecycleCallbacks",
        OBSERVER_ARG,
        &[callback],
    )
}
fn unregister(app: &mut App, callback: Value) -> Result<Value, VmExit> {
    let application = field(app, "app").as_ref().unwrap().unwrap();
    app.vm.invoke_instance(
        application,
        "unregisterActivityLifecycleCallbacks",
        OBSERVER_ARG,
        &[callback],
    )
}
fn thrown(result: Result<(), VmExit>, class: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(t)) if t.class==class),
        "expected {class}"
    );
}
#[test]
fn all_seven_notifications_run_inside_actual_super_calls_and_share_the_real_bundle() {
    for fixture in ["lifecycle", "renamed"] {
        let (mut app, _) = app(fixture, 0);
        app.launch().unwrap();
        assert_eq!(log(&app), "c<;AC;BC;c>;s<;AS;BS;s>;r<;AR;BR;r>;");
        assert_eq!(field(&app, "input"), Value::Null);
        let saved = app.save_state().unwrap();
        assert_eq!(field(&app, "saved"), Value::Obj(saved));
        assert_eq!(field(&app, "observedSaved"), Value::Obj(saved));
        for (name, expected) in [("before", 11), ("observer", 123), ("after", 22)] {
            assert_eq!(
                app.vm
                    .invoke_instance(
                        saved,
                        "getInt",
                        "(Ljava/lang/String;)I",
                        &[Value::StrPlaceholder(name.into())]
                    )
                    .unwrap(),
                Value::Int(expected)
            );
        }
        app.vm.gc();
        assert_eq!(field(&app, "saved"), Value::Obj(saved));
        app.android.stop(&mut app.vm).unwrap();
        assert_eq!(
            log(&app),
            "c<;AC;BC;c>;s<;AS;BS;s>;r<;AR;BR;r>;v<;AV;BV;v>;p<;AP;BP;p>;t<;AT;BT;t>;d<;AD;BD;d>;"
        );
    }
}
#[test]
fn registration_duplicates_and_snapshot_mutation_match_arraylist_oracle_in_both_packages() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/application-lifecycle-fixture/jvm-list-oracle.txt"),
    )
    .unwrap();
    assert_eq!(oracle.lines().count(), 3);
    for fixture in ["lifecycle", "renamed"] {
        for line in oracle.lines() {
            let (mode, expected) = line.split_once('\t').unwrap();
            let mode: i32 = mode.parse().unwrap();
            let (mut app, hooks) = app(fixture, mode);
            app.launch().unwrap();
            let transcript = app
                .vm
                .invoke_static(PROBE, "transcript", "()Ljava/lang/String;", &[])
                .unwrap();
            assert_eq!(text(&app, transcript), expected);
            if mode == 1 {
                assert_eq!(hooks.collections.get(), 1);
                let (_, second) = hooks.captured.get().unwrap();
                assert_ne!(
                    app.vm.heap.class_of(second).unwrap(),
                    rd_vm::gc::FREED_CLASS
                );
                app.vm.gc();
                assert_eq!(
                    app.vm.heap.class_of(second).unwrap(),
                    rd_vm::gc::FREED_CLASS
                );
            }
        }
    }
}
#[test]
fn null_observer_exceptions_and_missing_super_are_real_failures_with_correct_ancestry() {
    for (mode, class) in [
        (3, "Ljava/lang/NullPointerException;"),
        (4, "Ljava/lang/IllegalStateException;"),
        (5, "Landroid/app/SuperNotCalledException;"),
    ] {
        let (mut app, _) = app("lifecycle", mode);
        thrown(app.launch(), class);
        if mode == 3 {
            assert_eq!(log(&app), "c<;");
        }
        if mode == 4 {
            assert_eq!(log(&app), "c<;AC;");
        }
        if mode == 5 {
            assert_eq!(log(&app), "c<;c>;");
            assert!(app
                .vm
                .cp
                .is_subtype(class, "Landroid/util/AndroidRuntimeException;"));
            assert!(app.vm.cp.is_subtype(class, "Ljava/lang/RuntimeException;"));
        }
    }
}
#[test]
fn invalid_types_and_missing_callback_methods_fail_without_silent_delivery() {
    let (mut app, _) = app("lifecycle", 0);
    app.launch().unwrap();
    assert_eq!(
        rd_vm::classpath::host_interface_modifiers(OBSERVER),
        Some(0x0609)
    );
    assert!(app
        .vm
        .cp
        .is_subtype(APPLICATION, "Landroid/app/Application;"));
    let wrong = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    assert!(
        matches!(register(&mut app,Value::Obj(wrong)),Err(VmExit::Exception(t)) if t.class=="Ljava/lang/ClassCastException;")
    );
    let missing = app
        .vm
        .heap
        .alloc_instance(
            "Lorg/androswitch/contracts/LifecycleProbe$MissingObserver;".into(),
            vec![],
        )
        .unwrap();
    register(&mut app, Value::Obj(missing)).unwrap();
    assert!(
        matches!(app.save_state(),Err(VmExit::Exception(t)) if t.class=="Ljava/lang/AbstractMethodError;")
    );
    unregister(&mut app, Value::Obj(missing)).unwrap();
    app.save_state().unwrap();
}
#[test]
fn application_owns_its_callbacks_and_weak_side_tables_do_not_keep_dead_applications_alive() {
    let (mut app, hooks) = app("lifecycle", 0);
    app.launch().unwrap();
    call(&mut app, "weakRegistration").unwrap();
    let (detached, callback) = hooks.captured.get().unwrap();
    app.vm.gc();
    assert_eq!(
        app.vm.heap.class_of(detached).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(
        app.vm.heap.class_of(callback).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    call(&mut app, "detached").unwrap();
    app.vm.gc();
    app.save_state().unwrap();
    assert!(!log(&app).contains("WV;"));
    call(&mut app, "clearDetached").unwrap();
    app.vm.gc();
    app.android.stop(&mut app.vm).unwrap();
    assert!(!log(&app).contains("WD;"));
}

#[test]
fn save_without_super_is_permitted_but_pause_checks_target_and_observers_only_run_at_super() {
    let (mut saved, _) = app("lifecycle", 7);
    saved.launch().unwrap();
    saved.save_state().unwrap();
    assert!(log(&saved).ends_with("v<;v>;"));
    assert_eq!(field(&saved, "observedSaved"), Value::Null);
    let (mut legacy, _) = app("legacy-pause", 6);
    legacy.launch().unwrap();
    legacy.android.stop(&mut legacy.vm).unwrap();
    assert!(log(&legacy).contains("p<;p>;t<;"));
    assert!(!log(&legacy).contains("AP;"));
    let (mut modern, _) = app("lifecycle", 6);
    modern.launch().unwrap();
    thrown(
        modern.android.stop(&mut modern.vm),
        "Landroid/app/SuperNotCalledException;",
    );
}
#[test]
fn super_called_marker_uses_real_api26_before_or_after_observer_exception_timing() {
    let (mut created, _) = app("lifecycle", 8);
    thrown(created.launch(), "Landroid/app/SuperNotCalledException;");
    assert_eq!(log(&created), "c<;AC;caughtCreate;c>;");
    let (mut started, _) = app("lifecycle", 9);
    started.launch().unwrap();
    assert_eq!(
        log(&started),
        "c<;AC;BC;c>;s<;AS;caughtStart;s>;r<;AR;BR;r>;"
    );
    // A caught observer exception must never become a Thread uncaught report.
    assert!(started.android.uncaught_exception_reports().is_empty());
}
#[test]
fn unregister_uses_real_equality_first_match_and_rejects_mutation_during_equals() {
    let (mut app, _) = app("lifecycle", 0);
    app.launch().unwrap();
    let equal = app
        .vm
        .heap
        .alloc_instance(
            "Lorg/androswitch/contracts/LifecycleProbe$EqualObserver;".into(),
            vec![],
        )
        .unwrap();
    app.vm
        .attach_host_state(
            "Lorg/androswitch/contracts/LifecycleProbe$EqualObserver;",
            equal,
        )
        .unwrap();
    app.vm
        .invoke_instance(
            equal,
            "<init>",
            "(Ljava/lang/String;)V",
            &[Value::StrPlaceholder("A".into())],
        )
        .unwrap();
    register(&mut app, Value::Obj(equal)).unwrap();
    unregister(&mut app, Value::Obj(equal)).unwrap();
    let original = field(&app, "a");
    unregister(&mut app, original).unwrap();
    app.save_state().unwrap();
    assert!(log(&app).ends_with("v<;BV;AV;v>;"));
    let mutating = app
        .vm
        .heap
        .alloc_instance(
            "Lorg/androswitch/contracts/LifecycleProbe$MutatingObserver;".into(),
            vec![],
        )
        .unwrap();
    app.vm
        .attach_host_state(
            "Lorg/androswitch/contracts/LifecycleProbe$MutatingObserver;",
            mutating,
        )
        .unwrap();
    app.vm
        .invoke_instance(mutating, "<init>", "()V", &[])
        .unwrap();
    assert!(
        matches!(unregister(&mut app,Value::Obj(mutating)),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE"&&e.cause.contains("equals"))
    );
}
fn proof_transcript(app: &App) -> String {
    let mut bytes = vec![0; 4096];
    let size = app
        .vm
        .guest_filesystem()
        .unwrap()
        .read_at(
            "/sdcard/Download/androswitch-lifecycle-proof.txt",
            0,
            &mut bytes,
        )
        .unwrap();
    String::from_utf8(bytes[..size].to_vec()).unwrap()
}
#[test]
fn device_proof_uses_standard_apis_visible_output_and_real_return_transcript_without_native_hooks()
{
    let (mut app, hooks) = app("lifecycle-proof", 0);
    app.launch().unwrap();
    assert!(app
        .android
        .views()
        .iter()
        .any(|view| view.text.contains("LIFECYCLE STARTUP PASS")));
    let initial = proof_transcript(&app);
    assert!(initial.starts_with("startup=PASS\nreturn=PENDING\nsave=not_observed\n"));
    app.android.stop(&mut app.vm).unwrap();
    let returned = proof_transcript(&app);
    assert!(returned.starts_with("startup=PASS\nreturn=PASS\nsave=not_observed\n"));
    assert!(returned.contains("p<;P;p>;t<;T;t>;d<;D;d>;"));
    assert_eq!(hooks.collections.get(), 0);
    assert_eq!(hooks.captured.get(), None);
}

#[test]
fn suffix_and_object_equality_preserve_jvm_null_unicode_override_and_super_behavior() {
    use rd_vm::framework_host::{FrameworkHost, HostMethodKind};
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/application-lifecycle-fixture/jvm-value-oracle.txt"),
    )
    .unwrap();
    assert_eq!(oracle.lines().count(), 37);
    for fixture in ["lifecycle", "renamed"] {
        let (mut app, _) = app(fixture, 0);
        for line in oracle.lines() {
            let (index, expected) = line.split_once('\t').unwrap();
            let case: i32 = index.parse().unwrap();
            let actual = app.vm.invoke_static(
                "Lorg/androswitch/contracts/LifecycleValueOracle;",
                "evaluate",
                "(I)I",
                &[Value::Int(case)],
            );
            // Collection value methods now share the active product store.
            // Cases 26..29 (including DEX subclasses) must match the JVM,
            // rather than retaining the earlier API_UNAVAILABLE expectation.
            assert_eq!(
                actual.unwrap(),
                Value::Int(expected.parse().unwrap()),
                "{fixture} case {index}"
            );
        }
        for (class, name, sig) in [
            ("Ljava/lang/Object;", "equals", "(Ljava/lang/Object;)Z"),
            ("Ljava/lang/String;", "endsWith", "(Ljava/lang/String;)Z"),
        ] {
            assert_eq!(
                app.android.method_metadata(class, name, sig),
                Some(HostMethodKind::Instance)
            );
            assert_eq!(app.android.method_metadata(class, name, "()Z"), None);
        }
    }
}
