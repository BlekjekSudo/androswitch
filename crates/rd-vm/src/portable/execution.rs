//! Interpreter stacks belong to the actual executing native thread, rather
//! than to the order in which independent native calls happen to finish.
//! The VM lease still serializes mutations; suspended owners remain GC roots.
use crate::{err, heap::ObjRef, native_host::NativeHost, VmExit};
use alloc::{collections::BTreeMap, rc::Rc, string::String, vec::Vec};
use core::fmt;

const MAX_OWNERS: usize = 1024;

#[cfg(feature = "portable-test-support")]
#[path = "execution_contracts.rs"]
mod invocation_contracts;

/// No native provider means the original single interpreter. A multithreaded
/// provider must return its actual stable, nonzero platform thread identity.
#[derive(Clone, Default)]
pub(crate) struct Identity(Option<Rc<dyn NativeHost>>);
impl Identity {
    pub(crate) fn bind(&mut self, provider: Rc<dyn NativeHost>) {
        self.0 = Some(provider);
    }
    pub(crate) fn owner(&self) -> u64 {
        self.0
            .as_ref()
            .and_then(|provider| provider.current_thread_id())
            .filter(|owner| *owner != 0)
            .unwrap_or(0)
    }
    /// Class initialization/native scopes must never let an unidentified
    /// platform thread impersonate the original single interpreter.
    pub(crate) fn checked_owner(&self) -> Result<u64, VmExit> {
        match &self.0 {
            None => Ok(0),
            Some(provider) => provider
                .current_thread_id()
                .filter(|owner| *owner != 0)
                .ok_or_else(|| {
                    err::vm_error("API_UNAVAILABLE", "actual execution thread identity").into()
                }),
        }
    }
}
impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecutionIdentity")
            .field("native_provider", &self.0.is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTicket {
    owner: u64,
    serial: u64,
}
struct Frame {
    serial: u64,
    label: String,
    declaring_class: String,
    roots: Vec<ObjRef>,
}
#[derive(Default)]
pub(crate) struct Frames {
    owners: BTreeMap<u64, Vec<Frame>>,
    serial: u64,
}
impl Frames {
    pub(crate) fn enter(
        &mut self,
        owner: u64,
        label: String,
        declaring_class: String,
        roots: Vec<ObjRef>,
    ) -> Result<FrameTicket, VmExit> {
        if self.owners.len() >= MAX_OWNERS && !self.owners.contains_key(&owner) {
            return Err(err::vm_error("VM_FRAME_BUDGET", "1024 active execution owners").into());
        }
        let serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| err::vm_error("VM_FRAME_BUDGET", "execution frame ticket exhausted"))?;
        self.serial = serial;
        self.owners.entry(owner).or_default().push(Frame {
            serial,
            label,
            declaring_class,
            roots,
        });
        Ok(FrameTicket { owner, serial })
    }
    /// Remove exactly this invocation's root/diagnostic frame.
    pub(crate) fn leave(&mut self, ticket: FrameTicket) {
        let Some(frames) = self.owners.get_mut(&ticket.owner) else {
            return;
        };
        let Some(index) = frames
            .iter()
            .position(|frame| frame.serial == ticket.serial)
        else {
            return;
        };
        frames.remove(index);
        if frames.is_empty() {
            self.owners.remove(&ticket.owner);
        }
    }
    pub(crate) fn depth(&self, owner: u64) -> usize {
        self.owners.get(&owner).map_or(0, Vec::len)
    }
    pub(crate) fn declaring_class(&self, owner: u64) -> Option<&str> {
        self.owners
            .get(&owner)?
            .last()
            .map(|frame| frame.declaring_class.as_str())
    }
    pub(crate) fn labels(
        &self,
        owner: u64,
    ) -> impl DoubleEndedIterator<Item = &String> + ExactSizeIterator {
        self.owners
            .get(&owner)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|frame| &frame.label)
    }
    pub(crate) fn current_roots(&mut self, owner: u64) -> Option<&mut Vec<ObjRef>> {
        self.owners
            .get_mut(&owner)
            .and_then(|frames| frames.last_mut())
            .map(|frame| &mut frame.roots)
    }
    pub(crate) fn roots(&self) -> impl Iterator<Item = ObjRef> + '_ {
        self.owners
            .values()
            .flatten()
            .flat_map(|frame| frame.roots.iter().copied())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScopeMark {
    owner: u64,
    depth: usize,
}
#[derive(Default, Debug)]
struct Scopes {
    stack: Vec<Option<Vec<ObjRef>>>,
    existing_cutoffs: Vec<(usize, u64)>,
}
#[derive(Default, Debug)]
pub(crate) struct OwnerScopes {
    owners: BTreeMap<u64, Scopes>,
}
impl OwnerScopes {
    pub(crate) fn mark(&self, owner: u64) -> ScopeMark {
        ScopeMark {
            owner,
            depth: self
                .owners
                .get(&owner)
                .map_or(0, |scopes| scopes.stack.len()),
        }
    }
    pub(crate) fn host_enter(&mut self, owner: u64, cutoff: Option<u64>) {
        let scopes = self.owners.entry(owner).or_default();
        if let Some(cutoff) = cutoff {
            scopes.existing_cutoffs.push((scopes.stack.len(), cutoff));
        }
        scopes.stack.push(Some(Vec::new()));
    }
    pub(crate) fn guest_enter(&mut self, owner: u64) {
        self.owners.entry(owner).or_default().stack.push(None);
    }
    pub(crate) fn restore(&mut self, mark: ScopeMark) {
        if let Some(scopes) = self.owners.get_mut(&mark.owner) {
            scopes.stack.truncate(mark.depth);
            scopes
                .existing_cutoffs
                .retain(|(depth, _)| *depth < mark.depth);
            if scopes.stack.is_empty() {
                self.owners.remove(&mark.owner);
            }
        }
    }
    /// Restore exactly the captured owner's scope, then retain only the
    /// returned reference in that owner's enclosing host callback. Other
    /// native temporaries end here; an intervening guest scope is a boundary.
    pub(crate) fn restore_return(&mut self, mark: ScopeMark, object: Option<ObjRef>) {
        self.restore(mark);
        if let Some(object) = object {
            self.pin(mark.owner, object);
        }
    }
    pub(crate) fn pin(&mut self, owner: u64, object: ObjRef) {
        if let Some(Some(roots)) = self
            .owners
            .get_mut(&owner)
            .and_then(|scopes| scopes.stack.last_mut())
        {
            roots.push(object);
        }
    }
    pub(crate) fn cutoff(&self) -> u64 {
        self.owners
            .values()
            .flat_map(|scopes| scopes.existing_cutoffs.iter().map(|(_, cutoff)| *cutoff))
            .max()
            .unwrap_or(0)
    }
    pub(crate) fn roots(&self) -> impl Iterator<Item = ObjRef> + '_ {
        self.owners.values().flat_map(|scopes| {
            scopes
                .stack
                .iter()
                .filter_map(Option::as_ref)
                .flatten()
                .copied()
        })
    }
}

