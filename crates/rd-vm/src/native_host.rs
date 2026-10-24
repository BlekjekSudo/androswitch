//! Reentrant native bridge contract for the portable VM. Backend callbacks
//! must release their state borrows before calling back into this Engine.
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::{rc::Rc, vec::Vec};
pub trait NativeHost {
    /// Stable, nonzero identity of the actual executing platform thread.
    /// A backend that cannot provide this must not impersonate the single
    /// interpreter thread when guest code acquires a Java monitor.
    fn current_thread_id(&self) -> Option<u64> {
        None
    }
    fn load(&self, vm: &mut Engine, name: &str, absolute: bool) -> Result<(), VmExit>;
    fn invoke(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        descriptor: &str,
        is_static: bool,
        slots: &[Value],
    ) -> Result<Value, VmExit>;
    fn roots(&self) -> Vec<ObjRef>;
}
impl Engine {
    pub fn bind_native(&mut self, backend: Rc<dyn NativeHost>) {
        self.heap.bind_execution_identity(backend.clone());
        self.portable.native = Some(backend);
    }
    pub fn native_host(&self) -> Option<Rc<dyn NativeHost>> {
        self.portable.native.clone()
    }
}
