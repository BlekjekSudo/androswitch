//! `android.os.Bundle` / `BaseBundle` / `PersistableBundle` and the
//! `android.content.Intent` data surface.
//!
//! **Bundles** are a typed key → value store (`HostObj::Bundle.map`, in
//! insertion order). Rather than listing ~60 typed accessors, the module
//! dispatches on the method shape:
//! - `putX(String, v)V` stores `v`. Any X works: Int, String, Parcelable,
//!   IntArray, StringArrayList, Bundle, …
//! - `getX(String)` returns the stored value, or the typed default: 0,
//!   false, 0.0 or null.
//! - `getX(String, default)` returns the stored value, else `default`.
//! - `getParcelable(String, Class)` and similar (API 33) filter by class:
//!   a stored value that isn't an instance comes back as null, as in AOSP.
//!
//! The rest is modelled directly: `containsKey`, `remove`, `keySet`, `size`,
//! `isEmpty`, `clear`, `putAll`, `get`, copy constructors and
//! `setClassLoader`.
//!
//! **Intents** keep `component` and `extras` in `HostObj::Intent`, the
//! layout launch and MCP rely on. Action, data, type, flags, package and
//! categories live in `Engine.intent_meta`.
//!
//! Like AOSP, a value is returned as stored, without type coercion. AOSP
//! logs a ClassCastException warning and returns the default on a type
//! mismatch; apps don't rely on that path.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::value::Value;

#[derive(Debug, Default, Clone)]
pub(crate) struct IntentMeta {
    pub action: Option<Value>,
    pub data: Option<Value>,
    pub mime: Option<Value>,
    pub package: Option<Value>,
    pub flags: i32,
    pub categories: Vec<Value>,
    pub component_obj: Option<Value>,
    pub identifier: Option<Value>,
    pub selector: Option<Value>,
    // Flattened Rect text, kept only for toUri/parseUri round-trips;
    // getSourceBounds itself is not modelled.
    pub source_bounds: Option<String>,
}

fn is_bundle_class(c: &str) -> bool {
    matches!(
        c,
        "Landroid/os/Bundle;" | "Landroid/os/BaseBundle;" | "Landroid/os/PersistableBundle;"
    )
}

fn key_of(vm: &Engine, v: Option<&Value>) -> Result<Option<String>, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => Some(vm.heap.as_str(*r)?.to_string()),
        // Bundle allows a null key
        _ => None,
    })
}

const NULL_KEY: &str = "\u{0}rd-null-key";

fn ret_of(sig: &str) -> &str {
    sig.rsplit_once(')').map_or("V", |(_, r)| r)
}

fn params_of(sig: &str) -> Vec<String> {
    let inner = sig.trim_start_matches('(').split(')').next().unwrap_or("");
    let mut out = Vec::new();
    let b = inner.as_bytes();
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

fn typed_default(ret: &str) -> Value {
    match ret {
        "J" => Value::Long(0),
        "F" => Value::Float(0.0),
        "D" => Value::Double(0.0),
        "I" | "Z" | "S" | "B" | "C" => Value::Int(0),
        _ => Value::Null,
    }
}

fn map_mut(vm: &mut Engine, r: ObjRef) -> Option<&mut Vec<(String, Value)>> {
    match vm.fw.objects.get_mut(&r) {
        Some(HostObj::Bundle { map }) => Some(map),
        _ => None,
    }
}

fn map_of(vm: &Engine, r: ObjRef) -> Option<Vec<(String, Value)>> {
    match vm.fw.get(r) {
        Some(HostObj::Bundle { map }) => Some(map.clone()),
        _ => None,
    }
}

fn store(list: &mut Vec<(String, Value)>, k: String, v: Value) {
    if let Some(e) = list.iter_mut().find(|(x, _)| *x == k) {
        e.1 = v;
    } else {
        list.push((k, v));
    }
}

fn new_string_set(vm: &mut Engine, keys: Vec<String>) -> Result<Value, VmExit> {
    let mut elems = Vec::new();
    for k in keys {
        elems.push(Value::Obj(crate::intrinsics::alloc_string(vm, k)?));
    }
    let set = vm
        .heap
        .alloc_instance("Ljava/util/HashSet;".to_string(), Vec::new())?;
    vm.fw.objects.insert(set, HostObj::List { elems });
    Ok(Value::Obj(set))
}

/// `BaseBundle.dumpStats(IndentingPrintWriter, BaseBundle)`: report, one
/// indentation level in, every entry whose `writeValue` size exceeds
/// AOSP's 1 KiB threshold as `key [size=N]`, recursing into nested
/// Bundles; a null bundle prints `[null]`. The writer is a guest object;
/// its indent/println methods are invoked virtually.
fn dump_stats(vm: &mut Engine, writer: ObjRef, bundle: Option<ObjRef>) -> Result<(), VmExit> {
    const INDENTING_WRITER: &str = "Lcom/android/internal/util/IndentingPrintWriter;";
    vm.invoke_instance(
        writer,
        "increaseIndent",
        &format!("(){INDENTING_WRITER}"),
        &[],
    )?;
    match bundle {
        None => {
            let line = crate::intrinsics::alloc_string(vm, "[null]".to_string())?;
            vm.invoke_instance(
                writer,
                "println",
                "(Ljava/lang/String;)V",
                &[Value::Obj(line)],
            )?;
        }
        Some(bundle) => {
            // Iterating the entries materializes a parcelled Bundle, as in
            // AOSP (size/entry access goes through `unparcel`).
            crate::parcel::unparcel_bundle(vm, bundle)?;
            for (key, value) in map_of(vm, bundle).unwrap_or_default() {
                let size = crate::parcel::value_size(vm, &value)?;
                let nested = match &value {
                    Value::Obj(o)
                        if matches!(vm.fw.get(*o), Some(HostObj::Bundle { .. }))
                            || crate::parcel::parcelled_data(vm, *o).is_some() =>
                    {
                        Some(*o)
                    }
                    _ => None,
                };
                if size > 1024 {
                    let key = if key == NULL_KEY { "null" } else { &key };
                    let line = crate::intrinsics::alloc_string(vm, format!("{key} [size={size}]"))?;
                    vm.invoke_instance(
                        writer,
                        "println",
                        "(Ljava/lang/String;)V",
                        &[Value::Obj(line)],
                    )?;
                }
                if let Some(nested) = nested {
                    dump_stats(vm, writer, Some(nested))?;
                }
            }
        }
    }
    vm.invoke_instance(
        writer,
        "decreaseIndent",
        &format!("(){INDENTING_WRITER}"),
        &[],
    )?;
    Ok(())
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class == "Landroid/content/Intent;"
        && name == "parseUri"
        && sig == "(Ljava/lang/String;I)Landroid/content/Intent;"
    {
        let Some(uri) = value_text(vm, &args.first().cloned().unwrap_or(Value::Null)) else {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/NullPointerException;",
                "uri is null",
            )));
        };
        let flags = match args.get(1) {
            Some(Value::Int(v)) => *v,
            _ => 0,
        };
        let intent = parse_uri_internal(vm, &uri, flags)?;
        return Ok(Some(Value::Obj(intent)));
    }
    if !is_bundle_class(class)
        || name != "dumpStats"
        || sig != "(Lcom/android/internal/util/IndentingPrintWriter;Landroid/os/BaseBundle;)V"
    {
        return Ok(None);
    }
    let Some(Value::Obj(writer)) = args.first().cloned() else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "BaseBundle.dumpStats: writer is null",
        )));
    };
    let bundle = match args.get(1) {
        Some(Value::Obj(o)) => Some(*o),
        _ => None,
    };
    dump_stats(vm, writer, bundle)?;
    Ok(Some(Value::Null))
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if is_bundle_class(mref_class) {
        return bundle(vm, name, sig, recv, args);
    }
    if mref_class == "Landroid/content/Intent;" {
        return intent(vm, name, sig, recv, args);
    }
    Ok(None)
}

