//! Owned Java monitors under the exclusive Engine/platform VM lease.
//! There is no scheduler handoff here: contended acquisition and wait-set
//! operations fail explicitly, leaving the owner and depth unchanged.
use crate::{err, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{collections::BTreeMap, format};
use core::sync::atomic::{fence, Ordering};

const LIMIT: usize = 1024;
const DEPTH_LIMIT: u32 = 65535;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Interpreter,
    Native(u64),
}
struct Monitor {
    owner: Owner,
    depth: u32,
}
#[derive(Default)]
pub(crate) struct Monitors {
    held: BTreeMap<ObjRef, Monitor>,
}
impl Monitors {
    pub(crate) fn roots(&self) -> impl Iterator<Item = ObjRef> + '_ {
        self.held.keys().copied()
    }
}
fn owner(vm: &Engine) -> Result<Owner, VmExit> {
    match vm.native_host() {
        None => Ok(Owner::Interpreter),
        Some(backend) => backend
            .current_thread_id()
            .filter(|id| *id != 0)
            .map(Owner::Native)
            .ok_or_else(|| {
                err::vm_error(
                    "API_UNAVAILABLE",
                    "Java monitor requires the actual NativeHost current thread identity",
                )
                .into()
            }),
    }
}
fn validate(vm: &Engine, object: ObjRef) -> Result<(), VmExit> {
    if object == 0 {
        return Err(
            Throwable::new("Ljava/lang/NullPointerException;", "Java monitor is null").into(),
        );
    }
    if vm.heap.class_of(object)? == crate::gc::FREED_CLASS {
        return Err(
            err::vm_error("VM_TYPE_ERROR", "Java monitor refers to a collected object").into(),
        );
    }
    Ok(())
}
impl Engine {
    /// Enter a Java monitor from the actual executing VM/native thread. All
    /// callers must hold the same exclusive VM lease used for interpreter/JNI.
    pub fn monitor_enter(&mut self, object: ObjRef) -> Result<(), VmExit> {
        validate(self, object)?;
        let owner = owner(self)?;
        let monitors = &mut self.portable.monitors.held;
        if let Some(monitor) = monitors.get_mut(&object) {
            if monitor.owner != owner {
                return Err(err::vm_error("API_UNAVAILABLE", "contended Java monitor requires a scheduler/VM-lease handoff; ownership was preserved").into());
            }
            if monitor.depth >= DEPTH_LIMIT {
                return Err(err::vm_error(
                    "MONITOR_BUDGET",
                    "65535 recursive acquisitions per Java monitor",
                )
                .into());
            }
            monitor.depth += 1;
        } else {
            if monitors.len() >= LIMIT {
                return Err(err::vm_error(
                    "MONITOR_BUDGET",
                    "1024 simultaneously held Java monitors",
                )
                .into());
            }
            monitors.insert(object, Monitor { owner, depth: 1 });
        }
        fence(Ordering::SeqCst);
        Ok(())
    }
    /// Exit exactly one acquisition; an unowned exit never mutates state.
    pub fn monitor_exit(&mut self, object: ObjRef) -> Result<(), VmExit> {
        validate(self, object)?;
        let owner = owner(self)?;
        let monitor = self
            .portable
            .monitors
            .held
            .get_mut(&object)
            .filter(|monitor| monitor.owner == owner)
            .ok_or_else(|| {
                Throwable::new(
                    "Ljava/lang/IllegalMonitorStateException;",
                    "current thread does not own Java monitor",
                )
            })?;
        fence(Ordering::SeqCst);
        monitor.depth -= 1;
        if monitor.depth == 0 {
            self.portable.monitors.held.remove(&object);
        }
        Ok(())
    }
    pub fn monitor_holds_lock(&self, object: ObjRef) -> Result<bool, VmExit> {
        validate(self, object)?;
        let owner = owner(self)?;
        Ok(self
            .portable
            .monitors
            .held
            .get(&object)
            .is_some_and(|monitor| monitor.owner == owner))
    }
}
pub(crate) fn call_static(
    vm: &Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if (class, name, sig) != ("Ljava/lang/Thread;", "holdsLock", "(Ljava/lang/Object;)Z") {
        return Ok(None);
    }
    let object = args
        .first()
        .ok_or_else(|| err::vm_error("VM_TYPE_ERROR", "Thread.holdsLock argument is missing"))?
        .as_ref()?
        .unwrap_or(0);
    Ok(Some(Value::Int(i32::from(vm.monitor_holds_lock(object)?))))
}
pub(crate) fn call_instance(
    vm: &Engine,
    class: &str,
    name: &str,
    sig: &str,
    object: ObjRef,
) -> Result<Option<Value>, VmExit> {
    if class != "Ljava/lang/Object;"
        || !matches!(
            (name, sig),
            ("wait", "()V" | "(J)V" | "(JI)V") | ("notify" | "notifyAll", "()V")
        )
    {
        return Ok(None);
    }
    if !vm.monitor_holds_lock(object)? {
        return Err(Throwable::new(
            "Ljava/lang/IllegalMonitorStateException;",
            "current thread does not own Java monitor",
        )
        .into());
    }
    Err(err::vm_error("API_UNAVAILABLE", format!("Object.{name}{sig}: monitor wait-set/scheduler protocol is unavailable; ownership was preserved")).into())
}
