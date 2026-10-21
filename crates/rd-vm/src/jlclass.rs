//! `java.lang.Class` type-introspection surface beyond `getName` and
//! friends in intrinsics.rs.
//!
//! A Class object stores its descriptor in the heap field `class_name`
//! ("Lcom/x/Y;", "[I", or a single primitive letter for `int.class`). The
//! questions are answered from the class path: DEX class defs plus the host
//! builtin hierarchy.
//! - **Classification:** `isPrimitive`, `isArray`, `getComponentType`,
//!   `isInterface`, `isEnum`, `isAnonymousClass`, `isLocalClass`,
//!   `isMemberClass`.
//! - **Subtyping:** `isAssignableFrom`, `isInstance`, `cast` (throws
//!   ClassCastException), `asSubclass`.
//! - **Hierarchy and names:** `getSuperclass`, `getInterfaces`,
//!   `getCanonicalName`.
//! - **`newInstance`:** a public no-arg DEX constructor.
//!
//! Reflective member lookup (`getMethod`, `getDeclaredConstructor`, …) is
//! not part of this module. It stays an explicit gap.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const CLASS: &str = "Ljava/lang/Class;";
const ACC_INTERFACE: u32 = 0x200;
const ACC_ENUM: u32 = 0x4000;

fn desc_of(vm: &Engine, cls: ObjRef) -> Option<String> {
    match vm.heap.get_field(cls, "class_name") {
        Ok(Value::Obj(r)) => vm.heap.as_str(r).ok().map(str::to_string),
        _ => None,
    }
}

fn is_primitive(d: &str) -> bool {
    d.len() == 1 && "ZBSCIJFDV".contains(d)
}

/// Runtime type descriptor of an object (arrays and strings included).
fn runtime_desc(vm: &Engine, o: ObjRef) -> String {
    vm.heap
        .class_of(o)
        .map(str::to_string)
        .unwrap_or_else(|_| "Ljava/lang/Object;".into())
}

pub(crate) fn assignable(vm: &Engine, to: &str, from: &str) -> bool {
    if to == from {
        return true;
    }
    if is_primitive(to) || is_primitive(from) {
        return false;
    }
    if to == "Ljava/lang/Object;" {
        return true;
    }
    vm.cp.is_subtype(from, to)
}

fn access_flags(vm: &Engine, d: &str) -> Option<u32> {
    vm.cp.find_class(d).map(|(_, def)| def.access_flags)
}

fn last_segment(d: &str) -> &str {
    let inner = d.trim_start_matches('L').trim_end_matches(';');
    let simple = inner.rsplit('/').next().unwrap_or(inner);
    simple.rsplit_once('$').map_or("", |(_, s)| s)
}

