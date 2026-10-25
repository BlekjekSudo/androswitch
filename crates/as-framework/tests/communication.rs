use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{Value, VmExit};
use std::{path::PathBuf, rc::Rc};
const PROBE: &str = "Lorg/androswitch/contracts/CommunicationActivity;";
#[test]
fn local_binder_returns_real_sdk_owners_and_retains_only_gc_reachable_edges() {
    const BINDER_CASES: &str = "Lorg/androswitch/contracts/LocalBinderCases;";
    for package in [
        "org.fixture.communication",
        "net.example.renamedcommunication",
    ] {
        let (mut app, _) = app_package(package);
        assert_eq!(
            app.vm
                .invoke_static(BINDER_CASES, "proof", "()I", &[])
                .unwrap(),
            Value::Int(1)
        );
        let binder = app
            .vm
            .invoke_static(BINDER_CASES, "make", "()Landroid/os/Binder;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        app.vm.gc();
        let owner = app
            .vm
            .invoke_static(
                BINDER_CASES,
                "query",
                "(Ljava/lang/String;)Landroid/os/IInterface;",
                &[Value::StrPlaceholder("contract.echo".into())],
            )
            .unwrap();
        assert_eq!(owner, Value::Obj(binder));
        assert!(
            app.vm
                .invoke_instance(
                    binder,
                    "transact",
                    "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z",
                    &[Value::Int(1), Value::Null, Value::Null, Value::Int(0)]
                )
                .is_err(),
            "remote/Parcel dispatch is not simulated"
        );
        app.vm
            .invoke_static(BINDER_CASES, "clear", "()V", &[])
            .unwrap();
        app.vm.gc();
        assert_eq!(
            app.vm.heap.class_of(binder).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
}
#[allow(clippy::assertions_on_constants)]
fn app() -> (App, PathBuf) {
    app_package("org.fixture.communication")
}
#[allow(clippy::assertions_on_constants)]
fn app_package(package: &str) -> (App, PathBuf) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-communication-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    pm.initialize().unwrap();
    let artifacts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../local-artifacts/m9/communication-fixture");
    for file in ["communication.apk", "renamed.apk"] {
        std::fs::copy(artifacts.join(file), root.join("apks/import.apk"))
            .expect("run scripts/build-communication-fixture.ps1");
        pm.install_file("apks/import.apk", false, None).unwrap();
    }
    let apk = pm.load_apk(package, None).unwrap().1;
    let mut app = App::new(apk, fs, Rc::new(HostPlatform::default()), 640, 360, None).unwrap();
    app.launch().unwrap();
    (app, root)
}
fn call(app: &mut App, name: &str, sig: &str, args: &[Value]) -> Value {
    app.vm.invoke_static(PROBE, name, sig, args).unwrap()
}
fn send(app: &mut App, value: &str, category: bool) {
    call(
        app,
        "send",
        "(Ljava/lang/String;Z)V",
        &[
            Value::StrPlaceholder(value.into()),
            Value::Int(i32::from(category)),
        ],
    );
}
fn count(app: &App) -> i32 {
    app.vm
        .statics
        .get(&(PROBE.into(), "count".into()))
        .unwrap()
        .as_int()
        .unwrap()
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
#[test]
#[ignore = "requires scripts/build-communication-fixture.ps1"]
fn real_dex_broadcasts_are_queued_snapshot_extras_gc_rooted_reentrant_and_unregisterable() {
    let (mut app, _root) = app();
    assert_eq!(call(&mut app, "stickyQuery", "()Z", &[]), Value::Int(1));
    send(&mut app, "before", true);
    assert_eq!(count(&app), 0);
    app.vm.gc();
    app.android.tick(&mut app.vm).unwrap();
    assert_eq!(count(&app), 1);
    let events = app
        .vm
        .statics
        .get(&(PROBE.into(), "events".into()))
        .unwrap()
        .clone();
    assert_eq!(text(&app, events), "before:");
    send(&mut app, "filtered", false);
    app.android.tick(&mut app.vm).unwrap();
    assert_eq!(count(&app), 1);
    send(&mut app, "reenter", true);
    app.android.tick(&mut app.vm).unwrap();
    assert_eq!(count(&app), 2);
    app.vm.gc();
    app.android.tick(&mut app.vm).unwrap();
    assert_eq!(count(&app), 3);
    let receiver = app
        .vm
        .statics
        .get(&(PROBE.into(), "receiver".into()))
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    send(&mut app, "drop", true);
    call(&mut app, "remove", "()V", &[]);
    app.vm.gc();
    app.android.tick(&mut app.vm).unwrap();
    app.vm.gc();
    assert_eq!(count(&app), 3);
    assert_eq!(
        app.vm.heap.class_of(receiver).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(call(&mut app, "removeAgain", "()I", &[]), Value::Int(1));
}
#[test]
#[ignore = "requires scripts/build-communication-fixture.ps1"]
fn actual_registry_services_differ_from_absence_and_failed_binder_never_calls_connection() {
    let (mut app, _root) = app();
    for package in [
        "org.fixture.communication",
        "net.example.renamedcommunication",
    ] {
        let value = call(
            &mut app,
            "discover",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::StrPlaceholder(package.into())],
        );
        assert_eq!(
            text(&app, value),
            format!("1:{package}:org.androswitch.contracts.CommunicationActivity$LocalService")
        );
    }
    let value = call(
        &mut app,
        "discover",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Value::StrPlaceholder("net.absent.generic.service".into())],
    );
    assert_eq!(text(&app, value), "absent");
    let value = call(
        &mut app,
        "discover",
        "(Ljava/lang/String;)Ljava/lang/String;",
        &[Value::Null],
    );
    assert!(text(&app, value).starts_with("2:"));
    assert_eq!(call(&mut app, "managerIdentity", "()Z", &[]), Value::Int(1));
    let process = call(&mut app, "serviceProcess", "()Ljava/lang/String;", &[]);
    assert_eq!(text(&app, process), "org.fixture.communication");
    assert_eq!(
        call(
            &mut app,
            "bindAbsent",
            "(Ljava/lang/String;)I",
            &[Value::StrPlaceholder("net.absent.generic.service".into())]
        ),
        Value::Int(0)
    );
    assert_eq!(call(&mut app, "logPolicy", "()I", &[]), Value::Int(2));
    // Invalid nonzero flags must fail explicitly rather than return false absence.
    let activity = app
        .vm
        .statics
        .get(&(PROBE.into(), "activity".into()))
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let manager = app
        .vm
        .invoke_instance(
            activity,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    let intent = app
        .vm
        .heap
        .alloc_instance("Landroid/content/Intent;".into(), vec![])
        .unwrap();
    app.vm
        .attach_host_state("Landroid/content/Intent;", intent)
        .unwrap();
    let error = app
        .vm
        .invoke_instance(
            manager,
            "queryIntentServices",
            "(Landroid/content/Intent;I)Ljava/util/List;",
            &[Value::Obj(intent), Value::Int(1)],
        )
        .unwrap_err();
    assert!(matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE"));
}

#[test]
#[ignore = "requires scripts/build-communication-fixture.ps1"]
fn missing_platform_fields_and_resource_attributes_fail_without_losing_real_dex_defaults() {
    let (mut app, root) = app();
    assert_eq!(call(&mut app, "defaults", "()I", &[]), Value::Int(3));
    let service = app
        .vm
        .heap
        .alloc_instance(
            "Landroid/content/pm/ServiceInfo;".into(),
            vec![("flags".into(), Value::Int(0))],
        )
        .unwrap();
    assert_eq!(
        call(
            &mut app,
            "initializedZero",
            "(Landroid/content/pm/ServiceInfo;)I",
            &[Value::Obj(service)]
        ),
        Value::Int(0)
    );
    let error = app
        .vm
        .invoke_static(
            PROBE,
            "unboundApplication",
            "(Landroid/content/pm/ServiceInfo;)Ljava/lang/Object;",
            &[Value::Obj(service)],
        )
        .unwrap_err();
    assert!(
        matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE" && e.cause.contains("applicationInfo"))
    );
    let subclass = app
        .vm
        .heap
        .alloc_instance(
            "Lorg/androswitch/contracts/CommunicationActivity$ServiceShape;".into(),
            vec![],
        )
        .unwrap();
    let error = app
        .vm
        .invoke_static(
            PROBE,
            "unboundInherited",
            "(Lorg/androswitch/contracts/CommunicationActivity$ServiceShape;)I",
            &[Value::Obj(subclass)],
        )
        .unwrap_err();
    assert!(
        matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE" && e.cause.contains("flags"))
    );
    let array = app
        .vm
        .heap
        .alloc_array(rd_vm::heap::ElemKind::Int, 1, "[I")
        .unwrap();
    let error = app
        .vm
        .invoke_static(
            PROBE,
            "initializedZero",
            "(Landroid/content/pm/ServiceInfo;)I",
            &[Value::Obj(array)],
        )
        .unwrap_err();
    assert!(matches!(error,VmExit::Error(e) if e.code=="VM_TYPE_ERROR"));
    let artifacts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../local-artifacts/m9/communication-fixture");
    std::fs::copy(
        artifacts.join("resource.apk"),
        root.join("apks/resource.apk"),
    )
    .unwrap();
    let pm = as_package::PackageManager::new(Rc::new(HostFileSystem::new(&root).unwrap()));
    pm.install_file("apks/resource.apk", false, None).unwrap();
    let error = app
        .vm
        .invoke_static(
            PROBE,
            "discover",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[Value::StrPlaceholder(
                "org.fixture.resourcecommunication".into(),
            )],
        )
        .unwrap_err();
    assert!(
        matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE" && e.cause.contains("service enabled"))
    );
}

#[test]
#[ignore = "requires scripts/build-communication-fixture.ps1"]
fn unsupported_broadcast_routes_reject_before_partial_delivery_or_intent_mutation() {
    for package in [
        "org.fixture.communication",
        "net.example.renamedcommunication",
    ] {
        let (mut app, _root) = app_package(package);
        for mode in 0..3 {
            let candidate = call(
                &mut app,
                "rejectedCandidate",
                "(I)Landroid/content/Intent;",
                &[Value::Int(mode)],
            )
            .as_ref()
            .unwrap()
            .unwrap();
            let error = app
                .vm
                .invoke_static(
                    PROBE,
                    "dispatch",
                    "(Landroid/content/Intent;)V",
                    &[Value::Obj(candidate)],
                )
                .unwrap_err();
            assert!(matches!(error,VmExit::Error(e) if e.code=="API_UNAVAILABLE"));
            app.android.tick(&mut app.vm).unwrap();
            assert_eq!(count(&app), mode);
            let original = app
                .vm
                .invoke_instance(
                    candidate,
                    "getStringExtra",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[Value::StrPlaceholder("value".into())],
                )
                .unwrap();
            assert_eq!(text(&app, original), "original");
            let action = app
                .vm
                .invoke_instance(candidate, "getAction", "()Ljava/lang/String;", &[])
                .unwrap();
            assert_eq!(
                text(&app, action),
                if mode == 1 {
                    "contract.MANIFEST"
                } else {
                    "contract.PING"
                }
            );
            // The rejected Intent remains usable. Correct its route, send once,
            // then mutate the source: only the new valid snapshot may arrive.
            app.vm
                .invoke_instance(
                    candidate,
                    "setAction",
                    "(Ljava/lang/String;)Landroid/content/Intent;",
                    &[Value::StrPlaceholder("contract.PING".into())],
                )
                .unwrap();
            app.vm
                .invoke_instance(
                    candidate,
                    "setPackage",
                    "(Ljava/lang/String;)Landroid/content/Intent;",
                    &[Value::StrPlaceholder(package.into())],
                )
                .unwrap();
            call(
                &mut app,
                "dispatch",
                "(Landroid/content/Intent;)V",
                &[Value::Obj(candidate)],
            );
            app.vm
                .invoke_instance(
                    candidate,
                    "putExtra",
                    "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
                    &[
                        Value::StrPlaceholder("value".into()),
                        Value::StrPlaceholder("after-valid-send".into()),
                    ],
                )
                .unwrap();
            app.vm.gc();
            app.android.tick(&mut app.vm).unwrap();
            assert_eq!(count(&app), mode + 1);
            let events = app
                .vm
                .statics
                .get(&(PROBE.into(), "events".into()))
                .unwrap()
                .clone();
            assert_eq!(text(&app, events), "original:".repeat((mode + 1) as usize));
        }
    }
}

#[test]
fn pair_components_use_real_identity_value_callbacks_and_gc_in_two_packages() {
    const PAIRS: &str = "Lorg/androswitch/contracts/PairCases;";
    for package in [
        "org.fixture.communication",
        "net.example.renamedcommunication",
    ] {
        let (mut app, _) = app_package(package);
        assert_eq!(
            app.vm.invoke_static(PAIRS, "proof", "()I", &[]).unwrap(),
            Value::Int(0)
        );
        let pair = app
            .vm
            .invoke_static(PAIRS, "get", "()Landroid/util/Pair;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        let first = app
            .vm
            .heap
            .get_field(pair, "first")
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap();
        app.vm.gc();
        assert_ne!(app.vm.heap.class_of(first).unwrap(), rd_vm::gc::FREED_CLASS);
        assert_eq!(
            app.vm
                .invoke_instance(pair, "hashCode", "()I", &[])
                .unwrap(),
            Value::Int(14)
        );
        app.vm.invoke_static(PAIRS, "clear", "()V", &[]).unwrap();
        app.vm.gc();
        assert_eq!(app.vm.heap.class_of(pair).unwrap(), rd_vm::gc::FREED_CLASS);
        assert_eq!(app.vm.heap.class_of(first).unwrap(), rd_vm::gc::FREED_CLASS);
    }
}
