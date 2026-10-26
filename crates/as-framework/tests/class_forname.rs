//! Real portable DEX Class.forName, compared with ordinary Java on a JVM.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::framework_host::{FrameworkHost, HostMethodKind};
use rd_vm::{Value, VmExit};
use std::{path::PathBuf, rc::Rc};
const ACTIVITY: &str = "Lorg/androswitch/contracts/ForNameActivity;";
const PREFIX: &str = "org.androswitch.contracts.ForNameCases$";
#[test]
fn class_new_instance_uses_declared_constructors_access_gc_and_direct_exceptions_like_jvm() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/class-forname-fixture/new-instance-jvm-oracle.tsv"),
    )
    .unwrap();
    for fixture in ["class-forname", "renamed"] {
        let mut app = app(fixture);
        for line in oracle.lines() {
            let (index, expected) = line.split_once('\t').unwrap();
            let value = app
                .vm
                .invoke_static(
                    "Lorg/androswitch/contracts/ReflectionNewCases;",
                    "run",
                    "(I)Ljava/lang/String;",
                    &[Value::Int(index.parse().unwrap())],
                )
                .unwrap();
            let value = app
                .vm
                .heap
                .as_str(value.as_ref().unwrap().unwrap())
                .unwrap();
            assert_eq!(value, expected, "case {index} in {fixture}");
            app.vm.gc();
        }
    }
}
#[allow(clippy::assertions_on_constants)]
fn app(fixture: &str) -> App {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-forname-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/class-forname-fixture/{fixture}.apk"));
    let record = pm
        .install(
            std::fs::read(apk).expect("run fixtures/m9/class-forname/build.ps1"),
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
    app.launch().unwrap();
    app
}
fn loader(app: &App) -> Value {
    app.vm.statics[&(ACTIVITY.into(), "loader".into())].clone()
}
fn lookup(
    app: &mut App,
    name: Option<&str>,
    initialize: bool,
    loader: Value,
) -> Result<Value, VmExit> {
    let name = name.map_or(Value::Null, |name| Value::StrPlaceholder(name.into()));
    app.vm.invoke_static(
        ACTIVITY,
        "lookup",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
        &[name, Value::Int(initialize as i32), loader],
    )
}
fn text(app: &App, value: Value) -> String {
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn exception(result: Result<Value, VmExit>, expected: &str) {
    assert!(
        matches!(result,Err(VmExit::Exception(error)) if error.class==expected),
        "expected {expected}"
    );
}
#[test]
fn initialization_arrays_linkage_and_failure_poisoning_match_jvm_in_both_packages() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/class-forname-fixture/jvm-oracle.txt"),
    )
    .unwrap();
    assert_eq!(oracle.lines().count(), 34);
    for fixture in ["class-forname", "renamed"] {
        let mut app = app(fixture);
        for line in oracle.lines() {
            let (index, expected) = line.split_once('\t').unwrap();
            let actual = app
                .vm
                .invoke_static(
                    ACTIVITY,
                    "run",
                    "(I)Ljava/lang/String;",
                    &[Value::Int(index.parse().unwrap())],
                )
                .unwrap();
            assert_eq!(text(&app, actual), expected, "{fixture} case{index}");
        }
    }
}
#[test]
fn canonical_loader_visibility_and_class_identity_survive_collection() {
    let mut app = app("renamed");
    let app_loader = loader(&app);
    let string = lookup(
        &mut app,
        Some("java.lang.String"),
        false,
        app_loader.clone(),
    )
    .unwrap();
    let boot = app
        .vm
        .invoke_instance(
            string.as_ref().unwrap().unwrap(),
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap();
    assert_ne!(boot, app_loader);
    assert_eq!(
        lookup(&mut app, Some("java.lang.String"), false, Value::Null).unwrap(),
        string
    );
    assert_eq!(
        lookup(&mut app, Some("java.lang.String"), false, boot.clone()).unwrap(),
        string
    );
    let name = format!("{PREFIX}Lazy");
    let class = lookup(&mut app, Some(&name), false, app_loader.clone()).unwrap();
    let returned_loader = app
        .vm
        .invoke_instance(
            class.as_ref().unwrap().unwrap(),
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap();
    assert_eq!(returned_loader, app_loader);
    exception(
        lookup(&mut app, Some(&name), false, boot.clone()),
        "Ljava/lang/ClassNotFoundException;",
    );
    exception(
        lookup(&mut app, Some(&format!("[L{name};")), true, boot.clone()),
        "Ljava/lang/ClassNotFoundException;",
    );
    let array = lookup(
        &mut app,
        Some(&format!("[L{name};")),
        true,
        app_loader.clone(),
    )
    .unwrap();
    app.vm.gc();
    assert_eq!(
        lookup(&mut app, Some(&name), false, app_loader.clone()).unwrap(),
        class
    );
    assert_eq!(
        lookup(&mut app, Some(&format!("[L{name};")), false, app_loader).unwrap(),
        array
    );
    assert_eq!(
        lookup(&mut app, Some("[I"), false, boot).unwrap(),
        lookup(&mut app, Some("[I"), false, Value::Null).unwrap()
    );
}
#[test]
fn system_loader_parent_chain_and_boot_visibility_are_canonical_in_both_packages() {
    for fixture in ["class-forname", "renamed"] {
        let mut app = app(fixture);
        let application = loader(&app);
        let sig = "()Ljava/lang/ClassLoader;";
        let system = app
            .vm
            .invoke_static("Ljava/lang/ClassLoader;", "getSystemClassLoader", sig, &[])
            .unwrap();
        assert_ne!(system, application);
        assert_eq!(
            app.vm
                .heap
                .class_of(system.as_ref().unwrap().unwrap())
                .unwrap(),
            "Ldalvik/system/PathClassLoader;"
        );
        let parent = |app: &mut App, value: &Value| {
            app.vm
                .invoke_instance(value.as_ref().unwrap().unwrap(), "getParent", sig, &[])
                .unwrap()
        };
        assert_eq!(parent(&mut app, &application), system);
        let bootstrap = parent(&mut app, &system);
        assert_ne!(system, bootstrap);
        assert_ne!(application, bootstrap);
        assert_eq!(parent(&mut app, &bootstrap), Value::Null);
        let string = lookup(&mut app, Some("java.lang.String"), false, system.clone()).unwrap();
        assert_eq!(
            app.vm
                .invoke_instance(
                    string.as_ref().unwrap().unwrap(),
                    "getClassLoader",
                    sig,
                    &[]
                )
                .unwrap(),
            bootstrap
        );
        assert_eq!(
            app.vm
                .invoke_instance(
                    system.as_ref().unwrap().unwrap(),
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[Value::StrPlaceholder("java.lang.String".into())]
                )
                .unwrap(),
            string
        );
        exception(
            lookup(
                &mut app,
                Some(&format!("{PREFIX}Lazy")),
                false,
                system.clone(),
            ),
            "Ljava/lang/ClassNotFoundException;",
        );
        exception(
            app.vm.invoke_instance(
                system.as_ref().unwrap().unwrap(),
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[Value::StrPlaceholder(format!("{PREFIX}Lazy"))],
            ),
            "Ljava/lang/ClassNotFoundException;",
        );
        app.vm.gc();
        assert_eq!(
            app.vm
                .invoke_static("Ljava/lang/ClassLoader;", "getSystemClassLoader", sig, &[])
                .unwrap(),
            system
        );
        assert_eq!(parent(&mut app, &system), bootstrap);
        assert_eq!(
            app.android
                .method_metadata("Ljava/lang/ClassLoader;", "getSystemClassLoader", sig),
            Some(HostMethodKind::Static)
        );
        assert_eq!(
            app.android
                .method_metadata("Ljava/lang/ClassLoader;", "getParent", sig),
            Some(HostMethodKind::Instance)
        );
        assert_eq!(
            app.android
                .method_metadata("Ljava/lang/ClassLoader;", "getPlatformClassLoader", sig),
            None
        );
    }
}
#[test]
fn unknown_loader_identity_wrong_type_and_unsupported_descriptors_fail_explicitly() {
    let mut app = app("class-forname");
    for descriptor in [
        "(Ljava/lang/String;)Ljava/lang/Class;",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
    ] {
        assert_eq!(
            app.android
                .method_metadata("Ljava/lang/Class;", "forName", descriptor),
            Some(HostMethodKind::Static)
        );
    }
    assert_eq!(
        app.android.method_metadata(
            "Ljava/lang/Class;",
            "forName",
            "(Ljava/lang/String;Z)Ljava/lang/Class;"
        ),
        None
    );
    let custom = app
        .vm
        .heap
        .alloc_instance(
            "Lorg/androswitch/contracts/ForNameCases$CustomLoader;".into(),
            vec![],
        )
        .unwrap();
    assert!(
        matches!(lookup(&mut app,Some("java.lang.String"),false,Value::Obj(custom)),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
    let wrong = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    exception(
        lookup(&mut app, Some("java.lang.String"), false, Value::Obj(wrong)),
        "Ljava/lang/ClassCastException;",
    );
    exception(
        lookup(&mut app, None, false, Value::Obj(custom)),
        "Ljava/lang/NullPointerException;",
    );
    let app_loader = loader(&app);
    let name = "[".repeat(256) + "I";
    exception(
        lookup(&mut app, Some(&name), false, app_loader.clone()),
        "Ljava/lang/ClassNotFoundException;",
    );
    assert!(lookup(
        &mut app,
        Some(&("[".repeat(255) + "I")),
        true,
        app_loader.clone()
    )
    .is_ok());
    assert!(
        matches!(lookup(&mut app,Some(&"x".repeat(4097)),false,app_loader),Err(VmExit::Error(e)) if e.code=="CLASS_NAME_BUDGET")
    );
    let unsupported = app.vm.invoke_static(
        "Ljava/lang/Class;",
        "forName",
        "(Ljava/lang/String;Z)Ljava/lang/Class;",
        &[
            Value::StrPlaceholder("java.lang.String".into()),
            Value::Int(0),
        ],
    );
    assert!(matches!(unsupported,Err(VmExit::Error(e)) if e.code=="NOT_FOUND"));
}
#[test]
fn default_interface_initialization_is_explicitly_unavailable_without_mutating_or_poisoning_class()
{
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/class-forname-fixture/default-interface-jvm-oracle.txt"),
    )
    .unwrap();
    let expected: Vec<_> = oracle
        .lines()
        .map(|line| line.split_once('\t').unwrap().1)
        .collect();
    assert_eq!(expected.len(), 4);
    for fixture in ["class-forname", "renamed"] {
        let mut app = app(fixture);
        let first = app
            .vm
            .invoke_static(
                ACTIVITY,
                "defaults",
                "(I)Ljava/lang/String;",
                &[Value::Int(0)],
            )
            .unwrap();
        assert_eq!(text(&app, first), expected[0]);
        let unavailable = app.vm.invoke_static(
            ACTIVITY,
            "defaults",
            "(I)Ljava/lang/String;",
            &[Value::Int(1)],
        );
        assert!(
            matches!(unavailable,Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE" && e.cause.contains("DefaultContract"))
        );
        assert_eq!(
            app.vm
                .invoke_static(ACTIVITY, "defaultCount", "()I", &[])
                .unwrap(),
            Value::Int(0)
        );
        // Explicit initialization of the interface is supported. Its dependent
        // class can subsequently initialize without skipped dependencies.
        for case in [2, 3] {
            let actual = app
                .vm
                .invoke_static(
                    ACTIVITY,
                    "defaults",
                    "(I)Ljava/lang/String;",
                    &[Value::Int(case)],
                )
                .unwrap();
            assert_eq!(text(&app, actual), expected[case as usize]);
        }
    }
}

#[test]
fn implemented_callback_dependencies_have_explicit_interface_metadata_and_no_unknown_siblings() {
    for fixture in ["class-forname", "renamed"] {
        let mut app = app(fixture);
        let app_loader = loader(&app);
        assert_eq!(
            app.vm.statics[&(ACTIVITY.into(), "clicks".into())],
            Value::Int(1)
        );
        for (name, owner) in [
            ("android.view.View$OnClickListener", ACTIVITY),
            ("android.view.View$OnTouchListener", ACTIVITY),
            (
                "android.content.SharedPreferences$OnSharedPreferenceChangeListener",
                "Lorg/androswitch/contracts/ListenerContracts;",
            ),
            (
                "android.opengl.GLSurfaceView$Renderer",
                "Lorg/androswitch/contracts/ListenerContracts;",
            ),
            (
                "android.opengl.GLSurfaceView$EGLConfigChooser",
                "Lorg/androswitch/contracts/ListenerContracts;",
            ),
        ] {
            let descriptor = format!("L{};", name.replace('.', "/"));
            assert_eq!(
                rd_vm::classpath::host_interface_modifiers(&descriptor),
                Some(0x0609)
            );
            assert!(rd_vm::classpath::host_class_known(&descriptor));
            assert!(app.vm.cp.is_subtype(owner, &descriptor));
            let class = lookup(&mut app, Some(name), false, app_loader.clone()).unwrap();
            let loaded = app
                .vm
                .invoke_instance(
                    app_loader.as_ref().unwrap().unwrap(),
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[Value::StrPlaceholder(name.into())],
                )
                .unwrap();
            assert_eq!(class, loaded);
            assert_eq!(
                app.vm
                    .invoke_instance(
                        class.as_ref().unwrap().unwrap(),
                        "getSuperclass",
                        "()Ljava/lang/Class;",
                        &[]
                    )
                    .unwrap(),
                Value::Null
            );
        }
        for name in [
            "org.androswitch.contracts.ForNameActivity",
            "org.androswitch.contracts.ListenerContracts",
        ] {
            assert_eq!(
                lookup(&mut app, Some(name), false, app_loader.clone()).unwrap(),
                app.vm
                    .invoke_instance(
                        app_loader.as_ref().unwrap().unwrap(),
                        "loadClass",
                        "(Ljava/lang/String;)Ljava/lang/Class;",
                        &[Value::StrPlaceholder(name.into())]
                    )
                    .unwrap()
            );
        }
        for name in [
            "android.view.View$OnLongClickListener",
            "android.view.View$OnKeyListener",
            "android.opengl.GLSurfaceView$EGLContextFactory",
        ] {
            let descriptor = format!("L{};", name.replace('.', "/"));
            // Public SDK declarations are discoverable without implementing
            // their methods. Metadata availability must not grant a method ID.
            assert_eq!(
                rd_vm::classpath::host_interface_modifiers(&descriptor),
                Some(0x0609)
            );
            assert!(rd_vm::classpath::host_class_known(&descriptor));
            let class = lookup(&mut app, Some(name), false, app_loader.clone()).unwrap();
            assert!(class.as_ref().unwrap().is_some());
            let (method, sig) = match name {
                "android.view.View$OnLongClickListener" => ("onLongClick", "(Landroid/view/View;)Z"),
                "android.view.View$OnKeyListener" => ("onKey", "(Landroid/view/View;ILandroid/view/KeyEvent;)Z"),
                _ => ("createContext", "(Ljavax/microedition/khronos/egl/EGL10;Ljavax/microedition/khronos/egl/EGLDisplay;Ljavax/microedition/khronos/egl/EGLConfig;)Ljavax/microedition/khronos/egl/EGLContext;"),
            };
            assert_eq!(
                rd_vm::framework_host::FrameworkHost::method_metadata(
                    app.android.as_ref(),
                    &descriptor,
                    method,
                    sig
                ),
                None
            );
        }
        for name in [
            "android.view.View$AndroSwitchMissingListener",
            "android.opengl.GLSurfaceView$AndroSwitchMissingFactory",
        ] {
            let descriptor = format!("L{};", name.replace('.', "/"));
            assert_eq!(
                rd_vm::classpath::host_interface_modifiers(&descriptor),
                None
            );
            assert!(!rd_vm::classpath::host_class_known(&descriptor));
            exception(
                lookup(&mut app, Some(name), false, app_loader.clone()),
                "Ljava/lang/ClassNotFoundException;",
            );
        }
    }
}
