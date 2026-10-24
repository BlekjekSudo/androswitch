//! `java.lang.Enum` uses ordinary, budgeted heap fields so references participate
//! in the portable GC. DEX constructors, values(), and overrides still execute.
//! Reference: libcore android-8.0.0_r4, ojluni/src/main/java/java/lang/Enum.java.
use crate::{err, heap::HeapObj, Engine, ObjRef, Throwable, Value, VmExit};
use alloc::{format, string::ToString};

const ENUM: &str = "Ljava/lang/Enum;";
// Enum's private inherited fields must not collide with fields an enum itself
// declares. These VM-owned slots are not Android public/reflection fields.
const NAME: &str = "java.lang.Enum#name";
const ORDINAL: &str = "java.lang.Enum#ordinal";

fn exception(class: &str, message: impl Into<alloc::string::String>) -> VmExit {
    Throwable::new(class, message).into()
}
fn object(value: &Value) -> Result<ObjRef, VmExit> {
    value
        .as_ref()?
        .ok_or_else(|| exception("Ljava/lang/NullPointerException;", "null enum argument"))
}
fn state(vm: &Engine, receiver: ObjRef, field: &str) -> Result<Value, VmExit> {
    vm.heap.get_field(receiver, field).map_err(|_| {
        err::vm_error(
            "ENUM_STATE_UNINITIALIZED",
            "Enum constructor has not completed",
        )
        .into()
    })
}
fn declaring_class(vm: &Engine, receiver: ObjRef) -> Result<alloc::string::String, VmExit> {
    let class = vm.heap.class_of(receiver)?;
    if vm.cp.superclass_of(class).as_deref() == Some(ENUM) {
        return Ok(class.to_string());
    }
    if let Some(parent) = vm.cp.superclass_of(class) {
        if vm.cp.superclass_of(&parent).as_deref() == Some(ENUM) {
            return Ok(parent);
        }
    }
    Err(err::vm_error("ENUM_CLASS_METADATA", format!("invalid enum class {class}")).into())
}

pub(crate) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if (class, name, sig)
        != (
            ENUM,
            "valueOf",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;",
        )
    {
        return Ok(None);
    }
    let class_object = object(&args[0])?;
    let desc_object = object(&vm.heap.get_field(class_object, "class_name")?)?;
    let desc = vm.heap.as_str(desc_object)?.to_string();
    let wanted = vm.heap.as_str(object(&args[1])?)?.to_string();
    if vm.cp.superclass_of(&desc).as_deref() != Some(ENUM) {
        return Err(exception(
            "Ljava/lang/IllegalArgumentException;",
            format!("{desc} is not an enum class"),
        ));
    }
    vm.ensure_initialized(&desc)?;
    for (owner, field, ty, flags, is_static) in vm.cp.declared_fields_info(&desc) {
        if !is_static || ty != desc || flags & 0x4000 == 0 {
            continue;
        }
        if let Some(Value::Obj(constant)) = vm.statics.get(&(owner, field)) {
            if let Value::Obj(name) = state(vm, *constant, NAME)? {
                if vm.heap.as_str(name)? == wanted {
                    return Ok(Some(Value::Obj(*constant)));
                }
            }
        }
    }
    Err(exception(
        "Ljava/lang/IllegalArgumentException;",
        format!(
            "No enum constant {}.{wanted}",
            desc.trim_start_matches('L')
                .trim_end_matches(';')
                .replace('/', ".")
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn call_instance(
    vm: &mut Engine,
    receiver_class: &str,
    declaring: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Java enum values() methods return a shallow clone of their DEX-owned
    // constants array. The same rule applies to every primitive/object array.
    if receiver_class.starts_with('[') && (name, sig) == ("clone", "()Ljava/lang/Object;") {
        let (kind, length, descriptor) = match vm.heap.get(receiver)? {
            HeapObj::Array {
                elem,
                elems,
                elem_class,
            } => (*elem, elems.len(), elem_class.clone()),
            _ => return Ok(None),
        };
        let copy = vm.heap.alloc_array(kind, length, &descriptor)?;
        let elements = vm.heap.as_array_elems(receiver)?.0.to_vec();
        *vm.heap.array_elems_mut(copy)? = elements;
        return Ok(Some(Value::Obj(copy)));
    }
    if !vm.cp.is_subtype(receiver_class, ENUM) {
        return Ok(None);
    }
    let constructor = declaring == ENUM && (name, sig) == ("<init>", "(Ljava/lang/String;I)V");
    let supported = matches!(
        (name, sig),
        ("name" | "toString", "()Ljava/lang/String;")
            | ("ordinal" | "hashCode", "()I")
            | ("equals", "(Ljava/lang/Object;)Z")
            | ("compareTo", "(Ljava/lang/Enum;)I" | "(Ljava/lang/Object;)I")
            | ("getDeclaringClass", "()Ljava/lang/Class;")
            | ("clone", "()Ljava/lang/Object;")
    );
    if !constructor && !supported {
        return Ok(None);
    }
    // toString is virtual. A constant-specific class or enum may override it;
    // invoke-super passes Enum as receiver_class to select the base contract.
    if name == "toString" && vm.cp.resolve_method(receiver_class, name, sig).is_some() {
        return Ok(None);
    }
    let result = match name {
        "<init>" => {
            if let Some(name) = args[0].as_ref()? {
                vm.heap.as_str(name)?;
            }
            let ordinal = args[1].as_int()?;
            err::put_field_result(vm.heap.put_field(receiver, NAME, args[0].clone()))?;
            err::put_field_result(vm.heap.put_field(receiver, ORDINAL, Value::Int(ordinal)))?;
            Value::Null
        }
        "name" | "toString" => state(vm, receiver, NAME)?,
        "ordinal" => state(vm, receiver, ORDINAL)?,
        "equals" => Value::Int(i32::from(args[0].as_ref()? == Some(receiver))),
        "hashCode" => Value::Int(receiver as i32),
        "getDeclaringClass" => {
            let class = declaring_class(vm, receiver)?;
            Value::Obj(vm.class_object_for(&class)?)
        }
        "compareTo" => {
            let other = object(&args[0])?;
            if !vm.cp.is_subtype(vm.heap.class_of(other)?, ENUM)
                || declaring_class(vm, receiver)? != declaring_class(vm, other)?
            {
                return Err(exception(
                    "Ljava/lang/ClassCastException;",
                    "different enum types",
                ));
            }
            Value::Int(
                state(vm, receiver, ORDINAL)?
                    .as_int()?
                    .wrapping_sub(state(vm, other, ORDINAL)?.as_int()?),
            )
        }
        "clone" => {
            return Err(exception(
                "Ljava/lang/CloneNotSupportedException;",
                "cannot clone an enum",
            ))
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}
