//! Uncaught Java exception delivery, rooted through real DEX/native reentry.
//! This is a child of executors: Thread metadata remains owned in one place.
use super::*;

pub(super) const HANDLER: &str = "Ljava/lang/Thread$UncaughtExceptionHandler;";
pub(super) const GROUP: &str = "Ljava/lang/ThreadGroup;";
const HANDLER_SIG: &str = "(Ljava/lang/Thread;Ljava/lang/Throwable;)V";
const MAX_REPORTS: usize = 128;
const MAX_MAIN_SCOPES: usize = 128;

/// Runtime diagnostics retain the original failure independently of any
/// handler/materialization failure. Only the latest 128 records are retained.
pub struct UncaughtExceptionReport {
    pub serial: u64,
    pub origin: &'static str,
    pub thread: Option<ObjRef>,
    pub handler: Option<ObjRef>,
    pub original: Throwable,
    pub handler_failure: Option<VmExit>,
    pub via_thread_group: bool,
    pub handler_invoked: bool,
}
impl Clone for UncaughtExceptionReport {
    fn clone(&self) -> Self {
        Self {
            serial: self.serial,
            origin: self.origin,
            thread: self.thread,
            handler: self.handler,
            original: self.original.clone(),
            handler_failure: self.handler_failure.as_ref().map(copy_error),
            via_thread_group: self.via_thread_group,
            handler_invoked: self.handler_invoked,
        }
    }
}
struct MainEntry {
    owner: Owner,
    forwarded: Option<Throwable>,
}
/// Owned RAII ticket: callbacks hold no service borrow, and a suspended older
/// scope cannot remove a newer scope on another owner when it resumes.
pub struct MainExceptionScope {
    entries: Rc<RefCell<BTreeMap<u64, MainEntry>>>,
    ticket: u64,
    outermost: bool,
}
impl Drop for MainExceptionScope {
    fn drop(&mut self) {
        self.entries.borrow_mut().remove(&self.ticket);
    }
}
#[derive(Default)]
pub(super) struct Handlers {
    default: Cell<Option<ObjRef>>,
    group: Cell<Option<ObjRef>>,
    reports: RefCell<VecDeque<UncaughtExceptionReport>>,
    report_serial: Cell<u64>,
    main_scopes: Rc<RefCell<BTreeMap<u64, MainEntry>>>,
    scope_serial: Cell<u64>,
}
impl Handlers {
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let mut roots = Vec::new();
        roots.extend(self.default.get());
        roots.extend(self.group.get());
        for report in self.reports.borrow().iter() {
            roots.extend(report.thread);
            roots.extend(report.handler);
            roots.extend(report.original.obj);
            if let Some(VmExit::Exception(e)) = &report.handler_failure {
                roots.extend(e.obj);
            }
        }
        roots.extend(
            self.main_scopes
                .borrow()
                .values()
                .filter_map(|entry| entry.forwarded.as_ref().and_then(|failure| failure.obj)),
        );
        roots
    }
    fn record(&self, mut report: UncaughtExceptionReport) {
        // Serial is diagnostic, not a worker/ref identity. Never wrap it.
        let serial = self.report_serial.get().saturating_add(1);
        self.report_serial.set(serial);
        report.serial = serial;
        let mut reports = self.reports.borrow_mut();
        if reports.len() == MAX_REPORTS {
            reports.pop_front();
        }
        reports.push_back(report);
    }
}
fn same_throwable(a: &Throwable, b: &Throwable) -> bool {
    match (a.obj, b.obj) {
        (Some(a), Some(b)) => a == b,
        (None, None) => a.class == b.class && a.message == b.message,
        _ => false,
    }
}
impl Executors {
    pub(super) fn uncaught_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
        use HostMethodKind::{Instance, Static};
        match (class, name, sig) {
            (
                THREAD,
                "getDefaultUncaughtExceptionHandler",
                "()Ljava/lang/Thread$UncaughtExceptionHandler;",
            )
            | (
                THREAD,
                "setDefaultUncaughtExceptionHandler",
                "(Ljava/lang/Thread$UncaughtExceptionHandler;)V",
            ) => Some(Static),
            (
                THREAD,
                "getUncaughtExceptionHandler",
                "()Ljava/lang/Thread$UncaughtExceptionHandler;",
            )
            | (
                THREAD,
                "setUncaughtExceptionHandler",
                "(Ljava/lang/Thread$UncaughtExceptionHandler;)V",
            ) => Some(Instance),
            (GROUP, "uncaughtException", HANDLER_SIG) => Some(Instance),
            _ => None,
        }
    }
    fn validate_handler(vm: &Engine, value: &Value) -> Result<Option<ObjRef>> {
        let reference = value.as_ref()?;
        if let Some(reference) = reference {
            if !vm.cp.is_subtype(vm.heap.class_of(reference)?, HANDLER) {
                return Err(exception(
                    "Ljava/lang/ClassCastException;",
                    "uncaught handler type",
                ));
            }
        }
        Ok(reference)
    }
    pub(super) fn uncaught_call_static(
        &self,
        vm: &Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class != THREAD {
            return Ok(None);
        }
        match (name, sig) {
            (
                "getDefaultUncaughtExceptionHandler",
                "()Ljava/lang/Thread$UncaughtExceptionHandler;",
            ) => Ok(Some(
                self.uncaught.default.get().map_or(Value::Null, Value::Obj),
            )),
            (
                "setDefaultUncaughtExceptionHandler",
                "(Ljava/lang/Thread$UncaughtExceptionHandler;)V",
            ) => {
                let handler = Self::validate_handler(vm, &args[0])?;
                self.uncaught.default.set(handler);
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        }
    }
    fn root_group(&self, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(group) = self.uncaught.group.get() {
            return Ok(group);
        }
        let group = shell(vm, GROUP)?;
        self.uncaught.group.set(Some(group));
        Ok(group)
    }
    pub(super) fn uncaught_call_thread(
        &self,
        vm: &mut Engine,
        thread: ObjRef,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        match (name, sig) {
            ("getUncaughtExceptionHandler", "()Ljava/lang/Thread$UncaughtExceptionHandler;") => {
                let (handler, terminated) = {
                    let state = self.state.borrow();
                    let thread = state
                        .threads
                        .get(&thread)
                        .ok_or_else(|| illegal("Thread missing"))?;
                    (thread.uncaught_handler, thread.phase == Phase::Terminated)
                };
                let handler = match handler {
                    Some(handler) => Value::Obj(handler),
                    None if terminated => Value::Null,
                    None => Value::Obj(self.root_group(vm)?),
                };
                Ok(Some(handler))
            }
            ("setUncaughtExceptionHandler", "(Ljava/lang/Thread$UncaughtExceptionHandler;)V") => {
                let handler = Self::validate_handler(vm, &args[0])?;
                self.state
                    .borrow_mut()
                    .threads
                    .get_mut(&thread)
                    .ok_or_else(|| illegal("Thread missing"))?
                    .uncaught_handler = handler;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        }
    }
    pub(super) fn uncaught_call_group(
        &self,
        vm: &mut Engine,
        r: ObjRef,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if vm.heap.class_of(r)? != GROUP {
            return Ok(None);
        }
        if self.uncaught.group.get() != Some(r) || name != "uncaughtException" || sig != HANDLER_SIG
        {
            return Err(unavailable(
                "custom ThreadGroup/hierarchy API is not implemented",
            ));
        }
        let Some(handler) = self.uncaught.default.get() else {
            return Err(unavailable("direct ThreadGroup uncaught diagnostic requires a System.err/stack-trace sink; automatic uncaught failures remain runtime diagnostics"));
        };
        let thread = obj(&args[0])?;
        let throwable = obj(&args[1])?;
        let _roots = self.protect(vec![handler, thread, throwable])?;
        vm.invoke_instance(handler, "uncaughtException", HANDLER_SIG, args)?;
        Ok(Some(Value::Null))
    }
    pub(in crate::android) fn begin_main_exception_scope(
        &self,
        vm: &Engine,
    ) -> Result<MainExceptionScope> {
        // Scope bookkeeping can cover a single-interpreter host without a
        // NativeHost identity; actual callback Thread lookup remains strict.
        let current = match vm.native_host().and_then(|host| host.current_thread_id()) {
            Some(id) if id != 0 => Owner::Native(id),
            _ => Owner::Interpreter,
        };
        let mut scopes = self.uncaught.main_scopes.borrow_mut();
        if scopes.len() >= MAX_MAIN_SCOPES {
            return Err(error("UNCAUGHT_SCOPE_BUDGET", "128 main exception scopes"));
        }
        let ticket = next(
            &self.uncaught.scope_serial,
            u64::MAX,
            "uncaught scope ticket",
        )?;
        let outermost = !scopes.values().any(|entry| entry.owner == current);
        scopes.insert(
            ticket,
            MainEntry {
                owner: current,
                forwarded: None,
            },
        );
        Ok(MainExceptionScope {
            entries: self.uncaught.main_scopes.clone(),
            ticket,
            outermost,
        })
    }
    pub(in crate::android) fn forward_worker_exception(&self, vm: &Engine, failure: &VmExit) {
        let VmExit::Exception(failure) = failure else {
            return;
        };
        let current = match vm.native_host().and_then(|host| host.current_thread_id()) {
            Some(id) if id != 0 => Owner::Native(id),
            _ => Owner::Interpreter,
        };
        for entry in self.uncaught.main_scopes.borrow_mut().values_mut() {
            if entry.owner == current {
                entry.forwarded = Some(failure.clone());
            }
        }
    }
    pub(in crate::android) fn finish_main_exception_scope<T>(
        &self,
        android: &Android,
        vm: &mut Engine,
        scope: MainExceptionScope,
        result: Result<T>,
    ) -> Result<T> {
        let forwarded = self
            .uncaught
            .main_scopes
            .borrow()
            .get(&scope.ticket)
            .and_then(|entry| entry.forwarded.clone());
        let skip = matches!((&result, forwarded), (Err(VmExit::Exception(a)), Some(b)) if same_throwable(a, &b));
        match result {
            Err(failure) if scope.outermost && !skip => {
                Err(self.dispatch_uncaught(android, vm, "main", failure))
            }
            other => other,
        }
    }
    pub(in crate::android) fn uncaught_reports(&self) -> Vec<UncaughtExceptionReport> {
        self.uncaught.reports.borrow().iter().cloned().collect()
    }
    pub(in crate::android) fn dispatch_uncaught(
        &self,
        android: &Android,
        vm: &mut Engine,
        origin: &'static str,
        failure: VmExit,
    ) -> VmExit {
        let VmExit::Exception(mut original) = failure else {
            return failure;
        };
        let mut report = UncaughtExceptionReport {
            serial: 0,
            origin,
            thread: None,
            handler: None,
            original: original.clone(),
            handler_failure: None,
            via_thread_group: false,
            handler_invoked: false,
        };
        let attempted: Result<()> = (|| {
            let roots = self.protect(original.obj.into_iter().collect())?;
            let object = vm.materialize_throwable(&original)?;
            roots.push(object)?;
            original.obj = Some(object);
            report.original = original.clone();
            let thread = self.current_thread(android, vm)?;
            report.thread = Some(thread);
            roots.push(thread)?;
            // A subclass may override this public getter. Invoke it virtually,
            // and release all service borrows before any DEX/native callback.
            let selected = vm
                .invoke_instance(
                    thread,
                    "getUncaughtExceptionHandler",
                    "()Ljava/lang/Thread$UncaughtExceptionHandler;",
                    &[],
                )?
                .as_ref()?;
            let selected = selected.ok_or_else(|| {
                exception(
                    "Ljava/lang/NullPointerException;",
                    "uncaught handler is null",
                )
            })?;
            roots.push(selected)?;
            let handler = if self.uncaught.group.get() == Some(selected) {
                report.via_thread_group = true;
                self.uncaught.default.get()
            } else {
                Self::validate_handler(vm, &Value::Obj(selected))?
            };
            // The null default has no Java callback. The original exception
            // still escapes through the runtime's existing diagnostic/error
            // policy; do not invent a System.err or process-kill handler.
            let Some(handler) = handler else {
                return Ok(());
            };
            report.handler = Some(handler);
            roots.push(handler)?;
            report.handler_invoked = true;
            vm.invoke_instance(
                handler,
                "uncaughtException",
                HANDLER_SIG,
                &[Value::Obj(thread), Value::Obj(object)],
            )?;
            Ok(())
        })();
        if let Err(failure) = attempted {
            report.handler_failure = Some(failure);
        }
        report.original = original.clone();
        self.uncaught.record(report);
        VmExit::Exception(original)
    }
}
