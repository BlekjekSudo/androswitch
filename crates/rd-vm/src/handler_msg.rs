//! `android.os.Handler` messaging and `android.os.Message`.
//!
//! The base runtime models `Handler.post`/`postDelayed` of Runnables. Real
//! apps and libraries (Compose's `AndroidUiDispatcher`, WorkManager, Room's
//! invalidation tracker, Material) also use:
//! - **Messages:** `obtainMessage`, `sendMessage[Delayed|AtTime|AtFrontOfQueue]`,
//!   `sendEmptyMessage*` and `Message.sendToTarget`, dispatched as in AOSP
//!   `Handler.dispatchMessage`:
//!   1. `msg.callback.run()` if the message carries a Runnable;
//!   2. else the Handler's `Callback.handleMessage(msg)`; if that returns
//!      true, stop;
//!   3. else `handler.handleMessage(msg)`, the subclass override.
//! - **Tokens and removal:** `postDelayed(r, token, ms)`, `postAtTime`,
//!   `removeCallbacks(r[, token])`, `removeMessages(what[, obj])`,
//!   `removeCallbacksAndMessages(token|null)`, `hasMessages`,
//!   `hasCallbacks`. All removals are scoped to the receiving Handler.
//! - **Construction:** `Handler(Callback)`, `Handler(Looper, Callback[, async])`,
//!   `Handler.createAsync(Looper[, Callback])`, `getLooper`.
//!
//! Message objects are plain heap instances carrying the public fields
//! (`what`, `arg1`, `arg2`, `obj`) so DEX `iget`/`iput` work, plus `target`,
//! `callback` and `data`. `uptimeMillis`-based times use the virtual clock.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::{HostObj, QueuedMsg};
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const HANDLER: &str = "Landroid/os/Handler;";
pub(crate) const MESSAGE: &str = "Landroid/os/Message;";

fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))?;
    Ok(())
}

fn field(vm: &Engine, r: ObjRef, f: &str) -> Value {
    vm.heap.get_field(r, f).unwrap_or(Value::Null)
}

fn obj(v: Option<&Value>) -> Option<ObjRef> {
    v.and_then(|v| v.as_ref().ok().flatten())
}

fn init_message_fields(vm: &mut Engine, m: ObjRef) -> Result<(), VmExit> {
    for (f, v) in [
        ("what", Value::Int(0)),
        ("arg1", Value::Int(0)),
        ("arg2", Value::Int(0)),
        ("obj", Value::Null),
        ("replyTo", Value::Null),
        ("sendingUid", Value::Int(-1)),
        ("target", Value::Null),
        ("callback", Value::Null),
        ("data", Value::Null),
    ] {
        put(vm, m, f, v)?;
    }
    Ok(())
}

/// `Message.obtain(...)` / `Handler.obtainMessage(...)`.
fn new_message(
    vm: &mut Engine,
    target: Option<ObjRef>,
    what: i32,
    arg1: i32,
    arg2: i32,
    o: Value,
    callback: Option<ObjRef>,
) -> Result<ObjRef, VmExit> {
    let m = vm.heap.alloc_instance(MESSAGE.to_string(), Vec::new())?;
    init_message_fields(vm, m)?;
    put(vm, m, "what", Value::Int(what))?;
    put(vm, m, "arg1", Value::Int(arg1))?;
    put(vm, m, "arg2", Value::Int(arg2))?;
    put(vm, m, "obj", o)?;
    put(vm, m, "target", target.map_or(Value::Null, Value::Obj))?;
    put(vm, m, "callback", callback.map_or(Value::Null, Value::Obj))?;
    Ok(m)
}

fn looper_of(vm: &mut Engine, handler: ObjRef) -> Result<ObjRef, VmExit> {
    match vm.fw.get(handler) {
        Some(HostObj::Handler { looper }) => Ok(*looper),
        _ => vm.main_looper_obj(),
    }
}

fn queue_mut(vm: &mut Engine, looper: ObjRef) -> Option<&mut Vec<QueuedMsg>> {
    match vm.fw.objects.get_mut(&looper) {
        Some(HostObj::Looper { queue }) => Some(queue),
        _ => None,
    }
}

