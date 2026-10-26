//! Runtime-visible class annotations (`Class.getAnnotation` & co.).
//!
//! **Lookup.** Class annotations come from the DEX
//! `annotations_directory_item` of the class. Only RUNTIME visibility counts.
//! `getAnnotation`, `getDeclaredAnnotation` and `isAnnotationPresent` look
//! at the class itself; inherited (`@Inherited`) annotations are not
//! walked. `getAnnotations`/`getDeclaredAnnotations` return them all.
//!
//! **Annotation instances.** Each instance is a heap object whose class is
//! the annotation interface. Element values are stored as `rd_ann_<name>`
//! fields, and calling an element method returns its value: the explicit
//! value, else the `dalvik.annotation.AnnotationDefault` default.
//! - Encoded values map to Java values: strings, primitives, `Class` objects
//!   for types, enum constants via their static field, arrays, nested
//!   annotations.
//! - `annotationType()`, `toString()`, `hashCode()` and `equals()` are
//!   provided.
//!
//! **Deviation.** Runtime-visible method/constructor annotations are queried
//! by `AnnotatedElement`; repeatable-container expansion, field annotations,
//! and parameter annotations are not implemented.

use rd_dex::annotations::{
    parse_annotation_item, parse_annotation_set_item, parse_annotations_directory,
    EncodedAnnotation, EncodedValue, Visibility,
};

use std::cell::RefCell;
use std::collections::HashSet;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

