//! `java.util.concurrent.atomic.AtomicBoolean` / `AtomicLong`.
//!
//! The state lives in the instance field `value`, the same layout as the
//! existing AtomicInteger/AtomicReference intrinsics. The VM is
//! single-threaded with cooperative green threads (threads.rs): no
//! preemption happens inside an intrinsic, so CAS is exact.
//!
//! Dispatch keys on the *declaring* class of the method ref, never on the
//! receiver's runtime class. Libraries subclass atomics (Compose's snapshot
//! `AtomicInt extends AtomicInteger`, coroutines' internal nodes), and a
//! subclass instance must still reach these methods.

use crate::engine::Engine;
use crate::err::VmExit;
use crate::heap::ObjRef;
use crate::value::Value;

pub(crate) const ATOMIC_BOOL: &str = "Ljava/util/concurrent/atomic/AtomicBoolean;";
pub(crate) const ATOMIC_LONG: &str = "Ljava/util/concurrent/atomic/AtomicLong;";

fn put(vm: &mut Engine, r: ObjRef, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, "value", v))?;
    Ok(())
}

fn cur_i64(vm: &Engine, r: ObjRef) -> i64 {
    match vm.heap.get_field(r, "value") {
        Ok(Value::Long(v)) => v,
        Ok(Value::Int(v)) => v as i64,
        _ => 0,
    }
}

fn arg_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(i)) => *i as i64,
        _ => 0,
    }
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == ATOMIC_BOOL {
        let cur = cur_i64(vm, recv) != 0;
        let a = |i: usize| arg_i64(args.get(i)) != 0;
        let b = |x: bool| Value::Int(x as i32);
        return Ok(Some(match (name, sig) {
            ("<init>", "()V") => {
                put(vm, recv, Value::Int(0))?;
                Value::Null
            }
            ("<init>", "(Z)V")
            | ("set", "(Z)V")
            | ("lazySet", "(Z)V")
            | ("setPlain", "(Z)V")
            | ("setRelease", "(Z)V")
            | ("setOpaque", "(Z)V") => {
                put(vm, recv, b(a(0)))?;
                Value::Null
            }
            ("get" | "getPlain" | "getAcquire" | "getOpaque", "()Z") => b(cur),
            ("getAndSet", "(Z)Z") => {
                put(vm, recv, b(a(0)))?;
                b(cur)
            }
            (
                "compareAndSet"
                | "weakCompareAndSet"
                | "weakCompareAndSetPlain"
                | "weakCompareAndSetVolatile",
                "(ZZ)Z",
            ) => {
                let ok = cur == a(0);
                if ok {
                    put(vm, recv, b(a(1)))?;
                }
                b(ok)
            }
            ("toString", "()Ljava/lang/String;") => {
                let s = crate::intrinsics::alloc_string(
                    vm,
                    if cur { "true" } else { "false" }.to_string(),
                )?;
                Value::Obj(s)
            }
            _ => return Ok(None),
        }));
    }
    if mref_class == ATOMIC_LONG {
        let cur = cur_i64(vm, recv);
        let a0 = arg_i64(args.first());
        let a1 = arg_i64(args.get(1));
        let l = Value::Long;
        return Ok(Some(match (name, sig) {
            ("<init>", "()V") => {
                put(vm, recv, l(0))?;
                Value::Null
            }
            ("<init>", "(J)V") | ("set" | "lazySet" | "setPlain" | "setRelease", "(J)V") => {
                put(vm, recv, l(a0))?;
                Value::Null
            }
            ("get" | "longValue" | "getPlain" | "getAcquire", "()J") => l(cur),
            ("intValue", "()I") => Value::Int(cur as i32),
            ("doubleValue", "()D") => Value::Double(cur as f64),
            ("floatValue", "()F") => Value::Float(cur as f32),
            ("getAndSet", "(J)J") => {
                put(vm, recv, l(a0))?;
                l(cur)
            }
            ("compareAndSet" | "weakCompareAndSet", "(JJ)Z") => {
                let ok = cur == a0;
                if ok {
                    put(vm, recv, l(a1))?;
                }
                Value::Int(ok as i32)
            }
            ("incrementAndGet", "()J") => {
                put(vm, recv, l(cur.wrapping_add(1)))?;
                l(cur.wrapping_add(1))
            }
            ("decrementAndGet", "()J") => {
                put(vm, recv, l(cur.wrapping_sub(1)))?;
                l(cur.wrapping_sub(1))
            }
            ("getAndIncrement", "()J") => {
                put(vm, recv, l(cur.wrapping_add(1)))?;
                l(cur)
            }
            ("getAndDecrement", "()J") => {
                put(vm, recv, l(cur.wrapping_sub(1)))?;
                l(cur)
            }
            ("addAndGet", "(J)J") => {
                put(vm, recv, l(cur.wrapping_add(a0)))?;
                l(cur.wrapping_add(a0))
            }
            ("getAndAdd", "(J)J") => {
                put(vm, recv, l(cur.wrapping_add(a0)))?;
                l(cur)
            }
            ("toString", "()Ljava/lang/String;") => {
                let s = crate::intrinsics::alloc_string(vm, cur.to_string())?;
                Value::Obj(s)
            }
            _ => return Ok(None),
        }));
    }
    Ok(None)
}
