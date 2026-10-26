//! Cooperative green threads: `java.lang.Thread` and
//! `java.util.concurrent.locks.LockSupport`.
//!
//! RustyDroid stays single-threaded on the host so it remains
//! deterministic. Guest threads are scheduled cooperatively.
//!
//! - **Construction.** `new Thread(...)`, or `super(...)` from a DEX subclass
//!   (for example kotlinx.coroutines' `CoroutineScheduler.Worker`), records
//!   the target `Runnable`, the name and the daemon flag.
//! - **`start()`.** The thread is queued. The starter keeps running first, as
//!   on a real device.
//! - **Scheduling points.** Ready threads run one at a time, to completion
//!   or suspension. This happens when the main thread idles (after
//!   `Engine::drain_looper`) or blocks (`join`, `LockSupport.park`). Work
//!   they post to the main Looper runs on the next drain. Both queues are
//!   alternated until quiescent; the instruction fuel bounds that loop.
//! - **`LockSupport.park*` inside a green thread.** If no permit is
//!   available, the thread suspends: it unwinds with a VM control signal
//!   (`RD_THREAD_PARK`), not a Java exception, so `catch`/`finally` never
//!   intercept it. `unpark(t)`, or a `parkNanos` deadline reached on the
//!   virtual clock, makes it ready again, and its `run()` is re-entered from
//!   the top. Worker loops (kotlinx's scheduler, executors) keep their state
//!   in fields and tolerate this. This deviation is documented in
//!   compat_gaps.
//! - **`park` on the main thread.** Ready threads get a turn, then park
//!   returns. A spurious wakeup is allowed by the `LockSupport` contract.
//! - **Uncaught exceptions in a green thread** crash the app, matching
//!   Android's default uncaught-exception handler, which kills the process.

use crate::maps::HashMap;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use alloc::collections::VecDeque;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const THREAD: &str = "Ljava/lang/Thread;";
const LOCK_SUPPORT: &str = "Ljava/util/concurrent/locks/LockSupport;";
/// VM control signal (never a Java exception) used to unwind a parking
/// green thread back to the scheduler.
pub(crate) const PARK_SIGNAL: &str = "RD_THREAD_PARK";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    New,
    Ready,
    Running,
    Parked,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct GreenThread {
    pub id: i64,
    pub name: String,
    pub target: Option<ObjRef>,
    pub daemon: bool,
    pub state: ThreadState,
    pub permit: bool,
    pub interrupted: bool,
    /// virtual-clock deadline (ms) of a timed park
    pub wake_at_ms: Option<u64>,
    pub runs: u32,
}

#[derive(Debug, Default)]
pub struct Threads {
    pub table: HashMap<ObjRef, GreenThread>,
    pub ready: VecDeque<ObjRef>,
    pub current: Option<ObjRef>,
    next_id: i64,
    /// main thread's LockSupport permit
    main_permit: bool,
    main_interrupted: bool,
    /// Thread.setDefaultUncaughtExceptionHandler
    pub(crate) default_ueh: Option<ObjRef>,
    pub(crate) kill_handler: Option<ObjRef>,
    /// thread starts / resumptions, for runtime_health
    pub scheduled: u64,
}

impl Threads {
    fn entry(&mut self, t: ObjRef) -> &mut GreenThread {
        if let crate::maps::Entry::Vacant(entry) = self.table.entry(t) {
            self.next_id += 1;
            let id = self.next_id + 1; // 1 is "main"
            entry.insert(GreenThread {
                id,
                name: format!("Thread-{}", id - 2),
                target: None,
                daemon: false,
                state: ThreadState::New,
                permit: false,
                interrupted: false,
                wake_at_ms: None,
                runs: 0,
            });
        }
        self.table.get_mut(&t).expect("inserted")
    }

    /// Summary for runtime_health: (threads, ready, parked).
    pub fn summary(&self) -> (usize, usize, usize) {
        let parked = self
            .table
            .values()
            .filter(|t| t.state == ThreadState::Parked)
            .count();
        (self.table.len(), self.ready.len(), parked)
    }
}

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}

fn str_arg(vm: &Engine, v: Option<&Value>) -> Option<String> {
    let r = v?.as_ref().ok()??;
    vm.heap.as_str(r).ok().map(str::to_string)
}

