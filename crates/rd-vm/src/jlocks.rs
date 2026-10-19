//! `java.util.concurrent.locks`: `ReentrantLock`, `ReentrantReadWriteLock`
//! (+ its `ReadLock`/`WriteLock`), `Lock`/`Condition` interface calls on them,
//! and `AbstractOwnableSynchronizer`.
//!
//! The VM schedules green threads cooperatively, and a thread only yields at
//! blocking points (park/sleep/join), so a lock is a hold counter
//! (`rd_holds`, plus `rd_readers` for read locks):
//! - `lock`/`tryLock` always acquire;
//! - `unlock` at zero holds throws `IllegalMonitorStateException`, as AOSP
//!   does;
//! - `Condition.await*` returns at once — a spurious wakeup, which the JDK
//!   contract permits; callers re-check their predicate;
//! - `signal`/`signalAll` are no-ops.
//!
//! **Deviation:** holding a lock across a park does not block other green
//! threads that try to acquire it.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const RL: &str = "Ljava/util/concurrent/locks/ReentrantLock;";
const RWL: &str = "Ljava/util/concurrent/locks/ReentrantReadWriteLock;";
const RLOCK: &str = "Ljava/util/concurrent/locks/ReentrantReadWriteLock$ReadLock;";
const WLOCK: &str = "Ljava/util/concurrent/locks/ReentrantReadWriteLock$WriteLock;";
const LOCK: &str = "Ljava/util/concurrent/locks/Lock;";
const COND: &str = "Ljava/util/concurrent/locks/Condition;";
const AOS: &str = "Ljava/util/concurrent/locks/AbstractOwnableSynchronizer;";

fn get_i(vm: &Engine, r: ObjRef, f: &str) -> i32 {
    match vm.heap.get_field(r, f) {
        Ok(Value::Int(i)) => i,
        _ => 0,
    }
}
fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
fn sub_lock(vm: &mut Engine, owner: ObjRef, class: &str, field: &str) -> Result<Value, VmExit> {
    if let Ok(v @ Value::Obj(_)) = vm.heap.get_field(owner, field) {
        return Ok(v);
    }
    let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    put(vm, r, "rd_owner", Value::Obj(owner))?;
    put(vm, owner, field, Value::Obj(r))?;
    Ok(Value::Obj(r))
}

pub(crate) fn instance_intrinsic(vm: &mut Engine, recv_class: &str, mref_class: &str, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let class = match mref_class {
        RL | RWL | RLOCK | WLOCK | AOS => mref_class,
        LOCK | COND => recv_class,
        _ => return Ok(None),
    };
    if mref_class == AOS {
        return Ok(Some(match name {
            "setExclusiveOwnerThread" => {
                put(vm, recv, "rd_owner_thread", args.first().cloned().unwrap_or(Value::Null))?;
                Value::Null
            }
            "getExclusiveOwnerThread" => vm.heap.get_field(recv, "rd_owner_thread").unwrap_or(Value::Null),
            _ => Value::Null, // <init>
        }));
    }
    if class == COND || recv_class == "Lrd/Condition;" {
        return Ok(Some(match sig.rsplit(')').next().unwrap_or("V") {
            "Z" => Value::Int(1), // await(time, unit) / awaitUntil: "signalled" (spurious wakeup)
            "J" => Value::Long(0),
            _ => Value::Null,
        }));
    }
    let field = if class == RLOCK { "rd_readers" } else { "rd_holds" };
    // read/write locks keep their counters on the owning ReentrantReadWriteLock
    let target = if class == RLOCK || class == WLOCK {
        match vm.heap.get_field(recv, "rd_owner") {
            Ok(Value::Obj(o)) => o,
            _ => recv,
        }
    } else {
        recv
    };
    Ok(Some(match name {
        "<init>" => Value::Null,
        "readLock" if class == RWL => sub_lock(vm, recv, RLOCK, "rd_read_lock")?,
        "writeLock" if class == RWL => sub_lock(vm, recv, WLOCK, "rd_write_lock")?,
        "lock" | "lockInterruptibly" | "tryLock" => {
            let n = get_i(vm, target, field);
            put(vm, target, field, Value::Int(n + 1))?;
            if sig.ends_with(")Z") { Value::Int(1) } else { Value::Null }
        }
        "unlock" => {
            let n = get_i(vm, target, field);
            if n <= 0 {
                return Err(VmExit::Exception(Throwable::new("Ljava/lang/IllegalMonitorStateException;", String::new())));
            }
            put(vm, target, field, Value::Int(n - 1))?;
            Value::Null
        }
        "isLocked" | "isWriteLocked" => Value::Int((get_i(vm, target, "rd_holds") > 0) as i32),
        "isHeldByCurrentThread" | "isWriteLockedByCurrentThread" => Value::Int((get_i(vm, target, "rd_holds") > 0) as i32),
        "getHoldCount" | "getWriteHoldCount" => Value::Int(get_i(vm, target, "rd_holds")),
        "getReadLockCount" | "getReadHoldCount" => Value::Int(get_i(vm, target, "rd_readers")),
        "hasQueuedThreads" | "isFair" => Value::Int(0),
        "getQueueLength" => Value::Int(0),
        "newCondition" => Value::Obj(vm.heap.alloc_instance("Lrd/Condition;".into(), Vec::new())?),
        "toString" => Value::Obj(crate::intrinsics::alloc_string(vm, format!("{class}[holds={}]", get_i(vm, target, field)))?),
        _ => return Ok(None),
    }))
}