fn push(vm: &mut Engine, handler: ObjRef, entry: QueuedMsg) -> Result<(), VmExit> {
    let l = looper_of(vm, handler)?;
    if let Some(q) = queue_mut(vm, l) {
        q.push(entry);
    }
    Ok(())
}

fn when_after(vm: &Engine, delay: i64) -> u64 {
    vm.fw
        .clock_ms
        .checked_add(delay.max(0) as u64)
        .unwrap_or(u64::MAX / 2)
}

fn enqueue_message(vm: &mut Engine, handler: ObjRef, m: ObjRef, when: u64) -> Result<(), VmExit> {
    put(vm, m, "target", Value::Obj(handler))?;
    push(
        vm,
        handler,
        QueuedMsg {
            when_ms: when,
            runnable: m,
            handler: Some(handler),
            is_message: true,
            token: None,
        },
    )
}

fn enqueue_runnable(
    vm: &mut Engine,
    handler: ObjRef,
    r: ObjRef,
    token: Option<ObjRef>,
    when: u64,
) -> Result<(), VmExit> {
    push(
        vm,
        handler,
        QueuedMsg {
            when_ms: when,
            runnable: r,
            handler: Some(handler),
            is_message: false,
            token,
        },
    )
}

/// Remove this handler's queue entries matching `pred`. Returns how many.
fn remove_where(
    vm: &mut Engine,
    handler: ObjRef,
    pred: impl Fn(&Engine, &QueuedMsg) -> bool,
) -> Result<usize, VmExit> {
    let l = looper_of(vm, handler)?;
    let entries = queue_mut(vm, l).map(std::mem::take).unwrap_or_default();
    let (gone, keep): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .partition(|e| e.handler == Some(handler) && pred(vm, e));
    if let Some(q) = queue_mut(vm, l) {
        *q = keep;
    }
    Ok(gone.len())
}

fn msg_what(vm: &Engine, m: ObjRef) -> i32 {
    field(vm, m, "what").as_int().unwrap_or(0)
}