/// Shared typed get/put over a key/value list. Returns `None` when the
/// method isn't a typed accessor.
fn typed_access(
    vm: &mut Engine,
    list_owner: ObjRef,
    is_intent: bool,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let params = params_of(sig);
    let ret = ret_of(sig).to_string();
    let first_is_key = params.first().map(String::as_str) == Some("Ljava/lang/String;");
    if !first_is_key {
        return Ok(None);
    }
    let key = key_of(vm, args.first())?.unwrap_or_else(|| NULL_KEY.to_string());
    let read = |vm: &Engine| -> Option<Value> {
        let list = if is_intent {
            match vm.fw.get(list_owner) {
                Some(HostObj::Intent { extras, .. }) => extras.clone(),
                _ => Vec::new(),
            }
        } else {
            map_of(vm, list_owner).unwrap_or_default()
        };
        list.into_iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    };
    let is_put = if is_intent {
        name == "putExtra"
    } else {
        name.starts_with("put") && name != "putAll"
    };
    if is_put && params.len() == 2 {
        let v = args.get(1).cloned().unwrap_or(Value::Null);
        // AOSP keeps boxed values in the map: `get(key)` must return an
        // object (Integer/Long/…), and typed getters unbox it
        let v = match wrapper_of(&params[1]) {
            Some(w) => Value::Obj(crate::boxes::box_value(vm, w, v)?),
            None => v,
        };
        if is_intent {
            if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&list_owner) {
                store(extras, key, v);
            }
            return Ok(Some(Value::Obj(list_owner))); // Intent builder returns this
        }
        if let Some(m) = map_mut(vm, list_owner) {
            store(m, key, v);
        }
        return Ok(Some(Value::Null));
    }
    let is_get = if is_intent {
        name.starts_with("get") && name.ends_with("Extra")
    } else {
        name.starts_with("get")
    };
    if is_get {
        // API-33 class-filtered getters: `getParcelable(key, Class)`,
        // `getParcelableArray/List`, `getSparseParcelableArray`,
        // `getSerializable(key, Class)` (and the Intent equivalents).
        if params.len() == 2 && params[1] == "Ljava/lang/Class;" {
            let stored = read(vm);
            return Ok(Some(class_filtered_get(vm, name, args, stored)?));
        }
        let cur = read(vm).map(|v| coerce_stored(vm, v, &ret)).transpose()?;
        return Ok(Some(match (cur, params.len()) {
            (Some(v), _) => v,
            // getX(key, default)
            (None, 2) => args[1].clone(),
            (None, _) => typed_default(&ret),
        }));
    }
    Ok(None)
}

/// The descriptor a `java.lang.Class` object stands for (the canonical
/// Class object stores it in the `class_name` heap field).
fn class_desc_of(vm: &Engine, class_obj: ObjRef) -> Option<String> {
    match vm.heap.get_field(class_obj, "class_name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}

fn is_instance(vm: &Engine, value: &Value, target: &str) -> bool {
    match value {
        Value::Null => true,
        Value::Obj(o) => match vm.heap.class_of(*o) {
            Ok(class) => class == target || vm.cp.is_subtype(class, target),
            Err(_) => false,
        },
        _ => false,
    }
}

/// AOSP's API-33 typed getters: the stored value comes back only if it is
/// an instance of the supplied `Class` (element-wise for arrays, lists and
/// sparse arrays, matching the filter AOSP applies while materializing
/// lazy values). Any mismatch yields null (AOSP logs a type warning —
/// unobservable to apps). A null `Class` is rejected exactly where AOSP
/// calls `Objects.requireNonNull`.
fn class_filtered_get(
    vm: &mut Engine,
    name: &str,
    args: &[Value],
    stored: Option<Value>,
) -> Result<Value, VmExit> {
    let base = name.strip_suffix("Extra").unwrap_or(name);
    let container = matches!(
        base,
        "getParcelableArray"
            | "getParcelableArrayList"
            | "getSparseParcelableArray"
            | "getArrayList"
    );
    let class_obj = match args.get(1) {
        Some(Value::Obj(o)) => Some(*o),
        _ => None,
    };
    let Some(class_obj) = class_obj else {
        if container {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/NullPointerException;",
                format!("{name}: clazz is null"),
            )));
        }
        return Ok(stored.unwrap_or(Value::Null));
    };
    let stored = stored.unwrap_or(Value::Null);
    if stored == Value::Null {
        return Ok(Value::Null);
    }
    let Some(target) = class_desc_of(vm, class_obj) else {
        return Ok(stored);
    };
    let Value::Obj(o) = stored else {
        return Ok(Value::Null);
    };
    let matches = match base {
        "getParcelableArray" => match vm.heap.as_array_elems(o) {
            Ok((elems, _)) => {
                let elems: Vec<Value> = elems.to_vec();
                elems.iter().all(|v| is_instance(vm, v, &target))
            }
            Err(_) => false,
        },
        "getParcelableArrayList" | "getArrayList" => {
            let size = vm
                .invoke_instance(o, "size", "()I", &[])?
                .as_int()
                .unwrap_or(-1);
            let mut ok = size >= 0;
            let mut i = 0;
            while ok && i < size {
                let item =
                    vm.invoke_instance(o, "get", "(I)Ljava/lang/Object;", &[Value::Int(i)])?;
                ok = is_instance(vm, &item, &target);
                i += 1;
            }
            ok
        }
        "getSparseParcelableArray" => {
            let size = vm
                .invoke_instance(o, "size", "()I", &[])?
                .as_int()
                .unwrap_or(-1);
            let mut ok = size >= 0;
            let mut i = 0;
            while ok && i < size {
                let item =
                    vm.invoke_instance(o, "valueAt", "(I)Ljava/lang/Object;", &[Value::Int(i)])?;
                ok = is_instance(vm, &item, &target);
                i += 1;
            }
            ok
        }
        _ => is_instance(vm, &stored, &target),
    };
    Ok(if matches { stored } else { Value::Null })
}

/// Wrapper class for a primitive descriptor.
fn wrapper_of(desc: &str) -> Option<&'static str> {
    Some(match desc {
        "I" => "Ljava/lang/Integer;",
        "J" => "Ljava/lang/Long;",
        "Z" => "Ljava/lang/Boolean;",
        "F" => "Ljava/lang/Float;",
        "D" => "Ljava/lang/Double;",
        "S" => "Ljava/lang/Short;",
        "B" => "Ljava/lang/Byte;",
        "C" => "Ljava/lang/Character;",
        _ => return None,
    })
}