fn is_anonymous(d: &str) -> bool {
    let s = last_segment(d);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn is_local(d: &str) -> bool {
    let s = last_segment(d);
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && digits < s.len()
}

fn cce(from: &str, to: &str) -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/ClassCastException;",
        format!("{from} cannot be cast to {to}"),
    ))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class != CLASS {
        return Ok(None);
    }
    let Some(d) = desc_of(vm, recv) else {
        return Ok(None);
    };
    let b = |x: bool| Value::Int(x as i32);
    let arg_class = |vm: &Engine, i: usize| -> Option<String> {
        match args.get(i) {
            Some(Value::Obj(c)) => desc_of(vm, *c),
            _ => None,
        }
    };
    Ok(Some(match (name, sig) {
        ("isPrimitive", "()Z") => b(is_primitive(&d)),
        ("isArray", "()Z") => b(d.starts_with('[')),
        ("getComponentType", "()Ljava/lang/Class;") => match d.strip_prefix('[') {
            Some(c) => Value::Obj(vm.class_object_for(c)?),
            None => Value::Null,
        },
        ("getModifiers", "()I") => {
            const CLASS_MODIFIER_MASK: u32 = 0x0000_761f;
            Value::Int((access_flags(vm, &d).unwrap_or(0) & CLASS_MODIFIER_MASK) as i32)
        }
        ("isInterface", "()Z") => b(access_flags(vm, &d).is_some_and(|f| f & ACC_INTERFACE != 0)
            || (crate::classpath::host_class_known(&d)
                && vm.cp.superclass_of(&d).is_none()
                && d != "Ljava/lang/Object;"
                && !d.starts_with('['))),
        ("isEnum", "()Z") => b(access_flags(vm, &d).is_some_and(|f| f & ACC_ENUM != 0)
            && vm.cp.superclass_of(&d).as_deref() == Some("Ljava/lang/Enum;")),
        ("isAnonymousClass", "()Z") => b(is_anonymous(&d)),
        ("isLocalClass", "()Z") => b(is_local(&d)),
        ("isMemberClass", "()Z") => {
            b(!last_segment(&d).is_empty() && !is_anonymous(&d) && !is_local(&d))
        }
        ("isAssignableFrom", "(Ljava/lang/Class;)Z") => {
            let Some(o) = arg_class(vm, 0) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "isAssignableFrom(null)",
                )));
            };
            b(assignable(vm, &d, &o))
        }
        ("isInstance", "(Ljava/lang/Object;)Z") => match args.first() {
            Some(Value::Obj(o)) => {
                let rt = runtime_desc(vm, *o);
                b(assignable(vm, &d, &rt))
            }
            _ => b(false),
        },
        ("cast", "(Ljava/lang/Object;)Ljava/lang/Object;") => match args.first() {
            Some(Value::Obj(o)) => {
                let rt = runtime_desc(vm, *o);
                if !assignable(vm, &d, &rt) {
                    return Err(cce(&rt, &d));
                }
                Value::Obj(*o)
            }
            _ => Value::Null,
        },
        ("asSubclass", "(Ljava/lang/Class;)Ljava/lang/Class;") => {
            let o = arg_class(vm, 0).unwrap_or_default();
            if !assignable(vm, &o, &d) {
                return Err(cce(&d, &o));
            }
            Value::Obj(recv)
        }
        ("getSuperclass", "()Ljava/lang/Class;") => {
            if is_primitive(&d)
                || access_flags(vm, &d).is_some_and(|f| f & ACC_INTERFACE != 0)
                || d == "Ljava/lang/Object;"
            {
                Value::Null
            } else if d.starts_with('[') {
                Value::Obj(vm.class_object_for("Ljava/lang/Object;")?)
            } else {
                match vm.cp.superclass_of(&d) {
                    Some(s) => Value::Obj(vm.class_object_for(&s)?),
                    None => Value::Obj(vm.class_object_for("Ljava/lang/Object;")?),
                }
            }
        }
        ("getInterfaces", "()[Ljava/lang/Class;") => {
            let ifaces = vm.cp.interfaces_of(&d);
            let mut elems = Vec::new();
            for i in ifaces {
                elems.push(Value::Obj(vm.class_object_for(&i)?));
            }
            let arr = vm
                .heap
                .alloc_array(crate::heap::ElemKind::Obj, elems.len(), CLASS)?;
            let slot = vm
                .heap
                .array_elems_mut(arr)
                .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
            slot.clear();
            slot.extend(elems);
            Value::Obj(arr)
        }
        ("getCanonicalName", "()Ljava/lang/String;") => {
            fn canon(d: &str) -> Option<String> {
                if let Some(c) = d.strip_prefix('[') {
                    return canon(c).map(|s| format!("{s}[]"));
                }
                if is_primitive(d) {
                    return Some(
                        match d {
                            "Z" => "boolean",
                            "B" => "byte",
                            "S" => "short",
                            "C" => "char",
                            "I" => "int",
                            "J" => "long",
                            "F" => "float",
                            "D" => "double",
                            _ => "void",
                        }
                        .into(),
                    );
                }
                if is_anonymous(d) || is_local(d) {
                    return None;
                }
                Some(
                    d.trim_start_matches('L')
                        .trim_end_matches(';')
                        .replace(['/', '$'], "."),
                )
            }
            match canon(&d) {
                Some(s) => Value::Obj(crate::intrinsics::alloc_string(vm, s)?),
                None => Value::Null,
            }
        }
        ("newInstance", "()Ljava/lang/Object;") => {
            if vm.cp.find_class(&d).is_none() {
                return Ok(None);
            }
            vm.ensure_initialized(&d)?;
            let Some((dx, def, m)) = vm.cp.resolve_method(&d, "<init>", "()V") else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/InstantiationException;",
                    format!("{d} has no zero argument constructor"),
                )));
            };
            let o = vm.new_object(&d)?;
            vm.call(dx, def, &m, vec![Value::Obj(o)])?;
            Value::Obj(o)
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_modifiers_returns_class_flags_without_acc_super() {
        let mut builder = rd_testsupport::vm::DexBuilder::new();
        builder.class("Lexample/PublicFragment;", "Ljava/lang/Object;");
        let dex = rd_dex::Dex::parse(builder.finish()).unwrap();
        let mut vm = crate::engine::Engine::new(vec![dex], crate::engine::VmConfig::default());
        let class = vm.class_object_for("Lexample/PublicFragment;").unwrap();
        assert_eq!(
            vm.invoke_instance(class, "getModifiers", "()I", &[])
                .unwrap(),
            Value::Int(0x1)
        );
    }

    #[test]
    fn name_shapes() {
        assert!(is_anonymous("Lcom/x/A$1;"));
        assert!(!is_anonymous("Lcom/x/A$B;"));
        assert!(is_local("Lcom/x/A$1Local;"));
        assert!(is_primitive("I"));
        assert!(!is_primitive("[I"));
    }
}
