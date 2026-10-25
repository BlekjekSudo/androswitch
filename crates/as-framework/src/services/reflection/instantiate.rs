//! Class.newInstance invokes the declared no-arg DEX constructor directly;
//! unlike Constructor.newInstance it does not wrap the target exception.
use super::*;
pub(super) fn call(vm: &mut Engine, mirror: ObjRef, class: &str) -> Result<Value> {
    let denied = || thrown("Ljava/lang/InstantiationException;", class);
    if class.starts_with('[')
        || class.len() == 1
        || rd_vm::classpath::host_interface_modifiers(class).is_some()
    {
        return Err(denied());
    }
    if vm.device_class_object(class)? != mirror {
        return Err(error(
            "INVALID_CLASS_METADATA",
            "noncanonical Class identity",
        ));
    }
    class_for_name::link_class(vm, class, super::super::package::LoaderScope::Application)?;
    let Some((_, definition)) = vm.cp.find_class(class) else {
        return Err(error(
            "API_UNAVAILABLE",
            format!("Class.newInstance platform constructor: {class}"),
        ));
    };
    let class_flags = definition.access_flags;
    if class_flags & (0x0200 | 0x0400 | 0x4000) != 0 {
        return Err(denied());
    }
    let Some((dex, definition, constructor)) = vm.cp.resolve_method(class, "<init>", "()V") else {
        return Err(denied());
    };
    if vm.cp.type_str(dex, definition.class_idx) != class {
        return Err(denied());
    }
    let flags = constructor.access_flags;
    if flags & (0x0008 | 0x0100 | 0x0400) != 0 {
        return Err(error(
            "INVALID_CLASS_METADATA",
            "noninstance/nonexecutable constructor",
        ));
    }
    if class_flags & 1 == 0 || flags & 1 == 0 {
        let caller = vm.active_declaring_class()?.ok_or_else(|| {
            error(
                "API_UNAVAILABLE",
                "nonpublic constructor requires an active DEX caller",
            )
        })?;
        let package = |d: &str| {
            d.rsplit_once('/')
                .map(|(p, _)| p.to_string())
                .unwrap_or_default()
        };
        let same_package = package(&caller) == package(class);
        let class_allowed = class_flags & 1 != 0 || same_package;
        let member_allowed = flags & 1 != 0
            || if flags & 2 != 0 {
                caller == class
            } else {
                same_package
            };
        if !class_allowed || !member_allowed {
            return Err(thrown(
                "Ljava/lang/IllegalAccessException;",
                format!("{caller} cannot construct {class}"),
            ));
        }
    }
    class_for_name::initialize_class(vm, class)?;
    vm.new_instance(class).map(Value::Obj)
}