/// A stored map value as the getter's declared return type: primitive
/// getters unbox (and widen, e.g. getLong over an int), object getters box
/// raw primitives stored by host code.
fn coerce_stored(vm: &mut Engine, v: Value, ret: &str) -> Result<Value, VmExit> {
    let prim = match &v {
        Value::Obj(o) if wrapper_of(ret).is_some() => {
            let cls = vm.heap.class_of(*o)?.to_string();
            if cls.starts_with("Ljava/lang/") {
                vm.heap.get_field(*o, "value").unwrap_or(v.clone())
            } else {
                v.clone()
            }
        }
        _ => v.clone(),
    };
    Ok(match ret {
        "J" => Value::Long(match prim {
            Value::Int(i) => i as i64,
            other => other.as_long().unwrap_or(0),
        }),
        "I" | "Z" | "S" | "B" | "C" => Value::Int(prim.as_int().unwrap_or(0)),
        "F" => Value::Float(prim.as_float().unwrap_or(0.0)),
        "D" => Value::Double(prim.as_double().unwrap_or(0.0)),
        "Ljava/lang/Object;"
        | "Ljava/lang/Number;"
        | "Ljava/io/Serializable;"
        | "Ljava/lang/Integer;"
        | "Ljava/lang/Long;"
        | "Ljava/lang/Float;"
        | "Ljava/lang/Double;" => match v {
            Value::Int(i) => Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Integer;",
                Value::Int(i),
            )?),
            Value::Long(l) => Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Long;",
                Value::Long(l),
            )?),
            Value::Float(f) => Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Float;",
                Value::Float(f),
            )?),
            Value::Double(d) => Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Double;",
                Value::Double(d),
            )?),
            other => other,
        },
        _ => v,
    })
}

/// `BundleProto.PARCELLED_DATA_SIZE` (field 1, int32): written by
/// `BaseBundle.dumpDebug` for a still-parcelled Bundle.
const PROTO_PARCELLED_DATA_SIZE: i64 = 1_120_986_464_257;
/// `BundleProto.MAP_DATA` (field 2, string): written by
/// `BaseBundle.dumpDebug` for a materialized Bundle.
const PROTO_MAP_DATA: i64 = 1_138_166_333_442;

/// A Bundle value the way `ArrayMap.toString` renders it inside
/// `dumpDebug`'s map string.
fn value_string(vm: &mut Engine, v: &Value) -> Result<String, VmExit> {
    Ok(match v {
        Value::Null => "null".to_string(),
        Value::Obj(o) => crate::intrinsics::object_to_string(vm, *o)?,
        Value::Int(i) => i.to_string(),
        Value::Long(l) => l.to_string(),
        Value::Float(f) => crate::repr::java_float(*f),
        Value::Double(d) => crate::repr::java_double(*d),
        other => format!("{other:?}"),
    })
}

/// `BaseBundle.dumpDebug(ProtoOutputStream, long)`: start a nested proto
/// object, then write either the retained parcelled data size (without
/// materializing) or the materialized map as a string, and end with the
/// token `start` returned. The proto stream is a guest object: its
/// start/write/end methods are invoked virtually, as in AOSP.
fn dump_debug(vm: &mut Engine, recv: ObjRef, args: &[Value]) -> Result<Value, VmExit> {
    let Some(Value::Obj(proto)) = args.first().cloned() else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "Bundle.dumpDebug: proto is null",
        )));
    };
    let field_id = args.get(1).and_then(|v| v.as_long().ok()).unwrap_or(0);
    let token = vm.invoke_instance(proto, "start", "(J)J", &[Value::Long(field_id)])?;
    if let Some(payload) = crate::parcel::parcelled_payload(vm, recv) {
        vm.invoke_instance(
            proto,
            "write",
            "(JI)V",
            &[
                Value::Long(PROTO_PARCELLED_DATA_SIZE),
                Value::Int(payload.len() as i32),
            ],
        )?;
    } else {
        let entries = map_of(vm, recv).unwrap_or_default();
        let mut parts = Vec::with_capacity(entries.len());
        for (k, v) in &entries {
            let key = if k == NULL_KEY { "null" } else { k };
            parts.push(format!("{key}={}", value_string(vm, v)?));
        }
        let text = crate::intrinsics::alloc_string(vm, format!("{{{}}}", parts.join(", ")))?;
        vm.invoke_instance(
            proto,
            "write",
            "(JLjava/lang/String;)V",
            &[Value::Long(PROTO_MAP_DATA), Value::Obj(text)],
        )?;
    }
    vm.invoke_instance(proto, "end", "(J)V", &[token])?;
    Ok(Value::Null)
}

fn bundle(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if map_mut(vm, recv).is_none() {
        vm.fw
            .objects
            .insert(recv, HostObj::Bundle { map: Vec::new() });
    }
    // Parcelled-state observers that must not materialize the Bundle.
    match (name, sig) {
        ("isParcelled", "()Z") => {
            return Ok(Some(Value::Int(
                crate::parcel::parcelled_data(vm, recv).is_some() as i32,
            )))
        }
        ("dumpDebug", "(Landroid/util/proto/ProtoOutputStream;J)V") => {
            return Ok(Some(dump_debug(vm, recv, args)?))
        }
        _ => {}
    }
    // Everything else observes or mutates the map, except the two parcel
    // transfers: `writeToParcel` appends retained data verbatim and
    // `readFromParcel` adopts a new retained frame.
    if !matches!(name, "writeToParcel" | "readFromParcel") {
        crate::parcel::unparcel_bundle(vm, recv)?;
    }
    match (name, sig) {
        ("<init>", "()V") | ("<init>", "(I)V") | ("<init>", "(Ljava/lang/ClassLoader;)V") => {
            return Ok(Some(Value::Null))
        }
        ("<init>", "(Landroid/os/Bundle;)V")
        | ("<init>", "(Landroid/os/PersistableBundle;)V")
        | ("putAll", "(Landroid/os/Bundle;)V")
        | ("putAll", "(Landroid/os/PersistableBundle;)V") => {
            // Copying from a parcelled source materializes the source
            // (AOSP `putAll` unparcels it to iterate its map).
            if let Some(Value::Obj(o)) = args.first() {
                crate::parcel::unparcel_bundle(vm, *o)?;
            }
            let src = match args.first() {
                Some(Value::Obj(o)) => map_of(vm, *o).unwrap_or_default(),
                _ => Vec::new(),
            };
            if let Some(m) = map_mut(vm, recv) {
                for (k, v) in src {
                    store(m, k, v);
                }
            }
            return Ok(Some(Value::Null));
        }
        ("setClassLoader", _) | ("setDefusable", _) => return Ok(Some(Value::Null)),
        ("getClassLoader", "()Ljava/lang/ClassLoader;") => return Ok(Some(Value::Null)),
        ("describeContents", "()I") => return Ok(Some(Value::Int(0))),
        ("writeToParcel", "(Landroid/os/Parcel;I)V") => {
            let Some(Value::Obj(parcel)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Bundle.writeToParcel: parcel is null",
                )));
            };
            crate::parcel::write_bundle_map(vm, *parcel, recv)?;
            return Ok(Some(Value::Null));
        }
        ("readFromParcel", "(Landroid/os/Parcel;)V") => {
            let Some(Value::Obj(parcel)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Bundle.readFromParcel: parcel is null",
                )));
            };
            // AOSP `readFromParcelInner` adopts the frame as retained
            // parcelled data; the map materializes on first access.
            crate::parcel::adopt_bundle_frame(vm, *parcel, recv)?;
            return Ok(Some(Value::Null));
        }
        ("size", "()I") => {
            return Ok(Some(Value::Int(
                map_of(vm, recv).map_or(0, |m| m.len()) as i32
            )))
        }
        ("isEmpty", "()Z") => {
            return Ok(Some(Value::Int(
                map_of(vm, recv).map_or(true, |m| m.is_empty()) as i32,
            )))
        }
        ("clear", "()V") => {
            if let Some(m) = map_mut(vm, recv) {
                m.clear();
            }
            return Ok(Some(Value::Null));
        }
        ("containsKey", "(Ljava/lang/String;)Z") => {
            let k = key_of(vm, args.first())?.unwrap_or_else(|| NULL_KEY.to_string());
            let has = map_of(vm, recv).is_some_and(|m| m.iter().any(|(x, _)| *x == k));
            return Ok(Some(Value::Int(has as i32)));
        }
        ("remove", "(Ljava/lang/String;)V") => {
            let k = key_of(vm, args.first())?.unwrap_or_else(|| NULL_KEY.to_string());
            if let Some(m) = map_mut(vm, recv) {
                m.retain(|(x, _)| *x != k);
            }
            return Ok(Some(Value::Null));
        }
        ("keySet", "()Ljava/util/Set;") => {
            let keys = map_of(vm, recv)
                .unwrap_or_default()
                .into_iter()
                .map(|(k, _)| k)
                .filter(|k| k != NULL_KEY)
                .collect();
            return Ok(Some(new_string_set(vm, keys)?));
        }
        ("deepCopy", _) | ("clone", "()Ljava/lang/Object;") => {
            let src = map_of(vm, recv).unwrap_or_default();
            let class = vm.heap.class_of(recv)?.to_string();
            let b = vm.heap.alloc_instance(class, Vec::new())?;
            vm.fw.objects.insert(b, HostObj::Bundle { map: src });
            return Ok(Some(Value::Obj(b)));
        }
        ("toString", "()Ljava/lang/String;") => {
            let n = map_of(vm, recv).map_or(0, |m| m.len());
            let s = crate::intrinsics::alloc_string(vm, format!("Bundle[{n} keys]"))?;
            return Ok(Some(Value::Obj(s)));
        }
        _ => {}
    }
    typed_access(vm, recv, false, name, sig, args)
}

