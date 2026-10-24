//! Device-owned state. No process globals, fake TLS, or desktop services.
use crate::{err, maps::HashMap, Engine, ObjRef, Value, VmExit};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
};
pub(crate) mod atomic;
mod charset_catalog;
pub(crate) mod classes;
pub(crate) mod enums;
pub(crate) mod execution;
pub(crate) mod file;
pub(crate) mod format;
pub(crate) mod monitor;
mod string_encoding;
pub(crate) mod throwables;
/// Preserve Java's zero/null defaults only for real DEX declarations. Platform
/// adapters must explicitly populate implemented fields; absence is a missing
/// API, never evidence of a legitimate zero value.
pub(crate) fn instance_field(
    vm: &mut Engine,
    receiver: ObjRef,
    owner: &str,
    name: &str,
    ty: &str,
) -> Result<Value, VmExit> {
    match vm.heap.get(receiver)? {
        crate::heap::HeapObj::Instance { class, fields } => {
            if class == crate::gc::FREED_CLASS {
                return Err(
                    err::vm_error("VM_TYPE_ERROR", "field read on collected object").into(),
                );
            }
            if let Some((_, value)) = fields.iter().find(|(field, _)| field == name) {
                return Ok(value.clone());
            }
        }
        _ => {
            return Err(err::vm_error("VM_TYPE_ERROR", "field receiver is not an instance").into())
        }
    }
    let mut class = owner.to_string();
    for _ in 0..64 {
        let Some((dex, definition)) = vm.cp.find_class(&class) else {
            return Err(err::vm_error("API_UNAVAILABLE",format!("instance field {owner}->{name}:{ty}; platform declaration at {class} is not bound")).into());
        };
        let definition = *definition;
        if let Some(data) = vm.cp.class_data(dex, &definition)? {
            for field in &data.instance_fields {
                let (_, field_name, field_type) = vm.cp.field_ref(dex, field.field_idx)?;
                if field_name == name && field_type == ty {
                    return Ok(crate::engine::default_for(ty));
                }
            }
        }
        let Some(parent) = vm.cp.superclass_of(&class) else {
            break;
        };
        class = parent;
    }
    Err(err::vm_error(
        "INVALID_FIELD",
        format!("instance field {owner}->{name}:{ty} is not declared"),
    )
    .into())
}
#[derive(Debug, Clone)]
pub(crate) struct FileState {
    path: String,
    position: u64,
    mode: u8,
    closed: bool,
}

#[derive(Default)]
pub(crate) struct State {
    pub monitors: monitor::Monitors,
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
        if let Some(host) = self.framework_host() {
            if host.attach(self, class, _object)? {
                return Ok(());
            }
        }
        let mut current = class.to_string();
        if matches!(
            class,
            "Ljava/io/File;" | "Ljava/io/FileInputStream;" | "Ljava/io/FileOutputStream;"
        ) {
            return Ok(());
        }
        let mut visited = crate::maps::HashSet::new();
        while visited.insert(current.clone()) {
            if current == atomic::CLASS {
                return atomic::attach(self, _object);
            }
            if matches!(
                current.as_str(),
                "Ljava/io/File;" | "Ljava/io/FileInputStream;" | "Ljava/io/FileOutputStream;"
            ) {
                return Ok(());
            }
            if matches!(current.as_str(), "Ljava/lang/Object;" | "Ljava/lang/Enum;") {
                return Ok(());
            }
            if self.cp.find_class(&current).is_none() {
                if throwables::is_host_type(self, &current) {
                    return throwables::attach(self, _object);
                }
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
