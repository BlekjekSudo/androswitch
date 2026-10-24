//! API26 Application observers dispatch from the actual Activity super bodies.
//! Registrations belong to their Application; in-flight snapshots own GC roots.
use super::*;
use core::cell::Cell;

const APPLICATION: &str = "Landroid/app/Application;";
const ACTIVITY: &str = "Landroid/app/Activity;";
const OBSERVER: &str = "Landroid/app/Application$ActivityLifecycleCallbacks;";
const REGISTER: &str = "(Landroid/app/Application$ActivityLifecycleCallbacks;)V";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Created,
    Started,
    Resumed,
    Paused,
    Stopped,
    Save,
    Destroyed,
    Restart,
}
impl Event {
    pub(crate) fn from_method(name: &str, sig: &str) -> Option<Self> {
        Some(match (name, sig) {
            ("onCreate", "(Landroid/os/Bundle;)V") => Self::Created,
            ("onSaveInstanceState", "(Landroid/os/Bundle;)V") => Self::Save,
            ("onStart", "()V") => Self::Started,
            ("onResume", "()V") => Self::Resumed,
            ("onPause", "()V") => Self::Paused,
            ("onStop", "()V") => Self::Stopped,
            ("onDestroy", "()V") => Self::Destroyed,
            ("onRestart", "()V") => Self::Restart,
            _ => return None,
        })
    }
    fn method(self) -> (&'static str, &'static str) {
        match self {
            Self::Created => ("onCreate", "(Landroid/os/Bundle;)V"),
            Self::Save => ("onSaveInstanceState", "(Landroid/os/Bundle;)V"),
            Self::Started => ("onStart", "()V"),
            Self::Resumed => ("onResume", "()V"),
            Self::Paused => ("onPause", "()V"),
            Self::Stopped => ("onStop", "()V"),
            Self::Destroyed => ("onDestroy", "()V"),
            Self::Restart => ("onRestart", "()V"),
        }
    }
    fn observer(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::Created => (
                "onActivityCreated",
                "(Landroid/app/Activity;Landroid/os/Bundle;)V",
            ),
            Self::Save => (
                "onActivitySaveInstanceState",
                "(Landroid/app/Activity;Landroid/os/Bundle;)V",
            ),
            Self::Started => ("onActivityStarted", "(Landroid/app/Activity;)V"),
            Self::Resumed => ("onActivityResumed", "(Landroid/app/Activity;)V"),
            Self::Paused => ("onActivityPaused", "(Landroid/app/Activity;)V"),
            Self::Stopped => ("onActivityStopped", "(Landroid/app/Activity;)V"),
            Self::Destroyed => ("onActivityDestroyed", "(Landroid/app/Activity;)V"),
            Self::Restart => return None,
        })
    }
}
#[derive(Default)]
struct Registrations {
    version: u64,
    callbacks: Vec<Option<ObjRef>>,
}
struct Performing {
    owner: u64,
    activity: ObjRef,
    event: Event,
    called: bool,
    roots: Vec<ObjRef>,
}
#[derive(Default)]
pub(crate) struct Observers {
    registrations: RefCell<BTreeMap<ObjRef, Registrations>>,
    snapshots: RefCell<BTreeMap<u64, Vec<ObjRef>>>,
    performing: RefCell<BTreeMap<u64, Performing>>,
    serial: Cell<u64>,
}
struct Snapshot<'a> {
    owner: &'a Observers,
    ticket: u64,
}
impl Drop for Snapshot<'_> {
    fn drop(&mut self) {
        self.owner.snapshots.borrow_mut().remove(&self.ticket);
    }
}
struct Perform<'a> {
    owner: &'a Observers,
    ticket: u64,
}
impl Drop for Perform<'_> {
    fn drop(&mut self) {
        self.owner.performing.borrow_mut().remove(&self.ticket);
    }
}
fn owner(vm: &Engine) -> Result<u64> {
    match vm.native_host() {
        None => Ok(0),
        Some(host) => host
            .current_thread_id()
            .filter(|id| *id != 0)
            .ok_or_else(|| error("API_UNAVAILABLE", "lifecycle execution thread identity")),
    }
}
impl Observers {
    fn next(&self) -> Result<u64> {
        let next = self
            .serial
            .get()
            .checked_add(1)
            .ok_or_else(|| error("LIFECYCLE_BUDGET", "scope serial exhausted"))?;
        self.serial.set(next);
        Ok(next)
    }
    fn protect(&self, roots: Vec<ObjRef>) -> Result<Snapshot<'_>> {
        if self.snapshots.borrow().len() >= 64 {
            return Err(error("LIFECYCLE_BUDGET", "64 active observer snapshots"));
        }
        let ticket = self.next()?;
        self.snapshots.borrow_mut().insert(ticket, roots);
        Ok(Snapshot {
            owner: self,
            ticket,
        })
    }
    pub(super) fn metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<rd_vm::framework_host::HostMethodKind> {
        (class == APPLICATION
            && matches!(
                name,
                "registerActivityLifecycleCallbacks" | "unregisterActivityLifecycleCallbacks"
            )
            && sig == REGISTER)
            .then_some(rd_vm::framework_host::HostMethodKind::Instance)
    }
    pub(super) fn call(
        &self,
        vm: &mut Engine,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if sig != REGISTER
            || !matches!(
                name,
                "registerActivityLifecycleCallbacks" | "unregisterActivityLifecycleCallbacks"
            )
            || !vm.cp.is_subtype(vm.heap.class_of(r)?, APPLICATION)
        {
            return Ok(None);
        }
        let callback = args[0].as_ref()?;
        if let Some(callback) = callback {
            if !vm.cp.is_subtype(vm.heap.class_of(callback)?, OBSERVER) {
                return Err(Throwable::new(
                    "Ljava/lang/ClassCastException;",
                    "expected ActivityLifecycleCallbacks",
                )
                .into());
            }
        }
        if name == "registerActivityLifecycleCallbacks" {
            if self
                .registrations
                .borrow()
                .values()
                .map(|r| r.callbacks.len())
                .sum::<usize>()
                >= 4096
            {
                return Err(error("LIFECYCLE_BUDGET", "4096 observer registrations"));
            }
            let version = self.next()?;
            let mut map = self.registrations.borrow_mut();
            let list = map.entry(r).or_default();
            list.version = version;
            list.callbacks.push(callback);
        } else {
            let Some((version, list)) = self
                .registrations
                .borrow()
                .get(&r)
                .map(|r| (r.version, r.callbacks.clone()))
            else {
                return Ok(Some(Value::Null));
            };
            let mut roots = vec![r];
            roots.extend(callback);
            roots.extend(list.iter().flatten().copied());
            let _snapshot = self.protect(roots)?;
            for (index, candidate) in list.into_iter().enumerate() {
                let same = match callback {
                    None => candidate.is_none(),
                    Some(callback) => {
                        vm.invoke_instance(
                            callback,
                            "equals",
                            "(Ljava/lang/Object;)Z",
                            &[candidate.map_or(Value::Null, Value::Obj)],
                        )?
                        .as_int()?
                            != 0
                    }
                };
                if self.registrations.borrow().get(&r).map(|r| r.version) != Some(version) {
                    return Err(error(
                        "API_UNAVAILABLE",
                        "lifecycle registration mutation during callback.equals",
                    ));
                }
                if same {
                    let version = self.next()?;
                    let mut map = self.registrations.borrow_mut();
                    let list = map.get_mut(&r).ok_or_else(|| {
                        error(
                            "LIFECYCLE_STATE",
                            "registration disappeared after version validation",
                        )
                    })?;
                    list.callbacks.remove(index);
                    list.version = version;
                    break;
                }
            }
        }
        Ok(Some(Value::Null))
    }
    pub(crate) fn perform(
        &self,
        vm: &mut Engine,
        activity: ObjRef,
        event: Event,
        args: &[Value],
        target_sdk: u32,
    ) -> Result<()> {
        if self.performing.borrow().len() >= 64 {
            return Err(error(
                "LIFECYCLE_BUDGET",
                "64 active Activity lifecycle calls",
            ));
        }
        let ticket = self.next()?;
        let thread = owner(vm)?;
        let roots = args
            .iter()
            .filter_map(|v| v.as_ref().ok().flatten())
            .collect();
        self.performing.borrow_mut().insert(
            ticket,
            Performing {
                owner: thread,
                activity,
                event,
                called: false,
                roots,
            },
        );
        let _scope = Perform {
            owner: self,
            ticket,
        };
        let (name, sig) = event.method();
        vm.invoke_instance(activity, name, sig, args)?;
        // API 26 does not enforce a super call for saved application state,
        // and retains the pre-Gingerbread onPause compatibility behavior.
        let required = event != Event::Save && !(event == Event::Paused && target_sdk < 9);
        if required
            && !self
                .performing
                .borrow()
                .get(&ticket)
                .is_some_and(|p| p.called)
        {
            return Err(Throwable::new(
                "Landroid/app/SuperNotCalledException;",
                format!("Activity did not call super.{name}"),
            )
            .into());
        }
        Ok(())
    }
    fn mark_called(&self, thread: u64, activity: ObjRef, event: Event) {
        if let Some((_, frame)) = self
            .performing
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|(_, p)| p.owner == thread && p.activity == activity && p.event == event)
        {
            frame.called = true;
        }
    }
    pub(crate) fn base(
        &self,
        vm: &mut Engine,
        application: ObjRef,
        activity: ObjRef,
        event: Event,
        args: &[Value],
    ) -> Result<()> {
        if !vm.cp.is_subtype(vm.heap.class_of(activity)?, ACTIVITY) {
            return Err(error("VM_TYPE_ERROR", "lifecycle receiver is not Activity"));
        }
        let thread = owner(vm)?;
        // Activity's API 26 bodies set mCalled at different points. A callback
        // exception caught by the Activity must retain that distinction.
        let mark_before = matches!(event, Event::Started | Event::Destroyed | Event::Restart);
        if mark_before {
            self.mark_called(thread, activity, event);
        }
        let Some((name, sig)) = event.observer() else {
            return Ok(());
        };
        let mut values = vec![Value::Obj(activity)];
        if matches!(event, Event::Created | Event::Save) {
            let bundle = args[0].as_ref()?;
            if event == Event::Save && bundle.is_none() {
                return Err(Throwable::new("Ljava/lang/NullPointerException;", "outState").into());
            }
            if let Some(bundle) = bundle {
                if !vm
                    .cp
                    .is_subtype(vm.heap.class_of(bundle)?, "Landroid/os/Bundle;")
                {
                    return Err(Throwable::new(
                        "Ljava/lang/ClassCastException;",
                        "expected Bundle",
                    )
                    .into());
                }
            }
            values.push(bundle.map_or(Value::Null, Value::Obj));
        }
        let callbacks = self
            .registrations
            .borrow()
            .get(&application)
            .map(|r| r.callbacks.clone())
            .unwrap_or_default();
        let mut roots = vec![application, activity];
        roots.extend(callbacks.iter().flatten().copied());
        roots.extend(values.iter().filter_map(|v| v.as_ref().ok().flatten()));
        let _snapshot = self.protect(roots)?;
        for callback in callbacks {
            let callback = callback.ok_or_else(|| {
                Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "null ActivityLifecycleCallbacks",
                )
            })?;
            let concrete = vm
                .cp
                .resolve_method(vm.heap.class_of(callback)?, name, sig)
                .is_some_and(|(_, _, method)| {
                    method.access_flags & 0x0001 != 0
                        && method.access_flags & (0x0400 | 0x0008 | 0x0002) == 0
                });
            if !concrete {
                return Err(Throwable::new(
                    "Ljava/lang/AbstractMethodError;",
                    format!("{OBSERVER}->{name}{sig}"),
                )
                .into());
            }
            vm.invoke_instance(callback, name, sig, &values)?;
        }
        if !mark_before {
            self.mark_called(thread, activity, event);
        }
        Ok(())
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        let mut roots = self
            .snapshots
            .borrow()
            .values()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        for frame in self.performing.borrow().values() {
            roots.push(frame.activity);
            roots.extend(&frame.roots);
        }
        roots
    }
    pub(super) fn references(&self, r: ObjRef) -> Vec<ObjRef> {
        self.registrations
            .borrow()
            .get(&r)
            .map(|r| r.callbacks.iter().flatten().copied().collect())
            .unwrap_or_default()
    }
    pub(super) fn collect(&self, marked: &[bool]) {
        self.registrations
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
}