fn intent_meta(vm: &mut Engine, r: ObjRef) -> &mut IntentMeta {
    vm.intent_meta.entry(r).or_default()
}

fn intent(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !matches!(vm.fw.get(recv), Some(HostObj::Intent { .. })) {
        vm.fw.objects.insert(
            recv,
            HostObj::Intent {
                component: None,
                extras: Vec::new(),
            },
        );
    }
    let a0 = args.first().cloned().unwrap_or(Value::Null);
    let this = Some(Value::Obj(recv));
    Ok(match (name, sig) {
        ("<init>", "()V") => Some(Value::Null),
        ("<init>", "(Ljava/lang/String;)V") => {
            intent_meta(vm, recv).action = Some(a0);
            Some(Value::Null)
        }
        ("<init>", "(Ljava/lang/String;Landroid/net/Uri;)V") => {
            let m = intent_meta(vm, recv);
            m.action = Some(a0);
            m.data = args.get(1).cloned();
            Some(Value::Null)
        }
        ("<init>", "(Landroid/content/Intent;)V") => {
            if let Some(Value::Obj(src)) = args.first() {
                let (c, e) = match vm.fw.get(*src) {
                    Some(HostObj::Intent { component, extras }) => {
                        (component.clone(), extras.clone())
                    }
                    _ => (None, Vec::new()),
                };
                vm.fw.objects.insert(
                    recv,
                    HostObj::Intent {
                        component: c,
                        extras: e,
                    },
                );
                let meta = vm.intent_meta.get(src).cloned().unwrap_or_default();
                vm.intent_meta.insert(recv, meta);
            }
            Some(Value::Null)
        }
        ("getExtras", "()Landroid/os/Bundle;") => {
            let extras = match vm.fw.get(recv) {
                Some(HostObj::Intent { extras, .. }) => extras.clone(),
                _ => Vec::new(),
            };
            if extras.is_empty() {
                Some(Value::Null) // AOSP: null when no extras were ever put
            } else {
                let b = vm
                    .heap
                    .alloc_instance("Landroid/os/Bundle;".to_string(), Vec::new())?;
                vm.fw.objects.insert(b, HostObj::Bundle { map: extras });
                Some(Value::Obj(b))
            }
        }
        ("putExtras", "(Landroid/os/Bundle;)Landroid/content/Intent;") => {
            let src = match args.first() {
                Some(Value::Obj(o)) => map_of(vm, *o).unwrap_or_default(),
                _ => Vec::new(),
            };
            if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&recv) {
                for (k, v) in src {
                    store(extras, k, v);
                }
            }
            this
        }
        ("hasExtra", "(Ljava/lang/String;)Z") => {
            let k = key_of(vm, args.first())?.unwrap_or_default();
            let has = matches!(vm.fw.get(recv), Some(HostObj::Intent { extras, .. }) if extras.iter().any(|(x, _)| *x == k));
            Some(Value::Int(has as i32))
        }
        ("removeExtra", "(Ljava/lang/String;)V") => {
            let k = key_of(vm, args.first())?.unwrap_or_default();
            if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&recv) {
                extras.retain(|(x, _)| *x != k);
            }
            Some(Value::Null)
        }
        ("setAction", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).action = Some(a0);
            this
        }
        ("getAction", "()Ljava/lang/String;") => {
            Some(intent_meta(vm, recv).action.clone().unwrap_or(Value::Null))
        }
        ("setData", "(Landroid/net/Uri;)Landroid/content/Intent;") => {
            let m = intent_meta(vm, recv);
            m.data = Some(a0);
            m.mime = None; // AOSP: setData clears the type
            this
        }
        ("getData", "()Landroid/net/Uri;") => {
            Some(intent_meta(vm, recv).data.clone().unwrap_or(Value::Null))
        }
        ("setType", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            let m = intent_meta(vm, recv);
            m.mime = Some(a0);
            m.data = None; // AOSP: setType clears the data
            this
        }
        ("setDataAndType", "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;") => {
            let m = intent_meta(vm, recv);
            m.data = Some(a0);
            m.mime = args.get(1).cloned();
            this
        }
        ("getType", "()Ljava/lang/String;") => {
            Some(intent_meta(vm, recv).mime.clone().unwrap_or(Value::Null))
        }
        ("setPackage", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).package = Some(a0);
            this
        }
        ("getPackage", "()Ljava/lang/String;") => {
            Some(intent_meta(vm, recv).package.clone().unwrap_or(Value::Null))
        }
        ("addFlags", "(I)Landroid/content/Intent;") => {
            let f = a0.as_int()?;
            intent_meta(vm, recv).flags |= f;
            this
        }
        ("setFlags", "(I)Landroid/content/Intent;") => {
            let f = a0.as_int()?;
            intent_meta(vm, recv).flags = f;
            this
        }
        ("getFlags", "()I") => Some(Value::Int(intent_meta(vm, recv).flags)),
        // extras live in host bundles (no Parcel class-loading step)
        ("setExtrasClassLoader", "(Ljava/lang/ClassLoader;)V") => Some(Value::Null),
        ("addCategory", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).categories.push(a0);
            this
        }
        ("setComponent", "(Landroid/content/ComponentName;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).component_obj = Some(a0);
            this
        }
        ("getComponent", "()Landroid/content/ComponentName;") => Some(
            intent_meta(vm, recv)
                .component_obj
                .clone()
                .unwrap_or(Value::Null),
        ),
        ("toUri", "(I)Ljava/lang/String;") => {
            let flags = match args.first() {
                Some(Value::Int(v)) => *v,
                _ => 0,
            };
            let text = intent_to_uri(vm, recv, flags)?;
            Some(guest_string(vm, text)?)
        }
        ("toURI", "()Ljava/lang/String;") => {
            let text = intent_to_uri(vm, recv, 0)?;
            Some(guest_string(vm, text)?)
        }
        ("setSelector", "(Landroid/content/Intent;)V") => {
            intent_meta(vm, recv).selector = Some(a0);
            Some(Value::Null)
        }
        ("getSelector", "()Landroid/content/Intent;") => Some(
            intent_meta(vm, recv)
                .selector
                .clone()
                .unwrap_or(Value::Null),
        ),
        ("setIdentifier", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).identifier = Some(a0);
            this
        }
        ("getIdentifier", "()Ljava/lang/String;") => Some(
            intent_meta(vm, recv)
                .identifier
                .clone()
                .unwrap_or(Value::Null),
        ),
        ("setClassName", "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;")
        | (
            "setClassName",
            "(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;",
        ) => {
            if let Some(Value::Obj(c)) = args.get(1) {
                let dotted = vm.heap.as_str(*c)?.to_string();
                let desc = format!("L{};", dotted.replace('.', "/"));
                if let Some(HostObj::Intent { component, .. }) = vm.fw.objects.get_mut(&recv) {
                    *component = Some(desc);
                }
            }
            this
        }
        _ => {
            if name == "putExtra" || (name.starts_with("get") && name.ends_with("Extra")) {
                return typed_access(vm, recv, true, name, sig, args);
            }
            if name.ends_with("Extra") && args.first().is_some_and(|v| matches!(v, Value::Null)) {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Intent extra key is null",
                )));
            }
            None
        }
    })
}