thread_local! {
    /// Live annotation instances (cheap gate for instance dispatch).
    static INSTANCES: RefCell<HashSet<ObjRef>> = RefCell::new(HashSet::new());
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    INSTANCES.with(|m| m.borrow_mut().retain(|k| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — sem isto, um objeto qualquer do
/// próximo `Engine` que reutilize o mesmo `ObjRef` seria despachado
/// como se fosse uma instância de anotação do heap anterior.
pub(crate) fn reset_all() {
    INSTANCES.with(|m| m.borrow_mut().clear());
}

/// Runtime annotations of `desc` as (dex index, annotation).
fn class_annotations(
    vm: &Engine,
    desc: &str,
    visibility: Visibility,
) -> Vec<(usize, EncodedAnnotation)> {
    let Some((di, def)) = vm.cp.find_class(desc) else {
        return Vec::new();
    };
    if def.annotations_off == 0 {
        return Vec::new();
    }
    let dex = &vm.cp.dexes[di];
    let Ok(dir) = parse_annotations_directory(&dex.data, def.annotations_off) else {
        return Vec::new();
    };
    let Ok(offs) = parse_annotation_set_item(&dex.data, dir.class_annotations_off) else {
        return Vec::new();
    };
    offs.into_iter()
        .filter_map(|o| parse_annotation_item(&dex.data, o).ok())
        .filter(|it| it.visibility == visibility)
        .map(|it| (di, it.annotation))
        .collect()
}

fn desc_of_class_obj(vm: &Engine, c: ObjRef) -> Option<String> {
    match vm.heap.get_field(c, "class_name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}

/// Runtime-visible annotations attached to one declared DEX method.
fn method_annotations(
    vm: &Engine,
    class: &str,
    name: &str,
    proto: &str,
) -> Vec<(usize, EncodedAnnotation)> {
    let Some((di, def)) = vm.cp.find_class(class) else {
        return Vec::new();
    };
    if def.annotations_off == 0 {
        return Vec::new();
    }
    let dex = &vm.cp.dexes[di];
    // Search only this class's declared methods (ADR 008 Stage 1b): the
    // previous scan walked every method_id in the DEX (50k+) per call,
    // which reflection-heavy frameworks hit repeatedly during launch.
    let declared = dex.class_data(def).ok().flatten();
    let Some(method_idx) = declared
        .iter()
        .flat_map(|data| {
            data.direct_methods
                .iter()
                .chain(data.virtual_methods.iter())
        })
        .map(|m| m.method_idx)
        .find(|&idx| {
            vm.cp
                .method_ref(di, idx)
                .map(|m| m.class == class && m.name == name && m.proto == proto)
                .unwrap_or(false)
        })
    else {
        return Vec::new();
    };
    let Ok(dir) = parse_annotations_directory(&dex.data, def.annotations_off) else {
        return Vec::new();
    };
    let Some((_, set_off)) = dir
        .method_annotations
        .iter()
        .find(|(idx, _)| *idx == method_idx)
    else {
        return Vec::new();
    };
    let Ok(offs) = parse_annotation_set_item(&dex.data, *set_off) else {
        return Vec::new();
    };
    offs.into_iter()
        .filter_map(|off| parse_annotation_item(&dex.data, off).ok())
        .filter(|item| item.visibility == Visibility::Runtime)
        .map(|item| (di, item.annotation))
        .collect()
}

fn to_value(vm: &mut Engine, di: usize, v: &EncodedValue) -> Result<Value, VmExit> {
    Ok(match v {
        EncodedValue::Byte(b) => Value::Int(*b as i32),
        EncodedValue::Short(s) => Value::Int(*s as i32),
        EncodedValue::Char(c) => Value::Int(*c as i32),
        EncodedValue::Int(i) => Value::Int(*i),
        EncodedValue::Long(l) => Value::Long(*l),
        EncodedValue::Float(f) => Value::Float(*f),
        EncodedValue::Double(d) => Value::Double(*d),
        EncodedValue::Boolean(b) => Value::Int(*b as i32),
        EncodedValue::Null => Value::Null,
        EncodedValue::String(idx) => {
            let s = vm.cp.dexes[di].string(*idx).to_string();
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        EncodedValue::Type(idx) => {
            let t = vm.cp.dexes[di].type_str(*idx).to_string();
            Value::Obj(vm.class_object_for(&t)?)
        }
        EncodedValue::Enum(fidx) => {
            let Ok(f) = vm.cp.dexes[di].field(*fidx) else {
                return Ok(Value::Null);
            };
            let owner = vm.cp.dexes[di].type_str(f.class_idx as u32).to_string();
            let name = vm.cp.dexes[di].string(f.name_idx).to_string();
            vm.ensure_initialized(&owner)?;
            vm.statics
                .get(&(owner, name))
                .cloned()
                .unwrap_or(Value::Null)
        }
        EncodedValue::Array(items) => {
            let mut vals = Vec::new();
            for it in items {
                vals.push(to_value(vm, di, it)?);
            }
            let (kind, desc) = match vals.first() {
                Some(Value::Int(_)) => (ElemKind::Int, "[I"),
                Some(Value::Long(_)) => (ElemKind::Long, "[J"),
                Some(Value::Float(_)) => (ElemKind::Float, "[F"),
                Some(Value::Double(_)) => (ElemKind::Double, "[D"),
                Some(Value::Obj(o)) if vm.heap.as_str(*o).is_ok() => {
                    (ElemKind::Obj, "[Ljava/lang/String;")
                }
                Some(Value::Obj(o))
                    if vm
                        .heap
                        .class_of(*o)
                        .map(|c| c == "Ljava/lang/Class;")
                        .unwrap_or(false) =>
                {
                    (ElemKind::Obj, "[Ljava/lang/Class;")
                }
                _ => (ElemKind::Obj, "[Ljava/lang/Object;"),
            };
            let arr = vm
                .heap
                .alloc_array(kind, vals.len(), desc)
                .map_err(crate::err::oom_throwable)?;
            *vm.heap
                .array_elems_mut(arr)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? = vals;
            Value::Obj(arr)
        }
        EncodedValue::Annotation(a) => Value::Obj(instance(vm, di, a)?),
        _ => Value::Null,
    })
}

/// Materializes an annotation instance (explicit values + defaults).
fn instance(vm: &mut Engine, di: usize, a: &EncodedAnnotation) -> Result<ObjRef, VmExit> {
    let ty = vm.cp.dexes[di].type_str(a.type_idx).to_string();
    let mut fields: Vec<(String, Value)> = vec![("rd_annotation".into(), Value::Int(1))];
    // Defaults: @dalvik.annotation.AnnotationDefault on the annotation type.
    for (ddi, def) in class_annotations(vm, &ty, Visibility::System) {
        if vm.cp.dexes[ddi].type_str(def.type_idx) != "Ldalvik/annotation/AnnotationDefault;" {
            continue;
        }
        for el in &def.elements {
            if let EncodedValue::Annotation(inner) = &el.value {
                for e in &inner.elements {
                    let n = vm.cp.dexes[ddi].string(e.name_idx).to_string();
                    let v = to_value(vm, ddi, &e.value)?;
                    fields.push((format!("rd_ann_{n}"), v));
                }
            }
        }
    }
    for el in &a.elements {
        let n = vm.cp.dexes[di].string(el.name_idx).to_string();
        let v = to_value(vm, di, &el.value)?;
        let key = format!("rd_ann_{n}");
        fields.retain(|(k, _)| *k != key);
        fields.push((key, v));
    }
    let r = vm
        .heap
        .alloc_instance(ty, fields)
        .map_err(crate::err::oom_throwable)?;
    INSTANCES.with(|s| s.borrow_mut().insert(r));
    Ok(r)
}

/// `Class` reflection over annotations; `None` = not an annotation query.
pub fn class_intrinsic(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(
        name,
        "getAnnotation"
            | "getDeclaredAnnotation"
            | "isAnnotationPresent"
            | "getAnnotations"
            | "getDeclaredAnnotations"
            | "getAnnotationsByType"
            | "getDeclaredAnnotationsByType"
    ) {
        return Ok(None);
    }
    let Some(desc) = desc_of_class_obj(vm, recv) else {
        return Ok(None);
    };
    let anns = class_annotations(vm, &desc, Visibility::Runtime);
    let want = match args.first() {
        Some(Value::Obj(c)) => desc_of_class_obj(vm, *c),
        _ => None,
    };
    let matching: Vec<(usize, EncodedAnnotation)> = anns
        .into_iter()
        .filter(|(di, a)| {
            want.as_deref()
                .map(|w| w == vm.cp.dexes[*di].type_str(a.type_idx))
                .unwrap_or(true)
        })
        .collect();
    Ok(Some(match name {
        "isAnnotationPresent" => Value::Int(!matching.is_empty() as i32),
        "getAnnotation" | "getDeclaredAnnotation" => match matching.first() {
            Some((di, a)) => Value::Obj(instance(vm, *di, a)?),
            None => Value::Null,
        },
        _ => {
            let mut vals = Vec::new();
            for (di, a) in &matching {
                vals.push(Value::Obj(instance(vm, *di, a)?));
            }
            let arr = vm
                .heap
                .alloc_array(
                    ElemKind::Obj,
                    vals.len(),
                    "[Ljava/lang/annotation/Annotation;",
                )
                .map_err(crate::err::oom_throwable)?;
            *vm.heap
                .array_elems_mut(arr)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? = vals;
            Value::Obj(arr)
        }
    }))
}

/// Method/`Executable` reflection over runtime-visible DEX annotations.
pub(crate) fn method_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let query = match (name, sig) {
        ("getAnnotation", "(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;")
        | ("getDeclaredAnnotation", "(Ljava/lang/Class;)Ljava/lang/annotation/Annotation;")
        | ("isAnnotationPresent", "(Ljava/lang/Class;)Z")
        | ("getAnnotationsByType", "(Ljava/lang/Class;)[Ljava/lang/annotation/Annotation;")
        | (
            "getDeclaredAnnotationsByType",
            "(Ljava/lang/Class;)[Ljava/lang/annotation/Annotation;",
        ) => true,
        ("getAnnotations", "()[Ljava/lang/annotation/Annotation;")
        | ("getDeclaredAnnotations", "()[Ljava/lang/annotation/Annotation;") => false,
        _ => return Ok(None),
    };
    let (Some(Value::Obj(class_name)), Some(Value::Obj(method_name)), Some(Value::Obj(proto))) = (
        vm.heap.get_field(recv, "rd_decl").ok(),
        vm.heap.get_field(recv, "rd_name").ok(),
        vm.heap.get_field(recv, "rd_sig").ok(),
    ) else {
        return Ok(None);
    };
    let decl = vm.heap.as_str(class_name)?.to_string();
    let method = vm.heap.as_str(method_name)?.to_string();
    let proto = vm.heap.as_str(proto)?.to_string();
    let want = if query {
        let Some(Value::Obj(class_obj)) = args.first() else {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/NullPointerException;",
                "annotation type == null",
            )));
        };
        Some(desc_of_class_obj(vm, *class_obj).ok_or_else(|| {
            VmExit::Exception(Throwable::new(
                "Ljava/lang/NullPointerException;",
                "annotation type has no class descriptor",
            ))
        })?)
    } else {
        None
    };
    let annotations = method_annotations(vm, &decl, &method, &proto);
    let matching: Vec<(usize, EncodedAnnotation)> = annotations
        .into_iter()
        .filter(|(di, annotation)| {
            want.as_deref()
                .map(|wanted| wanted == vm.cp.dexes[*di].type_str(annotation.type_idx))
                .unwrap_or(true)
        })
        .collect();
    Ok(Some(match name {
        "isAnnotationPresent" => Value::Int(!matching.is_empty() as i32),
        "getAnnotation" | "getDeclaredAnnotation" => match matching.first() {
            Some((di, annotation)) => Value::Obj(instance(vm, *di, annotation)?),
            None => Value::Null,
        },
        _ => {
            let mut values = Vec::with_capacity(matching.len());
            for (di, annotation) in &matching {
                values.push(Value::Obj(instance(vm, *di, annotation)?));
            }
            let array = vm
                .heap
                .alloc_array(
                    ElemKind::Obj,
                    values.len(),
                    "[Ljava/lang/annotation/Annotation;",
                )
                .map_err(crate::err::oom_throwable)?;
            *vm.heap
                .array_elems_mut(array)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))? = values;
            Value::Obj(array)
        }
    }))
}

