//! DEX contracts with explicit deterministic worker delivery, not concurrency proof.
use as_framework::App;
use as_platform::{host_fs::HostFileSystem, BackgroundWorkers, Clock, PlatformError};
use rd_vm::framework_host::FrameworkHost;
use rd_vm::{ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
const ACTIVITY: &str = "Lorg/androswitch/contracts/ExecutorActivity;";
const TOKEN_BIT: u64 = 1 << 62;
fn fault(code: &'static str) -> PlatformError {
    PlatformError {
        code,
        operation: "executor contract",
        cause: "injected exact failure".into(),
    }
}
struct Time(Cell<u64>);
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        self.0.get()
    }
    fn wall_time_millis(&self) -> Result<i64, PlatformError> {
        Ok(10_000 + self.0.get() as i64 / 1000)
    }
}
#[derive(Default)]
struct Workers {
    starts: RefCell<Vec<u64>>,
    fail: Cell<Option<&'static str>>,
}
impl BackgroundWorkers for Workers {
    fn start(&self, token: u64) -> Result<(), PlatformError> {
        if self.fail.get() == Some("start") {
            return Err(fault("WORKER_START_FAILED"));
        }
        self.starts.borrow_mut().push(token);
        Ok(())
    }
    fn wake(&self, _: u64) -> Result<(), PlatformError> {
        if self.fail.get() == Some("wake") {
            Err(fault("WORKER_WAKE_FAILED"))
        } else {
            Ok(())
        }
    }
    fn cancel(&self, _: u64) -> Result<(), PlatformError> {
        Err(fault("TEST_UNEXPECTED_CANCEL"))
    }
}
struct Test {
    app: App,
    workers: Rc<Workers>,
    time: Rc<Time>,
    root: PathBuf,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(fixture: &str, bind: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-executor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/executor-fixture/{fixture}.apk"));
        let record = pm
            .install(
                std::fs::read(apk).expect("run fixtures/m9/executors/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let time = Rc::new(Time(Cell::new(0)));
        let workers = Rc::new(Workers::default());
        let mut app = App::new(
            pm.load_apk(&record.package, None).unwrap().1,
            fs,
            time.clone(),
            640,
            360,
            None,
        )
        .unwrap();
        if bind {
            app.android.bind_background_workers(workers.clone());
        }
        app.launch().unwrap();
        Self {
            app,
            workers,
            time,
            root,
        }
    }
    fn invoke(&mut self, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
        self.app.vm.invoke_static(ACTIVITY, name, sig, args)
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.invoke(name, sig, args).unwrap()
    }
    fn create(&mut self, mode: i32, keep: i64, timeout: bool) {
        self.call(
            "create",
            "(IJZ)V",
            &[
                Value::Int(mode),
                Value::Long(keep),
                Value::Int(timeout as i32),
            ],
        );
    }
    fn enqueue(&mut self, id: i32, mode: i32) {
        self.call("enqueue", "(II)V", &[Value::Int(id), Value::Int(mode)]);
    }
    fn enqueue_error(&mut self, id: i32, mode: i32) -> VmExit {
        self.invoke("enqueue", "(II)V", &[Value::Int(id), Value::Int(mode)])
            .unwrap_err()
    }
    fn token(&self) -> u64 {
        *self.workers.starts.borrow().last().unwrap()
    }
    fn step(&mut self, token: u64) -> Result<Option<u64>, VmExit> {
        self.app
            .android
            .background_worker_step(&mut self.app.vm, token)
    }
    fn count(&mut self) -> i32 {
        self.call("count", "()I", &[]).as_int().unwrap()
    }
    fn queue(&mut self) -> i32 {
        self.call("queueSize", "()I", &[]).as_int().unwrap()
    }
    fn reference(&mut self, name: &str, sig: &str) -> ObjRef {
        self.call(name, sig, &[]).as_ref().unwrap().unwrap()
    }
}
fn exception(e: VmExit, class: &str) {
    assert!(
        matches!(e,VmExit::Exception(e) if e.class==class),
        "expected {class}"
    );
}
fn code(e: VmExit, wanted: &str) {
    assert!(
        matches!(e,VmExit::Error(e) if e.code==wanted),
        "expected {wanted}"
    );
}
const REJECTED: &str = "Ljava/util/concurrent/RejectedExecutionException;";

#[test]
fn custom_factory_real_thread_identity_fifo_and_graceful_shutdown_in_two_packages() {
    for fixture in ["executor", "renamed"] {
        let mut t = Test::new(fixture, true);
        t.create(0, 60, true);
        assert!(t.workers.starts.borrow().is_empty());
        t.enqueue(1, 6);
        t.enqueue(2, 6);
        let token = t.token();
        assert_eq!(token & TOKEN_BIT, TOKEN_BIT);
        assert_eq!(t.workers.starts.borrow().len(), 1);
        let thread = t.reference("saved", "()Ljava/lang/Thread;");
        assert_eq!(t.call("factoryCount", "()I", &[]), Value::Int(1));
        t.app.draw().unwrap();
        assert_eq!(t.count(), 0);
        t.call("shutdown", "()V", &[]);
        assert_eq!(t.call("shutdownFlag", "()Z", &[]), Value::Int(1));
        assert_eq!(t.call("terminated", "()Z", &[]), Value::Int(0));
        exception(t.enqueue_error(3, 0), REJECTED);
        t.step(token).unwrap();
        t.step(token).unwrap();
        assert_eq!(t.count(), 2);
        assert_eq!(t.call("order", "()I", &[]), Value::Int(12));
        assert_eq!(t.reference("seen", "()Ljava/lang/Thread;"), thread);
        let name = t.call("name", "()Ljava/lang/String;", &[]);
        assert_eq!(
            t.app
                .vm
                .heap
                .as_str(name.as_ref().unwrap().unwrap())
                .unwrap(),
            "contract pool worker"
        );
        assert_eq!(t.call("noLooper", "()Z", &[]), Value::Int(1));
        assert_eq!(t.call("completed", "()J", &[]), Value::Long(2));
        assert_eq!(t.step(token).unwrap(), None);
        t.app.android.background_worker_stopped(token);
        assert_eq!(t.call("terminated", "()Z", &[]), Value::Int(1));
        assert_eq!(t.call("alive", "()Z", &[]), Value::Int(0));
    }
}
#[test]
fn pool_idle_timeout_uses_monotonic_clock_and_next_submission_creates_new_thread() {
    let mut t = Test::new("executor", true);
    t.create(0, 1, true);
    t.enqueue(1, 0);
    let token = t.token();
    t.step(token).unwrap();
    assert_eq!(t.step(token).unwrap(), Some(1000));
    t.time.0.set(999_000);
    assert_eq!(t.step(token).unwrap(), Some(1));
    t.time.0.set(1_000_000);
    assert_eq!(t.step(token).unwrap(), None);
    t.app.android.background_worker_stopped(token);
    assert_eq!(t.call("poolSize", "()I", &[]), Value::Int(0));
    assert_eq!(t.call("shutdownFlag", "()Z", &[]), Value::Int(0));
    t.enqueue(2, 0);
    let next = t.token();
    assert!(next > token);
    assert_eq!(t.call("factoryCount", "()I", &[]), Value::Int(2));
    t.step(next).unwrap();
    assert_eq!(t.count(), 2);
}
#[test]
fn backend_and_factory_failures_reject_before_queue_acceptance_or_start() {
    let mut t = Test::new("executor", false);
    t.create(0, 60, false);
    code(t.enqueue_error(1, 0), "API_UNAVAILABLE");
    assert_eq!(t.queue(), 0);
    for (mode, expected) in [
        (1, "THREAD_FACTORY_REJECTED"),
        (2, "API_UNAVAILABLE"),
        (3, "API_UNAVAILABLE"),
    ] {
        let mut t = Test::new("executor", true);
        t.create(mode, 60, false);
        code(t.enqueue_error(1, 0), expected);
        assert_eq!(t.queue(), 0);
        assert!(t.workers.starts.borrow().is_empty());
    }
    let mut t = Test::new("executor", true);
    t.create(4, 60, false);
    exception(t.enqueue_error(1, 0), "Ljava/lang/IllegalStateException;");
    assert_eq!(t.queue(), 0);
    assert!(t.workers.starts.borrow().is_empty());
    let mut t = Test::new("executor", true);
    t.create(0, 60, false);
    t.workers.fail.set(Some("start"));
    code(t.enqueue_error(1, 0), "WORKER_START_FAILED");
    assert_eq!(t.queue(), 0);
    assert!(t.workers.starts.borrow().is_empty());
    t.workers.fail.set(None);
    t.enqueue(1, 0);
    let token = t.token();
    t.workers.fail.set(Some("wake"));
    code(t.enqueue_error(2, 0), "WORKER_WAKE_FAILED");
    assert_eq!(t.queue(), 0);
    code(
        t.invoke("shutdown", "()V", &[]).unwrap_err(),
        "WORKER_WAKE_FAILED",
    );
    assert_eq!(t.call("shutdownFlag", "()Z", &[]), Value::Int(0));
    t.workers.fail.set(None);
    t.step(token).unwrap();
    assert_eq!(t.count(), 1);
}
#[test]
fn unsupported_config_and_interrupt_contracts_fail_without_mutation() {
    let mut t = Test::new("executor", true);
    code(
        t.invoke("unsupportedPool", "()V", &[]).unwrap_err(),
        "API_UNAVAILABLE",
    );
    t.create(0, 60, false);
    t.enqueue(1, 0);
    let token = t.token();
    code(
        t.invoke("shutdownNow", "()V", &[]).unwrap_err(),
        "API_UNAVAILABLE",
    );
    assert_eq!(t.call("shutdownFlag", "()Z", &[]), Value::Int(0));
    assert_eq!(t.queue(), 0);
    t.step(token).unwrap();
    assert_eq!(t.count(), 1);
}
#[test]
fn real_standalone_thread_targets_and_subclass_run_are_once_only() {
    for mode in [0, 1] {
        let mut t = Test::new("executor", true);
        t.call("thread", "(I)V", &[Value::Int(mode)]);
        assert_eq!(t.call("alive", "()Z", &[]), Value::Int(0));
        t.call("start", "()V", &[]);
        let token = t.token();
        assert_eq!(t.call("alive", "()Z", &[]), Value::Int(1));
        exception(
            t.invoke("start", "()V", &[]).unwrap_err(),
            "Ljava/lang/IllegalThreadStateException;",
        );
        assert_eq!(t.step(token).unwrap(), None);
        assert_eq!(t.count(), if mode == 0 { 1 } else { 10 });
        let saved = t.reference("saved", "()Ljava/lang/Thread;");
        assert_eq!(t.reference("seen", "()Ljava/lang/Thread;"), saved);
        assert_eq!(t.step(token).unwrap(), None);
        t.app.android.background_worker_stopped(token);
        assert_eq!(t.call("alive", "()Z", &[]), Value::Int(0));
        exception(
            t.invoke("start", "()V", &[]).unwrap_err(),
            "Ljava/lang/IllegalThreadStateException;",
        );
    }
}
#[test]
fn active_thread_targets_pool_queue_and_pending_task_survive_gc_and_release_after_stop() {
    let mut t = Test::new("executor", true);
    t.create(0, 60, false);
    t.enqueue(1, 3);
    let task = t.reference("task", "()Ljava/lang/Runnable;");
    let thread = t.reference("saved", "()Ljava/lang/Thread;");
    let pool = t.reference("owner", "()Ljava/util/concurrent/ThreadPoolExecutor;");
    let token = t.token();
    t.call("shutdown", "()V", &[]);
    t.call("forget", "()V", &[]);
    t.app.vm.gc();
    for object in [task, thread, pool] {
        assert_ne!(
            t.app.vm.heap.class_of(object).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
    t.step(token).unwrap();
    assert_eq!(t.count(), 1);
    assert_eq!(t.step(token).unwrap(), None);
    t.app.android.background_worker_stopped(token);
    t.app.vm.gc();
    for object in [task, thread, pool] {
        assert_eq!(
            t.app.vm.heap.class_of(object).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
}
#[test]
fn reentrant_execute_shutdown_and_uncaught_task_failure_preserve_exact_policy() {
    let mut t = Test::new("executor", true);
    t.create(0, 60, false);
    t.enqueue(1, 1);
    let token = t.token();
    t.step(token).unwrap();
    assert_eq!(t.queue(), 1);
    t.step(token).unwrap();
    assert_eq!(t.call("order", "()I", &[]), Value::Int(19));
    assert_eq!(t.step(token).unwrap(), None);
    let mut t = Test::new("executor", true);
    t.create(0, 60, false);
    t.enqueue(1, 2);
    t.enqueue(2, 0);
    let token = t.token();
    let error = t.step(token).unwrap_err();
    let message = match error {
        VmExit::Exception(e) => {
            assert_eq!(e.class, "Ljava/lang/ArithmeticException;");
            e.message
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(t.call("completed", "()J", &[]), Value::Long(0));
    exception(t.enqueue_error(3, 0), REJECTED);
    let pump = t.app.draw().unwrap_err();
    assert!(
        matches!(pump,VmExit::Exception(e) if e.class=="Ljava/lang/ArithmeticException;"&&e.message==message)
    );
    assert_eq!(t.step(token).unwrap(), None);
    assert_eq!(t.count(), 1);
    t.app.android.background_worker_stopped(token);
    assert_eq!(t.queue(), 0);
}
#[test]
fn normal_sdk_proof_writes_fifo_identity_and_returns_ui_to_main_handler() {
    let mut t = Test::new("executor-proof", true);
    let token = t.token();
    for _ in 0..3 {
        t.step(token).unwrap();
    }
    assert_eq!(t.step(token).unwrap(), None);
    t.app.android.background_worker_stopped(token);
    t.app.draw().unwrap();
    let text =
        std::fs::read_to_string(t.root.join("Download/androswitch-executor-proof.txt")).unwrap();
    for (index, line) in text.lines().take(3).enumerate() {
        assert!(
            line.starts_with(&format!("task={} thread=", index + 1)),
            "{text}"
        );
        assert!(
            line.ends_with("name=SDK executor proof noLooper=true"),
            "{text}"
        );
    }
    let ids: Vec<_> = text
        .lines()
        .take(3)
        .map(|line| {
            line.split("thread=")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
        })
        .collect();
    assert!(ids.windows(2).all(|v| v[0] == v[1]));
    assert!(text.contains("EXECUTOR_PROOF_PASS"));
}
#[test]
fn twelve_shared_java_validation_cases_match_actual_jvm_in_two_packages() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/executor-fixture/oracle.txt"),
    )
    .unwrap();
    for fixture in ["executor", "renamed"] {
        let mut t = Test::new(fixture, true);
        for line in oracle.lines() {
            let (case, expected) = line.split_once(':').unwrap();
            let case = case.parse::<i32>().unwrap();
            let expected = expected.parse::<i32>().unwrap();
            assert_eq!(
                t.app
                    .vm
                    .invoke_static(
                        "Lorg/androswitch/contracts/ExecutorValidation;",
                        "run",
                        "(I)I",
                        &[Value::Int(case)]
                    )
                    .unwrap(),
                Value::Int(expected),
                "case{case}/{fixture}"
            );
        }
    }
}

#[test]
fn native_reentry_keeps_main_handler_separate_from_active_worker_owner() {
    struct Native {
        identity: Cell<u64>,
        calls: Cell<u32>,
    }
    impl rd_vm::native_host::NativeHost for Native {
        fn current_thread_id(&self) -> Option<u64> {
            Some(self.identity.get())
        }
        fn roots(&self) -> Vec<ObjRef> {
            Vec::new()
        }
        fn load(&self, _: &mut rd_vm::Engine, _: &str, _: bool) -> Result<(), VmExit> {
            Err(rd_vm::err::vm_error("API_UNAVAILABLE", "test has no libraries").into())
        }
        fn invoke(
            &self,
            vm: &mut rd_vm::Engine,
            class: &str,
            name: &str,
            _: &str,
            _: bool,
            _: &[Value],
        ) -> Result<Value, VmExit> {
            assert_eq!(class, ACTIVITY);
            assert_eq!(name, "nativeHop");
            exception(
                vm.invoke_static(ACTIVITY, "handler", "()V", &[])
                    .unwrap_err(),
                "Ljava/lang/RuntimeException;",
            );
            self.identity.set(1);
            vm.invoke_static(ACTIVITY, "handler", "()V", &[])?;
            self.identity.set(2);
            exception(
                vm.invoke_static(ACTIVITY, "handler", "()V", &[])
                    .unwrap_err(),
                "Ljava/lang/RuntimeException;",
            );
            self.calls.set(self.calls.get() + 1);
            Ok(Value::Null)
        }
    }
    let mut t = Test::new("executor", true);
    t.create(0, 60, false);
    t.enqueue(1, 4);
    let token = t.token();
    let native = Rc::new(Native {
        identity: Cell::new(2),
        calls: Cell::new(0),
    });
    t.app.vm.bind_native(native.clone());
    t.step(token).unwrap();
    assert_eq!(native.calls.get(), 1);
    let saved = t.reference("saved", "()Ljava/lang/Thread;");
    assert_eq!(
        t.app
            .vm
            .invoke_static(
                "Ljava/lang/Thread;",
                "currentThread",
                "()Ljava/lang/Thread;",
                &[]
            )
            .unwrap(),
        Value::Obj(saved)
    );
    // Native TLS callbacks occur after a step unwinds but before event2/stopped.
    exception(
        t.invoke("handler", "()V", &[]).unwrap_err(),
        "Ljava/lang/RuntimeException;",
    );
    native.identity.set(1);
    t.call("handler", "()V", &[]);
    native.identity.set(2);
    t.app.android.background_worker_stopped(token);
    // A newly attached foreign thread may reuse the OS handle after the old
    // worker exited. It gets a fresh Java identity; retained old Thread state
    // remains terminated rather than being rebound to that owner.
    t.app.android.thread_detached(2);
    t.app
        .android
        .thread_attached(2, Some("reused native owner"))
        .unwrap();
    let fresh = t
        .app
        .vm
        .invoke_static(
            "Ljava/lang/Thread;",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )
        .unwrap()
        .as_ref()
        .unwrap()
        .unwrap();
    assert_ne!(fresh, saved);
    assert_eq!(
        t.app
            .vm
            .invoke_instance(saved, "isAlive", "()Z", &[])
            .unwrap(),
        Value::Int(0)
    );
}