/// `true` when `class` is `java.lang.Thread` or a DEX subclass of it.
pub(crate) fn is_thread_class(vm: &Engine, class: &str) -> bool {
    class == THREAD || vm.cp.is_subtype(class, THREAD)
}

fn is_main(vm: &Engine, t: ObjRef) -> bool {
    vm.fw.main_thread == Some(t)
}

/// `Thread.currentThread()`: the running green thread, else main.
pub(crate) fn current_thread(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    match vm.threads.current {
        Some(t) => Ok(t),
        None => vm.main_thread_obj(),
    }
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (THREAD, "currentThread") => Ok(Some(Value::Obj(current_thread(vm)?))),
        (THREAD, "setDefaultUncaughtExceptionHandler") => {
            vm.threads.default_ueh = match args.first() {
                Some(Value::Obj(h)) => Some(*h),
                _ => None,
            };
            Ok(Some(Value::Null))
        }
        (THREAD, "getDefaultUncaughtExceptionHandler") => {
            Ok(Some(vm.threads.default_ueh.map_or(Value::Null, Value::Obj)))
        }
        (THREAD, "interrupted") if sig == "()Z" => {
            let was = match vm.threads.current {
                Some(t) => core::mem::take(&mut vm.threads.entry(t).interrupted),
                None => core::mem::take(&mut vm.threads.main_interrupted),
            };
            Ok(Some(Value::Int(was as i32)))
        }
        (THREAD, "onSpinWait") => Ok(Some(Value::Null)),
        (LOCK_SUPPORT, "unpark") => {
            if let Some(t) = args.first().and_then(|v| v.as_ref().ok().flatten()) {
                unpark(vm, t);
            }
            Ok(Some(Value::Null))
        }
        (LOCK_SUPPORT, "park" | "parkNanos" | "parkUntil") => {
            // the timeout is the last argument for parkNanos/parkUntil
            let nanos = if name == "parkNanos" {
                args.iter().rev().find_map(|v| v.as_long().ok())
            } else {
                None
            };
            park(vm, nanos)?;
            Ok(Some(Value::Null))
        }
        (LOCK_SUPPORT, "getBlocker") => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}

fn unpark(vm: &mut Engine, t: ObjRef) {
    if is_main(vm, t) {
        vm.threads.main_permit = true;
        return;
    }
    let th = vm.threads.entry(t);
    match th.state {
        ThreadState::Parked => {
            th.state = ThreadState::Ready;
            th.wake_at_ms = None;
            vm.threads.ready.push_back(t);
        }
        ThreadState::Terminated => {}
        _ => th.permit = true,
    }
}

fn park(vm: &mut Engine, nanos: Option<i64>) -> Result<(), VmExit> {
    match vm.threads.current {
        Some(t) => {
            let clock = vm.fw.clock();
            let th = vm.threads.entry(t);
            if core::mem::take(&mut th.permit) || th.interrupted {
                return Ok(());
            }
            if let Some(n) = nanos {
                if n <= 0 {
                    return Ok(());
                }
                th.wake_at_ms = Some(clock.saturating_add((n as u64).div_ceil(1_000_000)));
            }
            Err(crate::err::vm_error(PARK_SIGNAL, "green thread parked").into())
        }
        None => {
            // main: consume the permit, or give ready threads a turn and
            // return (a spurious wakeup is permitted by LockSupport).
            if core::mem::take(&mut vm.threads.main_permit) {
                return Ok(());
            }
            if run_ready(vm, 1)? == 0 {
                // nothing runnable: a timed park lets that much virtual time
                // pass, up to the earliest timed sleeper, so threads blocked
                // in parkNanos/sleep (e.g. idle CoroutineScheduler workers)
                // wake. The main Looper is not drained: a thread blocked in
                // park does not process its queue on a device either.
                if let Some(n) = nanos.filter(|n| *n > 0) {
                    let now = vm.fw.clock();
                    let limit = now.saturating_add((n as u64).div_ceil(1_000_000));
                    let earliest = vm
                        .threads
                        .table
                        .values()
                        .filter(|th| th.state == ThreadState::Parked)
                        .filter_map(|th| th.wake_at_ms)
                        .min();
                    if let Some(w) = earliest.filter(|w| *w <= limit) {
                        vm.fw.clock_ms = w.max(now);
                        run_ready(vm, 1)?;
                    }
                }
            }
            Ok(())
        }
    }
}

