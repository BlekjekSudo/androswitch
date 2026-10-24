//! Bounded ThreadLocal maps owned by real Java Thread objects.
//! Keys are weak; values are ordinary strong edges from their owning Thread.
use super::*;

pub(super) const CLASS: &str = "Ljava/lang/ThreadLocal;";
const INHERITABLE: &str = "Ljava/lang/InheritableThreadLocal;";
const MAX_KEYS: usize = 1024;
const MAX_PER_THREAD: usize = 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Default)]
pub(super) struct Locals {
    // Attachment is not a GC root. False means the base constructor has not run.
    keys: BTreeMap<ObjRef, bool>,
    // A present None is a stored Java null, distinct from a missing entry.
    threads: BTreeMap<ObjRef, BTreeMap<ObjRef, Option<ObjRef>>>,
}
impl Locals {
    pub(super) fn forget_thread(&mut self, thread: ObjRef) {
        self.threads.remove(&thread);
    }
    pub(super) fn references(&self, thread: ObjRef) -> Vec<ObjRef> {
        self.threads
            .get(&thread)
            .into_iter()
            .flat_map(|values| values.values().copied().flatten())
            .collect()
    }
    pub(super) fn collect(&mut self, marked: &[bool]) {
        let live = |r: &ObjRef| marked.get(*r as usize).copied().unwrap_or(false);
        self.keys.retain(|r, _| live(r));
        self.threads.retain(|thread, entries| {
            if !live(thread) {
                return false;
            }
            // Expunge dead weak keys during collection. Their values may have
            // survived this mark through Thread; the next GC can reclaim them.
            entries.retain(|key, _| live(key));
            !entries.is_empty()
        });
    }
    fn put(&mut self, thread: ObjRef, key: ObjRef, value: Option<ObjRef>) -> Result<()> {
        let entries = self.threads.get(&thread);
        if !entries.is_some_and(|entries| entries.contains_key(&key)) {
            if entries.is_some_and(|entries| entries.len() >= MAX_PER_THREAD) {
                return Err(error("THREAD_LOCAL_BUDGET", "1024 entries per Java Thread"));
            }
            if self.threads.values().map(BTreeMap::len).sum::<usize>() >= MAX_ENTRIES {
                return Err(error("THREAD_LOCAL_BUDGET", "4096 ThreadLocal entries"));
            }
            if !self.threads.contains_key(&thread) && self.threads.len() >= MAX_OBJECTS {
                return Err(error("THREAD_LOCAL_BUDGET", "128 Java Thread maps"));
            }
        }
        self.threads.entry(thread).or_default().insert(key, value);
        Ok(())
    }
}
impl Executors {
    pub(super) fn thread_local_metadata(
        class: &str,
        name: &str,
        sig: &str,
    ) -> Option<HostMethodKind> {
        use HostMethodKind::{Constructor, Instance};
        match (class, name, sig) {
            (CLASS, "<init>", "()V") => Some(Constructor),
            (CLASS, "get" | "initialValue", "()Ljava/lang/Object;")
            | (CLASS, "set", "(Ljava/lang/Object;)V")
            | (CLASS, "remove", "()V") => Some(Instance),
            _ => None,
        }
    }
    pub(super) fn thread_local_attach(&self, vm: &Engine, class: &str, r: ObjRef) -> Result<bool> {
        if !vm.cp.is_subtype(class, CLASS) {
            return Ok(false);
        }
        if vm.cp.is_subtype(class, INHERITABLE) {
            return Err(unavailable(
                "InheritableThreadLocal inheritance/childValue contract",
            ));
        }
        let mut locals = self.locals.borrow_mut();
        if !locals.keys.contains_key(&r) {
            if locals.keys.len() >= MAX_KEYS {
                return Err(error("THREAD_LOCAL_BUDGET", "1024 live ThreadLocal keys"));
            }
            locals.keys.insert(r, false);
        }
        Ok(true)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn thread_local_call(
        &self,
        android: &Android,
        vm: &mut Engine,
        actual: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if !vm.cp.is_subtype(actual, CLASS) {
            return Ok(None);
        }
        if vm.cp.is_subtype(actual, INHERITABLE) {
            return Err(unavailable(
                "InheritableThreadLocal inheritance/childValue contract",
            ));
        }
        if Self::thread_local_metadata(CLASS, name, sig).is_none() {
            return Ok(None);
        }
        if name == "<init>" {
            *self.locals.borrow_mut().keys.get_mut(&r).ok_or_else(|| {
                error(
                    "THREAD_LOCAL_STATE",
                    "ThreadLocal receiver was not attached",
                )
            })? = true;
            return Ok(Some(Value::Null));
        }
        if !self.locals.borrow().keys.get(&r).copied().unwrap_or(false) {
            return Err(error(
                "THREAD_LOCAL_STATE",
                "ThreadLocal base constructor has not run",
            ));
        }
        // invoke-super selects this base implementation, while ordinary virtual
        // invokes are resolved to a DEX override before entering the host.
        if name == "initialValue" {
            return Ok(Some(Value::Null));
        }
        let protected = self.protect(vec![r])?;
        let supplied = if name == "set" {
            let value = args[0].as_ref()?;
            if let Some(value) = value {
                protected.push(value)?;
            }
            value
        } else {
            None
        };
        let thread = self.current_thread(android, vm)?;
        protected.push(thread)?;
        match name {
            "set" => {
                self.locals.borrow_mut().put(thread, r, supplied)?;
                Ok(Some(Value::Null))
            }
            "remove" => {
                let mut locals = self.locals.borrow_mut();
                if let Some(values) = locals.threads.get_mut(&thread) {
                    values.remove(&r);
                    if values.is_empty() {
                        locals.threads.remove(&thread);
                    }
                }
                Ok(Some(Value::Null))
            }
            "get" => {
                let stored = self
                    .locals
                    .borrow()
                    .threads
                    .get(&thread)
                    .and_then(|entries| entries.get(&r))
                    .copied();
                if let Some(value) = stored {
                    return Ok(Some(value.map_or(Value::Null, Value::Obj)));
                }
                // No service borrow crosses an initializer or native reentry.
                // API26 commits the successful returned value even if the
                // initializer called set/remove, without invoking overridden set.
                // If it throws, preserve its actual mutations and retry next get.
                let value = vm.invoke_instance(r, "initialValue", "()Ljava/lang/Object;", &[])?;
                let reference = value.as_ref()?;
                if let Some(value) = reference {
                    protected.push(value)?;
                }
                self.locals.borrow_mut().put(thread, r, reference)?;
                Ok(Some(value))
            }
            _ => Ok(None),
        }
    }
}
