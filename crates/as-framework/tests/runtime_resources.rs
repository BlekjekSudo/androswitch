//! Ordinary Java/JVM and SDK APK contracts; the host provider is explicit.
use as_framework::{Android, App};
use as_platform::{host::HostPlatform, host_fs::HostFileSystem, PlatformError, RuntimeResources};
use rd_vm::{framework_host::HostMethodKind, gc::FREED_CLASS, ObjRef, Value, VmExit};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::{Rc, Weak},
};

const CLASS: &str = "Ljava/lang/Runtime;";
struct Test {
    app: App,
    cases: String,
    root: PathBuf,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(variant: &str, proof: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-runtime-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/runtime-fixture")
            .join(variant)
            .join(if proof {
                "runtime-proof.apk"
            } else {
                "runtime.apk"
            });
        let record = pm
            .install(
                std::fs::read(file).expect("run fixtures/m9/runtime/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let app = App::new(
            pm.load_apk(&record.package, None).unwrap().1,
            fs,
            Rc::new(HostPlatform::default()),
            640,
            360,
            None,
        )
        .unwrap();
        let namespace = if variant == "original" {
            "org/androswitch/contracts"
        } else {
            "net/example/runtime"
        };
        Self {
            app,
            cases: format!("L{namespace}/RuntimeCases;"),
            root,
        }
    }
    fn expose(&mut self) -> ObjRef {
        self.app
            .vm
            .invoke_static(&self.cases, "expose", "()Ljava/lang/Runtime;", &[])
            .unwrap()
            .as_ref()
            .unwrap()
            .unwrap()
    }
    fn processors(&mut self) -> Result<Value, VmExit> {
        self.app
            .vm
            .invoke_static(&self.cases, "processors", "()I", &[])
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn code<T: std::fmt::Debug>(result: Result<T, VmExit>, expected: &str) {
    assert!(
        matches!(result, Err(VmExit::Error(ref error)) if error.code==expected),
        "{result:?}"
    );
}
struct Resources {
    count: Cell<u32>,
    calls: Cell<u32>,
    fail: Cell<bool>,
}
impl Resources {
    fn new(count: u32) -> Self {
        Self {
            count: Cell::new(count),
            calls: Cell::new(0),
            fail: Cell::new(false),
        }
    }
}
impl RuntimeResources for Resources {
    fn available_processors(&self) -> Result<u32, PlatformError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail.get() {
            return Err(PlatformError {
                code: "RESOURCE_TEST_FAILURE",
                operation: "available_processors",
                cause: "exact capability query failure".into(),
            });
        }
        Ok(self.count.get())
    }
}

#[test]
fn sixteen_jvm_comparisons_preserve_singleton_gc_and_actual_host_resources() {
    for variant in ["original", "renamed"] {
        let oracle = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/runtime-fixture")
                .join(variant)
                .join("oracle.tsv"),
        )
        .unwrap();
        assert_eq!(oracle.lines().count(), 4);
        for _ in 0..2 {
            let mut test = Test::new(variant, false);
            test.app
                .android
                .bind_runtime_resources(Rc::new(HostPlatform::default()));
            test.app.launch().unwrap();
            for line in oracle.lines() {
                let (index, expected) = line.split_once('\t').unwrap();
                let result = test
                    .app
                    .vm
                    .invoke_static(
                        &test.cases,
                        "run",
                        "(I)Ljava/lang/String;",
                        &[Value::Int(index.parse().unwrap())],
                    )
                    .unwrap();
                let actual = test
                    .app
                    .vm
                    .heap
                    .as_str(result.as_ref().unwrap().unwrap())
                    .unwrap();
                assert_eq!(actual, expected, "{variant}: {line}");
                test.app.vm.gc();
            }
            let java_count = test.processors().unwrap().as_int().unwrap();
            assert_eq!(
                java_count as usize,
                std::thread::available_parallelism().unwrap().get()
            );
        }
    }
}

#[test]
fn provider_is_required_fresh_and_errors_never_publish_a_guessed_count() {
    let mut test = Test::new("original", false);
    let singleton = test.expose();
    code(test.processors(), "API_UNAVAILABLE");
    assert_eq!(test.expose(), singleton);
    let provider = Rc::new(Resources::new(7));
    test.app.android.bind_runtime_resources(provider.clone());
    assert_eq!(test.processors().unwrap(), Value::Int(7));
    provider.count.set(2);
    assert_eq!(test.processors().unwrap(), Value::Int(2));
    assert_eq!(provider.calls.get(), 2);
    for invalid in [0, u32::MAX] {
        provider.count.set(invalid);
        code(test.processors(), "RESOURCE_INFO_RANGE");
    }
    provider.fail.set(true);
    let failure = test.processors().unwrap_err();
    assert!(
        matches!(failure,VmExit::Error(ref error) if error.code=="RESOURCE_TEST_FAILURE" && error.cause.contains("exact capability query failure"))
    );
    provider.fail.set(false);
    provider.count.set(3);
    assert_eq!(test.processors().unwrap(), Value::Int(3));
    assert_eq!(test.expose(), singleton);
}

#[test]
fn only_the_per_vm_singleton_is_rooted_and_constructor_unsupported_ops_are_not_advertised() {
    let mut test = Test::new("original", false);
    test.app
        .android
        .bind_runtime_resources(Rc::new(Resources::new(5)));
    let singleton = test.expose();
    let garbage = test
        .app
        .vm
        .heap
        .alloc_instance("LUnrelatedRuntimeGarbage;".into(), vec![])
        .unwrap();
    test.app.vm.gc();
    assert_eq!(test.app.vm.heap.class_of(singleton).unwrap(), CLASS);
    assert_eq!(test.app.vm.heap.class_of(garbage).unwrap(), FREED_CLASS);
    assert_eq!(test.expose(), singleton);
    let forged = test
        .app
        .vm
        .heap
        .alloc_instance(CLASS.into(), vec![])
        .unwrap();
    code(
        test.app
            .vm
            .invoke_instance(forged, "availableProcessors", "()I", &[]),
        "INVALID_RECEIVER",
    );
    let framework = test.app.vm.framework_host().unwrap();
    assert_eq!(
        framework.method_metadata(CLASS, "getRuntime", "()Ljava/lang/Runtime;"),
        Some(HostMethodKind::Static)
    );
    assert_eq!(
        framework.method_metadata(CLASS, "availableProcessors", "()I"),
        Some(HostMethodKind::Instance)
    );
    for (name, sig, args) in [
        ("<init>", "()V", vec![]),
        ("maxMemory", "()J", vec![]),
        ("totalMemory", "()J", vec![]),
        ("freeMemory", "()J", vec![]),
        ("gc", "()V", vec![]),
        ("runFinalization", "()V", vec![]),
        ("exit", "(I)V", vec![Value::Int(0)]),
        ("halt", "(I)V", vec![Value::Int(0)]),
        (
            "exec",
            "(Ljava/lang/String;)Ljava/lang/Process;",
            vec![Value::StrPlaceholder("unavailable-command".into())],
        ),
        (
            "addShutdownHook",
            "(Ljava/lang/Thread;)V",
            vec![Value::Null],
        ),
    ] {
        assert_eq!(framework.method_metadata(CLASS, name, sig), None);
        let result = test.app.vm.invoke_instance(singleton, name, sig, &args);
        assert!(result.is_err(), "unsupported {name}{sig} must not succeed");
    }
    assert_eq!(test.processors().unwrap(), Value::Int(5));
}

struct Rebinding {
    android: Weak<Android>,
    next: Rc<Resources>,
}
impl RuntimeResources for Rebinding {
    fn available_processors(&self) -> Result<u32, PlatformError> {
        self.android
            .upgrade()
            .unwrap()
            .bind_runtime_resources(self.next.clone());
        Ok(9)
    }
}
#[test]
fn provider_rebinding_during_query_does_not_hold_a_refcell_borrow() {
    let mut test = Test::new("original", false);
    let next = Rc::new(Resources::new(4));
    test.app.android.bind_runtime_resources(Rc::new(Rebinding {
        android: Rc::downgrade(&test.app.android),
        next: next.clone(),
    }));
    assert_eq!(test.processors().unwrap(), Value::Int(9));
    assert_eq!(test.processors().unwrap(), Value::Int(4));
    assert_eq!(next.calls.get(), 1);
}

#[test]
fn ordinary_sdk_proof_uses_the_same_singleton_provider_and_shared_vfs() {
    let mut test = Test::new("original", true);
    let provider = Rc::new(Resources::new(6));
    test.app.android.bind_runtime_resources(provider.clone());
    test.app.launch().unwrap();
    test.app.draw().unwrap();
    assert_eq!(provider.calls.get(), 2);
    let report =
        std::fs::read_to_string(test.root.join("Download/androswitch-runtime-proof.txt")).unwrap();
    assert_eq!(
        report,
        "RUNTIME_PROOF_PASS\nsingleton=true\nfirstAvailableProcessors=6\navailableProcessors=6\n"
    );
}
