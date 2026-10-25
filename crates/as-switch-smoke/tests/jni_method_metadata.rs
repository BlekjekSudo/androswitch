//! Real shared framework metadata/dispatch, with deterministic actual-owner
//! changes. This is not a concurrent platform transport proof.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem, BackgroundWorkers, PlatformError};
use as_switch_smoke::jni_constructor::{construct, method_owner};
use rd_vm::{
    framework_host::{FrameworkHost, HostMethodKind},
    native_host::NativeHost,
    Engine, ObjRef, Value, VmExit,
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
const THREAD: &str = "Ljava/lang/Thread;";
const CURRENT: &str = "()Ljava/lang/Thread;";
struct Identity {
    owner: Cell<u64>,
    roots: RefCell<Vec<ObjRef>>,
    invoked: RefCell<Vec<String>>,
    fs: Rc<HostFileSystem>,
}
impl NativeHost for Identity {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.owner.get())
    }
    fn roots(&self) -> Vec<ObjRef> {
        self.roots.borrow().clone()
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_LOAD",
            "native fixture is not executed by this host adapter",
        )
        .into())
    }
    fn invoke(
        &self,
        _: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        self.invoked
            .borrow_mut()
            .push(format!("{class}->{name}{sig}"));
        Err(rd_vm::err::vm_error(
            "TEST_NATIVE_BODY",
            "actual DEX native declaration won over the platform catalog",
        )
        .into())
    }
}
#[allow(clippy::assertions_on_constants)]
fn app() -> (App, Rc<Identity>) {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-jni-methods-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/jni-constructor-fixture/constructor-proof.apk");
    let installed = pm
        .install(
            std::fs::read(path).expect("run fixtures/m9/jni-construct/build.ps1"),
            true,
            None,
        )
        .unwrap();
    let mut app = App::new(
        pm.load_apk(&installed.package, None).unwrap().1,
        fs.clone(),
        Rc::new(HostPlatform::default()),
        640,
        360,
        None,
    )
    .unwrap();
    let native = Rc::new(Identity {
        owner: Cell::new(11),
        roots: RefCell::new(vec![]),
        invoked: RefCell::new(vec![]),
        fs,
    });
    app.vm.bind_native(native.clone());
    app.android.bind_main_thread(&app.vm).unwrap();
    (app, native)
}
#[test]
fn upstream_wrappers_share_real_jni_constructor_method_and_static_field_metadata() {
    let (mut app, native) = app();
    let class = "Ljava/lang/Integer;";
    let object = construct(&mut app.vm, class, "(I)V", &[Value::Int(42)], |r| {
        native.roots.borrow_mut().push(r);
        Ok(r)
    })
    .unwrap();
    assert_eq!(
        app.vm
            .invoke_instance(object, "intValue", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        method_owner(&mut app.vm, class, "intValue", "()I", false).unwrap(),
        class
    );
    assert_eq!(
        method_owner(
            &mut app.vm,
            class,
            "valueOf",
            "(I)Ljava/lang/Integer;",
            true
        )
        .unwrap(),
        class
    );
    assert!(method_owner(
        &mut app.vm,
        class,
        "valueOf",
        "(I)Ljava/lang/Integer;",
        false
    )
    .is_err());
    assert!(method_owner(&mut app.vm, class, "intValue", "()J", false).is_err());
    let shared = app
        .vm
        .invoke_static(
            class,
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[Value::Int(42)],
        )
        .unwrap();
    assert_ne!(
        shared,
        Value::Obj(object),
        "constructors do not use valueOf caches"
    );
    app.vm.gc();
    assert_eq!(
        app.vm
            .invoke_static(
                class,
                "valueOf",
                "(I)Ljava/lang/Integer;",
                &[Value::Int(42)]
            )
            .unwrap(),
        shared
    );
    let host = app.vm.framework_host().unwrap();
    assert!(host.static_field_metadata(class, "MAX_VALUE", "I"));
    assert!(!host.static_field_metadata(class, "MAX_VALUE", "J"));
    assert_eq!(
        host.static_field(&mut app.vm, class, "MAX_VALUE", "I")
            .unwrap(),
        Some(Value::Int(i32::MAX))
    );
    assert_eq!(
        host.static_field(&mut app.vm, class, "MAX_VALUE", "J")
            .unwrap(),
        None
    );
}

fn current(app: &mut App) -> ObjRef {
    app.vm
        .invoke_static(THREAD, "currentThread", CURRENT, &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap()
}
fn name(app: &mut App, r: ObjRef) -> String {
    let value = app
        .vm
        .invoke_instance(r, "getName", "()Ljava/lang/String;", &[])
        .unwrap();
    app.vm
        .heap
        .as_str(value.as_ref().unwrap().unwrap())
        .unwrap()
        .into()
}
fn alive(app: &mut App, r: ObjRef) -> i32 {
    app.vm
        .invoke_instance(r, "isAlive", "()Z", &[])
        .unwrap()
        .as_int()
        .unwrap()
}
fn context(app: &mut App, r: ObjRef) -> Option<ObjRef> {
    app.vm
        .invoke_instance(r, "getContextClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
}
fn set_context(app: &mut App, r: ObjRef, loader: Option<ObjRef>) -> Result<Value, VmExit> {
    app.vm.invoke_instance(
        r,
        "setContextClassLoader",
        "(Ljava/lang/ClassLoader;)V",
        &[loader.map_or(Value::Null, Value::Obj)],
    )
}
fn defining_loader(app: &mut App, class: &str) -> ObjRef {
    let class = app.vm.device_class_object(class).unwrap();
    app.vm
        .invoke_instance(class, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap()
}
#[test]
fn actual_loader_identity_context_inheritance_and_unknown_loader_failures() {
    let (mut app, native) = app();
    let main = current(&mut app);
    let application = defining_loader(&mut app, "Lorg/androswitch/contracts/ConstructorCases;");
    let bootstrap = defining_loader(&mut app, "Ljava/lang/Object;");
    assert_ne!(application, bootstrap);
    assert_eq!(context(&mut app, main), Some(application));
    assert!(app.android.references(main).contains(&application));
    for (name, sig) in [
        ("getContextClassLoader", "()Ljava/lang/ClassLoader;"),
        ("setContextClassLoader", "(Ljava/lang/ClassLoader;)V"),
    ] {
        assert_eq!(
            method_owner(&mut app.vm, THREAD, name, sig, false).unwrap(),
            THREAD
        );
        assert!(method_owner(&mut app.vm, THREAD, name, sig, true).is_err());
    }
    let result = app
        .vm
        .invoke_static(
            "Lorg/androswitch/contracts/LoaderCases;",
            "inheritance",
            "()I",
            &[],
        )
        .unwrap();
    assert_eq!(result, Value::Int(107));
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/jni-constructor-fixture/jvm-oracle.txt"),
    )
    .unwrap();
    assert!(oracle
        .lines()
        .any(|line| line == "loader\tinheritance\t107"));
    let unknown = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/ClassLoader;".into(), vec![])
        .unwrap();
    let wrong = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    assert!(
        matches!(set_context(&mut app, main, Some(unknown)),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
    assert!(
        matches!(set_context(&mut app, main, Some(wrong)),Err(VmExit::Exception(e)) if e.class=="Ljava/lang/ClassCastException;")
    );
    assert_eq!(context(&mut app, main), Some(application));
    let app_name = Value::StrPlaceholder("org.androswitch.contracts.LoaderParentThread".into());
    assert!(
        matches!(app.vm.invoke_instance(bootstrap,"loadClass","(Ljava/lang/String;)Ljava/lang/Class;",std::slice::from_ref(&app_name)),Err(VmExit::Exception(e)) if e.class=="Ljava/lang/ClassNotFoundException;")
    );
    let expected = app
        .vm
        .device_class_object("Lorg/androswitch/contracts/LoaderParentThread;")
        .unwrap();
    assert_eq!(
        app.vm
            .invoke_instance(
                application,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[app_name]
            )
            .unwrap(),
        Value::Obj(expected)
    );
    assert!(
        matches!(app.vm.invoke_instance(unknown,"loadClass","(Ljava/lang/String;)Ljava/lang/Class;",&[Value::StrPlaceholder("java.lang.Object".into())]),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
    set_context(&mut app, main, Some(bootstrap)).unwrap();
    let child = construct(&mut app.vm, THREAD, "()V", &[], |r| {
        native.roots.borrow_mut().push(r);
        Ok(r)
    })
    .unwrap();
    assert_eq!(context(&mut app, child), Some(bootstrap));
    set_context(&mut app, main, Some(application)).unwrap();
    app.vm.gc();
    assert_eq!(context(&mut app, child), Some(bootstrap));
    native.owner.set(22);
    app.android
        .thread_attached(22, Some("context native"))
        .unwrap();
    let attached = current(&mut app);
    assert_eq!(
        context(&mut app, attached),
        None,
        "native JNI attach defaults bootstrap/null"
    );
    set_context(&mut app, attached, Some(application)).unwrap();
    native.roots.borrow_mut().push(attached);
    app.android.thread_detached(22);
    app.vm.gc();
    assert_eq!(alive(&mut app, attached), 0);
    assert_eq!(context(&mut app, attached), Some(application));
    let reused = current(&mut app);
    assert_ne!(reused, attached);
    assert_eq!(context(&mut app, reused), None);
    native.roots.borrow_mut().push(main);
    native.owner.set(11);
    app.android.thread_detached(11);
    app.android
        .thread_attached(11, Some("reattached original main owner"))
        .unwrap();
    let reattached_main = current(&mut app);
    assert_ne!(reattached_main, main);
    assert_eq!(context(&mut app, reattached_main), None);
    assert_eq!(
        name(&mut app, reattached_main),
        "reattached original main owner"
    );
    app.vm.gc();
    assert_eq!(alive(&mut app, main), 0);
    assert_eq!(name(&mut app, main), "main");
    assert_eq!(context(&mut app, main), Some(application));
}
#[derive(Default)]
struct Workers(RefCell<Vec<u64>>);
impl BackgroundWorkers for Workers {
    fn start(&self, token: u64) -> Result<(), PlatformError> {
        self.0.borrow_mut().push(token);
        Ok(())
    }
    fn wake(&self, _: u64) -> Result<(), PlatformError> {
        Ok(())
    }
    fn cancel(&self, _: u64) -> Result<(), PlatformError> {
        Ok(())
    }
}
#[test]
fn thread_constructor_inherits_actual_parent_virtual_context_getter_on_worker() {
    let (mut app, native) = app();
    let workers = Rc::new(Workers::default());
    app.android.bind_background_workers(workers.clone());
    let class = "Lorg/androswitch/contracts/LoaderParentThread;";
    let thread = construct(&mut app.vm, class, "()V", &[], |r| {
        native.roots.borrow_mut().push(r);
        Ok(r)
    })
    .unwrap();
    let application = defining_loader(&mut app, class);
    assert_eq!(
        method_owner(
            &mut app.vm,
            class,
            "getContextClassLoader",
            "()Ljava/lang/ClassLoader;",
            false
        )
        .unwrap(),
        class
    );
    app.vm.invoke_instance(thread, "start", "()V", &[]).unwrap();
    let token = workers.0.borrow()[0];
    native.owner.set(44);
    assert_eq!(
        app.android
            .background_worker_step(&mut app.vm, token)
            .unwrap(),
        None
    );
    assert_eq!(
        app.vm.statics.get(&(class.into(), "reads".into())),
        Some(&Value::Int(1))
    );
    let child = app
        .vm
        .statics
        .get(&(class.into(), "child".into()))
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    app.vm.gc();
    assert_eq!(context(&mut app, child), Some(application));
    app.android.background_worker_stopped(token);
    assert_eq!(alive(&mut app, thread), 0);
}
#[test]
fn catalog_ids_use_same_real_static_instance_and_constructor_handlers() {
    let (mut app, native) = app();
    let roots = app.android.roots();
    assert_eq!(
        method_owner(&mut app.vm, THREAD, "currentThread", CURRENT, true).unwrap(),
        THREAD
    );
    assert_eq!(
        method_owner(
            &mut app.vm,
            THREAD,
            "getName",
            "()Ljava/lang/String;",
            false
        )
        .unwrap(),
        THREAD
    );
    assert_eq!(
        method_owner(
            &mut app.vm,
            THREAD,
            "<init>",
            "(Ljava/lang/String;)V",
            false
        )
        .unwrap(),
        THREAD
    );
    assert_eq!(
        app.android.roots(),
        roots,
        "catalog lookup executed no Thread handler"
    );
    let first = current(&mut app);
    app.vm.gc();
    assert_eq!(current(&mut app), first);
    assert_eq!(name(&mut app, first), "main");
    assert_eq!(alive(&mut app, first), 1);
    let created = construct(
        &mut app.vm,
        THREAD,
        "(Ljava/lang/String;)V",
        &[Value::StrPlaceholder("JNI created".into())],
        |r| {
            native.roots.borrow_mut().push(r);
            Ok(r)
        },
    )
    .unwrap();
    assert_eq!(name(&mut app, created), "JNI created");
    assert_eq!(alive(&mut app, created), 0);
    for (class, name, sig, static_kind) in [
        (THREAD, "currentThread", CURRENT, false),
        (THREAD, "getName", "()Ljava/lang/String;", true),
        (THREAD, "<init>", "()V", true),
        (THREAD, "join", "()V", false),
        (THREAD, "missing", "()V", false),
        (THREAD, "getName", "()I", false),
    ] {
        assert!(
            matches!(method_owner(&mut app.vm,class,name,sig,static_kind),Err(VmExit::Exception(e)) if e.class=="Ljava/lang/NoSuchMethodError;")
        );
    }
    assert_eq!(
        app.android.method_metadata(
            "Ljava/util/concurrent/Executors;",
            "defaultThreadFactory",
            "()Ljava/util/concurrent/ThreadFactory;"
        ),
        Some(HostMethodKind::Static)
    );
    let factory = app
        .vm
        .invoke_static(
            "Ljava/util/concurrent/Executors;",
            "defaultThreadFactory",
            "()Ljava/util/concurrent/ThreadFactory;",
            &[],
        )
        .unwrap();
    assert_eq!(
        app.vm
            .heap
            .class_of(factory.as_ref().unwrap().unwrap())
            .unwrap(),
        "Ljava/util/concurrent/Executors$DefaultThreadFactory;"
    );
}
#[test]
fn actual_dex_owners_static_kind_native_declarations_and_abstract_barriers_win() {
    let (mut app, native) = app();
    for class in ["LookupThread", "NativeLookupThread", "AbstractLookupThread"] {
        let class = format!("Lorg/androswitch/contracts/{class};");
        assert_eq!(
            method_owner(&mut app.vm, &class, "run", "()V", false).unwrap(),
            class
        );
        assert!(method_owner(&mut app.vm, &class, "run", "()V", true).is_err());
    }
    let lookup = "Lorg/androswitch/contracts/LookupThread;";
    assert_eq!(
        method_owner(&mut app.vm, lookup, "currentThread", CURRENT, true).unwrap(),
        lookup
    );
    assert_eq!(
        app.vm
            .invoke_static(lookup, "currentThread", CURRENT, &[])
            .unwrap()
            .as_ref()
            .unwrap(),
        None
    );
    assert_eq!(
        app.vm.statics.get(&(lookup.into(), "calls".into())),
        Some(&Value::Int(1))
    );
    assert!(method_owner(&mut app.vm, lookup, "currentThread", CURRENT, false).is_err());
    let inherited = "Lorg/androswitch/contracts/InheritedLookupThread;";
    assert_eq!(
        method_owner(&mut app.vm, inherited, "currentThread", CURRENT, true).unwrap(),
        THREAD
    );
    assert!(method_owner(
        &mut app.vm,
        inherited,
        "<init>",
        "(Ljava/lang/String;)V",
        false
    )
    .is_err());
    let object = construct(
        &mut app.vm,
        "Lorg/androswitch/contracts/NativeLookupThread;",
        "()V",
        &[],
        |r| {
            native.roots.borrow_mut().push(r);
            Ok(r)
        },
    )
    .unwrap();
    assert!(
        matches!(app.vm.invoke_instance(object,"run","()V",&[]),Err(VmExit::Error(e)) if e.code=="TEST_NATIVE_BODY")
    );
    assert_eq!(
        native.invoked.borrow().as_slice(),
        ["Lorg/androswitch/contracts/NativeLookupThread;->run()V"]
    );
}
#[test]
fn real_owner_identity_detach_global_lifetime_reuse_and_main_handler_are_separate() {
    let (mut app, native) = app();
    let main = current(&mut app);
    native.owner.set(22);
    app.android
        .thread_attached(22, Some("JNI named worker"))
        .unwrap();
    let old = current(&mut app);
    native.roots.borrow_mut().push(old);
    assert_ne!(old, main);
    assert_eq!(name(&mut app, old), "JNI named worker");
    app.android.thread_detached(22);
    app.android.thread_detached(22);
    app.vm.gc();
    assert_eq!(alive(&mut app, old), 0);
    assert_eq!(name(&mut app, old), "JNI named worker");
    app.android
        .thread_attached(22, Some("reused owner"))
        .unwrap();
    let fresh = current(&mut app);
    assert_ne!(fresh, old);
    assert_eq!(name(&mut app, fresh), "reused owner");
    native.owner.set(33);
    let generated = current(&mut app);
    assert_ne!(name(&mut app, generated), "main");
    native.owner.set(11);
    assert_eq!(current(&mut app), main);
    assert!(
        app.vm.new_instance("Landroid/os/Handler;").is_ok(),
        "main currentThread adoption must not mark main as an executor callback"
    );
    app.android.thread_detached(22);
    app.android.thread_detached(33);
    native.roots.borrow_mut().clear();
    app.vm.gc();
    assert_eq!(app.vm.heap.class_of(old).unwrap(), rd_vm::gc::FREED_CLASS);
    assert_eq!(current(&mut app), main);
}
#[test]
fn adopted_thread_records_are_bounded_and_weak_dead_records_are_pruned() {
    let (mut app, native) = app();
    current(&mut app);
    for owner in 100..227 {
        native.owner.set(owner);
        current(&mut app);
    }
    native.owner.set(999);
    assert!(
        matches!(app.vm.invoke_static(THREAD,"currentThread",CURRENT,&[]),Err(VmExit::Error(e)) if e.code=="EXECUTOR_BUDGET")
    );
    for owner in 100..227 {
        app.android.thread_detached(owner);
    }
    app.vm.gc();
    current(&mut app);
    native.owner.set(0);
    assert!(
        matches!(app.vm.invoke_static(THREAD,"currentThread",CURRENT,&[]),Err(VmExit::Error(e)) if e.code=="API_UNAVAILABLE")
    );
}
#[test]
fn inherited_platform_method_ids_resolve_the_actual_base_without_inheriting_constructors() {
    let (mut app, _) = app();
    let class = "Ldalvik/system/PathClassLoader;";
    let sig = "(Ljava/lang/String;)Ljava/lang/Class;";
    assert_eq!(
        method_owner(&mut app.vm, class, "loadClass", sig, false).unwrap(),
        "Ljava/lang/ClassLoader;"
    );
    assert_eq!(
        method_owner(
            &mut app.vm,
            "Ljava/lang/BootClassLoader;",
            "getParent",
            "()Ljava/lang/ClassLoader;",
            false
        )
        .unwrap(),
        "Ljava/lang/ClassLoader;"
    );
    let system = app
        .vm
        .invoke_static(
            "Ljava/lang/ClassLoader;",
            "getSystemClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .unwrap();
    let string = app
        .vm
        .invoke_instance(
            system.as_ref().unwrap().unwrap(),
            "loadClass",
            sig,
            &[Value::StrPlaceholder("java.lang.String".into())],
        )
        .unwrap();
    assert_eq!(
        app.vm
            .heap
            .class_of(string.as_ref().unwrap().unwrap())
            .unwrap(),
        "Ljava/lang/Class;"
    );
    for (name, descriptor, static_method) in [
        ("loadClass", sig, true),
        ("loadClass", "(I)Ljava/lang/Class;", false),
        ("<init>", "()V", false),
    ] {
        assert!(
            matches!(method_owner(&mut app.vm, class, name, descriptor, static_method), Err(VmExit::Exception(e)) if e.class == "Ljava/lang/NoSuchMethodError;")
        );
    }
}
#[test]
fn debug_queries_report_this_vms_absent_java_debugger_without_inventing_other_debug_services() {
    let (mut app, _) = app();
    let class = "Landroid/os/Debug;";
    assert!(as_switch_smoke::jni_constructor::class_known(
        &app.vm, class
    ));
    for name in ["isDebuggerConnected", "waitingForDebugger"] {
        assert_eq!(
            method_owner(&mut app.vm, class, name, "()Z", true).unwrap(),
            class
        );
        assert_eq!(
            app.vm.invoke_static(class, name, "()Z", &[]).unwrap(),
            Value::Int(0)
        );
        assert!(method_owner(&mut app.vm, class, name, "()Z", false).is_err());
    }
    assert!(method_owner(
        &mut app.vm,
        class,
        "getNativeHeapAllocatedSize",
        "()J",
        true
    )
    .is_err());
}
#[test]
fn exception_describe_logs_actual_object_detail_clears_pending_and_is_empty_on_repeat() {
    let (mut app, native) = app();
    let mut pending = as_switch_smoke::jni_exceptions::PendingException::default();
    let exception = rd_vm::Throwable::new("Ljava/lang/IllegalStateException;", "initial detail");
    let object = app.vm.materialize_throwable(&exception).unwrap();
    app.vm
        .invoke_instance(
            object,
            "<init>",
            "(Ljava/lang/String;)V",
            &[Value::StrPlaceholder("current detail".into())],
        )
        .unwrap();
    pending.set(
        &app.vm,
        rd_vm::Throwable {
            obj: Some(object),
            ..exception
        },
    );
    pending.describe(&mut app.vm).unwrap();
    assert!(pending.pending.is_none());
    assert!(pending.trace.is_empty());
    let path = format!("logs/android/{}.log", app.android.apk.manifest.package);
    let output = as_platform::fs::read_all(native.fs.as_ref(), &path, 65536).unwrap();
    let text = String::from_utf8(output.clone()).unwrap();
    assert!(
        text.contains("[w] System.err: java.lang.IllegalStateException: current detail"),
        "{text}"
    );
    assert!(!text.contains("initial detail"));
    pending.describe(&mut app.vm).unwrap();
    assert_eq!(
        as_platform::fs::read_all(native.fs.as_ref(), &path, 65536).unwrap(),
        output
    );
    app.vm.gc();
    assert_eq!(
        app.vm.heap.class_of(object).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
}