#[cfg(any(test, feature = "portable-test-support"))]
pub(crate) mod contracts {
    #[cfg(feature = "portable-test-support")]
    pub use super::invocation_contracts::*;
    use super::*;
    use alloc::{vec, vec::Vec};
    use core::cell::Cell;

    struct ControlledIdentity(Cell<u64>);
    impl NativeHost for ControlledIdentity {
        fn current_thread_id(&self) -> Option<u64> {
            Some(self.0.get())
        }
        fn load(&self, _: &mut crate::Engine, _: &str, _: bool) -> Result<(), VmExit> {
            unreachable!("identity/GC contract does not load a native library")
        }
        fn invoke(
            &self,
            _: &mut crate::Engine,
            _: &str,
            _: &str,
            _: &str,
            _: bool,
            _: &[crate::Value],
        ) -> Result<crate::Value, VmExit> {
            unreachable!("identity/GC contract does not execute machine code")
        }
        fn roots(&self) -> Vec<ObjRef> {
            vec![]
        }
    }

    /// This models the exact scheduling point at the native VM-lease release,
    /// then uses the production Engine collector and actual Heap allocation
    /// pins. Objects are local-only; JNI/framework tables cannot mask a loss.
    #[cfg_attr(test, test)]
    pub fn real_gc_retains_suspended_owner_after_older_owner_publishes_and_unwinds() {
        use crate::{Engine, Value, VmConfig};
        let mut vm = Engine::new(vec![], VmConfig::default());
        let identity = Rc::new(ControlledIdentity(Cell::new(10)));
        vm.bind_native(identity.clone());

        let scope_a = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(true);
        vm.heap.gc_guest_enter();
        let frame_a = vm
            .execution_frames
            .enter(10, "A.run".into(), "LA;".into(), vec![])
            .unwrap();
        let local_a = vm.heap.alloc_instance("LOnlyA;".into(), vec![]).unwrap();
        vm.publish_frame_roots(&[Value::Obj(local_a)], &Value::Null, None);

        // A is in native code. B enters on a real different owner, publishes
        // its own Java locals, then also suspends in native code.
        identity.0.set(20);
        let scope_b = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(true);
        vm.heap.gc_guest_enter();
        let frame_b = vm
            .execution_frames
            .enter(20, "B.run".into(), "LB;".into(), vec![])
            .unwrap();
        let local_b = vm.heap.alloc_instance("LOnlyB;".into(), vec![]).unwrap();
        vm.publish_frame_roots(&[Value::Obj(local_b)], &Value::Null, None);
        let b_host_mark = vm.heap.gc_scope_depth();
        vm.heap.gc_host_enter(false);
        let b_host_temporary = vm
            .heap
            .alloc_instance("LBHostTemporary;".into(), vec![])
            .unwrap();

        // A resumes first. Its guest allocation must not accidentally become
        // pinned in B's newer host scope. Publishing must not replace B roots.
        identity.0.set(10);
        let garbage = vm
            .heap
            .alloc_instance("LCollectMe;".into(), vec![])
            .unwrap();
        vm.publish_frame_roots(&[Value::Obj(local_a)], &Value::Null, None);
        vm.gc();
        assert_eq!(vm.heap.class_of(garbage), Ok(crate::gc::FREED_CLASS));
        assert_eq!(vm.heap.class_of(local_b), Ok("LOnlyB;"));
        assert_eq!(vm.heap.class_of(b_host_temporary), Ok("LBHostTemporary;"));
        assert_eq!(
            vm.frame_labels().map(String::as_str).collect::<Vec<_>>(),
            vec!["A.run"]
        );
        vm.execution_frames.leave(frame_a);
        vm.heap.gc_scope_restore(scope_a);
        vm.gc();
        assert_eq!(vm.heap.class_of(local_b), Ok("LOnlyB;"));
        assert_eq!(vm.heap.class_of(b_host_temporary), Ok("LBHostTemporary;"));

        // B can resume and restore its own nested host scope after A has left.
        identity.0.set(20);
        assert_eq!(
            vm.frame_labels().map(String::as_str).collect::<Vec<_>>(),
            vec!["B.run"]
        );
        vm.heap.gc_scope_restore(b_host_mark);
        vm.publish_frame_roots(&[Value::Obj(local_b)], &Value::Null, None);
        vm.gc();
        assert_eq!(
            vm.heap.class_of(b_host_temporary),
            Ok(crate::gc::FREED_CLASS)
        );
        assert_eq!(vm.heap.class_of(local_b), Ok("LOnlyB;"));
        vm.execution_frames.leave(frame_b);
        vm.heap.gc_scope_restore(scope_b);
        vm.gc();
        assert_eq!(vm.heap.class_of(local_a), Ok(crate::gc::FREED_CLASS));
        assert_eq!(vm.heap.class_of(local_b), Ok(crate::gc::FREED_CLASS));
        assert!(vm.execution_frames.owners.is_empty());
        assert!(vm.heap.gc_pinned_roots().next().is_none());
    }