// ---- Intent.toUri / Intent.parseUri (AOSP 14 Intent.java) ------------------

const ACTION_VIEW: &str = "android.intent.action.VIEW";
const ACTION_MAIN: &str = "android.intent.action.MAIN";
// FLAG_GRANT_READ_URI_PERMISSION | WRITE | PERSISTABLE | PREFIX
const IMMUTABLE_FLAGS: i32 = 0x1 | 0x2 | 0x40 | 0x80;

fn uri_syntax(msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/net/URISyntaxException;", msg.into()))
}

fn number_format(msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/NumberFormatException;",
        msg.into(),
    ))
}

fn new_intent_obj(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Landroid/content/Intent;".to_string(), Vec::new())?;
    vm.fw.objects.insert(
        r,
        HostObj::Intent {
            component: None,
            extras: Vec::new(),
        },
    );
    vm.intent_meta.insert(r, IntentMeta::default());
    Ok(r)
}

fn guest_string(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

fn value_text(vm: &Engine, v: &Value) -> Option<String> {
    match v {
        Value::Obj(r) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

fn intent_extras_of(vm: &Engine, r: ObjRef) -> Vec<(String, Value)> {
    match vm.fw.get(r) {
        Some(HostObj::Intent { extras, .. }) => extras.clone(),
        _ => Vec::new(),
    }
}

// Object.toString() for an extras value, via boxing for scalar values.
fn extra_value_text(vm: &mut Engine, v: &Value) -> Result<String, VmExit> {
    let obj = match v {
        Value::Obj(r) => *r,
        Value::Int(i) => return Ok(i.to_string()),
        Value::Long(l) => return Ok(l.to_string()),
        Value::Float(_) => crate::boxes::box_value(vm, "Ljava/lang/Float;", v.clone())?,
        Value::Double(_) => crate::boxes::box_value(vm, "Ljava/lang/Double;", v.clone())?,
        _ => return Ok(String::new()),
    };
    let text = vm.invoke_instance(obj, "toString", "()Ljava/lang/String;", &[])?;
    Ok(value_text(vm, &text).unwrap_or_default())
}

fn extra_entry_type(vm: &Engine, v: &Value) -> Option<char> {
    Some(match v {
        Value::Int(_) => 'i',
        Value::Long(_) => 'l',
        Value::Float(_) => 'f',
        Value::Double(_) => 'd',
        Value::Obj(r) => match vm.heap.class_of(*r) {
            Ok("Ljava/lang/String;") => 'S',
            Ok("Ljava/lang/Boolean;") => 'B',
            Ok("Ljava/lang/Byte;") => 'b',
            Ok("Ljava/lang/Character;") => 'c',
            Ok("Ljava/lang/Double;") => 'd',
            Ok("Ljava/lang/Float;") => 'f',
            Ok("Ljava/lang/Integer;") => 'i',
            Ok("Ljava/lang/Long;") => 'l',
            Ok("Ljava/lang/Short;") => 's',
            _ => return None,
        },
        _ => return None,
    })
}

fn to_uri_inner(
    vm: &mut Engine,
    r: ObjRef,
    scheme: Option<&str>,
    def_action: Option<&str>,
    def_package: Option<&str>,
) -> Result<String, VmExit> {
    let meta = intent_meta(vm, r).clone();
    let mut frag = String::new();
    if let Some(scheme) = scheme {
        frag.push_str(&format!(
            "scheme={};",
            crate::android_uri::encode(scheme, "")
        ));
    }
    if let Some(action) = meta.action.as_ref().and_then(|v| value_text(vm, v)) {
        if Some(action.as_str()) != def_action {
            frag.push_str(&format!(
                "action={};",
                crate::android_uri::encode(&action, "")
            ));
        }
    }
    for cat in &meta.categories {
        if let Some(c) = value_text(vm, cat) {
            frag.push_str(&format!("category={};", crate::android_uri::encode(&c, "")));
        }
    }
    if let Some(mime) = meta.mime.as_ref().and_then(|v| value_text(vm, v)) {
        frag.push_str(&format!("type={};", crate::android_uri::encode(&mime, "/")));
    }
    if let Some(identifier) = meta.identifier.as_ref().and_then(|v| value_text(vm, v)) {
        frag.push_str(&format!(
            "identifier={};",
            crate::android_uri::encode(&identifier, "/")
        ));
    }
    if meta.flags != 0 {
        frag.push_str(&format!("launchFlags=0x{:x};", meta.flags));
    }
    if let Some(package) = meta.package.as_ref().and_then(|v| value_text(vm, v)) {
        if Some(package.as_str()) != def_package {
            frag.push_str(&format!(
                "package={};",
                crate::android_uri::encode(&package, "")
            ));
        }
    }
    if let Some(Value::Obj(component)) = meta.component_obj {
        let flat = vm.invoke_instance(
            component,
            "flattenToShortString",
            "()Ljava/lang/String;",
            &[],
        )?;
        let flat = value_text(vm, &flat).unwrap_or_default();
        frag.push_str(&format!(
            "component={};",
            crate::android_uri::encode(&flat, "/")
        ));
    }
    if let Some(bounds) = &meta.source_bounds {
        frag.push_str(&format!(
            "sourceBounds={};",
            crate::android_uri::encode(bounds, "")
        ));
    }
    for (key, value) in intent_extras_of(vm, r) {
        if let Some(entry_type) = extra_entry_type(vm, &value) {
            let text = extra_value_text(vm, &value)?;
            frag.push(entry_type);
            frag.push('.');
            frag.push_str(&crate::android_uri::encode(&key, ""));
            frag.push('=');
            frag.push_str(&crate::android_uri::encode(&text, ""));
            frag.push(';');
        }
    }
    Ok(frag)
}

fn to_uri_fragment(
    vm: &mut Engine,
    r: ObjRef,
    scheme: Option<&str>,
    def_action: Option<&str>,
    def_package: Option<&str>,
) -> Result<String, VmExit> {
    let mut frag = to_uri_inner(vm, r, scheme, def_action, def_package)?;
    let selector = intent_meta(vm, r).selector.clone();
    if let Some(Value::Obj(sel)) = selector {
        frag.push_str("SEL;");
        let sel_scheme = match intent_meta(vm, sel).data.clone() {
            Some(Value::Obj(data)) => {
                let s = vm.invoke_instance(data, "getScheme", "()Ljava/lang/String;", &[])?;
                value_text(vm, &s)
            }
            _ => None,
        };
        frag.push_str(&to_uri_inner(vm, sel, sel_scheme.as_deref(), None, None)?);
    }
    if frag.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("#Intent;{frag}end"))
    }
}

fn uri_part(vm: &mut Engine, data: ObjRef, getter: &str) -> Result<Option<String>, VmExit> {
    let v = vm.invoke_instance(data, getter, "()Ljava/lang/String;", &[])?;
    Ok(value_text(vm, &v))
}

fn intent_to_uri(vm: &mut Engine, recv: ObjRef, flags: i32) -> Result<String, VmExit> {
    let meta = intent_meta(vm, recv).clone();
    let mut uri = String::new();
    if flags & 2 != 0 {
        // URI_ANDROID_APP_SCHEME
        let package = match meta.package.as_ref().and_then(|v| value_text(vm, v)) {
            Some(p) => p,
            None => {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "Intent must include an explicit package name to build an android-app: "
                        .to_string(),
                )));
            }
        };
        uri.push_str("android-app://");
        uri.push_str(&package);
        let mut scheme: Option<String> = None;
        if let Some(Value::Obj(data)) = meta.data {
            scheme = uri_part(vm, data, "getScheme")?;
            if let Some(s) = &scheme {
                uri.push('/');
                uri.push_str(s);
                if let Some(authority) = uri_part(vm, data, "getEncodedAuthority")? {
                    uri.push('/');
                    uri.push_str(&authority);
                    if let Some(path) = uri_part(vm, data, "getEncodedPath")? {
                        uri.push_str(&path);
                    }
                    if let Some(query) = uri_part(vm, data, "getEncodedQuery")? {
                        uri.push('?');
                        uri.push_str(&query);
                    }
                    if let Some(fragment) = uri_part(vm, data, "getEncodedFragment")? {
                        uri.push('#');
                        uri.push_str(&fragment);
                    }
                }
            }
        }
        let def_action = if scheme.is_none() {
            ACTION_MAIN
        } else {
            ACTION_VIEW
        };
        uri.push_str(&to_uri_fragment(
            vm,
            recv,
            None,
            Some(def_action),
            Some(&package),
        )?);
        return Ok(uri);
    }
    let mut scheme: Option<String> = None;
    if let Some(Value::Obj(data)) = meta.data {
        let data_value = vm.invoke_instance(data, "toString", "()Ljava/lang/String;", &[])?;
        let mut data = value_text(vm, &data_value).unwrap_or_default();
        if flags & 1 != 0 {
            // URI_INTENT_SCHEME: extract a leading valid scheme.
            let chars: Vec<char> = data.chars().collect();
            for (idx, c) in chars.iter().enumerate() {
                if c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '+' {
                    continue;
                }
                if *c == ':' && idx > 0 {
                    scheme = Some(chars[..idx].iter().collect());
                    uri.push_str("intent:");
                    data = chars[idx + 1..].iter().collect();
                }
                break;
            }
        }
        uri.push_str(&data);
    } else if flags & 1 != 0 {
        uri.push_str("intent:");
    }
    uri.push_str(&to_uri_fragment(
        vm,
        recv,
        scheme.as_deref(),
        Some(ACTION_VIEW),
        None,
    )?);
    Ok(uri)
}

