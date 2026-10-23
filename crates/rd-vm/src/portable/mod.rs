//! Device-owned state. No process globals, fake TLS, or desktop services.
use crate::{err, maps::HashMap, Engine, ObjRef, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
};

#[derive(Default)]
pub(crate) struct State {
    pub classes: HashMap<String, ObjRef>,
    pub strings: HashMap<String, ObjRef>,
}

impl Engine {
    /// Plain DEX objects need no host-side state. Platform-backed objects must
    /// fail before they can be mistaken for functioning framework objects.
    pub fn attach_host_state(&mut self, class: &str, _object: ObjRef) -> Result<(), VmExit> {
        let mut current = class.to_string();
        let mut visited = crate::maps::HashSet::new();
        while visited.insert(current.clone()) {
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
