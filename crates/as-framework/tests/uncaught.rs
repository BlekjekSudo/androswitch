//! Actual portable VM/DEX exception delivery. Controlled native identities and
//! explicit worker steps are deterministic host contracts, not OS concurrency.
use as_framework::{App, EglConfigBackend, EglConfigs, GlEvent, GraphicsCaps};
use as_platform::{host_fs::HostFileSystem, BackgroundWorkers, Clock, PlatformError};
use rd_vm::{native_host::NativeHost, Engine, ObjRef, Value, VmExit};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
const CASES: &str = "Lorg/androswitch/contracts/UncaughtCases;";
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
struct NoGpu;
impl EglConfigBackend for NoGpu {
    fn choose_config(&self, _: u64, _: &[i32], _: usize) -> Result<EglConfigs, VmExit> {
        panic!("queue-only exception test must not choose an EGL config")
    }
    fn get_config_attrib(&self, _: u64, _: u64, _: i32) -> Result<i32, VmExit> {
        panic!("queue-only exception test must not query EGL attributes")
    }
}
impl Clock for Time {
    fn monotonic_micros(&self) -> u64 {
        0
    }
    fn wall_time_millis(&self) -> Result<i64, PlatformError> {
        Ok(10000)
    }
}
struct Native(Cell<u64>);
impl NativeHost for Native {
    fn current_thread_id(&self) -> Option<u64> {
        Some(self.0.get())
    }
    fn roots(&self) -> Vec<ObjRef> {
        vec![]
    }
    fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
        Err(rd_vm::err::vm_error(
            "TEST_UNEXPECTED_LOAD",
            "no native library in host scheduler contract",
        )
        .into())
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
        assert_eq!((class, name, sig), (CASES, "nativeReenter", "()I"));
        vm.gc();
        let value = vm.invoke_static(CASES, "reenter", "()I", &[])?;
        vm.gc();
        Ok(value)
    }
}
struct Test {
    app: App,
    workers: Rc<Workers>,
    native: Rc<Native>,
}
impl Test {
    #[allow(clippy::assertions_on_constants)]
    fn new(fixture: &str, launch: bool) -> Self {
        assert!(rd_vm::PORTABLE_CORE);
        let directory = std::env::temp_dir().join(format!(
            "as-uncaught-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let fs = Rc::new(HostFileSystem::new(directory).unwrap());
        let pm = as_package::PackageManager::new(fs.clone());
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/uncaught-fixture");
        let record = pm
            .install(
                std::fs::read(root.join(format!("{fixture}.apk")))
                    .expect("run fixtures/m9/uncaught/build.ps1"),
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
        let workers = Rc::new(Workers::default());
        let native = Rc::new(Native(Cell::new(1)));
        app.vm.bind_native(native.clone());
        app.android.bind_background_workers(workers.clone());
        if launch {
            app.launch().unwrap();
        }
        Self {
            app,
            workers,
            native,
        }
    }
    fn invoke(&mut self, name: &str, sig: &str, args: &[Value]) -> Result<Value, VmExit> {
        self.app.vm.invoke_static(CASES, name, sig, args)
    }
    fn call(&mut self, name: &str, sig: &str, args: &[Value]) -> Value {
        self.invoke(name, sig, args).unwrap()
    }
    fn setup(&mut self, mode: i32) {
        self.call("setup", "(I)V", &[Value::Int(mode)]);
    }
    fn int(&mut self, name: &str) -> i32 {
        self.call(name, "()I", &[]).as_int().unwrap()
    }
    fn boolean(&mut self, name: &str) -> bool {
        self.call(name, "()Z", &[]).as_int().unwrap() != 0
    }
    fn start(&mut self, selection: i32, mode: i32) -> u64 {
        self.call("start", "(II)V", &[Value::Int(selection), Value::Int(mode)]);
        *self.workers.0.borrow().last().unwrap()
    }
    fn step(&mut self, token: u64) -> Result<Option<u64>, VmExit> {
        self.native.0.set(2);
        let result = self
            .app
            .android
            .background_worker_step(&mut self.app.vm, token);
        self.native.0.set(1);
        result
    }
    fn stop(&mut self, token: u64) {
        self.native.0.set(2);
        self.app.android.background_worker_stopped(token);
        self.app.vm.framework_host().unwrap().thread_detached(2);
        self.native.0.set(1);
    }
    fn worker_row(&mut self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.int("calls"),
            self.int("tag"),
            self.int("payload"),
            self.boolean("sameThread"),
            self.boolean("sameException"),
            self.boolean("aliveInHandler")
        )
    }
}
fn original(error: &VmExit) -> rd_vm::Throwable {
    match error {
        VmExit::Exception(error) => {
            assert_eq!(error.class, "Ljava/lang/RuntimeException;");
            assert!(error.message.as_ref().unwrap().contains("original-failure"));
            error.clone()
        }
        other => panic!("expected original exception: {other:?}"),
    }
}
#[test]
fn default_per_thread_clearing_virtual_getter_and_termination_match_jvm_except_pinned_android26_difference(
) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/uncaught-fixture");
    let text = std::fs::read_to_string(root.join("oracle.tsv")).unwrap();
    let rows: std::collections::BTreeMap<_, _> = text
        .lines()
        .map(|row| row.split_once('\t').unwrap())
        .collect();
    assert_eq!(rows.len(), 11);
    let mut t = Test::new("uncaught", true);
    assert_eq!(t.boolean("defaultNull").to_string(), rows["defaultNull"]);
    assert_eq!(
        t.boolean("groupFallback").to_string(),
        rows["groupFallback"]
    );
    for selection in 0..4 {
        t.setup(0);
        let token = t.start(selection, 0);
        let failure = t.step(token).unwrap_err();
        original(&failure);
        assert_eq!(t.worker_row(), rows[format!("worker{selection}").as_str()]);
        if selection == 3 {
            assert_eq!(t.int("getterCalls"), 1);
        }
        t.stop(token);
    }
    t.setup(0);
    let token = t.start(0, 1);
    assert_eq!(t.step(token).unwrap(), None);
    assert_eq!(
        format!("{}:{}", t.int("calls"), t.int("caught")),
        rows["caught"]
    );
    t.stop(token);
    assert!(t
        .call(
            "workerHandler",
            "()Ljava/lang/Thread$UncaughtExceptionHandler;",
            &[]
        )
        .as_ref()
        .unwrap()
        .is_none());
    assert_eq!(rows["terminatedNull"], "true");
    assert_eq!(
        rows["setAfterStop"], "false",
        "JDK21 ignores terminated setter"
    );
    assert!(
        t.boolean("setAfterStop"),
        "Android26 Thread.java stores and returns explicit handler before cleared group"
    );
    assert_eq!(
        t.boolean("clearAfterStop").to_string(),
        rows["clearAfterStop"]
    );
}
#[test]
fn handler_snapshot_survives_mutation_gc_and_real_dex_native_reentry() {
    for mode in [1, 3, 4] {
        let mut t = Test::new("uncaught", true);
        t.setup(mode);
        let token = t.start(0, 0);
        let failure = t.step(token).unwrap_err();
        let original = original(&failure);
        assert_eq!(t.int("calls"), 1);
        assert_eq!(t.int("tag"), 10);
        assert_eq!(t.int("payload"), 271);
        assert!(
            t.boolean("sameThread") && t.boolean("sameException") && t.boolean("aliveInHandler")
        );
        if mode == 1 {
            assert!(t.boolean("defaultNull"));
        }
        if mode == 4 {
            assert_eq!(t.int("reentered"), 813);
        }
        let report = t.app.android.uncaught_exception_reports().pop().unwrap();
        assert_eq!(report.original.obj, original.obj);
        assert!(report.handler_invoked);
        assert!(report.handler_failure.is_none());
        t.stop(token);
        t.call("dropOriginalAndHandlerStatics", "()V", &[]);
        t.app.vm.gc();
        assert_eq!(
            t.app
                .vm
                .heap
                .class_of(report.original.obj.unwrap())
                .unwrap(),
            "Ljava/lang/RuntimeException;"
        );
        assert_ne!(
            t.app.vm.heap.class_of(report.handler.unwrap()).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
}
#[test]
fn handler_failure_and_vm_built_exception_are_separate_and_real_heap_objects() {
    let mut t = Test::new("uncaught", true);
    t.setup(2);
    let token = t.start(1, 0);
    let failure = t.step(token).unwrap_err();
    let first = original(&failure);
    let report = t.app.android.uncaught_exception_reports().pop().unwrap();
    assert_eq!(report.original.obj, first.obj);
    assert!(
        matches!(report.handler_failure,Some(VmExit::Exception(ref e))if e.class=="Ljava/lang/IllegalStateException;"&&e.obj.is_some()&&e.message.as_ref().unwrap().contains("handler-failure"))
    );
    assert_eq!(t.int("calls"), 1);
    t.stop(token);
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(0, 3);
    assert!(
        matches!(t.step(token),Err(VmExit::Exception(ref e))if e.class=="Ljava/lang/ArithmeticException;"&&e.obj.is_some())
    );
    let report = t.app.android.uncaught_exception_reports().pop().unwrap();
    assert!(report.handler_invoked && report.handler_failure.is_none());
    assert_eq!(
        t.app
            .vm
            .heap
            .class_of(report.original.obj.unwrap())
            .unwrap(),
        "Ljava/lang/ArithmeticException;"
    );
}
#[test]
fn caught_exceptions_and_non_java_failures_never_call_uncaught_handler() {
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(0, 1);
    assert_eq!(t.step(token).unwrap(), None);
    assert_eq!(t.int("calls"), 0);
    assert!(t.app.android.uncaught_exception_reports().is_empty());
    t.stop(token);
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(0, 2);
    assert!(matches!(t.step(token), Err(VmExit::Error(_))));
    assert_eq!(t.int("calls"), 0);
    assert!(t.app.android.uncaught_exception_reports().is_empty());
}
#[test]
fn main_lifecycle_scope_dispatches_once_and_does_not_redispatch_forwarded_worker_exception() {
    let mut t = Test::new("uncaught-main", false);
    let failure = t.app.launch().unwrap_err();
    original(&failure);
    assert_eq!(t.int("calls"), 1);
    let reports = t.app.android.uncaught_exception_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].origin, "main");
    assert!(t.boolean("sameThread") && t.boolean("sameException"));
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(0, 0);
    let thrown = t.step(token).unwrap_err();
    let first = original(&thrown);
    assert_eq!(t.int("calls"), 1);
    let forwarded = t.app.draw().unwrap_err();
    assert_eq!(original(&forwarded).obj, first.obj);
    assert_eq!(t.int("calls"), 1);
    assert_eq!(t.app.android.uncaught_exception_reports().len(), 1);
}
#[test]
fn timer_and_gl_queue_exceptions_dispatch_on_actual_owner_before_scope_exit() {
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    t.call("startTimer", "()V", &[]);
    let token = *t.workers.0.borrow().last().unwrap();
    assert_eq!(token & (1 << 62), 0);
    let failure = t.step(token).unwrap_err();
    original(&failure);
    let reports = t.app.android.uncaught_exception_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].origin, "timer");
    assert!(t.boolean("sameThread") && t.boolean("sameException") && t.boolean("aliveInHandler"));
    t.stop(token);
    let mut t = Test::new("uncaught", false);
    t.app
        .android
        .bind_graphics_caps(GraphicsCaps {
            width: 1280,
            height: 720,
            gles_version: 0x20000,
            density: 2.,
            density_dpi: 320,
            rotation: 0,
        })
        .unwrap();
    t.app.android.bind_egl_backend(Rc::new(NoGpu));
    t.app.launch().unwrap();
    t.setup(0);
    let view = t
        .call("prepareGl", "()Landroid/opengl/GLSurfaceView;", &[])
        .as_ref()
        .unwrap()
        .unwrap();
    t.native.0.set(2);
    let failure = t
        .app
        .android
        .gl_callback(&mut t.app.vm, view, GlEvent::DrainQueue)
        .unwrap_err();
    t.native.0.set(1);
    original(&failure);
    let reports = t.app.android.uncaught_exception_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].origin, "gl");
    assert!(t.boolean("sameThread") && t.boolean("sameException") && t.boolean("aliveInHandler"));
}
#[test]
fn default_null_and_explicit_group_delegation_keep_truthful_contracts() {
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    t.call("clearDefault", "()V", &[]);
    let token = t.start(0, 0);
    let failure = t.step(token).unwrap_err();
    original(&failure);
    assert_eq!(t.int("calls"), 0);
    let report = t.app.android.uncaught_exception_reports().pop().unwrap();
    assert!(report.via_thread_group && !report.handler_invoked && report.handler_failure.is_none());
    assert!(report.thread.is_some() && report.original.obj.is_some());
}