// java.lang.Integer.decode
fn integer_decode(s: &str) -> Result<i32, VmExit> {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, s.strip_prefix('+').unwrap_or(s)),
    };
    let (radix, digits) =
        if let Some(r) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            (16, r)
        } else if let Some(r) = rest.strip_prefix('#') {
            (16, r)
        } else if rest.len() > 1 && rest.starts_with('0') {
            (8, &rest[1..])
        } else {
            (10, rest)
        };
    if digits.is_empty() {
        return Err(number_format(format!("Zero length string at \"{s}\"")));
    }
    let magnitude = i64::from_str_radix(digits, radix)
        .map_err(|_| number_format(format!("For input string: \"{s}\" under radix {radix}")))?;
    let value = sign * magnitude;
    i32::try_from(value).map_err(|_| number_format(format!("For input string: \"{s}\"")))
}

struct UriChars {
    chars: Vec<char>,
}

impl UriChars {
    fn new(s: &str) -> Self {
        Self {
            chars: s.chars().collect(),
        }
    }
    fn starts_at(&self, i: i32, pat: &str) -> bool {
        if i < 0 {
            return false;
        }
        let pat: Vec<char> = pat.chars().collect();
        let i = i as usize;
        i + pat.len() <= self.chars.len() && self.chars[i..i + pat.len()] == pat[..]
    }
    fn index_of(&self, ch: char, from: i32) -> i32 {
        if from < 0 {
            return -1;
        }
        self.chars[from as usize..]
            .iter()
            .position(|c| *c == ch)
            .map(|p| from + p as i32)
            .unwrap_or(-1)
    }
    fn last_index_of(&self, ch: char) -> i32 {
        self.chars
            .iter()
            .rposition(|c| *c == ch)
            .map(|p| p as i32)
            .unwrap_or(-1)
    }
    fn sub(&self, a: i32, b: i32) -> Result<String, VmExit> {
        if a < 0 || b < a || b as usize > self.chars.len() {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        Ok(self.chars[a as usize..b as usize].iter().collect())
    }
    fn char_at(&self, i: i32) -> Result<char, VmExit> {
        self.chars
            .get(i as usize)
            .copied()
            .ok_or_else(|| uri_syntax("illegal Intent URI format"))
    }
}

fn uri_parse_guest(vm: &mut Engine, text: &str, whole_uri: &str) -> Result<Value, VmExit> {
    let arg = guest_string(vm, text.to_string())?;
    match vm.invoke_static(
        "Landroid/net/Uri;",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[arg],
    ) {
        Ok(v) => Ok(v),
        Err(VmExit::Exception(t)) if t.class == "Ljava/lang/IllegalArgumentException;" => {
            Err(uri_syntax(whole_uri.to_string()))
        }
        Err(e) => Err(e),
    }
}

fn put_typed_extra(
    vm: &mut Engine,
    extras: &mut Vec<(String, Value)>,
    type_char: char,
    key: String,
    value: &str,
    wrap_number_errors: bool,
    whole_uri: &str,
) -> Result<(), VmExit> {
    let num_err = |whole: &str, input: &str| -> VmExit {
        if wrap_number_errors {
            uri_syntax(whole.to_string())
        } else {
            number_format(format!("For input string: \"{input}\""))
        }
    };
    let boxed = match type_char {
        'S' => guest_string(vm, value.to_string())?,
        'B' => Value::Obj(crate::boxes::box_value(
            vm,
            "Ljava/lang/Boolean;",
            Value::Int(i32::from(value.eq_ignore_ascii_case("true"))),
        )?),
        'b' => {
            let n: i32 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            if !(-128..=127).contains(&n) {
                return Err(num_err(whole_uri, value));
            }
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Byte;",
                Value::Int(n),
            )?)
        }
        'c' => {
            let ch = crate::android_uri::decode(value, false)
                .chars()
                .next()
                .ok_or_else(|| uri_syntax("illegal Intent URI format"))?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Character;",
                Value::Int(ch as u32 as i32),
            )?)
        }
        'd' => {
            let n: f64 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Double;",
                Value::Double(n),
            )?)
        }
        'f' => {
            let n: f32 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Float;",
                Value::Float(n),
            )?)
        }
        'i' => {
            let n: i32 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Integer;",
                Value::Int(n),
            )?)
        }
        'l' => {
            let n: i64 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Long;",
                Value::Long(n),
            )?)
        }
        's' => {
            let n: i32 = value.parse().map_err(|_| num_err(whole_uri, value))?;
            if !(-32768..=32767).contains(&n) {
                return Err(num_err(whole_uri, value));
            }
            Value::Obj(crate::boxes::box_value(
                vm,
                "Ljava/lang/Short;",
                Value::Int(n),
            )?)
        }
        _ => return Err(uri_syntax("unknown EXTRA type")),
    };
    store(extras, key, boxed);
    Ok(())
}

