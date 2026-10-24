//! Genuine portable DEX/JVM contracts with controlled native owner identities.
//! Actual Horizon worker execution is a separate SDK-proof/emulator scenario.
use as_framework::App;
use as_platform::{host_fs::HostFileSystem, BackgroundWorkers, Clock, PlatformError};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

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
struct Time;
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        0
    }
}
struct Native {
    owner: Cell<u64>,
    cases: String,
    reentries: Cell<usize>,
}
impl NativeHost for Native {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.owner.get())
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        panic!("no ELF in host ThreadLocal fixture")
    }
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        _: bool,
        _: &[Value],
    ) -> Result<Value, VmExit> {
        assert_eq!((class, name), (self.cases.as_str(), "nativeBounce"));
        assert!(sig.ends_with(")Ljava/lang/Object;"));
        self.reentries.set(self.reentries.get() + 1);
        vm.gc();
        let result = vm.invoke_static(&self.cases, "nativeInner", "()Ljava/lang/Object;", &[])?;
        // Return is rooted by the real nested ThreadLocal table until owner exits.
        vm.gc();
        Ok(result)
    }
}
struct Test {
    app: App,
    native: Rc<Native>,
    workers: Rc<Workers>,
    cases: String,
    root: PathBuf,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(variant: &str, proof: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-thread-local-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/thread-local-fixture")
            .join(variant)
            .join(if proof {
                "thread-local-proof.apk"
            } else {
                "thread-local.apk"
            });
        let record = pm
            .install(
                std::fs::read(fixture).expect("run fixtures/m9/thread-local/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let mut app = App::new(
            pm.load_apk(&record.package, None).unwrap().1,
            fs,
            Rc::new(Time),
            640,
            360,
            None,
        )
        .unwrap();
        let namespace = if variant == "original" {
            "org/androswitch/contracts"
        } else {
            "net/example/locals"
        };
        let cases = format!("L{namespace}/ThreadLocalCases;");
        let native = Rc::new(Native {
            owner: Cell::new(1),
            cases: cases.clone(),
            reentries: Cell::new(0),
        });
        let workers = Rc::new(Workers::default());
        app.vm.bind_native(native.clone());
        app.android.bind_background_workers(workers.clone());
        app.launch().unwrap();
        Self {
            app,
            native,
            workers,
            cases,
            root,
        }
    }
    fn invoke(&mut self, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
        self.app.vm.invoke_static(&self.cases, name, sig, args)
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.invoke(name, sig, args).unwrap()
    }
    fn int(&mut self, name: &str) -> i32 {
        self.call(name, "()I", &[]).as_int().unwrap()
    }
    fn reference(&mut self, name: &str) -> ObjRef {
        self.call(name, "()Ljava/lang/Object;", &[])
            .as_ref()
            .unwrap()
            .unwrap()
    }
    fn text(&mut self, name: &str, sig: &str, args: &[Value]) -> String {
        let r = self.call(name, sig, args).as_ref().unwrap().unwrap();
        self.app.vm.heap.as_str(r).unwrap().to_string()
    }
    fn setup(&mut self) {
        self.call("setup", "()V", &[]);
    }
    fn start(&mut self, value: i32, mode: i32) -> u64 {
        self.call("start", "(II)V", &[Value::Int(value), Value::Int(mode)]);
        *self.workers.0.borrow().last().unwrap()
    }
    fn step(&mut self, token: u64, id: u64) -> Result<Option<u64>, VmExit> {
        self.native.owner.set(id);
        let r = self
            .app
            .android
            .background_worker_step(&mut self.app.vm, token);
        self.native.owner.set(1);
        r
    }
    fn stopped(&mut self, token: u64, id: u64) {
        self.native.owner.set(id);
        self.app.android.background_worker_stopped(token);
        self.app.vm.framework_host().unwrap().thread_detached(id);
        self.native.owner.set(1);
    }
    fn freed(&self, r: ObjRef) -> bool {
        self.app.vm.heap.class_of(r).unwrap() == rd_vm::gc::FREED_CLASS
    }
}
fn oracle(variant: &str) -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/thread-local-fixture/{variant}/oracle.tsv"
    )))
    .unwrap()
    .lines()
    .map(|line| {
        let (k, v) = line.split_once('\t').unwrap();
        (k.to_string(), v.to_string())
    })
    .collect()
}