    #[cfg_attr(test, test)]
    pub fn independent_frames_resume_and_unwind_in_non_lifo_order() {
        let mut frames = Frames::default();
        let a = frames
            .enter(10, "A.run".into(), "LA;".into(), vec![101])
            .unwrap();
        let b = frames
            .enter(20, "B.run".into(), "LB;".into(), vec![202])
            .unwrap();
        assert_eq!(frames.declaring_class(10), Some("LA;"));
        assert_eq!(frames.declaring_class(20), Some("LB;"));
        *frames.current_roots(10).unwrap() = vec![103];
        assert_eq!(frames.roots().collect::<Vec<_>>(), vec![103, 202]);
        assert_eq!(
            frames.labels(10).map(String::as_str).collect::<Vec<_>>(),
            vec!["A.run"]
        );
        assert_eq!(
            frames.labels(20).map(String::as_str).collect::<Vec<_>>(),
            vec!["B.run"]
        );
        frames.leave(a);
        assert_eq!(frames.depth(10), 0);
        assert_eq!(frames.depth(20), 1);
        assert_eq!(frames.roots().collect::<Vec<_>>(), vec![202]);
        frames.leave(b);
        assert!(frames.owners.is_empty());
    }

    #[cfg_attr(test, test)]
    pub fn same_owner_nested_callbacks_keep_distinct_frame_tickets() {
        let mut frames = Frames::default();
        let outer = frames
            .enter(10, "outer".into(), "LOuter;".into(), vec![101])
            .unwrap();
        let callback = frames
            .enter(10, "callback".into(), "LCallback;".into(), vec![102])
            .unwrap();
        assert_eq!(frames.depth(10), 2);
        frames.current_roots(10).unwrap().push(103);
        assert_eq!(frames.roots().collect::<Vec<_>>(), vec![101, 102, 103]);
        frames.leave(callback);
        assert_eq!(frames.roots().collect::<Vec<_>>(), vec![101]);
        frames.leave(outer);
        assert!(frames.owners.is_empty());
    }