/// Element/`annotationType` calls on an annotation instance.
pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
) -> Result<Option<Value>, VmExit> {
    if !sig.starts_with("()")
        || !INSTANCES.with(|s| s.borrow().contains(&recv))
        || !matches!(vm.heap.get_field(recv, "rd_annotation"), Ok(Value::Int(1)))
    {
        return Ok(None);
    }
    Ok(Some(match name {
        "annotationType" => Value::Obj(vm.class_object_for(recv_class)?),
        "hashCode" => Value::Int(recv as i32),
        "toString" => {
            let s = format!(
                "@{}()",
                recv_class
                    .trim_start_matches('L')
                    .trim_end_matches(';')
                    .replace('/', ".")
            );
            Value::Obj(crate::intrinsics::alloc_string(vm, s)?)
        }
        _ => match vm.heap.get_field(recv, &format!("rd_ann_{name}")) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_reset_clears_instances_from_previous_engine() {
        INSTANCES.with(|s| s.borrow_mut().insert(4242));
        assert!(INSTANCES.with(|s| s.borrow().contains(&4242)));
        crate::reset_host_thread_state();
        assert!(
            !INSTANCES.with(|s| s.borrow().contains(&4242)),
            "#230: ObjRef de heap morto não pode seguir marcado como instância de anotação"
        );
    }
}
