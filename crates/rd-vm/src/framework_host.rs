//! Portable framework adapter. Dispatch uses Android descriptors, never APK identity.
use crate::{Engine, ObjRef, Value, VmExit};
use alloc::{rc::Rc, string::String, vec::Vec};
/// An explicitly implemented host method. This catalog is metadata only:
/// looking up a method never executes it or implies other overloads exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostMethodKind {
    Instance,
    Static,
    Constructor,
}
pub trait FrameworkHost {
    /// Publish a staged collection operation to the same owner used by Java,
    /// JNI and GC. An adapter must not create reference-only fallback state.
    fn collection_replace(
        &self,
        _vm: &mut Engine,
        _object: ObjRef,
        _contents: crate::collection_factories::Contents,
    ) -> Result<(), VmExit> {
        Err(
            crate::err::vm_error("API_UNAVAILABLE", "collection owner has no update adapter")
                .into(),
        )
    }
    fn collection_snapshot(
        &self,
        _vm: &Engine,
        _object: ObjRef,
    ) -> Result<Option<crate::collection_factories::Contents>, VmExit> {
        Ok(None)
    }
    fn collection_create(
        &self,
        _vm: &mut Engine,
        _class: &str,
        _contents: crate::collection_factories::Contents,
    ) -> Result<ObjRef, VmExit> {
        Err(
            crate::err::vm_error("API_UNAVAILABLE", "collection owner has no factory adapter")
                .into(),
        )
    }
    fn monotonic_micros(&self) -> Option<u64> {
        None
    }
    fn write_log(&self, _fd: i32, _text: &str) -> Result<usize, VmExit> {
        Err(crate::err::vm_error("API_UNAVAILABLE", "application log sink").into())
    }
    fn system_property(&self, _key: &str) -> Result<Option<String>, VmExit> {
        Ok(None)
    }
    fn method_metadata(
        &self,
        _class: &str,
        _name: &str,
        _descriptor: &str,
    ) -> Option<HostMethodKind> {
        None
    }
    /// Called under the VM lease. Providers retain only bounded metadata and
    /// must not execute Java callbacks or borrow Engine asynchronously.
    fn thread_attached(&self, _owner: u64, _name: Option<&str>) -> Result<(), VmExit> {
        Ok(())
    }
    fn thread_detached(&self, _owner: u64) {}
    fn static_field(
        &self,
        _vm: &mut Engine,
        _class: &str,
        _name: &str,
        _descriptor: &str,
    ) -> Result<Option<Value>, VmExit> {
        Ok(None)
    }
    /// Exact implemented static declarations, independent of their current
    /// value. JNI lookup must not guess a field type from a nullable value.
    fn static_field_metadata(&self, _class: &str, _name: &str, _descriptor: &str) -> bool {
        false
    }
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
    fn asset_open(&self, _name: &str) -> Result<u64, VmExit> {
        Err(crate::err::vm_error("API_UNAVAILABLE", "AssetManager.open").into())
    }
    fn asset_read(&self, _handle: u64, _output: &mut [u8]) -> Result<usize, VmExit> {
        Err(crate::err::vm_error("API_UNAVAILABLE", "Asset.read").into())
    }
    fn asset_seek(&self, _handle: u64, _offset: i64, _origin: i32) -> Result<u64, VmExit> {
        Err(crate::err::vm_error("API_UNAVAILABLE", "Asset.seek").into())
    }
    fn asset_len(&self, _handle: u64) -> Result<u64, VmExit> {
        Err(crate::err::vm_error("API_UNAVAILABLE", "Asset.length").into())
    }
    fn asset_close(&self, _handle: u64) {}
}
impl Engine {
    pub fn device_class_object(&mut self, descriptor: &str) -> Result<ObjRef, VmExit> {
        #[cfg(not(feature = "desktop-runtime"))]
        {
            self.class_object_for(descriptor)
        }
        #[cfg(feature = "desktop-runtime")]
        {
            let _ = descriptor;
            Err(crate::err::vm_error("PORTABLE_CORE_REQUIRED", "device class object").into())
        }
    }
    pub fn bind_device_storage(
        &mut self,
        package: &str,
        filesystem: alloc::boxed::Box<dyn as_platform::vfs::GuestFileSystem>,
    ) -> Result<(), VmExit> {
        #[cfg(not(feature = "desktop-runtime"))]
        {
            self.bind_storage(package, filesystem)
        }
        #[cfg(feature = "desktop-runtime")]
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
