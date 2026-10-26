//! Reflective member lookup: `Class.getMethod/getDeclaredMethod`,
//! `Class.getConstructor/getDeclaredConstructor`, plus `Method` and
//! `Constructor`.
//!
//! **Signature resolution.** Java looks a method up by name and parameter
//! types; the VM dispatches on the full descriptor. The return type is
//! recovered from the `method_ids` of every loaded dex: a `method_id` exists
//! for each method the app defines and for each platform API the app
//! references anywhere. Search order:
//! - `getDeclaredMethod`: the class itself.
//! - `getMethod`: the class, then its superclass chain.
//!
//! If nothing matches, `NoSuchMethodException` is thrown, as on a device
//! where the member does not exist. Feature-detection idioms such as
//! kotlinx `Handler.createAsync` and AndroidX compat shims rely on this.
//!
//! **`Method.invoke(recv, Object[])`.**
//! - Arguments are unboxed per the parameter descriptors, and the return is
//!   boxed. `void` returns null.
//! - A null receiver means a static call. Host statics go through the
//!   normal static dispatch, so intrinsics answer them.
//! - An exception thrown by the target is wrapped in
//!   `InvocationTargetException`, with the original as its cause.
//! - Arity mismatches throw `IllegalArgumentException`.
//!
//! **Deviations.**
//! - Access checks are not enforced (`setAccessible` is a no-op).
//! - `getModifiers` reports `public`, plus `static` when the method is
//!   known static.
//! - `getMethods()` / `getDeclaredMethods()` enumeration is not provided.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

const METHOD: &str = "Ljava/lang/reflect/Method;";
const CTOR: &str = "Ljava/lang/reflect/Constructor;";
const CLASS: &str = "Ljava/lang/Class;";

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.into()))
}

fn str_field(vm: &Engine, r: ObjRef, f: &str) -> Option<String> {
    match vm.heap.get_field(r, f) {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}

pub(crate) fn class_desc(vm: &Engine, c: ObjRef) -> Option<String> {
    str_field(vm, c, "class_name").or_else(|| str_field(vm, c, "name"))
}

fn put_str(vm: &mut Engine, r: ObjRef, f: &str, s: &str) -> Result<(), VmExit> {
    let v = Value::Obj(crate::intrinsics::alloc_string(vm, s.to_string())?);
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}

/// "(Landroid/os/Looper;)" from a `Class[]` argument
fn params_desc(vm: &Engine, arr: Option<&Value>) -> Result<String, VmExit> {
    let mut s = String::from("(");
    if let Some(Value::Obj(a)) = arr {
        for v in vm.heap.as_array_elems(*a)?.0.iter() {
            match v {
                Value::Obj(c) => s.push_str(&class_desc(vm, *c).unwrap_or_default()),
                _ => {
                    return Err(exc(
                        "Ljava/lang/NullPointerException;",
                        "parameter type is null",
                    ))
                }
            }
        }
    }
    s.push(')');
    Ok(s)
}

/// full descriptor of `class.name(params…)` from the dex method_ids
fn find_sig(vm: &Engine, class: &str, name: &str, params: &str) -> Option<String> {
    for d in 0..vm.cp.dexes.len() {
        let dex = &vm.cp.dexes[d];
        for idx in 0..dex.method_ids.len() {
            let mid = &dex.method_ids[idx];
            if dex.type_str(mid.class_idx as u32) != class || dex.string(mid.name_idx) != name {
                continue;
            }
            if let Ok(mr) = vm.cp.method_ref(d, idx as u32) {
                if mr.proto.starts_with(params) {
                    return Some(mr.proto.clone());
                }
            }
        }
    }
    None
}

fn lookup(
    vm: &Engine,
    class: &str,
    name: &str,
    params: &str,
    inherited: bool,
) -> Option<(String, String)> {
    let mut cur = Some(class.to_string());
    let mut guard = 0;
    while let Some(c) = cur {
        if let Some(sig) = find_sig(vm, &c, name, params) {
            return Some((c, sig));
        }
        if !inherited || guard > 32 {
            break;
        }
        guard += 1;
        cur = vm.cp.superclass_of(&c);
    }
    None
}

fn is_static(vm: &mut Engine, class: &str, name: &str, sig: &str) -> Option<bool> {
    vm.cp
        .resolve_method(class, name, sig)
        .map(|(_, _, m)| m.access_flags & 0x0008 != 0)
}

fn param_types(sig: &str) -> Vec<String> {
    let inner = &sig[1..sig.find(')').unwrap_or(1)];
    let b = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while b[i] == b'[' {
            i += 1;
        }
        if b[i] == b'L' {
            while b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(inner[start..i].to_string());
    }
    out
}

fn unbox(vm: &Engine, desc: &str, v: &Value) -> Result<Value, VmExit> {
    if desc.len() != 1 {
        return Ok(v.clone());
    }
    let Value::Obj(b) = v else {
        return Err(exc(
            "Ljava/lang/IllegalArgumentException;",
            format!("null for primitive parameter {desc}"),
        ));
    };
    let raw = vm.heap.get_field(*b, "value").unwrap_or(Value::Int(0));
    Ok(match (desc, raw) {
        ("J", Value::Int(i)) => Value::Long(i as i64),
        ("F", Value::Int(i)) => Value::Float(i as f32),
        ("D", Value::Int(i)) => Value::Double(i as f64),
        ("D", Value::Float(f)) => Value::Double(f as f64),
        ("D", Value::Long(l)) => Value::Double(l as f64),
        ("F", Value::Long(l)) => Value::Float(l as f32),
        (_, r) => r,
    })
}

fn wrapper_of(desc: &str) -> Option<&'static str> {
    Some(match desc {
        "Z" => "Ljava/lang/Boolean;",
        "B" => "Ljava/lang/Byte;",
        "S" => "Ljava/lang/Short;",
        "C" => "Ljava/lang/Character;",
        "I" => "Ljava/lang/Integer;",
        "J" => "Ljava/lang/Long;",
        "F" => "Ljava/lang/Float;",
        "D" => "Ljava/lang/Double;",
        _ => return None,
    })
}