/// AOSP `Handler.dispatchMessage`.
pub(crate) fn dispatch_message(
    vm: &mut Engine,
    handler: Option<ObjRef>,
    m: ObjRef,
) -> Result<(), VmExit> {
    if let Value::Obj(cb) = field(vm, m, "callback") {
        let class = vm.heap.class_of(cb)?.to_string();
        if let Some((d, def, me)) = vm.cp.resolve_method(&class, "run", "()V") {
            vm.call(d, def, &me, vec![Value::Obj(cb)])?;
        }
        return Ok(());
    }
    let Some(h) = handler.or_else(|| obj(Some(&field(vm, m, "target")))) else {
        return Ok(());
    };
    if let Value::Obj(cb) = field(vm, h, "rd_handler_callback") {
        let class = vm.heap.class_of(cb)?.to_string();
        if let Some((d, def, me)) =
            vm.cp
                .resolve_method(&class, "handleMessage", "(Landroid/os/Message;)Z")
        {
            let handled = vm.call(d, def, &me, vec![Value::Obj(cb), Value::Obj(m)])?;
            if matches!(handled, Value::Int(x) if x != 0) {
                return Ok(());
            }
        }
    }
    let class = vm.heap.class_of(h)?.to_string();
    if let Some((d, def, me)) =
        vm.cp
            .resolve_method(&class, "handleMessage", "(Landroid/os/Message;)V")
    {
        vm.call(d, def, &me, vec![Value::Obj(h), Value::Obj(m)])?;
    }
    Ok(())
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == HANDLER && name == "createAsync" {
        let looper = match obj(args.first()) {
            Some(l) => l,
            None => vm.main_looper_obj()?,
        };
        let h = vm.heap.alloc_instance(HANDLER.to_string(), Vec::new())?;
        vm.fw.objects.insert(h, HostObj::Handler { looper });
        if sig == "(Landroid/os/Looper;Landroid/os/Handler$Callback;)Landroid/os/Handler;" {
            put(
                vm,
                h,
                "rd_handler_callback",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
        }
        return Ok(Some(Value::Obj(h)));
    }
    if class != MESSAGE || name != "obtain" {
        return Ok(None);
    }
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let m = match sig {
        "()Landroid/os/Message;" => new_message(vm, None, 0, 0, 0, Value::Null, None)?,
        "(Landroid/os/Handler;)Landroid/os/Message;" => {
            new_message(vm, obj(args.first()), 0, 0, 0, Value::Null, None)?
        }
        "(Landroid/os/Handler;I)Landroid/os/Message;" => {
            new_message(vm, obj(args.first()), int(1), 0, 0, Value::Null, None)?
        }
        "(Landroid/os/Handler;ILjava/lang/Object;)Landroid/os/Message;" => {
            new_message(vm, obj(args.first()), int(1), 0, 0, args[2].clone(), None)?
        }
        "(Landroid/os/Handler;III)Landroid/os/Message;" => new_message(
            vm,
            obj(args.first()),
            int(1),
            int(2),
            int(3),
            Value::Null,
            None,
        )?,
        "(Landroid/os/Handler;IIILjava/lang/Object;)Landroid/os/Message;" => new_message(
            vm,
            obj(args.first()),
            int(1),
            int(2),
            int(3),
            args[4].clone(),
            None,
        )?,
        "(Landroid/os/Handler;Ljava/lang/Runnable;)Landroid/os/Message;" => new_message(
            vm,
            obj(args.first()),
            0,
            0,
            0,
            Value::Null,
            obj(args.get(1)),
        )?,
        "(Landroid/os/Message;)Landroid/os/Message;" => {
            let Some(src) = obj(args.first()) else {
                return Ok(None);
            };
            let m = vm.heap.alloc_instance(MESSAGE.to_string(), Vec::new())?;
            init_message_fields(vm, m)?;
            for f in [
                "what", "arg1", "arg2", "obj", "target", "callback", "data", "replyTo",
            ] {
                let v = field(vm, src, f);
                put(vm, m, f, v)?;
            }
            m
        }
        _ => return Ok(None),
    };
    Ok(Some(Value::Obj(m)))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == MESSAGE {
        return message(vm, name, sig, recv, args);
    }
    if mref_class != HANDLER {
        return Ok(None);
    }
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let long = |i: usize| match args.get(i) {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(x)) => *x as i64,
        _ => 0,
    };
    let t = Value::Int(1);
    Ok(Some(match (name, sig) {
        ("<init>", "(Landroid/os/Handler$Callback;)V") => {
            let l = vm.main_looper_obj()?;
            vm.fw.objects.insert(recv, HostObj::Handler { looper: l });
            put(vm, recv, "rd_handler_callback", args[0].clone())?;
            Value::Null
        }
        ("<init>", "(Landroid/os/Looper;Landroid/os/Handler$Callback;)V")
        | ("<init>", "(Landroid/os/Looper;Landroid/os/Handler$Callback;Z)V") => {
            let l = match obj(args.first()) {
                Some(l) => l,
                None => vm.main_looper_obj()?,
            };
            vm.fw.objects.insert(recv, HostObj::Handler { looper: l });
            put(
                vm,
                recv,
                "rd_handler_callback",
                args.get(1).cloned().unwrap_or(Value::Null),
            )?;
            Value::Null
        }
        ("getLooper", "()Landroid/os/Looper;") => Value::Obj(looper_of(vm, recv)?),
        ("postDelayed", "(Ljava/lang/Runnable;Ljava/lang/Object;J)Z") => {
            let Some(r) = obj(args.first()) else {
                return Ok(None);
            };
            let w = when_after(vm, long(2));
            enqueue_runnable(vm, recv, r, obj(args.get(1)), w)?;
            t
        }
        ("postAtTime", "(Ljava/lang/Runnable;J)Z") => {
            let Some(r) = obj(args.first()) else {
                return Ok(None);
            };
            enqueue_runnable(vm, recv, r, None, long(1).max(0) as u64)?;
            t
        }
        ("postAtTime", "(Ljava/lang/Runnable;Ljava/lang/Object;J)Z") => {
            let Some(r) = obj(args.first()) else {
                return Ok(None);
            };
            enqueue_runnable(vm, recv, r, obj(args.get(1)), long(2).max(0) as u64)?;
            t
        }
        ("postAtFrontOfQueue", "(Ljava/lang/Runnable;)Z") => {
            let Some(r) = obj(args.first()) else {
                return Ok(None);
            };
            enqueue_runnable(vm, recv, r, None, 0)?;
            t
        }
        ("sendMessage", "(Landroid/os/Message;)Z") => {
            let Some(m) = obj(args.first()) else {
                return Ok(None);
            };
            let w = vm.fw.clock_ms;
            enqueue_message(vm, recv, m, w)?;
            t
        }
        ("sendMessageDelayed", "(Landroid/os/Message;J)Z") => {
            let Some(m) = obj(args.first()) else {
                return Ok(None);
            };
            let w = when_after(vm, long(1));
            enqueue_message(vm, recv, m, w)?;
            t
        }
        ("sendMessageAtTime", "(Landroid/os/Message;J)Z") => {
            let Some(m) = obj(args.first()) else {
                return Ok(None);
            };
            enqueue_message(vm, recv, m, long(1).max(0) as u64)?;
            t
        }
        ("sendMessageAtFrontOfQueue", "(Landroid/os/Message;)Z") => {
            let Some(m) = obj(args.first()) else {
                return Ok(None);
            };
            enqueue_message(vm, recv, m, 0)?;
            t
        }
        ("sendEmptyMessage", "(I)Z")
        | ("sendEmptyMessageDelayed", "(IJ)Z")
        | ("sendEmptyMessageAtTime", "(IJ)Z") => {
            let m = new_message(vm, Some(recv), int(0), 0, 0, Value::Null, None)?;
            let w = match name {
                "sendEmptyMessage" => vm.fw.clock_ms,
                "sendEmptyMessageDelayed" => when_after(vm, long(1)),
                _ => long(1).max(0) as u64,
            };
            enqueue_message(vm, recv, m, w)?;
            t
        }
        ("obtainMessage", "()Landroid/os/Message;") => {
            Value::Obj(new_message(vm, Some(recv), 0, 0, 0, Value::Null, None)?)
        }
        ("obtainMessage", "(I)Landroid/os/Message;") => Value::Obj(new_message(
            vm,
            Some(recv),
            int(0),
            0,
            0,
            Value::Null,
            None,
        )?),
        ("obtainMessage", "(ILjava/lang/Object;)Landroid/os/Message;") => Value::Obj(new_message(
            vm,
            Some(recv),
            int(0),
            0,
            0,
            args[1].clone(),
            None,
        )?),
        ("obtainMessage", "(III)Landroid/os/Message;") => Value::Obj(new_message(
            vm,
            Some(recv),
            int(0),
            int(1),
            int(2),
            Value::Null,
            None,
        )?),
        ("obtainMessage", "(IIILjava/lang/Object;)Landroid/os/Message;") => {
            Value::Obj(new_message(
                vm,
                Some(recv),
                int(0),
                int(1),
                int(2),
                args[3].clone(),
                None,
            )?)
        }
        ("removeCallbacks", "(Ljava/lang/Runnable;)V") => {
            let r = obj(args.first());
            remove_where(vm, recv, |vm, e| {
                !e.is_message && Some(e.runnable) == r
                    || e.is_message
                        && matches!(vm.heap.get_field(e.runnable, "callback"), Ok(Value::Obj(c)) if Some(c) == r)
            })?;
            Value::Null
        }
        ("removeCallbacks", "(Ljava/lang/Runnable;Ljava/lang/Object;)V") => {
            let (r, tok) = (obj(args.first()), obj(args.get(1)));
            remove_where(vm, recv, |_, e| {
                !e.is_message && Some(e.runnable) == r && (tok.is_none() || e.token == tok)
            })?;
            Value::Null
        }
        ("removeMessages", "(I)V") => {
            let w = int(0);
            remove_where(vm, recv, |vm, e| {
                e.is_message && msg_what(vm, e.runnable) == w
            })?;
            Value::Null
        }
        ("removeMessages", "(ILjava/lang/Object;)V") => {
            let (w, o) = (int(0), obj(args.get(1)));
            remove_where(vm, recv, |vm, e| {
                e.is_message
                    && msg_what(vm, e.runnable) == w
                    && (o.is_none()
                        || matches!(vm.heap.get_field(e.runnable, "obj"), Ok(Value::Obj(x)) if Some(x) == o))
            })?;
            Value::Null
        }
        ("removeCallbacksAndMessages", "(Ljava/lang/Object;)V") => {
            let tok = obj(args.first());
            remove_where(vm, recv, |vm, e| {
                tok.is_none()
                    || e.token == tok
                    || (e.is_message
                        && matches!(vm.heap.get_field(e.runnable, "obj"), Ok(Value::Obj(x)) if Some(x) == tok))
            })?;
            Value::Null
        }
        ("hasMessages", "(I)Z") => {
            let w = int(0);
            let l = looper_of(vm, recv)?;
            let has = match vm.fw.get(l) {
                Some(HostObj::Looper { queue }) => queue.iter().any(|e| {
                    e.handler == Some(recv) && e.is_message && msg_what(vm, e.runnable) == w
                }),
                _ => false,
            };
            Value::Int(has as i32)
        }
        ("hasCallbacks", "(Ljava/lang/Runnable;)Z") => {
            let r = obj(args.first());
            let l = looper_of(vm, recv)?;
            let has = match vm.fw.get(l) {
                Some(HostObj::Looper { queue }) => queue
                    .iter()
                    .any(|e| e.handler == Some(recv) && !e.is_message && Some(e.runnable) == r),
                _ => false,
            };
            Value::Int(has as i32)
        }
        ("dispatchMessage", "(Landroid/os/Message;)V") => {
            if let Some(m) = obj(args.first()) {
                dispatch_message(vm, Some(recv), m)?;
            }
            Value::Null
        }
        // base Handler.handleMessage: empty (subclasses override)
        ("handleMessage", "(Landroid/os/Message;)V") => Value::Null,
        _ => return Ok(None),
    }))
}