/// Instance methods on a Thread receiver that the DEX does not override.
/// Called before DEX dispatch, so it must decline DEX-overridden methods
/// such as a subclass `run()`.
pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !(mref_class == THREAD || is_thread_class(vm, recv_class))
        || !is_thread_class(vm, recv_class)
    {
        return Ok(None);
    }
    if name == "<init>" {
        if mref_class != THREAD {
            return Ok(None);
        }
        // Thread(), Thread(Runnable), Thread(String), Thread(Runnable,String),
        // Thread(ThreadGroup,Runnable[,String[,long]]), Thread(ThreadGroup,String)
        let mut target = None;
        let mut tname = None;
        for a in args {
            if let Ok(Some(r)) = a.as_ref() {
                let c = vm.heap.class_of(r)?.to_string();
                if c == "Ljava/lang/String;" {
                    tname = vm.heap.as_str(r).ok().map(str::to_string);
                } else if c != "Ljava/lang/ThreadGroup;" && target.is_none() {
                    target = Some(r);
                }
            }
        }
        let th = vm.threads.entry(recv);
        th.target = target;
        if let Some(n) = tname {
            th.name = n;
        }
        return Ok(Some(Value::Null));
    }
    // a DEX override wins over the host (e.g. Worker.run)
    if recv_class != THREAD {
        if let Some((d, def, _)) = vm.cp.resolve_method(recv_class, name, sig) {
            let declaring = vm.cp.dexes[d].type_str(def.class_idx).to_string();
            if declaring != THREAD {
                return Ok(None);
            }
        }
    }
    // uncaught-exception handlers (main thread included)
    match (name, sig) {
        ("setUncaughtExceptionHandler", _) => {
            let h = args.first().cloned().unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(recv, "rd_ueh", h))?;
            return Ok(Some(Value::Null));
        }
        // AOSP Thread.getUncaughtExceptionHandler: own handler, else the
        // ThreadGroup, which forwards to the default handler and finally to
        // the platform KillApplicationHandler
        ("getUncaughtExceptionHandler", _) => {
            return Ok(Some(match vm.heap.get_field(recv, "rd_ueh") {
                Ok(Value::Obj(h)) => Value::Obj(h),
                _ => Value::Obj(kill_handler(vm)?),
            }))
        }
        _ => {}
    }
    if is_main(vm, recv) {
        return Ok(None); // the synthetic main thread keeps the M8 host
    }
    let v = match (name, sig) {
        ("start", "()V") => {
            let th = vm.threads.entry(recv);
            if th.state != ThreadState::New {
                return Err(exc(
                    "Ljava/lang/IllegalThreadStateException;",
                    "thread already started",
                ));
            }
            th.state = ThreadState::Ready;
            vm.threads.ready.push_back(recv);
            Value::Null
        }
        ("run", "()V") => {
            // Thread.run(): delegate to the target Runnable
            if let Some(t) = vm.threads.entry(recv).target {
                call_run(vm, t)?;
            }
            Value::Null
        }
        ("getName", "()Ljava/lang/String;") => {
            let n = vm.threads.entry(recv).name.clone();
            Value::Obj(crate::intrinsics::alloc_string(vm, n)?)
        }
        ("setName", "(Ljava/lang/String;)V") => {
            if let Some(n) = str_arg(vm, args.first()) {
                vm.threads.entry(recv).name = n;
            }
            Value::Null
        }
        ("getId", "()J") => Value::Long(vm.threads.entry(recv).id),
        ("threadId", _) => Value::Long(vm.threads.entry(recv).id),
        ("isDaemon", "()Z") => Value::Int(vm.threads.entry(recv).daemon as i32),
        ("setDaemon", "(Z)V") => {
            let th = vm.threads.entry(recv);
            if th.state != ThreadState::New {
                return Err(exc(
                    "Ljava/lang/IllegalThreadStateException;",
                    "setDaemon after start",
                ));
            }
            th.daemon = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0) != 0;
            Value::Null
        }
        ("isAlive", "()Z") => Value::Int(matches!(
            vm.threads.entry(recv).state,
            ThreadState::Ready | ThreadState::Running | ThreadState::Parked
        ) as i32),
        ("interrupt", "()V") => {
            vm.threads.entry(recv).interrupted = true;
            unpark(vm, recv);
            Value::Null
        }
        ("isInterrupted", "()Z") => Value::Int(vm.threads.entry(recv).interrupted as i32),
        ("join", _) => {
            // run it now if it can make progress; a parked thread stays parked
            run_thread_until_blocked(vm, recv)?;
            Value::Null
        }
        ("setPriority", "(I)V") | ("setContextClassLoader", _) => Value::Null,
        ("getPriority", "()I") => Value::Int(5),
        ("getContextClassLoader", _) => return Ok(None),
        ("toString", "()Ljava/lang/String;") => {
            let n = vm.threads.entry(recv).name.clone();
            Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!("Thread[{n},5,main]"),
            )?)
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

