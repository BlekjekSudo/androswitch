//! java.util.Timer uses a dedicated platform worker, never the main Looper.
//! Tokens outlive weak Java owners; queued/running tasks are real GC roots.
use super::*;
use as_platform::BackgroundWorkers;
use core::cell::Cell;
const TIMER: &str = "Ljava/util/Timer;";
const TASK: &str = "Ljava/util/TimerTask;";
const MAX_TIMERS: usize = 64;
const MAX_TASKS: usize = 4096;
const MAX_QUEUE: usize = 4096;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Virgin,
    Scheduled,
    Executed,
    Cancelled,
}
struct TaskState {
    status: Status,
    timer: Option<u64>,
    last: i64,
}
#[derive(Clone, Copy)]
enum Repeat {
    Once,
    Delay(i64),
    Rate(i64),
}
#[derive(Clone, Copy)]
struct Entry {
    task: ObjRef,
    due: i64,
    order: u64,
    repeat: Repeat,
}
struct TimerState {
    owner: Option<ObjRef>,
    name: Option<String>,
    daemon: bool,
    backend: Rc<dyn BackgroundWorkers>,
    accepting: bool,
    stopped: bool,
    queue: Vec<Entry>,
    running: Option<ObjRef>,
}
#[derive(Default)]
struct State {
    timers: BTreeMap<u64, TimerState>,
    objects: BTreeMap<ObjRef, u64>,
    tasks: BTreeMap<ObjRef, TaskState>,
    errors: VecDeque<(u64, VmExit)>,
    failed: alloc::collections::BTreeSet<u64>,
}
#[derive(Default)]
pub(in crate::android) struct Timers {
    state: RefCell<State>,
    backend: RefCell<Option<Rc<dyn BackgroundWorkers>>>,
    serial: Cell<u64>,
    order: Cell<u64>,
    callback_owners: RefCell<BTreeMap<Owner, u32>>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Owner {
    Interpreter,
    Native(u64),
}
fn owner(vm: &Engine) -> Result<Owner> {
    match vm.native_host() {
        None => Ok(Owner::Interpreter),
        Some(backend) => backend
            .current_thread_id()
            .filter(|id| *id != 0)
            .map(Owner::Native)
            .ok_or_else(|| {
                error(
                    "API_UNAVAILABLE",
                    "Timer callback requires actual NativeHost thread identity",
                )
            }),
    }
}
struct Callback<'a> {
    owners: &'a RefCell<BTreeMap<Owner, u32>>,
    owner: Owner,
}
impl Drop for Callback<'_> {
    fn drop(&mut self) {
        let mut owners = self.owners.borrow_mut();
        if let Some(depth) = owners.get_mut(&self.owner) {
            *depth -= 1;
            if *depth == 0 {
                owners.remove(&self.owner);
            }
        }
    }
}
fn illegal(message: &str) -> VmExit {
    Throwable::new("Ljava/lang/IllegalStateException;", message).into()
}
fn invalid(message: &str) -> VmExit {
    Throwable::new("Ljava/lang/IllegalArgumentException;", message).into()
}
fn copy_error(value: &VmExit) -> VmExit {
    match value {
        VmExit::Exception(e) => e.clone().into(),
        VmExit::Error(e) => e.clone().into(),
    }
}
fn platform(value: as_platform::PlatformError) -> VmExit {
    error(value.code, value.to_string())
}
impl Timers {
    pub(in crate::android) fn in_callback(&self, vm: &Engine) -> Result<bool> {
        if self.callback_owners.borrow().is_empty() {
            return Ok(false);
        }
        Ok(self.callback_owners.borrow().contains_key(&owner(vm)?))
    }
    pub(in crate::android) fn bind(&self, backend: Rc<dyn BackgroundWorkers>) {
        *self.backend.borrow_mut() = Some(backend);
    }
    pub(super) fn attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if vm.cp.is_subtype(class, TASK) {
            let mut state = self.state.borrow_mut();
            if !state.tasks.contains_key(&r) {
                if state.tasks.len() >= MAX_TASKS {
                    return Err(error("TIMER_BUDGET", "4096 TimerTask objects"));
                }
                state.tasks.insert(
                    r,
                    TaskState {
                        status: Status::Virgin,
                        timer: None,
                        last: 0,
                    },
                );
            }
            return Ok(true);
        }
        Ok(vm.cp.is_subtype(class, TIMER))
    }
    fn token(&self, r: ObjRef) -> Result<u64> {
        self.state
            .borrow()
            .objects
            .get(&r)
            .copied()
            .ok_or_else(|| illegal("Timer is not initialized"))
    }
    pub(in crate::android) fn metadata(&self, token: u64) -> Option<(Option<String>, bool)> {
        self.state
            .borrow()
            .timers
            .get(&token)
            .map(|t| (t.name.clone(), t.daemon))
    }
    fn create(&self, r: ObjRef, name: Option<String>, daemon: bool) -> Result<()> {
        let backend = self.backend.borrow().clone().ok_or_else(|| {
            error(
                "API_UNAVAILABLE",
                "Timer dedicated background worker backend",
            )
        })?;
        {
            let state = self.state.borrow();
            if state.objects.contains_key(&r) {
                return Err(illegal("Timer is already initialized"));
            }
            if state.timers.len() + state.errors.len() >= MAX_TIMERS {
                return Err(error("TIMER_BUDGET", "64 Timer workers/pending errors"));
            }
        }
        let token = self
            .serial
            .get()
            .checked_add(1)
            .filter(|token| *token < super::executors::TOKEN_BIT)
            .ok_or_else(|| error("TIMER_TOKEN_LIMIT", "worker token overflow"))?;
        self.serial.set(token);
        // Platform must not synchronously call VM; its worker waits for VmGate.
        backend.start(token).map_err(platform)?;
        let mut state = self.state.borrow_mut();
        state.objects.insert(r, token);
        state.timers.insert(
            token,
            TimerState {
                owner: Some(r),
                name,
                daemon,
                backend,
                accepting: true,
                stopped: false,
                queue: Vec::new(),
                running: None,
            },
        );
        Ok(())
    }
    fn schedule(&self, r: ObjRef, task: ObjRef, due: i64, repeat: Repeat) -> Result<()> {
        let token = self.token(r)?;
        let backend = {
            let state = self.state.borrow();
            let timer = state
                .timers
                .get(&token)
                .ok_or_else(|| illegal("Timer worker stopped"))?;
            if !timer.accepting || timer.stopped {
                return Err(illegal("Timer was cancelled or terminated"));
            }
            let target = state
                .tasks
                .get(&task)
                .ok_or_else(|| error("INVALID_TIMER_TASK", "receiver is not a TimerTask"))?;
            if target.status != Status::Virgin {
                return Err(illegal("Task was already scheduled or cancelled"));
            }
            if state
                .timers
                .values()
                .map(|timer| timer.queue.len())
                .sum::<usize>()
                >= MAX_QUEUE
            {
                return Err(error("TIMER_BUDGET", "4096 queued Timer tasks"));
            }
            timer.backend.clone()
        };
        let order = self
            .order
            .get()
            .checked_add(1)
            .ok_or_else(|| error("TIMER_QUEUE_LIMIT", "queue order overflow"))?;
        // A failing wake leaves task state/queue unmodified. Workers cannot
        // enter VM until this call returns and releases VmGate.
        backend.wake(token).map_err(platform)?;
        self.order.set(order);
        let mut state = self.state.borrow_mut();
        let target = state
            .tasks
            .get_mut(&task)
            .ok_or_else(|| error("INVALID_TIMER_TASK", "task disappeared"))?;
        target.status = Status::Scheduled;
        target.timer = Some(token);
        target.last = due;
        state
            .timers
            .get_mut(&token)
            .ok_or_else(|| illegal("Timer worker stopped"))?
            .queue
            .push(Entry {
                task,
                due,
                order,
                repeat,
            });
        Ok(())
    }
    fn cancel_timer(&self, r: ObjRef) -> Result<()> {
        let token = self.token(r)?;
        let backend = {
            let state = self.state.borrow();
            let timer = state
                .timers
                .get(&token)
                .ok_or_else(|| illegal("Timer worker stopped"))?;
            if !timer.accepting {
                return Ok(());
            }
            timer.backend.clone()
        };
        backend.cancel(token).map_err(platform)?;
        if let Some(timer) = self.state.borrow_mut().timers.get_mut(&token) {
            timer.accepting = false;
            timer.queue.clear();
        }
        Ok(())
    }
    fn cancel_task(&self, r: ObjRef) -> Result<bool> {
        let (cancelled, wake) = {
            let state = self.state.borrow();
            let task = state
                .tasks
                .get(&r)
                .ok_or_else(|| error("INVALID_TIMER_TASK", "missing task state"))?;
            let cancelled = task.status == Status::Scheduled;
            let wake = task.timer.and_then(|token| {
                state
                    .timers
                    .get(&token)
                    .filter(|timer| timer.accepting && !timer.stopped)
                    .map(|timer| (token, timer.backend.clone()))
            });
            (cancelled, wake)
        };
        if let Some((token, backend)) = wake {
            backend.wake(token).map_err(platform)?;
        }
        if let Some(task) = self.state.borrow_mut().tasks.get_mut(&r) {
            task.status = Status::Cancelled;
        }
        Ok(cancelled)
    }
    fn purge(&self, r: ObjRef) -> Result<i32> {
        let token = self.token(r)?;
        let mut state = self.state.borrow_mut();
        let dead: alloc::collections::BTreeSet<_> = state
            .tasks
            .iter()
            .filter_map(|(r, task)| (task.status == Status::Cancelled).then_some(*r))
            .collect();
        let timer = state
            .timers
            .get_mut(&token)
            .ok_or_else(|| illegal("Timer worker stopped"))?;
        let before = timer.queue.len();
        timer.queue.retain(|entry| !dead.contains(&entry.task));
        Ok((before - timer.queue.len()) as i32)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        android: &Android,
        vm: &Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let class = vm.heap.class_of(r)?;
        if vm.cp.is_subtype(class, TASK) {
            return Ok(Some(match (name, sig) {
                ("<init>", "()V") => Value::Null,
                ("cancel", "()Z") => Value::Int(i32::from(self.cancel_task(r)?)),
                ("scheduledExecutionTime", "()J") => Value::Long(
                    self.state
                        .borrow()
                        .tasks
                        .get(&r)
                        .ok_or_else(|| error("INVALID_TIMER_TASK", "missing task"))?
                        .last,
                ),
                _ => return Ok(None),
            }));
        }
        if !vm.cp.is_subtype(class, TIMER) {
            return Ok(None);
        }
        Ok(Some(match (name, sig) {
            ("<init>", "()V" | "(Z)V" | "(Ljava/lang/String;)V" | "(Ljava/lang/String;Z)V") => {
                let name = if sig.starts_with("(Ljava/lang/String;") {
                    let name = text(vm, &args[0])?;
                    if name.len() > 4096 {
                        return Err(error("TIMER_BUDGET", "4096 byte worker name"));
                    }
                    Some(name)
                } else {
                    None
                };
                let daemon = match sig {
                    "(Z)V" => args[0].as_int()? != 0,
                    "(Ljava/lang/String;Z)V" => args[1].as_int()? != 0,
                    _ => false,
                };
                self.create(r, name, daemon)?;
                Value::Null
            }
            ("cancel", "()V") => {
                self.cancel_timer(r)?;
                Value::Null
            }
            ("purge", "()I") => Value::Int(self.purge(r)?),
            (
                "schedule" | "scheduleAtFixedRate",
                "(Ljava/util/TimerTask;J)V" | "(Ljava/util/TimerTask;JJ)V",
            ) => {
                if name == "scheduleAtFixedRate" && sig != "(Ljava/util/TimerTask;JJ)V" {
                    return Ok(None);
                }
                let task = obj(&args[0])?;
                let delay = args[1].as_long()?;
                if delay < 0 {
                    return Err(invalid("Negative Timer delay"));
                }
                let repeat = if args.len() == 3 {
                    let period = args[2].as_long()?;
                    if period <= 0 {
                        return Err(invalid("Non-positive Timer period"));
                    }
                    if name == "scheduleAtFixedRate" {
                        Repeat::Rate(period)
                    } else {
                        Repeat::Delay(period)
                    }
                } else {
                    Repeat::Once
                };
                let now = android.clock.wall_time_millis().map_err(platform)?;
                let due = now
                    .checked_add(delay)
                    .filter(|due| *due >= 0)
                    .ok_or_else(|| invalid("Timer execution time overflow"))?;
                self.schedule(r, task, due, repeat)?;
                Value::Null
            }
            (
                "schedule" | "scheduleAtFixedRate",
                "(Ljava/util/TimerTask;Ljava/util/Date;)V"
                | "(Ljava/util/TimerTask;Ljava/util/Date;J)V",
            ) => {
                return Err(error(
                    "API_UNAVAILABLE",
                    "Timer Date overload requires java.util.Date.getTime contract",
                ));
            }
            _ => return Ok(None),
        }))
    }
    pub(in crate::android) fn step(
        &self,
        android: &Android,
        vm: &mut Engine,
        token: u64,
    ) -> Result<Option<u64>> {
        let outcome = self.step_inner(android, vm, token);
        if let Err(failure) = &outcome {
            self.worker_error(token, copy_error(failure));
        }
        outcome
    }
    fn step_inner(&self, android: &Android, vm: &mut Engine, token: u64) -> Result<Option<u64>> {
        {
            let mut state = self.state.borrow_mut();
            let Some(timer) = state.timers.get_mut(&token) else {
                return Ok(None);
            };
            if !timer.accepting || timer.stopped {
                return Ok(None);
            }
            if timer.queue.is_empty() {
                if timer.owner.is_none() {
                    timer.accepting = false;
                    return Ok(None);
                }
                return Ok(Some(1000));
            }
        }
        let now = android.clock.wall_time_millis().map_err(platform)?;
        let run = {
            let mut state = self.state.borrow_mut();
            let Some(timer) = state.timers.get_mut(&token) else {
                return Ok(None);
            };
            if !timer.accepting || timer.stopped {
                return Ok(None);
            }
            let Some(index) = timer
                .queue
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| (entry.due, entry.order))
                .map(|(index, _)| index)
            else {
                if timer.owner.is_none() {
                    timer.accepting = false;
                    return Ok(None);
                }
                return Ok(Some(1000));
            };
            let entry = timer.queue[index];
            if entry.due > now {
                return Ok(Some((entry.due as i128 - now as i128).min(1000) as u64));
            }
            timer.queue.remove(index);
            let cancelled = state
                .tasks
                .get(&entry.task)
                .is_none_or(|task| task.status == Status::Cancelled);
            if cancelled {
                return Ok(Some(0));
            }
            let next = match entry.repeat {
                Repeat::Once => None,
                Repeat::Delay(period) => Some(now.checked_add(period)),
                Repeat::Rate(period) => Some(entry.due.checked_add(period)),
            };
            if next == Some(None) {
                return Err(error(
                    "TIMER_TIME_OVERFLOW",
                    "repeated Timer execution time",
                ));
            }
            if let Some(task) = state.tasks.get_mut(&entry.task) {
                task.last = if matches!(entry.repeat, Repeat::Delay(_)) {
                    now
                } else {
                    entry.due
                };
                if matches!(entry.repeat, Repeat::Once) {
                    task.status = Status::Executed;
                }
            }
            let timer = state
                .timers
                .get_mut(&token)
                .ok_or_else(|| illegal("Timer disappeared"))?;
            if let Some(Some(due)) = next {
                timer.queue.push(Entry { due, ..entry });
            }
            timer.running = Some(entry.task);
            entry.task
        };
        // GC may run or the task may cancel/reschedule from this invocation.
        // All Timer RefCell borrows are dropped; running/queued roots stay live.
        vm.reset_fuel_window();
        let callback_owner = owner(vm)?;
        *self
            .callback_owners
            .borrow_mut()
            .entry(callback_owner)
            .or_default() += 1;
        let _callback = Callback {
            owners: &self.callback_owners,
            owner: callback_owner,
        };
        let result = vm.invoke_instance(run, "run", "()V", &[]);
        if let Some(timer) = self.state.borrow_mut().timers.get_mut(&token) {
            timer.running = None;
        }
        result?;
        let mut state = self.state.borrow_mut();
        let Some(timer) = state.timers.get_mut(&token) else {
            return Ok(None);
        };
        if !timer.accepting {
            return Ok(None);
        }
        if timer.owner.is_none() && timer.queue.is_empty() {
            timer.accepting = false;
            return Ok(None);
        }
        Ok(Some(0))
    }
    pub(in crate::android) fn worker_error(&self, token: u64, failure: VmExit) {
        let mut state = self.state.borrow_mut();
        if let Some(timer) = state.timers.get_mut(&token) {
            timer.accepting = false;
            timer.queue.clear();
            // Called after the callback unwinds, before platform TLS detachment.
            timer.running = None;
        }
        if state.failed.insert(token) {
            state.errors.push_back((token, failure));
        }
    }
    pub(in crate::android) fn stopped(&self, token: u64) {
        let mut state = self.state.borrow_mut();
        state.failed.remove(&token);
        if let Some(timer) = state.timers.get_mut(&token) {
            timer.stopped = true;
            timer.accepting = false;
            timer.queue.clear();
            timer.running = None;
        }
        if state
            .timers
            .get(&token)
            .is_some_and(|timer| timer.owner.is_none())
        {
            state.timers.remove(&token);
        }
    }
    pub(super) fn pump(&self) -> Result<()> {
        if let Some((_, failure)) = self.state.borrow_mut().errors.pop_front() {
            return Err(failure);
        }
        Ok(())
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let state = self.state.borrow();
        let mut roots = Vec::new();
        for timer in state.timers.values() {
            roots.extend(timer.queue.iter().map(|entry| entry.task));
            roots.extend(timer.running);
        }
        roots.extend(
            state
                .errors
                .iter()
                .filter_map(|(_, failure)| match failure {
                    VmExit::Exception(e) => e.obj,
                    VmExit::Error(_) => None,
                }),
        );
        roots
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        let wakes = {
            let mut state = self.state.borrow_mut();
            state.tasks.retain(|r, _| live(r));
            state.objects.retain(|r, _| live(r));
            let mut wakes = Vec::new();
            for (token, timer) in &mut state.timers {
                if timer.owner.is_some_and(|r| !live(&r)) {
                    timer.owner = None;
                    if timer.accepting && timer.queue.is_empty() && timer.running.is_none() {
                        timer.accepting = false;
                        wakes.push((*token, timer.backend.clone()));
                    }
                }
            }
            state
                .timers
                .retain(|_, timer| timer.owner.is_some() || !timer.stopped);
            wakes
        };
        for (token, backend) in wakes {
            if let Err(failure) = backend.wake(token) {
                self.worker_error(token, platform(failure));
            }
        }
    }
}
