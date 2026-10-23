//! Device-owned state. No process globals, fake TLS, or desktop services.
use crate::{err, maps::HashMap, Engine, ObjRef, Value, VmExit};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
};
pub(crate) mod file;
#[derive(Debug, Clone)]
pub(crate) struct FileState {
    path: String,
    position: u64,
    mode: u8,
    closed: bool,
}

#[derive(Default)]
pub(crate) struct State {
    pub classes: HashMap<String, ObjRef>,
    pub strings: HashMap<String, ObjRef>,
    pub filesystem: Option<Box<dyn as_platform::vfs::GuestFileSystem>>,
    pub files: HashMap<ObjRef, FileState>,
    pub package: String,
    pub native: Option<alloc::rc::Rc<dyn crate::native_host::NativeHost>>,
}

impl Engine {
    pub fn bind_storage(
        &mut self,
        package: &str,
        filesystem: Box<dyn as_platform::vfs::GuestFileSystem>,
    ) -> Result<(), VmExit> {
        if !as_platform::vfs::valid_package(package) {
            return Err(err::vm_error("INVALID_PACKAGE", package).into());
        }
        self.portable.filesystem = Some(filesystem);
        self.portable.package = package.to_string();
        Ok(())
    }
    pub fn guest_filesystem(&self) -> Result<&dyn as_platform::vfs::GuestFileSystem, VmExit> {
        self.portable.filesystem.as_deref().ok_or_else(|| {
            err::vm_error(
                "API_UNAVAILABLE",
                "persistent storage is not bound to this Engine",
            )
            .into()
        })
    }
    /// Plain DEX objects need no host-side state. Platform-backed objects must
    /// fail before they can be mistaken for functioning framework objects.
    pub fn attach_host_state(&mut self, class: &str, _object: ObjRef) -> Result<(), VmExit> {
        let mut current = class.to_string();
        if matches!(
            class,
            "Ljava/io/File;" | "Ljava/io/FileInputStream;" | "Ljava/io/FileOutputStream;"
        ) {
            return Ok(());
        }
        let mut visited = crate::maps::HashSet::new();
        while visited.insert(current.clone()) {
            if matches!(
                current.as_str(),
                "Ljava/io/File;" | "Ljava/io/FileInputStream;" | "Ljava/io/FileOutputStream;"
            ) {
                return Ok(());
            }
            if current == "Ljava/lang/Object;" {
                return Ok(());
            }
            if self.cp.find_class(&current).is_none() {
                return Err(err::vm_error(
                    "API_UNAVAILABLE",
                    format!("new-instance {class}: platform state for {current} is unavailable"),
                )
                .into());
            }
            match self.cp.superclass_of(&current) {
                Some(parent) => current = parent,
                None => return Ok(()),
            }
        }
        Err(err::vm_error("INVALID_FORMAT", format!("cyclic class hierarchy: {class}")).into())
    }

    pub(crate) fn class_object_for(&mut self, desc: &str) -> Result<ObjRef, VmExit> {
        if let Some(object) = self.portable.classes.get(desc) {
            return Ok(*object);
        }
        let name = self.heap.alloc_string(desc.to_string())?;
        let object = self.heap.alloc_instance(
            "Ljava/lang/Class;".to_string(),
            vec![("class_name".to_string(), Value::Obj(name))],
        )?;
        self.portable.classes.insert(desc.to_string(), object);
        Ok(object)
    }
}