fn call_run(vm: &mut Engine, r: ObjRef) -> Result<(), VmExit> {
    let class = vm.heap.class_of(r)?.to_string();
    if class == THREAD {
        // plain Thread: run its target
        if let Some(t) = vm.threads.entry(r).target {
            return call_run(vm, t);
        }
        return Ok(());
    }
    let Some((d, def, m)) = vm.cp.resolve_method(&class, "run", "()V") else {
        return Err(crate::classpath::unresolved_method(&class, "run", "()V").into());
    };
    vm.call(d, def, &m, vec![Value::Obj(r)])?;
    Ok(())
}

fn run_thread_until_blocked(vm: &mut Engine, t: ObjRef) -> Result<(), VmExit> {
    if vm.threads.entry(t).state == ThreadState::Ready {
        vm.threads.ready.retain(|x| *x != t);
        run_one(vm, t)?;
    }
    Ok(())
}

/// Run one green thread to completion or suspension.
fn run_one(vm: &mut Engine, t: ObjRef) -> Result<(), VmExit> {
    let prev = vm.threads.current.replace(t);
    {
        let th = vm.threads.entry(t);
        th.state = ThreadState::Running;
        th.runs += 1;
    }
    vm.threads.scheduled += 1;
    // a DEX subclass run() (Worker) or Thread.run() → target
    let r = call_run(vm, t);
    vm.threads.current = prev;
    match r {
        Ok(()) => {
            vm.threads.entry(t).state = ThreadState::Terminated;
            Ok(())
        }
        Err(VmExit::Error(e)) if e.code == PARK_SIGNAL => {
            let th = vm.threads.entry(t);
            if core::mem::take(&mut th.permit) {
                // unparked while unwinding: runnable again
                th.state = ThreadState::Ready;
                vm.threads.ready.push_back(t);
            } else {
                th.state = ThreadState::Parked;
            }
            Ok(())
        }
        Err(e) => {
            vm.threads.entry(t).state = ThreadState::Terminated;
            Err(e)
        }
    }
}

/// Make timed parks whose deadline passed ready again.
fn wake_timed(vm: &mut Engine) {
    let now = vm.fw.clock();
    let mut due: Vec<(i64, ObjRef)> = vm
        .threads
        .table
        .iter()
        .filter(|(_, th)| {
            th.state == ThreadState::Parked && th.wake_at_ms.is_some_and(|w| w <= now)
        })
        .map(|(r, th)| (th.id, *r))
        .collect();
    due.sort(); // deterministic: by thread id
    for (_, r) in due {
        let th = vm.threads.entry(r);
        th.state = ThreadState::Ready;
        th.wake_at_ms = None;
        vm.threads.ready.push_back(r);
    }
}

/// Run up to `max` ready threads (FIFO). Returns how many ran. Never called
/// re-entrantly from inside a green thread.
pub(crate) fn run_ready(vm: &mut Engine, max: usize) -> Result<usize, VmExit> {
    if vm.threads.current.is_some() {
        return Ok(0);
    }
    wake_timed(vm);
    let mut ran = 0;
    while ran < max {
        let Some(t) = vm.threads.ready.pop_front() else {
            break;
        };
        if vm.threads.entry(t).state != ThreadState::Ready {
            continue;
        }
        run_one(vm, t)?;
        ran += 1;
    }
    Ok(ran)
}

