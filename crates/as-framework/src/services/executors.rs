//! Bounded serial executors and real platform-backed Java Thread targets.
//! Pool workers may use base Thread.run with the supplied Runnable unchanged;
//! resumable custom worker loops are explicitly outside this implementation.
use super::*;
use as_platform::BackgroundWorkers;
use core::cell::Cell;
use rd_vm::framework_host::HostMethodKind;

pub(in crate::android) const TOKEN_BIT: u64 = 1 << 62;
const THREAD: &str = "Ljava/lang/Thread;";
const POOL: &str = "Ljava/util/concurrent/ThreadPoolExecutor;";
const QUEUE: &str = "Ljava/util/concurrent/LinkedBlockingQueue;";
const UNIT: &str = "Ljava/util/concurrent/TimeUnit;";
const FACTORY: &str = "Ljava/util/concurrent/Executors$DefaultThreadFactory;";
const WRAPPER: &str = "Ljava/util/concurrent/Executors$DelegatedExecutorService;";
const WORKER: &str = "Lorg/androswitch/runtime/ExecutorWorker;";
const MAX_OBJECTS: usize = 128;
const MAX_QUEUE: usize = 4096;
const MAX_WORKERS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Running,
    Terminated,
}
#[derive(Clone, Copy)]
enum Kind {
    Plain,
    Pool(ObjRef),
}
struct JavaThread {
    id: u64,
    target: Option<ObjRef>,
    name: String,
    initialized: bool,
    phase: Phase,
    kind: Kind,
    token: Option<u64>,
    retiring: bool,
    failed: bool,
    owner: Option<Owner>,
    context_loader: Option<ObjRef>,
}
struct Queue {
    initialized: bool,
    entries: VecDeque<ObjRef>,
}
struct Pool {
    queue: ObjRef,
    factory: ObjRef,
    keep_alive: u64,
    timeout: bool,
    shutdown: bool,
    broken: bool,
    creating: bool,
    worker: Option<ObjRef>,
    first: Option<ObjRef>,
    running: Option<ObjRef>,
    idle_since: Option<u64>,
    completed: u64,
}
struct Factory {
    id: u64,
    serial: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Interpreter,
    Native(u64),
}
#[derive(Default)]
struct State {
    threads: BTreeMap<ObjRef, JavaThread>,
    tokens: BTreeMap<u64, ObjRef>,
    owners: BTreeMap<Owner, ObjRef>,
    adopted: BTreeMap<Owner, ObjRef>,
    attach_names: BTreeMap<Owner, String>,
    queues: BTreeMap<ObjRef, Queue>,
    pools: BTreeMap<ObjRef, Pool>,
    factories: BTreeMap<ObjRef, Factory>,
    wrappers: BTreeMap<ObjRef, ObjRef>,
    workers: BTreeMap<ObjRef, ObjRef>,
    seconds: Option<ObjRef>,
    errors: VecDeque<VmExit>,
}
#[derive(Default)]
pub(in crate::android) struct Executors {
    state: RefCell<State>,
    backend: RefCell<Option<Rc<dyn BackgroundWorkers>>>,
    serial: Cell<u64>,
    identities: Cell<u64>,
    factories: Cell<u64>,
    main_owner: Cell<Option<Owner>>,
    managed_main_live: Cell<bool>,
    active: RefCell<BTreeMap<Owner, Vec<ObjRef>>>,
    pending: RefCell<BTreeMap<u64, Vec<ObjRef>>>,
    scopes: Cell<u64>,
}
struct Active<'a> {
    owners: &'a RefCell<BTreeMap<Owner, Vec<ObjRef>>>,
    owner: Owner,
}
impl Drop for Active<'_> {
    fn drop(&mut self) {
        let mut map = self.owners.borrow_mut();
        if let Some(stack) = map.get_mut(&self.owner) {
            stack.pop();
            if stack.is_empty() {
                map.remove(&self.owner);
            }
        }
    }
}
struct Pending<'a> {
    roots: &'a RefCell<BTreeMap<u64, Vec<ObjRef>>>,
    ticket: u64,
}
impl Pending<'_> {
    fn push(&self, r: ObjRef) -> Result<()> {
        self.roots
            .borrow_mut()
            .get_mut(&self.ticket)
            .ok_or_else(|| {
                error(
                    "EXECUTOR_ROOT_SCOPE_LOST",
                    "suspended factory root scope disappeared",
                )
            })?
            .push(r);
        Ok(())
    }
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.roots.borrow_mut().remove(&self.ticket);
    }
}
struct Creating<'a> {
    state: &'a RefCell<State>,
    pool: ObjRef,
}
impl Drop for Creating<'_> {
    fn drop(&mut self) {
        if let Some(pool) = self.state.borrow_mut().pools.get_mut(&self.pool) {
            pool.creating = false;
        }
    }
}
fn exception(class: &str, text: &str) -> VmExit {
    Throwable::new(class, text).into()
}
fn invalid(text: &str) -> VmExit {
    exception("Ljava/lang/IllegalArgumentException;", text)
}
fn illegal(text: &str) -> VmExit {
    exception("Ljava/lang/IllegalStateException;", text)
}
fn unavailable(text: &str) -> VmExit {
    error("API_UNAVAILABLE", text)
}
fn platform(e: as_platform::PlatformError) -> VmExit {
    error(e.code, e.to_string())
}
fn copy_error(e: &VmExit) -> VmExit {
    match e {
        VmExit::Exception(e) => e.clone().into(),
        VmExit::Error(e) => e.clone().into(),
    }
}
fn owner(vm: &Engine) -> Result<Owner> {
    match vm.native_host() {
        None => Ok(Owner::Interpreter),
        Some(host) => host
            .current_thread_id()
            .filter(|id| *id != 0)
            .map(Owner::Native)
            .ok_or_else(|| {
                unavailable("background Java Thread requires actual NativeHost identity")
            }),
    }
}
fn next(counter: &Cell<u64>, limit: u64, label: &str) -> Result<u64> {
    let value = counter
        .get()
        .checked_add(1)
        .filter(|v| *v < limit)
        .ok_or_else(|| error("EXECUTOR_TOKEN_LIMIT", label))?;
    counter.set(value);
    Ok(value)
}
impl Executors {
    /// Exact implemented overloads only. Lookup neither invokes a handler nor
    /// allocates/starts workers, and unsupported overloads receive no method ID.
    pub(in crate::android) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<HostMethodKind> {
        use HostMethodKind::{Constructor, Instance, Static};
        match (class,name,sig){
            (THREAD,"currentThread","()Ljava/lang/Thread;")=>Some(Static),
            (THREAD,"<init>","()V"|"(Ljava/lang/Runnable;)V"|"(Ljava/lang/String;)V"|"(Ljava/lang/Runnable;Ljava/lang/String;)V")=>Some(Constructor),
            (THREAD,"start"|"run","()V")|(THREAD,"setName","(Ljava/lang/String;)V")
            |(THREAD,"getName","()Ljava/lang/String;")|(THREAD,"getId","()J")|(THREAD,"isAlive","()Z")=>Some(Instance),
            (THREAD,"getContextClassLoader","()Ljava/lang/ClassLoader;")
            |(THREAD,"setContextClassLoader","(Ljava/lang/ClassLoader;)V")=>Some(Instance),
            ("Ljava/util/concurrent/Executors;","defaultThreadFactory","()Ljava/util/concurrent/ThreadFactory;")
            |("Ljava/util/concurrent/Executors;","unconfigurableExecutorService","(Ljava/util/concurrent/ExecutorService;)Ljava/util/concurrent/ExecutorService;")=>Some(Static),
            (FACTORY,"newThread","(Ljava/lang/Runnable;)Ljava/lang/Thread;")=>Some(Instance),
            (QUEUE,"<init>","()V")=>Some(Constructor),
            (QUEUE,"offer","(Ljava/lang/Object;)Z")|(QUEUE,"poll"|"peek","()Ljava/lang/Object;")
            |(QUEUE,"size"|"remainingCapacity","()I")|(QUEUE,"clear","()V")=>Some(Instance),
            (POOL,"<init>","(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;)V")=>Some(Constructor),
            (POOL|WRAPPER,"execute","(Ljava/lang/Runnable;)V")|(POOL|WRAPPER,"shutdown","()V")
            |(POOL|WRAPPER,"isShutdown"|"isTerminated","()Z")=>Some(Instance),
            (POOL,"allowCoreThreadTimeOut","(Z)V")|(POOL,"allowsCoreThreadTimeOut","()Z")
            |(POOL,"getQueue","()Ljava/util/concurrent/BlockingQueue;")
            |(POOL,"getCorePoolSize"|"getMaximumPoolSize"|"getPoolSize","()I")|(POOL,"getCompletedTaskCount","()J")=>Some(Instance),
            _=>None,
        }
    }
    pub(in crate::android) fn bind_main(&self, vm: &Engine) -> Result<()> {
        let current = owner(vm)?;
        if self.main_owner.get().is_some_and(|old| old != current) {
            return Err(error(
                "THREAD_MAIN_IDENTITY",
                "main owner changed during one runtime",
            ));
        }
        if self.main_owner.get().is_none() {
            self.main_owner.set(Some(current));
            self.managed_main_live.set(true);
        }
        Ok(())
    }
    pub(in crate::android) fn attached(&self, current: u64, name: Option<&str>) -> Result<()> {
        if current == 0 {
            return Err(error(
                "THREAD_IDENTITY_UNAVAILABLE",
                "zero attached thread identity",
            ));
        }
        let mut state = self.state.borrow_mut();
        let owner = Owner::Native(current);
        if let Some(name) = name {
            if name.len() > 4096 {
                return Err(error("EXECUTOR_BUDGET", "4096-byte attached Thread name"));
            }
            if !state.attach_names.contains_key(&owner) && state.attach_names.len() >= MAX_OBJECTS {
                return Err(error("EXECUTOR_BUDGET", "128 attached thread names"));
            }
            state.attach_names.insert(owner, name.to_string());
        }
        Ok(())
    }
    pub(in crate::android) fn detached(&self, current: u64) {
        let mut state = self.state.borrow_mut();
        let owner = Owner::Native(current);
        if self.main_owner.get() == Some(owner) {
            self.managed_main_live.set(false);
        }
        state.attach_names.remove(&owner);
        if let Some(object) = state.adopted.remove(&owner) {
            if let Some(thread) = state.threads.get_mut(&object) {
                thread.phase = Phase::Terminated;
                thread.owner = None;
                thread.target = None;
            }
        }
    }
    fn current_thread(&self, android: &Android, vm: &mut Engine) -> Result<ObjRef> {
        if let Some(worker) = self.current(vm)? {
            return Ok(worker);
        }
        let current = owner(vm)?;
        if let Some(object) = self.state.borrow().adopted.get(&current).copied() {
            return Ok(object);
        }
        let managed_main = self.main_owner.get() == Some(current) && self.managed_main_live.get();
        let name = if managed_main {
            Some("main".to_string())
        } else {
            self.state.borrow().attach_names.get(&current).cloned()
        };
        // JNI-attached native peers begin with null (bootstrap) context.
        // The bound managed application main owner uses the installed loader.
        let context = if managed_main {
            Some(android.services.package_metadata.app_loader(vm)?)
        } else {
            None
        };
        let object = self.thread(vm, None, name, context)?;
        let mut state = self.state.borrow_mut();
        let thread = state
            .threads
            .get_mut(&object)
            .ok_or_else(|| illegal("Adopted Thread was not attached"))?;
        thread.phase = Phase::Running;
        thread.owner = Some(current);
        state.adopted.insert(current, object);
        Ok(object)
    }
    fn protect(&self, roots: Vec<ObjRef>) -> Result<Pending<'_>> {
        let ticket = next(&self.scopes, u64::MAX, "factory root scope ticket")?;
        self.pending.borrow_mut().insert(ticket, roots);
        Ok(Pending {
            roots: &self.pending,
            ticket,
        })
    }
    pub(in crate::android) fn bind(&self, backend: Rc<dyn BackgroundWorkers>) {
        *self.backend.borrow_mut() = Some(backend);
    }
    pub(in crate::android) fn in_callback(&self, vm: &Engine) -> Result<bool> {
        Ok(self.current(vm)?.is_some())
    }
    fn current(&self, vm: &Engine) -> Result<Option<ObjRef>> {
        if self.active.borrow().is_empty() && self.state.borrow().owners.is_empty() {
            return Ok(None);
        }
        let current = owner(vm)?;
        if let Some(thread) = self
            .active
            .borrow()
            .get(&current)
            .and_then(|s| s.last())
            .copied()
        {
            return Ok(Some(thread));
        }
        Ok(self.state.borrow().owners.get(&current).copied())
    }
    fn backend(&self) -> Result<Rc<dyn BackgroundWorkers>> {
        self.backend
            .borrow()
            .clone()
            .ok_or_else(|| unavailable("Java Thread dedicated worker backend"))
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if vm.cp.is_subtype(class, THREAD) {
            let mut state = self.state.borrow_mut();
            if !state.threads.contains_key(&r) {
                if state.threads.len() + state.errors.len() >= MAX_OBJECTS {
                    return Err(error(
                        "EXECUTOR_BUDGET",
                        "128 Java Thread records/pending errors",
                    ));
                }
                let id = next(&self.identities, i64::MAX as u64, "Java Thread identity")?;
                state.threads.insert(
                    r,
                    JavaThread {
                        id,
                        target: None,
                        name: format!("Thread-{}", id - 1),
                        initialized: false,
                        phase: Phase::New,
                        kind: Kind::Plain,
                        token: None,
                        retiring: false,
                        failed: false,
                        owner: None,
                        context_loader: None,
                    },
                );
            }
            return Ok(true);
        }
        if class == QUEUE {
            let mut state = self.state.borrow_mut();
            if state.queues.len() >= MAX_OBJECTS {
                return Err(error("EXECUTOR_BUDGET", "128 blocking queues"));
            }
            state.queues.entry(r).or_insert(Queue {
                initialized: false,
                entries: VecDeque::new(),
            });
            return Ok(true);
        }
        Ok(matches!(class, POOL | FACTORY | WRAPPER | WORKER | UNIT))
    }
    fn initialize_thread(
        &self,
        r: ObjRef,
        target: Option<ObjRef>,
        name: Option<String>,
        context: Option<ObjRef>,
    ) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let thread = state
            .threads
            .get_mut(&r)
            .ok_or_else(|| illegal("Thread was not attached"))?;
        if thread.initialized {
            return Err(illegal("Thread is already initialized"));
        }
        if name.as_ref().is_some_and(|n| n.len() > 4096) {
            return Err(error("EXECUTOR_BUDGET", "4096-byte Java Thread name"));
        }
        thread.target = target;
        thread.context_loader = context;
        if let Some(name) = name {
            thread.name = name;
        }
        thread.initialized = true;
        Ok(())
    }
    fn thread(
        &self,
        vm: &mut Engine,
        target: Option<ObjRef>,
        name: Option<String>,
        context: Option<ObjRef>,
    ) -> Result<ObjRef> {
        let r = shell(vm, THREAD)?;
        self.initialize_thread(r, target, name, context)?;
        Ok(r)
    }
    fn inherited_context(&self, android: &Android, vm: &mut Engine) -> Result<Option<ObjRef>> {
        let parent = self.current_thread(android, vm)?;
        // Respect a real DEX override; do not hold framework borrows across
        // invocation/native suspension. Validate provenance before mutation.
        let context = vm
            .invoke_instance(
                parent,
                "getContextClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )?
            .as_ref()?;
        android
            .services
            .package_metadata
            .resolve_loader(vm, context)?;
        Ok(context)
    }
    pub(super) fn static_field(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        _sig: &str,
    ) -> Result<Option<Value>> {
        if class != UNIT {
            return Ok(None);
        }
        if name != "SECONDS" {
            return Err(unavailable(
                "TimeUnit constant outside supported SECONDS contract",
            ));
        }
        if let Some(r) = self.state.borrow().seconds {
            return Ok(Some(Value::Obj(r)));
        }
        let r = shell(vm, UNIT)?;
        let label = vm.heap.alloc_string("SECONDS".into())?;
        rd_vm::intrinsics::call_instance_intrinsic_mref(
            vm,
            UNIT,
            "Ljava/lang/Enum;",
            "<init>",
            "(Ljava/lang/String;I)V",
            r,
            &[Value::Obj(label), Value::Int(3)],
        )?
        .ok_or_else(|| unavailable("TimeUnit Enum initialization"))?;
        self.state.borrow_mut().seconds = Some(r);
        Ok(Some(Value::Obj(r)))
    }
    pub(super) fn call_static(
        &self,
        android: &Android,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class == THREAD && name == "currentThread" && sig == "()Ljava/lang/Thread;" {
            return Ok(Some(Value::Obj(self.current_thread(android, vm)?)));
        }
        if class != "Ljava/util/concurrent/Executors;" {
            return Ok(None);
        }
        match (name, sig) {
            ("defaultThreadFactory", "()Ljava/util/concurrent/ThreadFactory;") => {
                if self.state.borrow().factories.len() >= MAX_OBJECTS {
                    return Err(error("EXECUTOR_BUDGET", "128 default thread factories"));
                }
                let id = next(&self.factories, i64::MAX as u64, "factory identity")?;
                let r = shell(vm, FACTORY)?;
                self.state
                    .borrow_mut()
                    .factories
                    .insert(r, Factory { id, serial: 0 });
                Ok(Some(Value::Obj(r)))
            }
            (
                "unconfigurableExecutorService",
                "(Ljava/util/concurrent/ExecutorService;)Ljava/util/concurrent/ExecutorService;",
            ) => {
                let pool = self.pool(obj(&args[0])?)?;
                if self.state.borrow().wrappers.len() >= MAX_OBJECTS {
                    return Err(error("EXECUTOR_BUDGET", "128 executor wrappers"));
                }
                let r = shell(vm, WRAPPER)?;
                self.state.borrow_mut().wrappers.insert(r, pool);
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        }
    }
    fn pool(&self, r: ObjRef) -> Result<ObjRef> {
        let state = self.state.borrow();
        let r = state.wrappers.get(&r).copied().unwrap_or(r);
        if !state.pools.contains_key(&r) {
            return Err(unavailable(
                "ExecutorService implementation outside bounded serial pool",
            ));
        }
        Ok(r)
    }
    fn reserve_queue(&self, queue: ObjRef) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if state
            .queues
            .values()
            .map(|q| q.entries.len())
            .sum::<usize>()
            >= MAX_QUEUE
        {
            return Err(error(
                "EXECUTOR_BUDGET",
                "4096 queued executor/BlockingQueue elements",
            ));
        }
        let q = state
            .queues
            .get_mut(&queue)
            .filter(|q| q.initialized)
            .ok_or_else(|| illegal("LinkedBlockingQueue not initialized"))?;
        q.entries
            .try_reserve(1)
            .map_err(|_| error("EXECUTOR_MEMORY_BUDGET", "BlockingQueue allocation"))?;
        Ok(())
    }
    fn start(&self, thread: ObjRef, pool: Option<ObjRef>) -> Result<u64> {
        let backend = self.backend()?;
        {
            let state = self.state.borrow();
            let target = state
                .threads
                .get(&thread)
                .ok_or_else(|| illegal("Thread not initialized"))?;
            if !target.initialized || target.phase != Phase::New {
                return Err(exception(
                    "Ljava/lang/IllegalThreadStateException;",
                    "Thread has already started or is not initialized",
                ));
            }
            if state.tokens.len() >= MAX_WORKERS {
                return Err(error("EXECUTOR_BUDGET", "32 live Java Thread workers"));
            }
            if pool.is_none()
                && target
                    .target
                    .is_some_and(|r| state.workers.contains_key(&r))
            {
                return Err(unavailable(
                    "Pool worker Thread is started only by its owning executor",
                ));
            }
        }
        let token = TOKEN_BIT | next(&self.serial, TOKEN_BIT, "Thread worker token")?;
        backend.start(token).map_err(platform)?;
        let mut state = self.state.borrow_mut();
        let target = state
            .threads
            .get_mut(&thread)
            .ok_or_else(|| illegal("Thread disappeared"))?;
        target.phase = Phase::Running;
        target.token = Some(token);
        target.kind = pool.map(Kind::Pool).unwrap_or(Kind::Plain);
        state.tokens.insert(token, thread);
        if let Some(pool) = pool {
            state
                .pools
                .get_mut(&pool)
                .ok_or_else(|| illegal("Pool disappeared"))?
                .worker = Some(thread);
        }
        Ok(token)
    }
    fn create_worker(&self, vm: &mut Engine, pool: ObjRef, task: ObjRef) -> Result<()> {
        self.backend()?;
        let factory = {
            let mut state = self.state.borrow_mut();
            let p = state
                .pools
                .get_mut(&pool)
                .ok_or_else(|| illegal("Pool missing"))?;
            if p.creating {
                return Err(unavailable("Reentrant executor worker creation"));
            }
            p.creating = true;
            p.factory
        };
        let _creating = Creating {
            state: &self.state,
            pool,
        };
        let runnable = shell(vm, WORKER)?;
        self.state.borrow_mut().workers.insert(runnable, pool);
        // Independent native workers can release VmGate and resume out of
        // order. Root ownership therefore uses a ticket, never global LIFO.
        let pending = self.protect(vec![pool, factory, task, runnable])?;
        let thread = vm
            .invoke_instance(
                factory,
                "newThread",
                "(Ljava/lang/Runnable;)Ljava/lang/Thread;",
                &[Value::Obj(runnable)],
            )?
            .as_ref()?
            .ok_or_else(|| error("THREAD_FACTORY_REJECTED", "ThreadFactory returned null"))?;
        pending.push(thread)?;
        let class = vm.heap.class_of(thread)?;
        if !vm.cp.is_subtype(class, THREAD)
            || vm.cp.resolve_method(class, "run", "()V").is_some()
            || vm.cp.resolve_method(class, "start", "()V").is_some()
        {
            return Err(unavailable("Pool ThreadFactory must return base Thread.run/start behavior; custom worker continuations are unsupported"));
        }
        {
            let state = self.state.borrow();
            let t = state
                .threads
                .get(&thread)
                .ok_or_else(|| unavailable("ThreadFactory returned unattached Thread"))?;
            if t.target != Some(runnable) {
                return Err(unavailable(
                    "Pool ThreadFactory must preserve the supplied worker Runnable",
                ));
            }
            if t.phase != Phase::New || !t.initialized {
                return Err(exception(
                    "Ljava/lang/IllegalThreadStateException;",
                    "Factory returned an already-started/uninitialized Thread",
                ));
            }
            let p = state
                .pools
                .get(&pool)
                .ok_or_else(|| illegal("Pool disappeared"))?;
            if p.shutdown {
                return Err(exception(
                    "Ljava/util/concurrent/RejectedExecutionException;",
                    "Executor shut down during ThreadFactory callback",
                ));
            }
        }
        self.start(thread, Some(pool))?;
        // The initial command belongs to the new Worker, not getQueue(). The
        // transport cannot enter VM until this operation releases VmGate.
        self.state.borrow_mut().pools.get_mut(&pool).unwrap().first = Some(task);
        Ok(())
    }
    fn execute(&self, vm: &mut Engine, pool: ObjRef, task: ObjRef) -> Result<()> {
        let (queue, worker) = {
            let state = self.state.borrow();
            let p = state
                .pools
                .get(&pool)
                .ok_or_else(|| illegal("Pool missing"))?;
            if p.shutdown || p.broken {
                return Err(exception(
                    "Ljava/util/concurrent/RejectedExecutionException;",
                    "Executor is shut down or failed",
                ));
            }
            if p.creating {
                return Err(unavailable("Reentrant executor worker creation"));
            }
            (p.queue, p.worker)
        };
        if let Some(thread) = worker {
            self.reserve_queue(queue)?;
            let token = self
                .state
                .borrow()
                .threads
                .get(&thread)
                .and_then(|t| t.token)
                .ok_or_else(|| illegal("Pool worker token missing"))?;
            self.backend()?.wake(token).map_err(platform)?;
            self.state
                .borrow_mut()
                .queues
                .get_mut(&queue)
                .ok_or_else(|| illegal("Pool queue disappeared"))?
                .entries
                .push_back(task);
        } else {
            self.create_worker(vm, pool, task)?;
        }
        Ok(())
    }
    fn shutdown(&self, pool: ObjRef) -> Result<()> {
        let token = {
            let state = self.state.borrow();
            let p = state
                .pools
                .get(&pool)
                .ok_or_else(|| illegal("Pool missing"))?;
            if p.shutdown {
                return Ok(());
            }
            p.worker
                .and_then(|r| state.threads.get(&r).and_then(|t| t.token))
        };
        if let Some(token) = token {
            self.backend()?.wake(token).map_err(platform)?;
        }
        self.state
            .borrow_mut()
            .pools
            .get_mut(&pool)
            .ok_or_else(|| illegal("Pool missing"))?
            .shutdown = true;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        android: &Android,
        vm: &mut Engine,
        _class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let actual = vm.heap.class_of(r)?.to_string();
        if vm.cp.is_subtype(&actual, THREAD) {
            return Ok(Some(match (name, sig) {
                (
                    "<init>",
                    "()V"
                    | "(Ljava/lang/Runnable;)V"
                    | "(Ljava/lang/String;)V"
                    | "(Ljava/lang/Runnable;Ljava/lang/String;)V",
                ) => {
                    let (target, name) = match sig {
                        "()V" => (None, None),
                        "(Ljava/lang/Runnable;)V" => (args[0].as_ref()?, None),
                        "(Ljava/lang/String;)V" => (None, Some(text(vm, &args[0])?)),
                        _ => (args[0].as_ref()?, Some(text(vm, &args[1])?)),
                    };
                    let context = self.inherited_context(android, vm)?;
                    self.initialize_thread(r, target, name, context)?;
                    Value::Null
                }
                ("getContextClassLoader", "()Ljava/lang/ClassLoader;") => self
                    .state
                    .borrow()
                    .threads
                    .get(&r)
                    .ok_or_else(|| illegal("Thread missing"))?
                    .context_loader
                    .map_or(Value::Null, Value::Obj),
                ("setContextClassLoader", "(Ljava/lang/ClassLoader;)V") => {
                    let context = args[0].as_ref()?;
                    android
                        .services
                        .package_metadata
                        .resolve_loader(vm, context)?;
                    self.state
                        .borrow_mut()
                        .threads
                        .get_mut(&r)
                        .ok_or_else(|| illegal("Thread missing"))?
                        .context_loader = context;
                    Value::Null
                }
                ("start", "()V") => {
                    self.start(r, None)?;
                    Value::Null
                }
                ("run", "()V") => {
                    let target = self
                        .state
                        .borrow()
                        .threads
                        .get(&r)
                        .ok_or_else(|| illegal("Thread missing"))?
                        .target;
                    if let Some(target) = target {
                        if self.state.borrow().workers.contains_key(&target) {
                            return Err(unavailable(
                                "Manual invocation of an executor worker loop",
                            ));
                        }
                        vm.invoke_instance(target, "run", "()V", &[])?;
                    }
                    Value::Null
                }
                ("setName", "(Ljava/lang/String;)V") => {
                    let name = text(vm, &args[0])?;
                    if name.len() > 4096 {
                        return Err(error("EXECUTOR_BUDGET", "4096-byte Thread name"));
                    }
                    self.state
                        .borrow_mut()
                        .threads
                        .get_mut(&r)
                        .ok_or_else(|| illegal("Thread missing"))?
                        .name = name;
                    Value::Null
                }
                ("getName", "()Ljava/lang/String;") => {
                    let name = self
                        .state
                        .borrow()
                        .threads
                        .get(&r)
                        .ok_or_else(|| illegal("Thread missing"))?
                        .name
                        .clone();
                    string(vm, name)?
                }
                ("getId", "()J") => Value::Long(
                    self.state
                        .borrow()
                        .threads
                        .get(&r)
                        .ok_or_else(|| illegal("Thread missing"))?
                        .id as i64,
                ),
                ("isAlive", "()Z") => Value::Int(i32::from(
                    self.state
                        .borrow()
                        .threads
                        .get(&r)
                        .is_some_and(|t| t.phase == Phase::Running),
                )),
                ("join", _)
                | ("interrupt", _)
                | ("setDaemon", _)
                | ("setPriority", _)
                | ("setUncaughtExceptionHandler", _) => return Err(unavailable(
                    "Thread waiting/interruption/daemon/priority/custom uncaught-handler contract",
                )),
                _ => return Ok(None),
            }));
        }
        if actual == FACTORY
            && name == "newThread"
            && sig == "(Ljava/lang/Runnable;)Ljava/lang/Thread;"
        {
            let name = {
                let mut state = self.state.borrow_mut();
                let factory = state
                    .factories
                    .get_mut(&r)
                    .ok_or_else(|| illegal("DefaultThreadFactory missing"))?;
                factory.serial = factory
                    .serial
                    .checked_add(1)
                    .ok_or_else(|| error("EXECUTOR_TOKEN_LIMIT", "factory thread counter"))?;
                format!("pool-{}-thread-{}", factory.id, factory.serial)
            };
            let context = self.inherited_context(android, vm)?;
            return Ok(Some(Value::Obj(self.thread(
                vm,
                args[0].as_ref()?,
                Some(name),
                context,
            )?)));
        }
        if actual == QUEUE {
            return Ok(Some(match (name, sig) {
                ("<init>", "()V") => {
                    self.state
                        .borrow_mut()
                        .queues
                        .get_mut(&r)
                        .ok_or_else(|| illegal("Queue missing"))?
                        .initialized = true;
                    Value::Null
                }
                ("offer" | "add", "(Ljava/lang/Object;)Z") => {
                    let target = obj(&args[0])?;
                    self.reserve_queue(r)?;
                    self.state
                        .borrow_mut()
                        .queues
                        .get_mut(&r)
                        .unwrap()
                        .entries
                        .push_back(target);
                    Value::Int(1)
                }
                ("poll", "()Ljava/lang/Object;") => self
                    .state
                    .borrow_mut()
                    .queues
                    .get_mut(&r)
                    .ok_or_else(|| illegal("Queue missing"))?
                    .entries
                    .pop_front()
                    .map(Value::Obj)
                    .unwrap_or(Value::Null),
                ("peek", "()Ljava/lang/Object;") => self
                    .state
                    .borrow()
                    .queues
                    .get(&r)
                    .ok_or_else(|| illegal("Queue missing"))?
                    .entries
                    .front()
                    .copied()
                    .map(Value::Obj)
                    .unwrap_or(Value::Null),
                ("size", "()I") => Value::Int(
                    self.state
                        .borrow()
                        .queues
                        .get(&r)
                        .ok_or_else(|| illegal("Queue missing"))?
                        .entries
                        .len() as i32,
                ),
                ("remainingCapacity", "()I") => Value::Int(
                    i32::MAX
                        - self
                            .state
                            .borrow()
                            .queues
                            .get(&r)
                            .ok_or_else(|| illegal("Queue missing"))?
                            .entries
                            .len() as i32,
                ),
                ("isEmpty", "()Z") => Value::Int(i32::from(
                    self.state
                        .borrow()
                        .queues
                        .get(&r)
                        .ok_or_else(|| illegal("Queue missing"))?
                        .entries
                        .is_empty(),
                )),
                ("clear", "()V") => {
                    self.state
                        .borrow_mut()
                        .queues
                        .get_mut(&r)
                        .ok_or_else(|| illegal("Queue missing"))?
                        .entries
                        .clear();
                    Value::Null
                }
                _ => {
                    return Err(unavailable(
                        "LinkedBlockingQueue overload/blocking wait outside bounded contract",
                    ))
                }
            }));
        }
        if actual == POOL && name == "<init>" {
            if sig!="(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;)V"{return Err(unavailable("ThreadPoolExecutor constructor overload"));}
            let core = args[0].as_int()?;
            let max = args[1].as_int()?;
            let alive = args[2].as_long()?;
            let unit = obj(&args[3])?;
            let queue = obj(&args[4])?;
            let factory = obj(&args[5])?;
            if core < 0 || max <= 0 || max < core || alive < 0 {
                return Err(invalid("Invalid pool sizes or negative keep-alive"));
            }
            if core != 1 || max != 1 {
                return Err(unavailable(
                    "Only core=max=1 serial ThreadPoolExecutor is supported",
                ));
            }
            let state = self.state.borrow();
            if state.seconds != Some(unit) {
                return Err(unavailable("Pool keep-alive TimeUnit outside SECONDS"));
            }
            if !state.queues.get(&queue).is_some_and(|q| q.initialized) {
                return Err(unavailable("Pool requires initialized LinkedBlockingQueue"));
            }
            if state.pools.len() >= MAX_OBJECTS {
                return Err(error("EXECUTOR_BUDGET", "128 serial executors"));
            }
            if state.pools.contains_key(&r) {
                return Err(illegal("Pool initialized twice"));
            }
            drop(state);
            if !vm.cp.is_subtype(
                vm.heap.class_of(factory)?,
                "Ljava/util/concurrent/ThreadFactory;",
            ) {
                return Err(invalid("Pool factory does not implement ThreadFactory"));
            }
            let keep_alive = (alive as u64)
                .checked_mul(1_000_000)
                .ok_or_else(|| unavailable("Pool keep-alive exceeds monotonic counter range"))?;
            self.state.borrow_mut().pools.insert(
                r,
                Pool {
                    queue,
                    factory,
                    keep_alive,
                    timeout: false,
                    shutdown: false,
                    broken: false,
                    creating: false,
                    worker: None,
                    first: None,
                    running: None,
                    idle_since: None,
                    completed: 0,
                },
            );
            return Ok(Some(Value::Null));
        }
        if actual == POOL || actual == WRAPPER {
            let pool = self.pool(r)?;
            return Ok(Some(match (name, sig) {
                ("execute", "(Ljava/lang/Runnable;)V") => {
                    self.execute(vm, pool, obj(&args[0])?)?;
                    Value::Null
                }
                ("shutdown", "()V") => {
                    self.shutdown(pool)?;
                    Value::Null
                }
                ("isShutdown", "()Z") => {
                    Value::Int(i32::from(self.state.borrow().pools[&pool].shutdown))
                }
                ("isTerminated", "()Z") => {
                    let state = self.state.borrow();
                    let p = &state.pools[&pool];
                    Value::Int(i32::from(
                        p.shutdown
                            && p.worker.is_none()
                            && p.first.is_none()
                            && state.queues[&p.queue].entries.is_empty(),
                    ))
                }
                ("allowCoreThreadTimeOut", "(Z)V") if actual == POOL => {
                    let enabled = args[0].as_int()? != 0;
                    let mut state = self.state.borrow_mut();
                    let p = state.pools.get_mut(&pool).unwrap();
                    if enabled && p.keep_alive == 0 {
                        return Err(invalid("Core timeout requires positive keep-alive"));
                    }
                    p.timeout = enabled;
                    Value::Null
                }
                ("allowsCoreThreadTimeOut", "()Z") if actual == POOL => {
                    Value::Int(i32::from(self.state.borrow().pools[&pool].timeout))
                }
                ("getQueue", "()Ljava/util/concurrent/BlockingQueue;") if actual == POOL => {
                    Value::Obj(self.state.borrow().pools[&pool].queue)
                }
                ("getCorePoolSize" | "getMaximumPoolSize", "()I") if actual == POOL => {
                    Value::Int(1)
                }
                ("getPoolSize", "()I") if actual == POOL => {
                    Value::Int(i32::from(self.state.borrow().pools[&pool].worker.is_some()))
                }
                ("getCompletedTaskCount", "()J") if actual == POOL => {
                    Value::Long(self.state.borrow().pools[&pool].completed as i64)
                }
                ("shutdownNow", _) | ("awaitTermination", _) | ("submit", _) => {
                    return Err(unavailable("Executor interruption/wait/Future contract"))
                }
                _ => return Ok(None),
            }));
        }
        if actual == WORKER {
            return Err(unavailable(
                "Executor worker Runnable requires its platform scheduler",
            ));
        }
        let _ = android;
        Ok(None)
    }
    pub(in crate::android) fn step(
        &self,
        android: &Android,
        vm: &mut Engine,
        token: u64,
    ) -> Result<Option<u64>> {
        let result = self.step_inner(android, vm, token);
        if let Err(e) = &result {
            self.worker_error(token, copy_error(e));
        }
        result
    }
    fn step_inner(&self, android: &Android, vm: &mut Engine, token: u64) -> Result<Option<u64>> {
        let (thread, kind) = {
            let state = self.state.borrow();
            let Some(thread) = state.tokens.get(&token).copied() else {
                return Ok(None);
            };
            let t = &state.threads[&thread];
            if t.failed || t.retiring {
                return Ok(None);
            }
            (thread, t.kind)
        };
        let current = owner(vm)?;
        if matches!(current, Owner::Native(_)) {
            let mut state = self.state.borrow_mut();
            if state.adopted.get(&current).is_some_and(|r| *r != thread) {
                return Err(error(
                    "THREAD_IDENTITY_CONFLICT",
                    "platform owner is already represented by another live adopted Java Thread",
                ));
            }
            if state.owners.get(&current).is_some_and(|r| *r != thread) {
                return Err(error(
                    "THREAD_IDENTITY_CONFLICT",
                    "actual worker identity is already bound to another live Java Thread",
                ));
            }
            state.owners.insert(current, thread);
            state.threads.get_mut(&thread).unwrap().owner = Some(current);
        }
        let task = match kind {
            Kind::Plain => None,
            Kind::Pool(pool) => {
                let mut state = self.state.borrow_mut();
                if state.pools[&pool].broken {
                    if state.pools[&pool].worker == Some(thread) {
                        state.pools.get_mut(&pool).unwrap().worker = None;
                    }
                    state.threads.get_mut(&thread).unwrap().retiring = true;
                    return Ok(None);
                }
                let queue = state.pools[&pool].queue;
                let task = state
                    .pools
                    .get_mut(&pool)
                    .unwrap()
                    .first
                    .take()
                    .or_else(|| state.queues.get_mut(&queue).unwrap().entries.pop_front());
                let p = state.pools.get_mut(&pool).unwrap();
                if let Some(task) = task {
                    p.running = Some(task);
                    p.idle_since = None;
                    Some((pool, task))
                } else {
                    let now = android.clock.monotonic_micros();
                    let since = *p.idle_since.get_or_insert(now);
                    let elapsed = now.saturating_sub(since);
                    if p.shutdown || (p.timeout && elapsed >= p.keep_alive) {
                        p.worker = None;
                        state.threads.get_mut(&thread).unwrap().retiring = true;
                        return Ok(None);
                    }
                    let delay = if p.timeout {
                        p.keep_alive
                            .saturating_sub(elapsed)
                            .div_ceil(1000)
                            .min(1000)
                    } else {
                        1000
                    };
                    return Ok(Some(delay));
                }
            }
        };
        self.active
            .borrow_mut()
            .entry(current)
            .or_default()
            .push(thread);
        let _active = Active {
            owners: &self.active,
            owner: current,
        };
        vm.reset_fuel_window();
        let result = if let Some((_, task)) = task {
            vm.invoke_instance(task, "run", "()V", &[])
        } else {
            vm.invoke_instance(thread, "run", "()V", &[])
        };
        if let Some((pool, _)) = task {
            let mut state = self.state.borrow_mut();
            let p = state.pools.get_mut(&pool).unwrap();
            p.running = None;
            if result.is_ok() {
                p.completed = p.completed.saturating_add(1);
            }
            p.idle_since = Some(android.clock.monotonic_micros());
        } else {
            self.state
                .borrow_mut()
                .threads
                .get_mut(&thread)
                .unwrap()
                .retiring = true;
        }
        result?;
        Ok(if task.is_some() { Some(0) } else { None })
    }
    pub(in crate::android) fn worker_error(&self, token: u64, failure: VmExit) {
        let mut state = self.state.borrow_mut();
        let Some(thread) = state.tokens.get(&token).copied() else {
            return;
        };
        let t = state.threads.get_mut(&thread).unwrap();
        if t.failed {
            return;
        }
        t.failed = true;
        let kind = t.kind;
        if let Kind::Pool(pool) = kind {
            if let Some(p) = state.pools.get_mut(&pool) {
                p.broken = true;
                p.shutdown = true;
                if p.worker == Some(thread) {
                    p.running = None;
                }
            }
        }
        state.errors.push_back(failure);
    }
    pub(in crate::android) fn stopped(&self, token: u64) {
        let mut state = self.state.borrow_mut();
        let Some(thread) = state.tokens.remove(&token) else {
            return;
        };
        let t = state.threads.get_mut(&thread).unwrap();
        t.phase = Phase::Terminated;
        // The actual worker and TLS destructors have stopped. Retaining the
        // Java Thread object must not retain its completed task/worker loop.
        t.target = None;
        let kind = t.kind;
        let current = t.owner.take();
        if let Some(current) = current {
            if state.owners.get(&current) == Some(&thread) {
                state.owners.remove(&current);
            }
        }
        if let Kind::Pool(pool) = kind {
            let queue = if let Some(p) = state.pools.get_mut(&pool) {
                if p.worker == Some(thread) {
                    p.worker = None;
                }
                if p.broken {
                    p.first = None;
                    Some(p.queue)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(queue) = queue {
                if let Some(q) = state.queues.get_mut(&queue) {
                    q.entries.clear();
                }
            }
        }
    }
    pub(super) fn pump(&self) -> Result<()> {
        if let Some(e) = self.state.borrow_mut().errors.pop_front() {
            return Err(e);
        }
        Ok(())
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let state = self.state.borrow();
        let mut roots: Vec<_> = state.tokens.values().copied().collect();
        roots.extend(state.adopted.values().copied());
        roots.extend(state.seconds);
        roots.extend(state.pools.values().filter_map(|p| p.running));
        roots.extend(state.errors.iter().filter_map(|e| match e {
            VmExit::Exception(e) => e.obj,
            _ => None,
        }));
        roots.extend(self.pending.borrow().values().flatten().copied());
        roots
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        let state = self.state.borrow();
        let mut refs = Vec::new();
        if let Some(thread) = state.threads.get(&r) {
            refs.extend(thread.target);
            refs.extend(thread.context_loader);
        }
        if let Some(q) = state.queues.get(&r) {
            refs.extend(q.entries.iter().copied());
        }
        if let Some(p) = state.pools.get(&r) {
            refs.extend([p.queue, p.factory]);
            refs.extend(p.worker);
            refs.extend(p.first);
            refs.extend(p.running);
        }
        refs.extend(state.wrappers.get(&r));
        refs.extend(state.workers.get(&r));
        refs
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        let mut state = self.state.borrow_mut();
        state.threads.retain(|r, _| live(r));
        state.queues.retain(|r, _| live(r));
        state.pools.retain(|r, _| live(r));
        state.factories.retain(|r, _| live(r));
        state.wrappers.retain(|r, _| live(r));
        state.workers.retain(|r, _| live(r));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Model two real workers suspended inside independent native factories:
    // the older scope resumes and unwinds while the newer scope stays live.
    #[test]
    fn suspended_factory_tickets_keep_other_workers_roots_alive_during_gc() {
        struct Host(Rc<Executors>);
        impl rd_vm::native_host::NativeHost for Host {
            fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<()> {
                Err(unavailable("test has no native libraries"))
            }
            fn invoke(
                &self,
                _: &mut Engine,
                _: &str,
                _: &str,
                _: &str,
                _: bool,
                _: &[Value],
            ) -> Result<Value> {
                Err(unavailable("test does not execute native methods"))
            }
            fn roots(&self) -> Vec<ObjRef> {
                self.0.roots()
            }
        }
        let runtime = Rc::new(Executors::default());
        let mut vm = Engine::new(vec![], rd_vm::VmConfig::default());
        vm.bind_native(Rc::new(Host(runtime.clone())));
        let a = vm
            .heap
            .alloc_instance("Ljava/lang/Object;".into(), vec![])
            .unwrap();
        let b = vm
            .heap
            .alloc_instance("Ljava/lang/Object;".into(), vec![])
            .unwrap();
        let returned_a = vm.heap.alloc_instance(THREAD.into(), vec![]).unwrap();
        let scope_a = runtime.protect(vec![a]).unwrap();
        let scope_b = runtime.protect(vec![b]).unwrap();
        scope_a.push(returned_a).unwrap();
        drop(scope_a);
        vm.gc();
        assert_eq!(vm.heap.class_of(a).unwrap(), rd_vm::gc::FREED_CLASS);
        assert_eq!(
            vm.heap.class_of(returned_a).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
        assert_ne!(vm.heap.class_of(b).unwrap(), rd_vm::gc::FREED_CLASS);
        let returned_b = vm.heap.alloc_instance(THREAD.into(), vec![]).unwrap();
        scope_b.push(returned_b).unwrap();
        vm.gc();
        assert_ne!(
            vm.heap.class_of(returned_b).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
        drop(scope_b);
        vm.gc();
        assert_eq!(vm.heap.class_of(b).unwrap(), rd_vm::gc::FREED_CLASS);
        assert_eq!(
            vm.heap.class_of(returned_b).unwrap(),
            rd_vm::gc::FREED_CLASS
        );
    }
}
