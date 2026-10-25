//! JNI constructor semantics shared by the device bridge and isolated host
//! contracts. The caller retains argument references and roots the allocated
//! receiver synchronously before host attachment or constructor callbacks.
use alloc::{format, string::ToString, vec::Vec};
use rd_vm::framework_host::HostMethodKind;
use rd_vm::{Engine, ObjRef, Throwable, Value, VmExit};

/// Resolve the actual declaration first, then explicit platform capabilities.
/// No arbitrary name or static-kind mismatch receives a callable method ID.
pub fn method_owner(
    vm: &mut Engine,
    class: &str,
    name: &str,
    signature: &str,
    is_static: bool,
) -> Result<alloc::string::String, VmExit> {
    rd_jni::descriptor(signature).map_err(|e| rd_vm::err::vm_error(&e.code, e.cause))?;
    let missing = || {
        Throwable::new(
            "Ljava/lang/NoSuchMethodError;",
            format!("{class}->{name}{signature}"),
        )
    };
    if let Some((dex, definition, method)) = vm.cp.resolve_method(class, name, signature) {
        let declaring = vm.cp.type_str(dex, definition.class_idx);
        if (method.access_flags & rd_vm::engine::ACC_STATIC != 0) != is_static
            || (name == "<init>"
                && (declaring != class
                    || is_static
                    || !signature.ends_with(")V")
                    || method.access_flags & (0x0100 | 0x0400) != 0))
        {
            return Err(missing().into());
        }
        return Ok(declaring);
    }
    let Some(host) = vm.framework_host() else {
        return Err(missing().into());
    };
    let mut current = class.to_string();
    for _ in 0..32 {
        if let Some(kind) = host.method_metadata(&current, name, signature) {
            if (kind == HostMethodKind::Static) != is_static
                || (kind == HostMethodKind::Constructor) != (name == "<init>")
            {
                return Err(missing().into());
            }
            return Ok(current);
        }
        if name == "<init>" {
            break;
        }
        let Some(next) = vm
            .cp
            .superclass_of(&current)
            .or_else(|| rd_vm::classpath::host_superclass(&current).map(ToString::to_string))
        else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    Err(missing().into())
}

/// Match the same DEX/builtin type registry as the VM. Array identities require
/// a valid component, rather than admitting every bracket-prefixed string.
pub fn class_known(vm: &Engine, class: &str) -> bool {
    let dimensions = class.bytes().take_while(|b| *b == b'[').count();
    if dimensions > 255 {
        return false;
    }
    let component = &class[dimensions..];
    if dimensions != 0 && matches!(component, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D") {
        return true;
    }
    component.starts_with('L')
        && component.ends_with(';')
        && (vm.cp.find_class(component).is_some() || rd_vm::classpath::host_class_known(component))
}

pub fn construct<T>(
    vm: &mut Engine,
    class: &str,
    signature: &str,
    args: &[Value],
    retain: impl FnOnce(ObjRef) -> Result<T, VmExit>,
) -> Result<T, VmExit> {
    let (types, result) =
        rd_jni::descriptor(signature).map_err(|e| rd_vm::err::vm_error(&e.code, e.cause))?;
    if result != "V" || types.len() != args.len() {
        return Err(
            rd_vm::err::vm_error("JNI_SIGNATURE", "constructor descriptor/arguments").into(),
        );
    }
    if class.starts_with('[') {
        return Err(Throwable::new("Ljava/lang/InstantiationException;", class).into());
    }
    if vm
        .cp
        .find_class(class)
        .is_some_and(|(_, d)| d.access_flags & (0x0200 | 0x0400) != 0)
    {
        return Err(Throwable::new("Ljava/lang/InstantiationException;", class).into());
    }
    if vm.cp.find_class(class).is_none()
        && vm
            .framework_host()
            .and_then(|h| h.method_metadata(class, "<init>", signature))
            != Some(HostMethodKind::Constructor)
    {
        return Err(rd_vm::err::vm_error(
            "JNI_API_UNAVAILABLE",
            format!("constructor metadata for {class}->{signature}"),
        )
        .into());
    }
    let declaring = method_owner(vm, class, "<init>", signature, false)?;
    if declaring != class {
        return Err(Throwable::new(
            "Ljava/lang/NoSuchMethodError;",
            "constructor must be declared on the allocated class",
        )
        .into());
    }
    vm.ensure_initialized(class)?;
    let object = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
    let local = retain(object)?;
    vm.attach_host_state(class, object)?;
    vm.invoke_instance(object, "<init>", signature, args)?;
    Ok(local)
}