pub(crate) const KILL_HANDLER: &str =
    "Lcom/android/internal/os/RuntimeInit$KillApplicationHandler;";

/// The platform's last-resort handler (one instance per VM).
fn kill_handler(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Some(h) = vm.threads.kill_handler {
        return Ok(h);
    }
    let h = vm
        .heap
        .alloc_instance(KILL_HANDLER.to_string(), Vec::new())?;
    vm.threads.kill_handler = Some(h);
    Ok(h)
}

/// `Throwable.toString()` as AOSP logs it: `pkg.Class: message` when the
/// object has no DEX `toString` (host default would be `Class@id`).
fn describe_throwable(vm: &mut Engine, e: ObjRef) -> Result<String, VmExit> {
    let text = crate::intrinsics::object_to_string(vm, e)?;
    let class = vm.heap.class_of(e)?.to_string();
    let java = class
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".");
    if !text.starts_with(&format!("{java}@")) {
        return Ok(text);
    }
    let msg = match crate::throwable_message::stored(vm, e) {
        Ok(Value::Obj(m)) => vm.heap.as_str(m).ok().map(str::to_string),
        _ => None,
    };
    Ok(match msg {
        Some(m) => format!("{java}: {m}"),
        None => java,
    })
}

/// `UncaughtExceptionHandler.uncaughtException(t, e)` on the platform
/// handler. The app's default handler, if any, runs first, as the AOSP
/// ThreadGroup does. Then `E/AndroidRuntime: FATAL EXCEPTION` is logged and
/// the throwable escapes as the app crash.
pub(crate) fn ueh_intrinsic(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name != "uncaughtException"
        || vm
            .heap
            .class_of(recv)
            .map(|c| c != KILL_HANDLER)
            .unwrap_or(true)
    {
        return Ok(None);
    }
    if let Some(h) = vm.threads.default_ueh.filter(|h| *h != recv) {
        let class = vm.heap.class_of(h)?.to_string();
        if let Some((d, def, m)) = vm.cp.resolve_method(
            &class,
            "uncaughtException",
            "(Ljava/lang/Thread;Ljava/lang/Throwable;)V",
        ) {
            vm.call(
                d,
                def,
                &m,
                vec![Value::Obj(h), args[0].clone(), args[1].clone()],
            )?;
            return Ok(Some(Value::Null));
        }
    }
    let Some(Value::Obj(e)) = args.get(1).cloned() else {
        return Ok(Some(Value::Null));
    };
    let class = vm.heap.class_of(e)?.to_string();
    let msg = match crate::throwable_message::stored(vm, e) {
        Ok(Value::Obj(m)) => vm.heap.as_str(m).ok().map(str::to_string),
        _ => None,
    };
    let mut text = describe_throwable(vm, e)?;
    if let Ok(Value::Obj(t)) = vm.heap.get_field(e, "rd_trace") {
        if let Ok(t) = vm.heap.as_str(t) {
            text.push_str(&t.replace("\n  at ", "\nE/AndroidRuntime:   at "));
        }
    }
    // AOSP prints the cause chain ("Caused by: …") after the stack.
    let mut cur = e;
    for _ in 0..8 {
        let Ok(Value::Obj(c)) = vm.heap.get_field(cur, "cause") else {
            break;
        };
        if c == cur {
            break;
        }
        text.push_str("\nE/AndroidRuntime: Caused by: ");
        text.push_str(&describe_throwable(vm, c)?);
        if let Ok(Value::Obj(t)) = vm.heap.get_field(c, "rd_trace") {
            if let Ok(t) = vm.heap.as_str(t) {
                text.push_str(&t.replace("\n  at ", "\nE/AndroidRuntime:   at "));
            }
        }
        cur = c;
    }
    crate::intrinsics::host_print(
        vm,
        1,
        &format!("E/AndroidRuntime: FATAL EXCEPTION: main\nE/AndroidRuntime: {text}\n"),
    );
    Err(VmExit::Exception(crate::err::Throwable {
        class,
        message: msg,
        obj: Some(e),
    }))
}
