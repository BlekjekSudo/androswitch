//! Reflective lookup uses declared DEX metadata, not just a referenced method ID.
use super::*;
const CLASS: &str = "Ljava/lang/Class;";
const METHOD: &str = "Ljava/lang/reflect/Method;";
mod class_for_name;
pub(super) fn link_class(
    vm: &Engine,
    class: &str,
    scope: super::package::LoaderScope,
) -> Result<()> {
    class_for_name::link_class(vm, class, scope)
}
pub(super) fn metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<rd_vm::framework_host::HostMethodKind> {
    (class == CLASS
        && name == "forName"
        && matches!(
            sig,
            "(Ljava/lang/String;)Ljava/lang/Class;"
                | "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;"
        ))
    .then_some(rd_vm::framework_host::HostMethodKind::Static)
}
fn thrown(class: &str, message: impl Into<String>) -> VmExit {
    Throwable::new(class, message).into()
}
fn desc(vm: &Engine, r: ObjRef) -> Result<String> {
    if vm.heap.class_of(r)? != CLASS {
        return Err(thrown(
            "Ljava/lang/IllegalArgumentException;",
            "expected Class",
        ));
    }
    text(vm, &vm.heap.get_field(r, "class_name")?)
}
fn class_name(d: &str) -> String {
    match d {
        "V" => "void".into(),
        "Z" => "boolean".into(),
        "B" => "byte".into(),
        "C" => "char".into(),
        "S" => "short".into(),
        "I" => "int".into(),
        "J" => "long".into(),
        "F" => "float".into(),
        "D" => "double".into(),
        d if d.starts_with('[') => d.replace('/', "."),
        _ => d
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', "."),
    }
}
pub(super) fn call_static(
    services: &Services,
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>> {
    if class != CLASS || name != "forName" {
        return Ok(None);
    }
    class_for_name::call(services, vm, sig, args)
}
fn unbox(vm: &Engine, target: &str, value: &Value) -> Result<Value> {
    if target.len() != 1 {
        if let Value::Obj(r) = value {
            if !vm.cp.is_subtype(vm.heap.class_of(*r)?, target) {
                return Err(thrown(
                    "Ljava/lang/IllegalArgumentException;",
                    "reference parameter type",
                ));
            }
        }
        return Ok(value.clone());
    }
    let r = obj(value).map_err(|_| {
        thrown(
            "Ljava/lang/IllegalArgumentException;",
            "null primitive argument",
        )
    })?;
    let class = vm.heap.class_of(r)?;
    let source = collections::primitive(class).ok_or_else(|| {
        thrown(
            "Ljava/lang/IllegalArgumentException;",
            "expected primitive wrapper",
        )
    })?;
    let allowed = source == target
        || match source {
            "B" => matches!(target, "S" | "I" | "J" | "F" | "D"),
            "S" | "C" => matches!(target, "I" | "J" | "F" | "D"),
            "I" => matches!(target, "J" | "F" | "D"),
            "J" => matches!(target, "F" | "D"),
            "F" => target == "D",
            _ => false,
        };
    if !allowed {
        return Err(thrown(
            "Ljava/lang/IllegalArgumentException;",
            "primitive narrowing conversion",
        ));
    }
    let raw = vm.heap.get_field(r, "value")?;
    Ok(match target {
        "J" => match raw {
            Value::Int(v) => Value::Long(v as i64),
            v => v,
        },
        "F" => match raw {
            Value::Int(v) => Value::Float(v as f32),
            Value::Long(v) => Value::Float(v as f32),
            v => v,
        },
        "D" => match raw {
            Value::Int(v) => Value::Double(v as f64),
            Value::Long(v) => Value::Double(v as f64),
            Value::Float(v) => Value::Double(v as f64),
            v => v,
        },
        _ => raw,
    })
}
fn wrap(vm: &mut Engine, error: VmExit) -> Result<Value> {
    let VmExit::Exception(t) = error else {
        return Err(error);
    };
    let cause = if let Some(r) = t.obj {
        r
    } else {
        let r = vm.heap.alloc_instance(t.class, vec![])?;
        if let Some(m) = t.message {
            let s = string(vm, m)?;
            write(vm, r, "message", s)?;
        }
        r
    };
    let class = "Ljava/lang/reflect/InvocationTargetException;";
    let r = vm
        .heap
        .alloc_instance(class.into(), vec![("cause".into(), Value::Obj(cause))])?;
    Err(VmExit::Exception(Throwable {
        class: class.into(),
        message: None,
        obj: Some(r),
    }))
}
#[allow(clippy::too_many_arguments)]
pub(super) fn call(
    services: &Services,
    vm: &mut Engine,
    _class: &str,
    name: &str,
    sig: &str,
    r: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    let actual = vm.heap.class_of(r)?.to_string();
    if actual == CLASS {
        let d = desc(vm, r)?;
        let value = match (name, sig) {
            ("getName", "()Ljava/lang/String;") => string(vm, class_name(&d))?,
            ("isPrimitive", "()Z") => Value::Int((d.len() == 1) as i32),
            ("isArray", "()Z") => Value::Int(d.starts_with('[') as i32),
            ("isAssignableFrom", "(Ljava/lang/Class;)Z") => {
                Value::Int(vm.cp.is_subtype(&desc(vm, obj(&args[0])?)?, &d) as i32)
            }
            ("isInstance", "(Ljava/lang/Object;)Z") => Value::Int(if let Value::Obj(r) = args[0] {
                vm.cp.is_subtype(vm.heap.class_of(r)?, &d) as i32
            } else {
                0
            }),
            ("getSuperclass", "()Ljava/lang/Class;") => {
                let parent = if d.starts_with('[') {
                    Some("Ljava/lang/Object;".into())
                } else if d.len() == 1
                    || d == "Ljava/lang/Object;"
                    || rd_vm::classpath::host_interface_modifiers(&d).is_some()
                    || vm
                        .cp
                        .find_class(&d)
                        .is_some_and(|(_, def)| def.access_flags & 0x200 != 0)
                {
                    None
                } else {
                    Some(vm.cp.superclass_of(&d).ok_or_else(|| {
                        error(
                            "API_UNAVAILABLE",
                            format!("Class.getSuperclass platform metadata: {d}"),
                        )
                    })?)
                };
                match parent {
                    Some(s) => Value::Obj(vm.device_class_object(&s)?),
                    None => Value::Null,
                }
            }
            (
                "getDeclaredMethod" | "getMethod",
                "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            ) => {
                let method = text(vm, &args[0])?;
                let mut prefix = String::from("(");
                if let Value::Obj(a) = args[1] {
                    for v in vm.heap.as_array_elems(a)?.0 {
                        prefix.push_str(&desc(vm, obj(v)?)?);
                    }
                }
                prefix.push(')');
                let mut c = d.clone();
                let mut found = None;
                for _ in 0..64 {
                    found = vm
                        .cp
                        .declared_methods(&c)
                        .into_iter()
                        .find(|(n, s, f)| {
                            n == &method
                                && s.starts_with(&prefix)
                                && (name == "getDeclaredMethod" || f & 1 != 0)
                        })
                        .map(|(_, s, f)| (c.clone(), s, f));
                    if found.is_some() || name == "getDeclaredMethod" {
                        break;
                    }
                    let Some(parent) = vm.cp.superclass_of(&c) else {
                        break;
                    };
                    c = parent;
                }
                let (owner, proto, flags) = found.ok_or_else(|| {
                    thrown(
                        "Ljava/lang/NoSuchMethodException;",
                        format!("{d}.{method}{prefix}"),
                    )
                })?;
                let owner = string(vm, owner)?;
                let method = string(vm, method)?;
                let proto = string(vm, proto)?;
                Value::Obj(vm.heap.alloc_instance(
                    METHOD.into(),
                    vec![
                        ("owner".into(), owner),
                        ("method".into(), method),
                        ("proto".into(), proto),
                        ("flags".into(), Value::Int(flags as i32)),
                        ("accessible".into(), Value::Int(0)),
                    ],
                )?)
            }
            _ => return Ok(None),
        };
        return Ok(Some(value));
    }
    if actual != METHOD {
        return Ok(None);
    }
    let owner = text(vm, &vm.heap.get_field(r, "owner")?)?;
    let method = text(vm, &vm.heap.get_field(r, "method")?)?;
    let proto = text(vm, &vm.heap.get_field(r, "proto")?)?;
    let flags = vm.heap.get_field(r, "flags")?.as_int()?;
    let value = match (name, sig) {
        ("getName", "()Ljava/lang/String;") => string(vm, method)?,
        ("getModifiers", "()I") => Value::Int(flags),
        ("getDeclaringClass", "()Ljava/lang/Class;") => Value::Obj(vm.device_class_object(&owner)?),
        ("getReturnType", "()Ljava/lang/Class;") => {
            Value::Obj(vm.device_class_object(proto.split_once(')').expect("DEX proto").1)?)
        }
        ("setAccessible", "(Z)V") => {
            write(vm, r, "accessible", args[0].clone())?;
            Value::Null
        }
        ("invoke", "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;") => {
            // Private access is explicitly unsupported unless the caller has
            // requested access suppression. Public lookup retains real flags.
            if flags & 1 == 0 && vm.heap.get_field(r, "accessible")?.as_int()? == 0 {
                return Err(thrown("Ljava/lang/IllegalAccessException;", method));
            }
            let params = rd_vm::engine::parse_param_types(&proto)
                .ok_or_else(|| error("INVALID_DESCRIPTOR", &proto))?;
            let given = if let Value::Obj(a) = args[1] {
                vm.heap.as_array_elems(a)?.0.to_vec()
            } else {
                vec![]
            };
            if given.len() != params.len() {
                return Err(thrown(
                    "Ljava/lang/IllegalArgumentException;",
                    "reflection arity",
                ));
            }
            let arguments = params
                .iter()
                .zip(&given)
                .map(|(p, v)| unbox(vm, p, v))
                .collect::<Result<Vec<_>>>()?;
            let outcome = if flags & 8 != 0 {
                vm.invoke_static(&owner, &method, &proto, &arguments)
            } else {
                let recv = obj(&args[0])?;
                if !vm.cp.is_subtype(vm.heap.class_of(recv)?, &owner) {
                    return Err(thrown(
                        "Ljava/lang/IllegalArgumentException;",
                        "receiver type",
                    ));
                }
                vm.invoke_instance(recv, &method, &proto, &arguments)
            };
            let v = match outcome {
                Ok(v) => v,
                Err(e) => return wrap(vm, e).map(Some),
            };
            let ret = proto.split_once(')').expect("DEX proto").1;
            if ret == "V" {
                Value::Null
            } else if let Some(class) = [
                "Ljava/lang/Boolean;",
                "Ljava/lang/Byte;",
                "Ljava/lang/Short;",
                "Ljava/lang/Character;",
                "Ljava/lang/Integer;",
                "Ljava/lang/Long;",
                "Ljava/lang/Float;",
                "Ljava/lang/Double;",
            ]
            .into_iter()
            .find(|c| collections::primitive(c) == Some(ret))
            {
                services.collections.boxed(vm, class, v)?
            } else {
                v
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
