//! Bounded Runtime surface. Environment observations come from an explicit
//! platform provider; allocator estimates are not Java heap memory statistics.
use super::*;
use core::cell::{Cell, RefCell};
use rd_vm::framework_host::HostMethodKind;

const CLASS: &str = "Ljava/lang/Runtime;";

#[derive(Default)]
pub(in crate::android) struct Runtime {
    singleton: Cell<Option<ObjRef>>,
    resources: RefCell<Option<Rc<dyn as_platform::RuntimeResources>>>,
}
impl Runtime {
    pub(in crate::android) fn bind(&self, resources: Rc<dyn as_platform::RuntimeResources>) {
        self.resources.replace(Some(resources));
    }
    pub(super) fn metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
        match (class, name, sig) {
            (CLASS, "getRuntime", "()Ljava/lang/Runtime;") => Some(HostMethodKind::Static),
            (CLASS, "availableProcessors", "()I") => Some(HostMethodKind::Instance),
            _ => None,
        }
    }
    pub(super) fn call_static(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if (class, name, sig) != (CLASS, "getRuntime", "()Ljava/lang/Runtime;") {
            return Ok(None);
        }
        if !args.is_empty() {
            return Err(error(
                "INVALID_ARG",
                "Runtime.getRuntime takes no arguments",
            ));
        }
        let object = if let Some(object) = self.singleton.get() {
            object
        } else {
            // Exactly one rooted instance per Android/application VM. No
            // Java constructor is advertised: Runtime's constructor is private.
            let object = vm.heap.alloc_instance(CLASS.into(), Vec::new())?;
            self.singleton.set(Some(object));
            object
        };
        Ok(Some(Value::Obj(object)))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn call(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        receiver: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if (class, name, sig) != (CLASS, "availableProcessors", "()I") {
            return Ok(None);
        }
        if !args.is_empty() {
            return Err(error(
                "INVALID_ARG",
                "Runtime.availableProcessors takes no arguments",
            ));
        }
        if self.singleton.get() != Some(receiver) || vm.heap.class_of(receiver)? != CLASS {
            return Err(error(
                "INVALID_RECEIVER",
                "Runtime receiver is not this VM's singleton",
            ));
        }
        // Clone the provider before executing it. It may reenter the runtime;
        // no RefCell borrow may span a platform or Java callback.
        let resources = self.resources.borrow().clone().ok_or_else(|| {
            error(
                "API_UNAVAILABLE",
                "Runtime.availableProcessors platform provider is not bound",
            )
        })?;
        let actual = resources.available_processors().map_err(storage_error)?;
        let count = i32::try_from(actual)
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| {
                error(
                    "RESOURCE_INFO_RANGE",
                    "available processor count must be a positive Java int",
                )
            })?;
        Ok(Some(Value::Int(count)))
    }
    pub(super) fn roots(&self) -> Vec<ObjRef> {
        self.singleton.get().into_iter().collect()
    }
    pub(in crate::android) fn system_property(&self, key: &str) -> Result<Option<String>> {
        let provider = self.resources.borrow().clone();
        match provider {
            Some(provider) => provider.system_property(key).map_err(storage_error),
            None => Ok(None),
        }
    }
}