fn parse_uri_old(vm: &mut Engine, uri: &str, flags: i32) -> Result<ObjRef, VmExit> {
    let u = UriChars::new(uri);
    let hash = u.last_index_of('#');
    if hash < 0 {
        let r = new_intent_obj(vm)?;
        intent_meta(vm, r).action = Some(guest_string(vm, ACTION_VIEW.to_string())?);
        let data = uri_parse_guest(vm, uri, uri)?;
        intent_meta(vm, r).data = Some(data);
        return Ok(r);
    }
    let fragment_start = hash;
    let mut is_fragment = false;
    let mut i = hash + 1;
    let mut action: Option<String> = None;
    let r = new_intent_obj(vm)?;
    if u.starts_at(i, "action(") {
        is_fragment = true;
        i += 7;
        let j = u.index_of(')', i);
        if j < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        action = Some(u.sub(i, j)?);
        i = j + 1;
    }
    if let Some(a) = &action {
        intent_meta(vm, r).action = Some(guest_string(vm, a.clone())?);
    }
    if u.starts_at(i, "categories(") {
        is_fragment = true;
        i += 11;
        let j = u.index_of(')', i);
        if j < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        let mut categories = Vec::new();
        while i < j {
            let mut sep = u.index_of('!', i);
            if sep < 0 || sep > j {
                sep = j;
            }
            if i < sep {
                categories.push(guest_string(vm, u.sub(i, sep)?)?);
            }
            i = sep + 1;
        }
        intent_meta(vm, r).categories = categories;
        i = j + 1;
    }
    if u.starts_at(i, "type(") {
        is_fragment = true;
        i += 5;
        let j = u.index_of(')', i);
        if j < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        let mime = u.sub(i, j)?;
        intent_meta(vm, r).mime = Some(guest_string(vm, mime)?);
        i = j + 1;
    }
    if u.starts_at(i, "launchFlags(") {
        is_fragment = true;
        i += 12;
        let j = u.index_of(')', i);
        if j < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        let mut parsed = integer_decode(&u.sub(i, j)?)?;
        if flags & 4 == 0 {
            parsed &= !IMMUTABLE_FLAGS;
        }
        intent_meta(vm, r).flags = parsed;
        i = j + 1;
    }
    if u.starts_at(i, "component(") {
        is_fragment = true;
        i += 10;
        let j = u.index_of(')', i);
        if j < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        let sep = u.index_of('!', i);
        if sep >= 0 && sep < j {
            let pkg = u.sub(i, sep)?;
            let cls = u.sub(sep + 1, j)?;
            let component = vm
                .heap
                .alloc_instance("Landroid/content/ComponentName;".to_string(), Vec::new())?;
            let pkg_v = guest_string(vm, pkg)?;
            let cls_v = guest_string(vm, cls)?;
            vm.invoke_instance(
                component,
                "<init>",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[pkg_v, cls_v],
            )?;
            intent_meta(vm, r).component_obj = Some(Value::Obj(component));
            if let Some(HostObj::Intent { component: c, .. }) = vm.fw.objects.get_mut(&r) {
                *c = None;
            }
        }
        i = j + 1;
    }
    if u.starts_at(i, "extras(") {
        is_fragment = true;
        i += 7;
        let close = u.index_of(')', i);
        if close == -1 {
            return Err(uri_syntax("EXTRA missing trailing ')'"));
        }
        let mut extras = intent_extras_of(vm, r);
        while i < close {
            let j = u.index_of('=', i);
            if j <= i + 1 || i >= close {
                return Err(uri_syntax("EXTRA missing '='"));
            }
            let type_char = u.char_at(i)?;
            i += 1;
            let key = u.sub(i, j)?;
            i = j + 1;
            let mut j = u.index_of('!', i);
            if j == -1 || j >= close {
                j = close;
            }
            if i >= j {
                return Err(uri_syntax("EXTRA missing '!'"));
            }
            let raw_value = u.sub(i, j)?;
            // AOSP decodes only 'S' and 'c' values in the old format.
            let value = if matches!(type_char, 'S' | 'c') {
                crate::android_uri::decode(&raw_value, false)
            } else {
                raw_value
            };
            i = j;
            put_typed_extra(vm, &mut extras, type_char, key, &value, true, uri)?;
            let ch = u.char_at(i)?;
            if ch == ')' {
                break;
            }
            if ch != '!' {
                return Err(uri_syntax("EXTRA missing '!'"));
            }
            i += 1;
        }
        if let Some(HostObj::Intent { extras: e, .. }) = vm.fw.objects.get_mut(&r) {
            *e = extras;
        }
    }
    let data_text = if is_fragment {
        u.sub(0, fragment_start)?
    } else {
        uri.to_string()
    };
    let data = uri_parse_guest(vm, &data_text, uri)?;
    intent_meta(vm, r).data = Some(data);
    if action.is_none() {
        intent_meta(vm, r).action = Some(guest_string(vm, ACTION_VIEW.to_string())?);
    }
    Ok(r)
}

