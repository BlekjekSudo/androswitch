//! Explicit session storage for the standalone upstream semantic oracle.
//! Horizon persistence uses the platform VFS; this handle is never a substitute.
use crate::{jfile::Vfs, module_tables::Local, Engine, VmExit};
use core::cell::RefCell;

#[derive(Clone)]
pub struct ReferenceFileSystem(Local<RefCell<Vfs>>);
impl Default for ReferenceFileSystem {
    fn default() -> Self {
        Self(Local::new(RefCell::new(Vfs::default())))
    }
}
impl Engine {
    pub fn bind_reference_filesystem(
        &mut self,
        filesystem: &ReferenceFileSystem,
    ) -> Result<(), VmExit> {
        if self.framework_host().is_some() || self.portable.filesystem.is_some() {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "reference filesystem cannot replace a bound platform VFS",
            )
            .into());
        }
        self.parity_vfs = filesystem.0.clone();
        Ok(())
    }
    pub fn clear_reference_package_data(&mut self, package: &str) -> Result<(), VmExit> {
        if self.framework_host().is_some() || self.portable.filesystem.is_some() {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "package data belongs to platform storage",
            )
            .into());
        }
        crate::jfile::clear_package_data(self, package);
        Ok(())
    }
    pub fn reset_reference_filesystem(&mut self) -> Result<(), VmExit> {
        if self.framework_host().is_some() || self.portable.filesystem.is_some() {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "session data belongs to platform storage",
            )
            .into());
        }
        crate::jfile::reset_all(self);
        Ok(())
    }
}
