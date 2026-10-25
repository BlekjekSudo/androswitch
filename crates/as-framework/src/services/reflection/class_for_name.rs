//! API26 Class.forName shares canonical application/bootstrap loader identity.
//! Name lookup and linking never imply initialization of an array component.
use super::super::package::LoaderScope;
use super::*;

fn missing(name: &str) -> VmExit {
    thrown("Ljava/lang/ClassNotFoundException;", name)
}
fn binary_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', ';', '[', '\0']) && !name.split('.').any(str::is_empty)
}
fn descriptor_for(name: &str) -> Option<String> {
    if !name.starts_with('[') {
        return binary_name(name).then(|| descriptor(name));
    }
    let component = name.trim_start_matches('[');
    let rank = name.len() - component.len();
    if rank > 255 {
        return None;
    }
    if matches!(component, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D") {
        return Some(name.into());
    }
    let binary = component.strip_prefix('L')?.strip_suffix(';')?;
    binary_name(binary).then(|| name.replace('.', "/"))
}
/// Resolve real required superclass/interface declarations before initialization.
/// Bounds protect malformed class graphs; no placeholder class is manufactured.
fn link(
    vm: &Engine,
    class: &str,
    scope: LoaderScope,
    path: &mut Vec<String>,
    done: &mut alloc::collections::BTreeSet<String>,
) -> Result<()> {
    if rd_vm::classpath::host_class_known(class) {
        if vm.cp.find_class(class).is_some() {
            return Err(error(
                "API_UNAVAILABLE",
                "DEX shadows a bootstrap class identity",
            ));
        }
        return Ok(());
    }
    if !matches!(scope, LoaderScope::Application) {
        return Err(thrown("Ljava/lang/NoClassDefFoundError;", class));
    }
    if done.contains(class) {
        return Ok(());
    }
    if path.iter().any(|active| active == class) {
        return Err(error(
            "INVALID_CLASS_METADATA",
            "cyclic superclass/interface graph",
        ));
    }
    if path.len() >= 64 || done.len() >= 4096 {
        return Err(error(
            "CLASS_LINK_BUDGET",
            "64 hierarchy depth / 4096 linked classes",
        ));
    }
    let (dex, definition) = vm
        .cp
        .find_class(class)
        .ok_or_else(|| thrown("Ljava/lang/NoClassDefFoundError;", class))?;
    let definition = *definition;
    path.push(class.into());
    if let Some(parent) = vm.cp.superclass_of(class) {
        link(vm, &parent, scope, path, done)?;
    }
    for interface in vm.cp.dexes[dex].interfaces(&definition)? {
        link(vm, vm.cp.dexes[dex].type_str(interface), scope, path, done)?;
    }
    path.pop();
    done.insert(class.into());
    Ok(())
}
pub(super) fn link_class(vm: &Engine, class: &str, scope: LoaderScope) -> Result<()> {
    link(vm, class, scope, &mut Vec::new(), &mut Default::default())
}
fn check_default_interface_initialization(vm: &mut Engine, class: &str) -> Result<()> {
    if !vm
        .cp
        .find_class(class)
        .is_some_and(|(_, definition)| definition.access_flags & 0x200 == 0)
    {
        return Ok(()); // Initializing an interface does not initialize its parents.
    }
    let mut pending = vec![class.to_string()];
    let mut seen = alloc::collections::BTreeSet::new();
    while let Some(class) = pending.pop() {
        if !seen.insert(class.clone()) {
            continue;
        }
        if seen.len() > 4096 {
            return Err(error(
                "CLASS_LINK_BUDGET",
                "4096 initialization dependencies",
            ));
        }
        let Some((dex, definition)) = vm.cp.find_class(&class) else {
            continue;
        };
        let definition = *definition;
        if definition.access_flags & 0x200 != 0 && !vm.device_class_is_initialized(&class) {
            let data = vm.cp.class_data(dex, &definition)?;
            if data.is_some_and(|data| {
                data.virtual_methods
                    .iter()
                    .any(|method| method.access_flags & (0x0002 | 0x0008 | 0x0400) == 0)
            }) {
                return Err(error(
                    "API_UNAVAILABLE",
                    format!("required default-bearing interface initialization scheduler: {class}"),
                ));
            }
        }
        if let Some(parent) = vm.cp.superclass_of(&class) {
            pending.push(parent);
        }
        pending.extend(vm.cp.interfaces_of(&class));
    }
    Ok(())
}
/// Shared initialization admission for reflection operations that actively use
/// a class, without pretending unimplemented default-interface scheduling ran.
pub(super) fn initialize_class(vm: &mut Engine, class: &str) -> Result<()> {
    check_default_interface_initialization(vm, class)?;
    vm.ensure_initialized(class)
}
pub(super) fn call(
    services: &Services,
    vm: &mut Engine,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>> {
    let explicit = match sig {
        "(Ljava/lang/String;)Ljava/lang/Class;" => false,
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;" => true,
        _ => return Ok(None),
    };
    let name = text(vm, &args[0])?;
    if name.len() > 4096 {
        return Err(error(
            "CLASS_NAME_BUDGET",
            "4096 UTF8 bytes per binary class name",
        ));
    }
    let scope = if explicit {
        services
            .package_metadata
            .resolve_loader(vm, args[2].as_ref()?)?
    } else {
        // All interpreted DEX definitions in this runtime belong to its one
        // application loader. Platform classes have no interpreted callers.
        LoaderScope::Application
    };
    let d = descriptor_for(&name).ok_or_else(|| missing(&name))?;
    let component = d.trim_start_matches('[');
    let primitive_array = d.starts_with('[') && component.len() == 1;
    if !primitive_array {
        let visible = rd_vm::classpath::host_class_known(component)
            || (matches!(scope, LoaderScope::Application) && vm.cp.find_class(component).is_some());
        if !visible {
            return Err(missing(&name));
        }
        super::link_class(vm, component, scope)?;
    }
    if !d.starts_with('[') && (!explicit || int(args, 1)? != 0) {
        initialize_class(vm, &d)?;
    }
    Ok(Some(Value::Obj(vm.device_class_object(&d)?)))
}