#[test]
fn ten_initializer_contracts_match_actual_jvm_before_and_after_gc_in_renamed_dex() {
    for variant in ["original", "renamed"] {
        let expected = oracle(variant);
        assert_eq!(expected.len(), 12);
        let mut t = Test::new(variant, false);
        for _ in 0..2 {
            for mode in [0, 1, 2, 3, 4, 5, 7, 8, 9] {
                assert_eq!(
                    t.text("scenario", "(I)Ljava/lang/String;", &[Value::Int(mode)]),
                    expected[&format!("scenario{mode}")],
                    "{variant}/{mode}"
                );
            }
            assert_eq!(
                t.text("precedence", "()Ljava/lang/String;", &[]),
                expected["precedence"]
            );
            t.app.vm.gc();
        }
    }
}
#[test]
fn real_java_thread_ownership_and_reused_native_ids_match_jvm() {
    for variant in ["original", "renamed"] {
        let expected = oracle(variant);
        let mut t = Test::new(variant, false);
        t.setup();
        let a = t.start(202, 0);
        assert_eq!(t.step(a, 2).unwrap(), None);
        let identity = t.call("identity", "()Z", &[]).as_int().unwrap() != 0;
        assert_eq!(
            format!("{}:{}:{}", t.int("result"), t.int("mainValue"), identity),
            expected["worker"]
        );
        t.stopped(a, 2);
        let b = t.start(303, 2);
        assert_eq!(t.step(b, 2).unwrap(), None);
        assert_eq!(
            format!("{}:{}", t.int("result"), t.int("mainValue")),
            expected["newWorker"]
        );
        t.stopped(b, 2);
    }
}
#[test]
fn separate_live_workers_keep_values_while_a_common_native_id_is_reused_after_stop() {
    let mut t = Test::new("original", false);
    t.setup();
    let a = t.start(202, 0);
    t.step(a, 2).unwrap();
    let b = t.start(303, 0);
    t.step(b, 3).unwrap();
    for (owner, value) in [(1, 101), (2, 202), (3, 303)] {
        t.native.owner.set(owner);
        assert_eq!(t.int("value"), value);
    }
    t.native.owner.set(1);
    t.stopped(a, 2);
    let c = t.start(404, 2);
    t.step(c, 2).unwrap();
    assert_eq!(t.int("result"), -1);
    t.stopped(c, 2);
    t.stopped(b, 3);
}
#[test]
fn native_reentry_and_gc_inside_virtual_initializer_preserve_all_roots() {
    for variant in ["original", "renamed"] {
        let mut t = Test::new(variant, false);
        assert_eq!(
            t.text("scenario", "(I)Ljava/lang/String;", &[Value::Int(6)]),
            "88:1"
        );
        assert_eq!(t.native.reentries.get(), 1);
        t.app.vm.gc();
    }
}
#[test]
fn weak_keys_are_not_roots_and_stale_values_are_eventually_reclaimed() {
    let mut t = Test::new("original", false);
    t.setup();
    let key = t.reference("localObject");
    let value = t.reference("valueObject");
    t.app.vm.gc();
    assert!(!t.freed(key) && !t.freed(value));
    t.call("dropKey", "()V", &[]);
    t.app.vm.gc();
    assert!(t.freed(key));
    t.app.vm.gc();
    assert!(t.freed(value));
}
#[test]
fn termination_clears_values_even_when_guest_retains_thread_object() {
    let mut t = Test::new("original", false);
    t.setup();
    let token = t.start(202, 0);
    t.step(token, 2).unwrap();
    t.native.owner.set(2);
    let value = t.reference("valueObject");
    t.native.owner.set(1);
    t.app.vm.gc();
    assert!(!t.freed(value));
    t.stopped(token, 2);
    t.app.vm.gc();
    assert!(t.freed(value));
    assert_eq!(t.int("mainValue"), 101);
}
#[test]
fn native_detach_and_reattach_do_not_inherit_an_old_java_thread_map() {
    let mut t = Test::new("original", false);
    t.setup();
    t.native.owner.set(77);
    t.app
        .vm
        .framework_host()
        .unwrap()
        .thread_attached(77, Some("attached-local"))
        .unwrap();
    t.call("setValue", "(I)V", &[Value::Int(707)]);
    let value = t.reference("valueObject");
    t.app.vm.framework_host().unwrap().thread_detached(77);
    t.app.vm.gc();
    assert!(t.freed(value));
    t.app
        .vm
        .framework_host()
        .unwrap()
        .thread_attached(77, Some("new-attachment"))
        .unwrap();
    assert_eq!(t.int("value"), -1);
    t.app.vm.framework_host().unwrap().thread_detached(77);
    t.native.owner.set(1);
    assert_eq!(t.int("mainValue"), 101);
}
#[test]
fn bounded_keys_and_global_entries_reject_without_replacing_existing_values() {
    let mut t = Test::new("original", false);
    t.call("setupMany", "(I)V", &[Value::Int(1024)]);
    for owner in 1..=4 {
        t.native.owner.set(owner);
        t.call("setMany", "(I)V", &[Value::Int(1024)]);
    }
    t.native.owner.set(5);
    let err = t.invoke("setMany", "(I)V", &[Value::Int(1)]).unwrap_err();
    assert!(err.to_string().contains("THREAD_LOCAL_BUDGET"));
    t.native.owner.set(1);
    t.call("removeFirst", "()V", &[]);
    t.native.owner.set(5);
    t.call("setMany", "(I)V", &[Value::Int(1)]);
    let err = t
        .invoke("scenario", "(I)Ljava/lang/String;", &[Value::Int(0)])
        .unwrap_err();
    assert!(err.to_string().contains("THREAD_LOCAL_BUDGET"));
    t.native.owner.set(2);
    assert_eq!(
        t.call("firstMany", "()Ljava/lang/Object;", &[]),
        Value::Null
    );
}
#[test]
fn unsupported_inheritance_is_explicit_and_does_not_advertise_supplier_api() {
    let mut t = Test::new("original", false);
    let error = t.invoke("unsupportedInheritable", "()V", &[]).unwrap_err();
    assert!(error.to_string().contains("API_UNAVAILABLE"));
    use rd_vm::framework_host::HostMethodKind;
    let host = t.app.vm.framework_host().unwrap();
    assert_eq!(
        host.method_metadata("Ljava/lang/ThreadLocal;", "<init>", "()V"),
        Some(HostMethodKind::Constructor)
    );
    assert!(host
        .method_metadata(
            "Ljava/lang/ThreadLocal;",
            "withInitial",
            "(Ljava/util/function/Supplier;)Ljava/lang/ThreadLocal;"
        )
        .is_none());
}
#[test]
fn sdk_real_worker_proof_transcript_runs_through_the_same_java_callbacks_and_vfs() {
    let mut t = Test::new("original", true);
    let token = *t.workers.0.borrow().last().unwrap();
    assert_eq!(t.step(token, 2).unwrap(), None);
    t.stopped(token, 2);
    t.app.draw().unwrap();
    let text =
        std::fs::read_to_string(t.root.join("Download/androswitch-threadlocal-proof.txt")).unwrap();
    assert_eq!(text,"THREAD_LOCAL_PROOF_PASS\nmain=101\nworker=202\nseparate=true\nstoredNull=true\nactualThreadIdentity=true\n");
}
