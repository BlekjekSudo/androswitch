//! Standard DEX/API calls with deterministic clock and worker delivery.
//! This provider is a test scheduler, not proof of native-thread concurrency.
use as_framework::App;
use as_platform::{
    host::HostPlatform, host_fs::HostFileSystem, BackgroundWorkers, Clock, PlatformError,
};
use rd_vm::{ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
const PROBE: &str = "Lorg/androswitch/contracts/TimerActivity;";
struct Time {
    wall: Cell<i64>,
    fail: Cell<bool>,
}
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        1_234_000
    }
    fn wall_time_millis(&self) -> Result<i64, PlatformError> {
        if self.fail.get() {
            Err(fault("CLOCK_TEST_FAILURE"))
        } else {
            Ok(self.wall.get())
        }
    }
}
fn fault(code: &'static str) -> PlatformError {
    PlatformError {
        code,
        operation: "timer contract test",
        cause: "injected exact failure".into(),
    }
}
#[derive(Default)]
struct Workers {
    starts: RefCell<Vec<u64>>,
    wakes: RefCell<Vec<u64>>,
    cancels: RefCell<Vec<u64>>,
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
    fn wake(&self, token: u64) -> Result<(), PlatformError> {
        if self.fail.get() == Some("wake") {
            return Err(fault("WORKER_WAKE_FAILED"));
        }
        self.wakes.borrow_mut().push(token);
        Ok(())
    }
    fn cancel(&self, token: u64) -> Result<(), PlatformError> {
        if self.fail.get() == Some("cancel") {
            return Err(fault("WORKER_CANCEL_FAILED"));
        }
        self.cancels.borrow_mut().push(token);
        Ok(())
    }
}
struct Test {
    app: App,
    time: Rc<Time>,
    workers: Rc<Workers>,
    root: PathBuf,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(fixture: &str, bind: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let root = std::env::temp_dir().join(format!(
            "as-timer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(&root).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let apk = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/timer-fixture/{fixture}.apk"));
        let record = pm
            .install(
                std::fs::read(apk).expect("run fixtures/m9/timer/build.ps1"),
                true,
                None,
            )
            .unwrap();
        let time = Rc::new(Time {
            wall: Cell::new(10_000),
            fail: Cell::new(false),
        });
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
            time,
            workers,
            root,
        }
    }
    fn invoke(&mut self, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
        self.app.vm.invoke_static(PROBE, name, sig, args)
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.invoke(name, sig, args).unwrap()
    }
    fn create(&mut self, kind: i32) -> u64 {
        self.call("create", "(I)V", &[Value::Int(kind)]);
        *self.workers.starts.borrow().last().unwrap()
    }
    fn task(&mut self, mode: i32) -> ObjRef {
        self.call("makeTask", "(I)V", &[Value::Int(mode)]);
        self.call("reference", "()Ljava/util/TimerTask;", &[])
            .as_ref()
            .unwrap()
            .unwrap()
    }
    fn schedule(&mut self, delay: i64, period: i64, mode: i32) {
        self.call(
            "schedule",
            "(JJI)V",
            &[Value::Long(delay), Value::Long(period), Value::Int(mode)],
        );
    }
    fn schedule_error(&mut self, delay: i64, period: i64, mode: i32) -> VmExit {
        self.invoke(
            "schedule",
            "(JJI)V",
            &[Value::Long(delay), Value::Long(period), Value::Int(mode)],
        )
        .unwrap_err()
    }
    fn step(&mut self, token: u64) -> Result<Option<u64>, VmExit> {
        self.app.android.timer_worker_step(&mut self.app.vm, token)
    }
    fn count(&mut self) -> i32 {
        self.call("count", "()I", &[]).as_int().unwrap()
    }
    fn last(&mut self) -> i64 {
        self.call("scheduled", "()J", &[]).as_long().unwrap()
    }
    fn owner(&mut self) -> ObjRef {
        self.call("owner", "()Ljava/util/Timer;", &[])
            .as_ref()
            .unwrap()
            .unwrap()
    }
}
fn exception(error: VmExit, class: &str) {
    assert!(
        matches!(error,VmExit::Exception(e) if e.class==class),
        "expected {class}"
    );
}
fn code(error: VmExit, expected: &str) {
    assert!(
        matches!(error,VmExit::Error(e) if e.code==expected),
        "expected {expected}"
    );
}
const ISE: &str = "Ljava/lang/IllegalStateException;";
const IAE: &str = "Ljava/lang/IllegalArgumentException;";
#[test]
fn constructors_require_real_provider_and_keep_metadata_without_faking_java_threads() {
    let mut missing = Test::new("timer", false);
    code(
        missing
            .invoke("create", "(I)V", &[Value::Int(0)])
            .unwrap_err(),
        "API_UNAVAILABLE",
    );
    let mut t = Test::new("timer", true);
    t.workers.fail.set(Some("start"));
    code(
        t.invoke("create", "(I)V", &[Value::Int(0)]).unwrap_err(),
        "WORKER_START_FAILED",
    );
    assert!(t.workers.starts.borrow().is_empty());
    t.workers.fail.set(None);
    for (kind, name, daemon) in [
        (0, None, false),
        (1, None, true),
        (2, Some("contract worker"), false),
        (3, Some("contract daemon"), true),
    ] {
        let token = t.create(kind);
        assert_eq!(
            t.app.android.timer_worker_metadata(token),
            Some((name.map(str::to_string), daemon))
        );
        assert_eq!(t.step(token).unwrap(), Some(1000));
        t.call("cancelTimer", "()V", &[]);
        assert_eq!(t.step(token).unwrap(), None);
        t.app.android.timer_worker_stopped(token);
    }
    let before = t.workers.starts.borrow().len();
    exception(
        t.invoke("nullName", "()V", &[]).unwrap_err(),
        "Ljava/lang/NullPointerException;",
    );
    assert_eq!(t.workers.starts.borrow().len(), before);
    assert!(t.workers.starts.borrow().windows(2).all(|w| w[0] < w[1]));
}
#[test]
fn wall_time_is_distinct_and_one_shot_runs_only_on_worker_in_both_packages() {
    for fixture in ["timer", "renamed"] {
        let mut t = Test::new(fixture, true);
        let token = t.create(0);
        t.task(1);
        t.schedule(100, 0, 0);
        t.app.draw().unwrap();
        assert_eq!(t.count(), 0);
        assert_eq!(t.call("wall", "()J", &[]), Value::Long(10_000));
        assert_eq!(t.call("monotonic", "()J", &[]), Value::Long(1234));
        assert_eq!(t.step(token).unwrap(), Some(100));
        t.time.wall.set(9_000);
        assert_eq!(t.step(token).unwrap(), Some(1000));
        t.time.wall.set(10_125);
        assert_eq!(t.step(token).unwrap(), Some(0));
        assert_eq!(t.count(), 1);
        assert_eq!(t.last(), 10_100);
        assert_eq!(t.call("actual", "()J", &[]), Value::Long(10_125));
        assert_eq!(t.call("cancelled", "()Z", &[]), Value::Int(0));
        assert_eq!(t.call("workerLooperAbsent", "()Z", &[]), Value::Int(1));
        assert_eq!(t.step(token).unwrap(), Some(1000));
        exception(t.schedule_error(0, 0, 0), ISE);
    }
}
#[test]
fn fixed_delay_uses_actual_start_and_fixed_rate_catches_up_serially() {
    for mode in [1, 2] {
        let mut t = Test::new("timer", true);
        let token = t.create(0);
        t.task(0);
        t.schedule(100, 50, mode);
        t.time.wall.set(10_225);
        assert_eq!(t.step(token).unwrap(), Some(0));
        assert_eq!(t.last(), if mode == 1 { 10_225 } else { 10_100 });
        if mode == 1 {
            assert_eq!(t.step(token).unwrap(), Some(50));
            assert_eq!(t.count(), 1);
            t.time.wall.set(10_290);
            t.step(token).unwrap();
            assert_eq!(t.last(), 10_290);
        } else {
            t.step(token).unwrap();
            assert_eq!(t.last(), 10_150);
            t.step(token).unwrap();
            assert_eq!(t.last(), 10_200);
            assert_eq!(t.step(token).unwrap(), Some(25));
            assert_eq!(t.count(), 3);
        }
        assert_eq!(t.call("cancelTask", "()Z", &[]), Value::Int(1));
        assert_eq!(t.call("cancelTask", "()Z", &[]), Value::Int(0));
    }
}
#[test]
fn validation_cancellation_and_failed_platform_operations_do_not_partially_schedule() {
    let mut t = Test::new("timer", true);
    let token = t.create(0);
    t.task(0);
    exception(t.schedule_error(-1, 0, 0), IAE);
    exception(t.schedule_error(0, 0, 1), IAE);
    exception(t.schedule_error(0, -1, 2), IAE);
    exception(t.schedule_error(i64::MAX, 0, 0), IAE);
    code(t.invoke("date", "()V", &[]).unwrap_err(), "API_UNAVAILABLE");
    t.time.fail.set(true);
    code(
        t.invoke("wall", "()J", &[]).unwrap_err(),
        "CLOCK_TEST_FAILURE",
    );
    code(t.schedule_error(0, 0, 0), "CLOCK_TEST_FAILURE");
    t.time.fail.set(false);
    t.workers.fail.set(Some("wake"));
    code(t.schedule_error(10, 0, 0), "WORKER_WAKE_FAILED");
    t.workers.fail.set(None);
    t.schedule(10, 0, 0);
    exception(t.schedule_error(10, 0, 0), ISE);
    t.workers.fail.set(Some("cancel"));
    code(
        t.invoke("cancelTimer", "()V", &[]).unwrap_err(),
        "WORKER_CANCEL_FAILED",
    );
    t.workers.fail.set(None);
    t.time.wall.set(10_010);
    t.step(token).unwrap();
    assert_eq!(t.count(), 1);
    t.call("cancelTimer", "()V", &[]);
    t.call("cancelTimer", "()V", &[]);
    t.task(0);
    exception(t.schedule_error(0, 0, 0), ISE);
    assert_eq!(t.step(token).unwrap(), None);
    let mut t = Test::new("timer", true);
    t.create(0);
    t.task(0);
    assert_eq!(t.call("cancelTask", "()Z", &[]), Value::Int(0));
    exception(t.schedule_error(0, 0, 0), ISE);
    t.call("forgetTask", "()V", &[]);
    exception(
        t.schedule_error(0, 0, 0),
        "Ljava/lang/NullPointerException;",
    );
}
#[test]
fn queue_and_running_roots_survive_gc_while_timer_owner_is_weak() {
    let mut t = Test::new("timer", true);
    let token = t.create(0);
    let owner = t.owner();
    let task = t.task(5);
    t.schedule(0, 0, 0);
    t.call("forgetTask", "()V", &[]);
    t.call("forgetTimer", "()V", &[]);
    t.app.vm.gc();
    assert_eq!(
        t.app.vm.heap.class_of(owner).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_ne!(
        t.app.vm.heap.class_of(task).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(t.step(token).unwrap(), None);
    assert_eq!(t.count(), 1);
    t.app.android.timer_worker_stopped(token);
    t.app.vm.gc();
    assert_eq!(
        t.app.vm.heap.class_of(task).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    let token2 = t.create(0);
    assert!(token2 > token);
    let cancelled = t.task(0);
    t.schedule(1000, 0, 0);
    assert_eq!(t.call("cancelTask", "()Z", &[]), Value::Int(1));
    t.call("forgetTask", "()V", &[]);
    t.app.vm.gc();
    assert_ne!(
        t.app.vm.heap.class_of(cancelled).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(t.call("purge", "()I", &[]), Value::Int(1));
    t.app.vm.gc();
    assert_eq!(
        t.app.vm.heap.class_of(cancelled).unwrap(),
        rd_vm::gc::FREED_CLASS
    );
    assert_eq!(t.call("purge", "()I", &[]), Value::Int(0));
}
#[test]
fn task_can_reenter_cancel_and_exact_uncaught_error_kills_worker() {
    let mut t = Test::new("timer", true);
    let token = t.create(0);
    t.task(4);
    t.schedule(0, 0, 0);
    t.step(token).unwrap();
    assert_eq!(t.count(), 1);
    assert_eq!(t.step(token).unwrap(), None);
    assert_eq!(t.count(), 2);
    t.app.android.timer_worker_stopped(token);
    let token = t.create(0);
    t.task(3);
    t.schedule(0, 0, 0);
    let error = t.step(token).unwrap_err();
    let message = match error {
        VmExit::Exception(e) => {
            assert_eq!(e.class, "Ljava/lang/ArithmeticException;");
            e.message
        }
        error => panic!("expected original task exception: {error:?}"),
    };
    let pump = t.app.draw().unwrap_err();
    assert!(
        matches!(pump,VmExit::Exception(e) if e.class=="Ljava/lang/ArithmeticException;" && e.message==message)
    );
    t.app.draw().unwrap();
    t.task(0);
    exception(t.schedule_error(0, 0, 0), ISE);
    assert_eq!(t.step(token).unwrap(), None);
}
#[test]
fn sdk_proof_writes_real_vfs_and_posts_back_to_main_handler() {
    let mut t = Test::new("timer-proof", true);
    let token = t.workers.starts.borrow()[0];
    for now in [10_100, 10_180, 10_260] {
        t.time.wall.set(now);
        t.step(token).unwrap();
    }
    t.app.draw().unwrap();
    let bytes = std::fs::read(t.root.join("Download/androswitch-timer-proof.txt")).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.contains("task=1 scheduled=10100 actual=10100 noLooper=true"),
        "{text}"
    );
    assert!(
        text.contains("task=3 scheduled=10260 actual=10260 noLooper=true"),
        "{text}"
    );
    assert!(text.contains("TIMER_PROOF_PASS elapsed=160"), "{text}");
    assert_eq!(t.step(token).unwrap(), None);
}
#[test]
fn real_host_wall_clock_is_epoch_time() {
    let host = HostPlatform::default();
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let actual = host.wall_time_millis().unwrap();
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!((before..=after).contains(&actual));
}

#[test]
fn callback_owner_identity_survives_native_reentry_without_labelling_main_thread() {
    struct Native {
        identity: Cell<u64>,
        calls: Cell<u32>,
    }
    impl rd_vm::native_host::NativeHost for Native {
        fn current_thread_id(&self) -> Option<u64> {
            Some(self.identity.get())
        }
        fn load(&self, _: &mut rd_vm::Engine, _: &str, _: bool) -> Result<(), VmExit> {
            Err(rd_vm::err::vm_error("API_UNAVAILABLE", "test has no native libraries").into())
        }
        fn roots(&self) -> Vec<ObjRef> {
            Vec::new()
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
            assert_eq!(class, PROBE);
            assert_eq!(name, "nativeHop");
            assert_eq!(self.identity.get(), 2);
            exception(
                vm.invoke_static(PROBE, "handler", "()V", &[]).unwrap_err(),
                "Ljava/lang/RuntimeException;",
            );
            assert_eq!(
                vm.invoke_static(PROBE, "noLooper", "()Z", &[])?,
                Value::Int(1)
            );
            // Model a released VM lease and main-thread entry while the outer
            // Timer callback is still on the worker's stack.
            self.identity.set(1);
            vm.invoke_static(PROBE, "handler", "()V", &[])?;
            assert_eq!(
                vm.invoke_static(PROBE, "noLooper", "()Z", &[])?,
                Value::Int(0)
            );
            self.identity.set(2);
            assert_eq!(
                vm.invoke_static(PROBE, "noLooper", "()Z", &[])?,
                Value::Int(1)
            );
            self.calls.set(self.calls.get() + 1);
            Ok(Value::Null)
        }
    }
    let mut t = Test::new("timer", true);
    let token = t.create(0);
    t.task(6);
    t.schedule(0, 0, 0);
    let native = Rc::new(Native {
        identity: Cell::new(2),
        calls: Cell::new(0),
    });
    t.app.vm.bind_native(native.clone());
    t.step(token).unwrap();
    assert_eq!(native.calls.get(), 1);
    native.identity.set(1);
    t.call("handler", "()V", &[]);
}

#[test]
fn twelve_shared_java_validation_and_cancel_cases_match_actual_host_jvm() {
    let oracle = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/timer-fixture/oracle.txt"),
    )
    .expect("run fixtures/m9/timer/build.ps1");
    for fixture in ["timer", "renamed"] {
        let mut t = Test::new(fixture, true);
        for line in oracle.lines() {
            let (case, expected) = line.split_once(':').unwrap();
            let case = case.parse::<i32>().unwrap();
            let expected = expected.parse::<i32>().unwrap();
            assert_eq!(
                t.app
                    .vm
                    .invoke_static(
                        "Lorg/androswitch/contracts/TimerValidation;",
                        "run",
                        "(I)I",
                        &[Value::Int(case)]
                    )
                    .unwrap(),
                Value::Int(expected),
                "JVM case {case} / {fixture}"
            );
        }
    }
}
