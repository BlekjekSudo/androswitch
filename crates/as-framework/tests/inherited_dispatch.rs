//! Android host class implementations precede DEX interface candidates.
//! Independent APKs; no game code or package-specific runtime paths.
use as_framework::App;
use as_platform::{host::HostPlatform, host_fs::HostFileSystem};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{cell::Cell, path::PathBuf, rc::Rc};
const ACTIVITY: &str = "Lorg/androswitch/contracts/DispatchActivity;";
const CONTRACT: &str = "Lorg/androswitch/contracts/ResourceChild;";
const RESOURCES: &str = "()Landroid/content/res/Resources;";
#[allow(clippy::assertions_on_constants)]
fn app(fixture: &str) -> App {
    assert!(rd_vm::PORTABLE_CORE);
    let root = std::env::temp_dir().join(format!(
        "as-dispatch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fs = Rc::new(HostFileSystem::new(&root).unwrap());
    let pm = as_package::PackageManager::new(fs.clone());
    let artifact = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/inherited-dispatch-fixture/{fixture}.apk"
    ));
    let record = pm
        .install(
            std::fs::read(artifact).expect("run fixtures/m9/inherited-dispatch/build.ps1"),
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
fn field(app: &App, name: &str) -> Value {
    app.vm.statics[&(ACTIVITY.into(), name.into())].clone()
}
fn call(app: &mut App, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
    app.vm.invoke_static(ACTIVITY, name, sig, args)
}
fn through(app: &mut App, name: &str, owner: &str, receiver: Value) -> Result<Value, VmExit> {
    call(
        app,
        name,
        &format!("({owner})Landroid/content/res/Resources;"),
        &[receiver],
    )
}
fn expect_error<T: std::fmt::Debug>(result: Result<T, VmExit>, code: &str) {
    assert!(
        matches!(result,Err(VmExit::Error(error)) if error.code==code),
        "expected {code}"
    );
}

#[test]
fn abstract_interface_contracts_use_real_activity_context_resources_in_both_packages() {
    for fixture in ["dispatch", "renamed"] {
        let mut app = app(fixture);
        let receiver = field(&app, "instance");
        let resources = field(&app, "bootResources");
        assert_eq!(
            app.vm
                .heap
                .class_of(resources.as_ref().unwrap().unwrap())
                .unwrap(),
            "Landroid/content/res/Resources;"
        );
        // Reproduce the old selection shape: the ordinary DEX resolver finds
        // an abstract interface declaration beyond the builtin Activity chain.
        let (_, definition, method) = app
            .vm
            .cp
            .resolve_method(ACTIVITY, "getResources", RESOURCES)
            .unwrap();
        assert_ne!(definition.access_flags & 0x200, 0);
        assert_ne!(method.access_flags & 0x400, 0);
        for (name, owner) in [
            ("throughInterface", CONTRACT),
            (
                "throughMiddle",
                "Lorg/androswitch/contracts/DispatchActivityMiddle;",
            ),
            ("throughConcrete", ACTIVITY),
            ("throughContext", "Landroid/content/Context;"),
        ] {
            assert_eq!(
                through(&mut app, name, owner, receiver.clone()).unwrap(),
                resources
            );
        }
        assert_eq!(
            app.vm
                .invoke_instance(
                    receiver.as_ref().unwrap().unwrap(),
                    "getResources",
                    RESOURCES,
                    &[]
                )
                .unwrap(),
            resources
        );
        app.vm.gc();
        assert_eq!(
            through(&mut app, "throughInterface", CONTRACT, receiver).unwrap(),
            resources
        );
    }
}

#[test]
fn concrete_dex_override_super_interface_defaults_and_static_dispatch_keep_precedence() {
    let mut app = app("dispatch");
    let resources = field(&app, "bootResources");
    let receiver = call(&mut app, "makeOverride", &format!("(){CONTRACT}"), &[]).unwrap();
    assert_eq!(
        through(&mut app, "throughInterface", CONTRACT, receiver.clone()).unwrap(),
        resources
    );
    assert_eq!(field(&app, "overrides"), Value::Int(1));
    assert_eq!(
        app.vm
            .invoke_instance(
                receiver.as_ref().unwrap().unwrap(),
                "getResources",
                RESOURCES,
                &[]
            )
            .unwrap(),
        resources
    );
    assert_eq!(field(&app, "overrides"), Value::Int(2));
    assert_eq!(
        through(
            &mut app,
            "throughMiddle",
            "Lorg/androswitch/contracts/DispatchActivityMiddle;",
            receiver.clone()
        )
        .unwrap(),
        resources
    );
    assert_eq!(field(&app, "overrides"), Value::Int(3));
    assert_eq!(
        call(&mut app, "marker", &format!("({CONTRACT})I"), &[receiver]).unwrap(),
        Value::Int(17)
    );
    let original = field(&app, "instance");
    assert_eq!(
        through(&mut app, "throughStatic", ACTIVITY, original.clone()).unwrap(),
        resources
    );
    assert_eq!(field(&app, "statics"), Value::Int(1));
    assert_eq!(
        call(&mut app, "staticIdentity", "()I", &[]).unwrap(),
        Value::Int(73)
    );
    expect_error(
        app.vm.invoke_instance(
            original.as_ref().unwrap().unwrap(),
            "staticIdentity",
            "()I",
            &[],
        ),
        "INVALID_ARG",
    );
}

struct NativeResources {
    resources: ObjRef,
    calls: Cell<u32>,
}
impl NativeHost for NativeResources {
    fn current_thread_id(&self) -> Option<u64> {
        Some(1) // Controlled single-threaded host provider, not a device ID.
    }
    fn load(&self, _vm: &mut Engine, _name: &str, _absolute: bool) -> Result<(), VmExit> {
        unreachable!()
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!(class, "Lorg/androswitch/contracts/NativeResources;");
        assert_eq!((name, sig, is_static), ("getResources", RESOURCES, false));
        assert_eq!(
            vm.heap
                .class_of(slots[0].as_ref().unwrap().unwrap())
                .unwrap(),
            class
        );
        self.calls.set(self.calls.get() + 1);
        Ok(Value::Obj(self.resources))
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![self.resources]
    }
}
#[test]
fn native_dex_methods_and_explicit_abstract_class_barriers_are_never_replaced_by_host_success() {
    let mut app = app("dispatch");
    let resources = field(&app, "bootResources");
    let receiver = call(&mut app, "makeNative", &format!("(){CONTRACT}"), &[]).unwrap();
    let native = Rc::new(NativeResources {
        resources: resources.as_ref().unwrap().unwrap(),
        calls: Cell::new(0),
    });
    app.vm.bind_native(native.clone());
    assert_eq!(
        through(&mut app, "throughInterface", CONTRACT, receiver.clone()).unwrap(),
        resources
    );
    assert_eq!(
        app.vm
            .invoke_instance(
                receiver.as_ref().unwrap().unwrap(),
                "getResources",
                RESOURCES,
                &[]
            )
            .unwrap(),
        resources
    );
    assert_eq!(native.calls.get(), 2);
    let barrier = "Lorg/androswitch/contracts/AbstractBarrier;";
    let receiver = app.vm.heap.alloc_instance(barrier.into(), vec![]).unwrap();
    app.vm.attach_host_state(barrier, receiver).unwrap();
    expect_error(
        app.vm
            .invoke_instance(receiver, "getResources", RESOURCES, &[]),
        "NOT_FOUND",
    );
    expect_error(
        through(&mut app, "throughInterface", CONTRACT, Value::Obj(receiver)),
        "NOT_FOUND",
    );
    assert_eq!(native.calls.get(), 2);
}

#[test]
fn wrong_receivers_unbacked_interfaces_and_missing_methods_do_not_receive_android_values() {
    let mut app = app("dispatch");
    let unrelated = app
        .vm
        .heap
        .alloc_instance("Ljava/lang/Object;".into(), vec![])
        .unwrap();
    for (name, owner) in [
        ("throughInterface", CONTRACT),
        ("throughContext", "Landroid/content/Context;"),
    ] {
        expect_error(
            through(&mut app, name, owner, Value::Obj(unrelated)),
            "VM_TYPE_ERROR",
        );
    }
    let unbacked = "Lorg/androswitch/contracts/AbstractUnbacked;";
    let receiver = app.vm.heap.alloc_instance(unbacked.into(), vec![]).unwrap();
    app.vm.attach_host_state(unbacked, receiver).unwrap();
    expect_error(
        through(&mut app, "throughInterface", CONTRACT, Value::Obj(receiver)),
        "NOT_FOUND",
    );
    let original = field(&app, "instance").as_ref().unwrap().unwrap();
    expect_error(
        app.vm
            .invoke_instance(original, "missingResourcesMethod", RESOURCES, &[]),
        "NOT_FOUND",
    );
    assert!(
        matches!(through(&mut app,"throughInterface",CONTRACT,Value::Null),Err(VmExit::Exception(error)) if error.class=="Ljava/lang/NullPointerException;")
    );
}