fn wrap_target(vm: &mut Engine, e: VmExit) -> VmExit {
    match e {
        VmExit::Exception(t) => {
            let cause = match t.obj {
                Some(o) => Some(o),
                None => vm
                    .heap
                    .alloc_instance(t.class.clone(), Vec::new())
                    .ok()
                    .inspect(|o| {
                        if let Some(m) = &t.message {
                            if let Ok(s) = crate::intrinsics::alloc_string(vm, m.clone()) {
                                let _ = crate::throwable_message::set(vm, *o, Value::Obj(s));
                            }
                        }
                    }),
            };
            let Some(cause) = cause else {
                return VmExit::Exception(t);
            };
            let Ok(ite) = vm.heap.alloc_instance(
                "Ljava/lang/reflect/InvocationTargetException;".into(),
                Vec::new(),
            ) else {
                return VmExit::Exception(t);
            };
            let _ = vm.heap.put_field(ite, "cause", Value::Obj(cause));
            let _ = vm.heap.put_field(ite, "rd_cause_set", Value::Int(1));
            VmExit::Exception(Throwable {
                class: "Ljava/lang/reflect/InvocationTargetException;".into(),
                message: None,
                obj: Some(ite),
            })
        }
        other => other,
    }
}

fn new_class_array(vm: &mut Engine, descs: &[String]) -> Result<Value, VmExit> {
    let a = vm.heap.alloc_array(ElemKind::Obj, descs.len(), CLASS)?;
    let mut vals = Vec::new();
    for d in descs {
        vals.push(Value::Obj(vm.class_object_for(d)?));
    }
    *vm.heap.array_elems_mut(a)? = vals;
    Ok(Value::Obj(a))
}