fn message(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match (name, sig) {
        ("<init>", "()V") => {
            init_message_fields(vm, recv)?;
            Value::Null
        }
        ("sendToTarget", "()V") => {
            let Value::Obj(h) = field(vm, recv, "target") else {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Message.sendToTarget() without a target Handler",
                )));
            };
            let w = vm.fw.clock_ms;
            enqueue_message(vm, h, recv, w)?;
            Value::Null
        }
        ("getTarget", "()Landroid/os/Handler;") => field(vm, recv, "target"),
        ("setTarget", "(Landroid/os/Handler;)V") => {
            put(vm, recv, "target", args[0].clone())?;
            Value::Null
        }
        ("getCallback", "()Ljava/lang/Runnable;") => field(vm, recv, "callback"),
        ("getWhen", "()J") => Value::Long(vm.fw.clock_ms as i64),
        ("getData", "()Landroid/os/Bundle;") => match field(vm, recv, "data") {
            Value::Obj(b) => Value::Obj(b),
            _ => {
                let b = vm
                    .heap
                    .alloc_instance("Landroid/os/Bundle;".to_string(), Vec::new())?;
                vm.fw.objects.insert(b, HostObj::Bundle { map: Vec::new() });
                put(vm, recv, "data", Value::Obj(b))?;
                Value::Obj(b)
            }
        },
        ("peekData", "()Landroid/os/Bundle;") => field(vm, recv, "data"),
        ("setData", "(Landroid/os/Bundle;)V") => {
            put(vm, recv, "data", args[0].clone())?;
            Value::Null
        }
        ("setAsynchronous", "(Z)V") | ("recycle", "()V") => Value::Null,
        ("isAsynchronous", "()Z") => Value::Int(0),
        _ => return Ok(None),
    }))
}
