//! Descriptor-checked JNI static fields over real DEX storage and explicitly
//! declared platform facts. Instance fields and platform writes are separate.
use alloc::{
    collections::BTreeSet,
    format,
    string::{String, ToString},
};
use as_native::jni_refs::References;
use core::cell::RefCell;
use rd_jni::Handle;
use rd_vm::{Engine, Throwable, Value, VmExit};
type Result<T> = core::result::Result<T, VmExit>;
fn error(code: &str, detail: impl Into<String>) -> VmExit {
    rd_vm::err::vm_error(code, detail).into()
}
fn table_error(e: as_native::Error) -> VmExit {
    error(&e.code, e.cause)
}
fn missing(class: &str, name: &str, sig: &str) -> VmExit {
    Throwable::new(
        "Ljava/lang/NoSuchFieldError;",
        format!("{class}->{name}:{sig}"),
    )
    .into()
}
fn resolve(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    seen: &mut BTreeSet<String>,
) -> Result<Option<(String, bool)>> {
    if !seen.insert(class.into()) {
        return Ok(None);
    }
    if seen.len() > 128 {
        return Err(error("JNI_FIELD_BUDGET", "128 field lookup classes"));
    }
    if let Some(fields) = vm.cp.declared_fields_info_checked(class)? {
        if let Some(field) = fields.iter().find(|f| f.1 == name && f.2 == sig) {
            if field.4 && fields.iter().filter(|f| f.4 && f.1 == name).count() > 1 {
                return Err(error(
                    "API_UNAVAILABLE",
                    "same-name static fields require descriptor-qualified VM storage",
                ));
            }
            return Ok(Some((class.into(), field.4)));
        }
    } else if vm
        .framework_host()
        .is_some_and(|h| h.static_field_metadata(class, name, sig))
    {
        return Ok(Some((class.into(), true)));
    }
    for interface in vm.cp.interfaces_of(class) {
        if let Some(field) = resolve(vm, &interface, name, sig, seen)? {
            return Ok(Some(field));
        }
    }
    if let Some(parent) = vm
        .cp
        .superclass_of(class)
        .or_else(|| rd_vm::classpath::host_superclass(class).map(ToString::to_string))
    {
        return resolve(vm, &parent, name, sig, seen);
    }
    Ok(None)
}
pub fn id(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    class: &str,
    name: &str,
    sig: &str,
) -> Result<u64> {
    let parsed = rd_jni::descriptor(&format!("(){sig}")).map_err(|_| missing(class, name, sig))?;
    if parsed.1 == "V" {
        return Err(missing(class, name, sig));
    }
    let (owner, is_static) = resolve(vm, class, name, sig, &mut BTreeSet::new())?
        .ok_or_else(|| missing(class, name, sig))?;
    if !is_static {
        return Err(missing(class, name, sig));
    }
    as_framework::initialize_runtime_class(vm, class)?;
    refs.borrow_mut()
        .id(
            thread,
            Handle::Field {
                class: owner,
                name: name.into(),
                sig: sig.into(),
                is_static: true,
            },
        )
        .map_err(table_error)
}
fn field(refs: &RefCell<References>, thread: u64, token: u64) -> Result<(String, String, String)> {
    match refs.borrow().get(thread, token).map_err(table_error)? {
        Handle::Field {
            class,
            name,
            sig,
            is_static: true,
        } => Ok((class.clone(), name.clone(), sig.clone())),
        _ => Err(error("JNI_FIELD_KIND", "expected static field ID")),
    }
}
fn checked(vm: &Engine, class: &str, owner: &str, sig: &str, kind: u8) -> Result<()> {
    if !vm.cp.is_subtype(class, owner) {
        return Err(error(
            "JNI_FIELD_CLASS",
            "class does not inherit static field owner",
        ));
    }
    let valid = if kind == b'L' {
        sig.starts_with('L') || sig.starts_with('[')
    } else {
        sig.as_bytes() == [kind]
    };
    if !valid {
        return Err(error(
            "JNI_FIELD_TYPE",
            "field accessor does not match descriptor",
        ));
    }
    Ok(())
}
fn zero(sig: &str) -> Value {
    match sig {
        "J" => Value::Long(0),
        "F" => Value::Float(0.),
        "D" => Value::Double(0.),
        s if s.starts_with('L') || s.starts_with('[') => Value::Null,
        _ => Value::Int(0),
    }
}
pub fn get(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    class: &str,
    token: u64,
    kind: u8,
) -> Result<u64> {
    let (owner, name, sig) = field(refs, thread, token)?;
    checked(vm, class, &owner, &sig, kind)?;
    as_framework::initialize_runtime_class(vm, &owner)?;
    let value = if vm.cp.find_class(&owner).is_some() {
        vm.statics
            .get(&(owner.clone(), name.clone()))
            .cloned()
            .unwrap_or_else(|| zero(&sig))
    } else {
        vm.framework_host()
            .ok_or_else(|| error("JNI_FIELD_PLATFORM", "no framework field provider"))?
            .static_field(vm, &owner, &name, &sig)?
            .ok_or_else(|| missing(&owner, &name, &sig))?
    };
    crate::jni_objects::to_bits(refs, thread, &value, &sig)
}
pub fn set(
    vm: &mut Engine,
    refs: &RefCell<References>,
    thread: u64,
    class: &str,
    token: u64,
    kind: u8,
    bits: u64,
) -> Result<()> {
    let (owner, name, sig) = field(refs, thread, token)?;
    checked(vm, class, &owner, &sig, kind)?;
    if vm.cp.find_class(&owner).is_none() {
        return Err(error(
            "API_UNAVAILABLE",
            "JNI writes to platform static fields",
        ));
    }
    let value = crate::jni_objects::from_bits(refs, thread, bits, &sig)?;
    if let Some(object) = value.as_ref().ok().flatten() {
        if !vm.cp.is_subtype(vm.heap.class_of(object)?, &sig) {
            return Err(error(
                "JNI_FIELD_TYPE",
                "reference does not match static field type",
            ));
        }
    }
    as_framework::initialize_runtime_class(vm, &owner)?;
    vm.statics.insert((owner, name), value);
    Ok(())
}