#[test]
fn delegation_body_and_handler_roots_remain_real_without_guest_static_references() {
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    t.call("dropSavedHandlers", "()V", &[]);
    t.app.vm.gc();
    t.call("explicitGroup", "()V", &[]);
    assert_eq!(t.int("calls"), 1);
    assert_eq!(t.int("tag"), 10);
    assert!(t.boolean("sameThread") && t.boolean("sameException"));
    assert!(
        t.app.android.uncaught_exception_reports().is_empty(),
        "explicit handler call is not automatic uncaught dispatch"
    );
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(1, 0);
    t.call("clearDefault", "()V", &[]);
    t.call("dropSavedHandlers", "()V", &[]);
    t.app.vm.gc();
    original(&t.step(token).unwrap_err());
    assert_eq!(t.int("tag"), 20);
    assert_eq!(t.int("payload"), 271);
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let bad = t.app.vm.heap.alloc_string("not a handler".into()).unwrap();
    assert!(
        matches!(t.app.vm.invoke_static("Ljava/lang/Thread;","setDefaultUncaughtExceptionHandler",
        "(Ljava/lang/Thread$UncaughtExceptionHandler;)V",&[Value::Obj(bad)]),
        Err(VmExit::Exception(ref error)) if error.class=="Ljava/lang/ClassCastException;")
    );
    t.call("explicitGroup", "()V", &[]);
    assert_eq!(t.int("tag"), 10, "failed setter preserves previous handler");
}

#[test]
fn local_only_throwable_survives_gc_in_overridden_handler_getter_before_delivery() {
    let mut t = Test::new("uncaught", true);
    t.setup(0);
    let token = t.start(3, 4);
    let failure = t.step(token).unwrap_err();
    let VmExit::Exception(failure) = failure else {
        panic!("expected local-only Java exception")
    };
    assert!(failure
        .message
        .as_ref()
        .unwrap()
        .contains("local-only-failure"));
    assert_eq!(t.int("getterCalls"), 1);
    assert_eq!(t.int("tag"), 30);
    assert_eq!(t.int("payload"), 271);
    assert!(t.boolean("sameThread"));
    assert!(
        !t.boolean("sameException"),
        "fixture's static exception is a different object"
    );
    let report = t.app.android.uncaught_exception_reports().pop().unwrap();
    assert_eq!(report.original.obj, failure.obj);
    assert!(report.handler_failure.is_none());
    assert_eq!(
        t.app.vm.heap.class_of(failure.obj.unwrap()).unwrap(),
        "Ljava/lang/RuntimeException;"
    );
}