fn invoke_args(vm: &Engine, sig: &str, arr: Option<&Value>) -> Result<Vec<Value>, VmExit> {
    let pts = param_types(sig);
    let given: Vec<Value> = match arr {
        Some(Value::Obj(a)) => vm.heap.as_array_elems(*a)?.0.to_vec(),
        _ => Vec::new(),
    };
    if given.len() != pts.len() {
        return Err(exc(
            "Ljava/lang/IllegalArgumentException;",
            format!(
                "Wrong number of arguments; expected {}, got {}",
                pts.len(),
                given.len()
            ),
        ));
    }
    pts.iter()
        .zip(given.iter())
        .map(|(p, v)| unbox(vm, p, v))
        .collect()
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == CLASS {
        return Ok(match (name, sig) {
            ("getMethod", "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;")
            | (
                "getDeclaredMethod",
                "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            ) => {
                let Some(class) = class_desc(vm, recv) else {
                    return Ok(None);
                };
                let mname = match args.first() {
                    Some(Value::Obj(s)) => vm.heap.as_str(*s)?.to_string(),
                    _ => return Err(exc("Ljava/lang/NullPointerException;", "name == null")),
                };
                let params = params_desc(vm, args.get(1))?;
                let Some((decl, msig)) = lookup(vm, &class, &mname, &params, name == "getMethod")
                else {
                    let pretty = class
                        .trim_start_matches('L')
                        .trim_end_matches(';')
                        .replace('/', ".");
                    return Err(exc(
                        "Ljava/lang/NoSuchMethodException;",
                        format!("{pretty}.{mname} {params}"),
                    ));
                };
                let m = vm.heap.alloc_instance(METHOD.to_string(), Vec::new())?;
                put_str(vm, m, "rd_decl", &decl)?;
                put_str(vm, m, "rd_name", &mname)?;
                put_str(vm, m, "rd_sig", &msig)?;
                Some(Value::Obj(m))
            }
            ("getConstructors", "()[Ljava/lang/reflect/Constructor;")
            | ("getDeclaredConstructors", "()[Ljava/lang/reflect/Constructor;")
            | ("getMethods", "()[Ljava/lang/reflect/Method;")
            | ("getDeclaredMethods", "()[Ljava/lang/reflect/Method;") => {
                let Some(class) = class_desc(vm, recv) else {
                    return Ok(None);
                };
                let ctors = name.contains("Constructor");
                let public_only = !name.contains("Declared");
                // getMethods walks superclasses (public members are inherited);
                // the others look at the class itself only.
                let mut classes = vec![class.clone()];
                if name == "getMethods" {
                    let mut cur = vm.cp.superclass_of(&class);
                    while let Some(c) = cur {
                        if classes.len() > 32 {
                            break;
                        }
                        classes.push(c.clone());
                        cur = vm.cp.superclass_of(&c);
                    }
                }
                let mut seen = crate::maps::HashSet::new();
                let mut found: Vec<(String, String, String)> = Vec::new();
                for c in &classes {
                    for (mname, proto, flags) in vm.cp.declared_methods(c) {
                        if mname == "<clinit>" || (mname == "<init>") != ctors {
                            continue;
                        }
                        // skip synthetic/bridge members, and non-public ones when asked
                        if flags & 0x1040 != 0 || (public_only && flags & 0x0001 == 0) {
                            continue;
                        }
                        if seen.insert((mname.clone(), proto.clone())) {
                            found.push((c.clone(), mname, proto));
                        }
                    }
                }
                if ctors && found.is_empty() && crate::classpath::host_class_known(&class) {
                    found.push((class.clone(), "<init>".into(), "()V".into()));
                }
                let elem = if ctors { CTOR } else { METHOD };
                let mut objs = Vec::new();
                for (decl, mname, proto) in &found {
                    let o = vm.heap.alloc_instance(elem.to_string(), Vec::new())?;
                    put_str(vm, o, "rd_decl", decl)?;
                    put_str(vm, o, "rd_name", mname)?;
                    put_str(vm, o, "rd_sig", proto)?;
                    objs.push(Value::Obj(o));
                }
                let a = vm.heap.alloc_array(ElemKind::Obj, objs.len(), elem)?;
                *vm.heap.array_elems_mut(a)? = objs;
                Some(Value::Obj(a))
            }
            ("getConstructor", "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;")
            | ("getDeclaredConstructor", "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;") => {
                let Some(class) = class_desc(vm, recv) else {
                    return Ok(None);
                };
                let params = params_desc(vm, args.first())?;
                let csig = format!("{params}V");
                let known = vm.cp.resolve_method(&class, "<init>", &csig).is_some()
                    || find_sig(vm, &class, "<init>", &params).is_some()
                    || (params == "()" && crate::classpath::host_class_known(&class));
                if !known {
                    let pretty = class
                        .trim_start_matches('L')
                        .trim_end_matches(';')
                        .replace('/', ".");
                    return Err(exc(
                        "Ljava/lang/NoSuchMethodException;",
                        format!("{pretty}.<init> {params}"),
                    ));
                }
                let c = vm.heap.alloc_instance(CTOR.to_string(), Vec::new())?;
                put_str(vm, c, "rd_decl", &class)?;
                put_str(vm, c, "rd_name", "<init>")?;
                put_str(vm, c, "rd_sig", &csig)?;
                Some(Value::Obj(c))
            }
            _ => None,
        });
    }
    if mref_class != METHOD
        && mref_class != CTOR
        && mref_class != "Ljava/lang/reflect/AccessibleObject;"
        && mref_class != "Ljava/lang/reflect/Executable;"
    {
        return Ok(None);
    }
    let (Some(decl), Some(mname), Some(msig)) = (
        str_field(vm, recv, "rd_decl"),
        str_field(vm, recv, "rd_name"),
        str_field(vm, recv, "rd_sig"),
    ) else {
        return Ok(None);
    };
    let ret = msig.rsplit_once(')').map_or("V", |(_, r)| r).to_string();
    if let Some(value) = crate::jannotations::method_intrinsic(vm, name, sig, recv, args)? {
        return Ok(Some(value));
    }
    Ok(Some(match (name, sig) {
        ("setAccessible", "(Z)V") => Value::Null,
        ("isAccessible", "()Z") => Value::Int(1),
        ("getName", "()Ljava/lang/String;") => {
            let n = if mname == "<init>" {
                decl.trim_start_matches('L')
                    .trim_end_matches(';')
                    .replace('/', ".")
            } else {
                mname
            };
            Value::Obj(crate::intrinsics::alloc_string(vm, n)?)
        }
        ("getDeclaringClass", "()Ljava/lang/Class;") => Value::Obj(vm.class_object_for(&decl)?),
        ("getReturnType", "()Ljava/lang/Class;") => Value::Obj(vm.class_object_for(&ret)?),
        ("getParameterTypes", "()[Ljava/lang/Class;") => new_class_array(vm, &param_types(&msig))?,
        ("getParameterCount", "()I") => Value::Int(param_types(&msig).len() as i32),
        ("getModifiers", "()I") => Value::Int(
            1 | if is_static(vm, &decl, &mname, &msig) == Some(true) {
                8
            } else {
                0
            },
        ),
        ("invoke", "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;") => {
            let call_args = invoke_args(vm, &msig, args.get(1))?;
            let target = match args.first() {
                Some(Value::Obj(r)) if is_static(vm, &decl, &mname, &msig) != Some(true) => {
                    Some(*r)
                }
                _ => None,
            };
            let res = match target {
                Some(r) => vm.invoke_instance(r, &mname, &msig, &call_args),
                None => {
                    if is_static(vm, &decl, &mname, &msig) == Some(false) {
                        return Err(exc(
                            "Ljava/lang/NullPointerException;",
                            "null receiver for instance method",
                        ));
                    }
                    vm.invoke_static(&decl, &mname, &msig, &call_args)
                }
            };
            let v = res.map_err(|e| wrap_target(vm, e))?;
            match wrapper_of(&ret) {
                _ if ret == "V" => Value::Null,
                Some(w) => Value::Obj(crate::boxes::box_value(vm, w, v)?),
                None => v,
            }
        }
        ("newInstance", "([Ljava/lang/Object;)Ljava/lang/Object;") => {
            let call_args = invoke_args(vm, &msig, args.first())?;
            vm.ensure_initialized(&decl)?;
            let r = vm.heap.alloc_instance(decl.clone(), Vec::new())?;
            vm.attach_host_state(&decl, r)?;
            vm.invoke_instance(r, "<init>", &msig, &call_args)
                .map_err(|e| wrap_target(vm, e))?;
            Value::Obj(r)
        }
        ("toString", "()Ljava/lang/String;") => {
            let s = format!("{decl}->{mname}{msig}");
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::param_types;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;

    #[test]
    fn splits_params() {
        assert_eq!(
            param_types("(I[JLjava/lang/String;[[Lx/Y;Z)V"),
            vec!["I", "[J", "Ljava/lang/String;", "[[Lx/Y;", "Z"]
        );
        assert!(param_types("()V").is_empty());
    }
}