fn parse_uri_internal(vm: &mut Engine, uri: &str, flags: i32) -> Result<ObjRef, VmExit> {
    let u = UriChars::new(uri);
    let android_app = uri.starts_with("android-app:");
    if flags & 0x3 != 0 && !uri.starts_with("intent:") && !android_app {
        let r = new_intent_obj(vm)?;
        intent_meta(vm, r).action = Some(guest_string(vm, ACTION_VIEW.to_string())?);
        let data = uri_parse_guest(vm, uri, uri)?;
        intent_meta(vm, r).data = Some(data);
        return Ok(r);
    }
    let mut i = u.last_index_of('#');
    if i == -1 {
        if !android_app {
            let r = new_intent_obj(vm)?;
            intent_meta(vm, r).action = Some(guest_string(vm, ACTION_VIEW.to_string())?);
            let data = uri_parse_guest(vm, uri, uri)?;
            intent_meta(vm, r).data = Some(data);
            return Ok(r);
        }
    } else if !u.starts_at(i, "#Intent;") {
        if !android_app {
            return parse_uri_old(vm, uri, flags);
        }
        i = -1;
    }
    let base = new_intent_obj(vm)?;
    intent_meta(vm, base).action = Some(guest_string(vm, ACTION_VIEW.to_string())?);
    let mut current = base;
    let mut explicit_action = false;
    let mut in_selector = false;
    let mut scheme: Option<String> = None;
    let mut data: String;
    if i >= 0 {
        data = u.sub(0, i)?;
        i += 8; // "#Intent;".length
    } else {
        data = uri.to_string();
    }
    while i >= 0 && !u.starts_at(i, "end") {
        let mut eq = u.index_of('=', i);
        if eq < 0 {
            eq = i - 1;
        }
        let semi = u.index_of(';', i);
        if semi < 0 {
            return Err(uri_syntax("illegal Intent URI format"));
        }
        let value = if eq < semi {
            crate::android_uri::decode(&u.sub(eq + 1, semi)?, false)
        } else {
            String::new()
        };
        if u.starts_at(i, "action=") {
            intent_meta(vm, current).action = Some(guest_string(vm, value)?);
            if !in_selector {
                explicit_action = true;
            }
        } else if u.starts_at(i, "category=") {
            let category = guest_string(vm, value)?;
            intent_meta(vm, current).categories.push(category);
        } else if u.starts_at(i, "type=") {
            intent_meta(vm, current).mime = Some(guest_string(vm, value)?);
        } else if u.starts_at(i, "identifier=") {
            intent_meta(vm, current).identifier = Some(guest_string(vm, value)?);
        } else if u.starts_at(i, "launchFlags=") {
            let mut parsed = integer_decode(&value)?;
            if flags & 4 == 0 {
                parsed &= !IMMUTABLE_FLAGS;
            }
            intent_meta(vm, current).flags = parsed;
        } else if u.starts_at(i, "package=") {
            intent_meta(vm, current).package = Some(guest_string(vm, value)?);
        } else if u.starts_at(i, "component=") {
            let flat = guest_string(vm, value)?;
            let component = vm.invoke_static(
                "Landroid/content/ComponentName;",
                "unflattenFromString",
                "(Ljava/lang/String;)Landroid/content/ComponentName;",
                &[flat],
            )?;
            intent_meta(vm, current).component_obj = Some(component);
        } else if u.starts_at(i, "scheme=") {
            if in_selector {
                let parsed = uri_parse_guest(vm, &format!("{value}:"), uri)?;
                intent_meta(vm, current).data = Some(parsed);
            } else {
                scheme = Some(value);
            }
        } else if u.starts_at(i, "sourceBounds=") {
            intent_meta(vm, current).source_bounds = Some(value);
        } else if semi == i + 3 && u.starts_at(i, "SEL") {
            current = new_intent_obj(vm)?;
            in_selector = true;
        } else {
            let type_char = u.char_at(i)?;
            let key = crate::android_uri::decode(&u.sub(i + 2, eq)?, false);
            let mut extras = intent_extras_of(vm, current);
            put_typed_extra(vm, &mut extras, type_char, key, &value, false, uri)?;
            if let Some(HostObj::Intent { extras: e, .. }) = vm.fw.objects.get_mut(&current) {
                *e = extras;
            }
        }
        i = semi + 1;
    }
    if in_selector {
        if intent_meta(vm, base).package.is_none() {
            intent_meta(vm, base).selector = Some(Value::Obj(current));
        }
        current = base;
    }
    if let Some(stripped) = data.strip_prefix("intent:") {
        data = stripped.to_string();
        if let Some(s) = &scheme {
            data = format!("{s}:{data}");
        }
    } else if data.starts_with("android-app:") {
        let chars: Vec<char> = data.chars().collect();
        if chars.get(12) == Some(&'/') && chars.get(13) == Some(&'/') {
            let find = |from: usize| -> i32 {
                chars[from..]
                    .iter()
                    .position(|c| *c == '/')
                    .map(|p| (from + p) as i32)
                    .unwrap_or(-1)
            };
            let sub = |a: i32, b: i32| -> String { chars[a as usize..b as usize].iter().collect() };
            let total = chars.len() as i32;
            let mut end = find(14);
            if end < 0 {
                let pkg = sub(14, total);
                intent_meta(vm, current).package = Some(guest_string(vm, pkg)?);
                if !explicit_action {
                    intent_meta(vm, current).action =
                        Some(guest_string(vm, ACTION_MAIN.to_string())?);
                }
                data = String::new();
            } else {
                let pkg = sub(14, end);
                intent_meta(vm, current).package = Some(guest_string(vm, pkg)?);
                let mut authority: Option<String> = None;
                let mut found_scheme: Option<String> = None;
                if end + 1 < total {
                    let new_end = find((end + 1) as usize);
                    if new_end >= 0 {
                        found_scheme = Some(sub(end + 1, new_end));
                        end = new_end;
                        let newer = find((end + 1) as usize);
                        if end < total && newer >= 0 {
                            authority = Some(sub(end + 1, newer));
                            end = newer;
                        }
                    } else {
                        found_scheme = Some(sub(end + 1, total));
                    }
                }
                match (found_scheme, authority) {
                    (None, _) => {
                        if !explicit_action {
                            intent_meta(vm, current).action =
                                Some(guest_string(vm, ACTION_MAIN.to_string())?);
                        }
                        data = String::new();
                    }
                    (Some(s), None) => data = format!("{s}:"),
                    (Some(s), Some(a)) => {
                        data = format!("{s}://{a}{}", sub(end, total));
                    }
                }
            }
        } else {
            data = String::new();
        }
    }
    if !data.is_empty() {
        let parsed = uri_parse_guest(vm, &data, uri)?;
        intent_meta(vm, current).data = Some(parsed);
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_parsing() {
        assert_eq!(
            params_of("(Ljava/lang/String;I)V"),
            vec!["Ljava/lang/String;", "I"]
        );
        assert_eq!(
            params_of("(Ljava/lang/String;[Ljava/lang/String;)V").len(),
            2
        );
        assert_eq!(params_of("()V").len(), 0);
        assert_eq!(ret_of("(Ljava/lang/String;)J"), "J");
        assert!(matches!(typed_default("Z"), Value::Int(0)));
        assert!(matches!(typed_default("Ljava/lang/String;"), Value::Null));
    }
}
