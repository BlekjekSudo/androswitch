//! Portable framework adapter. Dispatch uses Android descriptors, never APK identity.
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::{rc::Rc, vec::Vec};
pub trait FrameworkHost {
    fn attach(&self, vm: &mut Engine, class: &str, object: ObjRef) -> Result<bool, VmExit>;
    fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>, VmExit>;
    fn call(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>, VmExit>;
    fn roots(&self) -> Vec<ObjRef>;
    fn references(&self, _object: ObjRef) -> Vec<ObjRef> {
        Vec::new()
    }
    fn collect(&self, _marked: &[bool]) {}
}
impl Engine {
    pub fn bind_device_storage(
        &mut self,
        package: &str,
        filesystem: alloc::boxed::Box<dyn as_platform::vfs::GuestFileSystem>,
    ) -> Result<(), VmExit> {
        #[cfg(not(feature = "std"))]
        {
            self.bind_storage(package, filesystem)
        }
        #[cfg(feature = "std")]
        {
            let _ = (package, filesystem);
            Err(crate::err::vm_error(
                "PORTABLE_CORE_REQUIRED",
                "build this adapter with rd-vm default features disabled",
            )
            .into())
        }
    }
    pub fn bind_framework(&mut self, framework: Rc<dyn FrameworkHost>) {
        self.framework_backend = Some(framework);
    }
    pub fn framework_host(&self) -> Option<Rc<dyn FrameworkHost>> {
        self.framework_backend.clone()
    }
}