    #[cfg_attr(test, test)]
    pub fn owner_bound_is_checked_and_completed_owners_do_not_accumulate() {
        let mut frames = Frames::default();
        let tickets: Vec<_> = (0..MAX_OWNERS as u64)
            .map(|owner| {
                frames
                    .enter(owner, "live".into(), "LLive;".into(), vec![])
                    .unwrap()
            })
            .collect();
        let error = frames
            .enter(
                MAX_OWNERS as u64,
                "excess".into(),
                "LExcess;".into(),
                vec![],
            )
            .unwrap_err();
        assert!(matches!(error, VmExit::Error(error) if error.code == "VM_FRAME_BUDGET"));
        for ticket in tickets {
            frames.leave(ticket);
        }
        for owner in 1..=4096 {
            let ticket = frames
                .enter(owner, "next".into(), "LNext;".into(), vec![])
                .unwrap();
            frames.leave(ticket);
        }
        assert!(frames.owners.is_empty());
    }

    #[cfg_attr(test, test)]
    pub fn heap_restore_and_allocation_pins_use_their_own_actual_owner() {
        let mut scopes = OwnerScopes::default();
        let a = scopes.mark(10);
        scopes.host_enter(10, Some(11));
        scopes.pin(10, 101);
        let b = scopes.mark(20);
        scopes.host_enter(20, Some(22));
        scopes.pin(20, 202);
        scopes.restore(a);
        scopes.pin(20, 203);
        assert_eq!(scopes.cutoff(), 22);
        assert_eq!(scopes.roots().collect::<Vec<_>>(), vec![202, 203]);
        scopes.restore(b);
        assert_eq!(scopes.cutoff(), 0);
        assert!(scopes.owners.is_empty());
    }

    #[cfg_attr(test, test)]
    pub fn guest_allocations_do_not_leak_into_another_owners_host_scope() {
        let mut scopes = OwnerScopes::default();
        let a = scopes.mark(10);
        scopes.host_enter(10, Some(11));
        scopes.guest_enter(10);
        let b = scopes.mark(20);
        scopes.host_enter(20, Some(22));
        scopes.pin(10, 101);
        scopes.pin(20, 202);
        assert_eq!(scopes.roots().collect::<Vec<_>>(), vec![202]);
        scopes.restore(a);
        scopes.restore(b);
        assert!(scopes.owners.is_empty());
    }
}
