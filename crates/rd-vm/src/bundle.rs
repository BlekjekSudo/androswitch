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
//! - `getParcelable(String, Class)` and similar (API 33) ignore the class.
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
    let set = vm.heap.alloc_instance("Ljava/util/HashSet;".to_string(), Vec::new())?;
    vm.fw.objects.insert(set, HostObj::List { elems });
    Ok(Value::Obj(set))
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
    let is_put = if is_intent { name == "putExtra" } else { name.starts_with("put") && name != "putAll" };
    if is_put && params.len() == 2 {
        let v = args.get(1).cloned().unwrap_or(Value::Null);
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
        let cur = read(vm);
        return Ok(Some(match (cur, params.len()) {
            (Some(v), _) => v,
            // getX(key, default) — but not getParcelable(key, Class) (API 33)
            (None, 2) if params[1] != "Ljava/lang/Class;" => args[1].clone(),
            (None, _) => typed_default(&ret),
        }));
    }
    Ok(None)
}

fn bundle(vm: &mut Engine, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if map_mut(vm, recv).is_none() {
        vm.fw.objects.insert(recv, HostObj::Bundle { map: Vec::new() });
    }
    match (name, sig) {
        ("<init>", "()V") | ("<init>", "(I)V") | ("<init>", "(Ljava/lang/ClassLoader;)V") => {
            return Ok(Some(Value::Null))
        }
        ("<init>", "(Landroid/os/Bundle;)V") | ("<init>", "(Landroid/os/PersistableBundle;)V")
        | ("putAll", "(Landroid/os/Bundle;)V") | ("putAll", "(Landroid/os/PersistableBundle;)V") => {
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
        ("size", "()I") => return Ok(Some(Value::Int(map_of(vm, recv).map_or(0, |m| m.len()) as i32))),
        ("isEmpty", "()Z") => return Ok(Some(Value::Int(map_of(vm, recv).is_none_or(|m| m.is_empty()) as i32))),
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

fn intent(vm: &mut Engine, name: &str, sig: &str, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if !matches!(vm.fw.get(recv), Some(HostObj::Intent { .. })) {
        vm.fw.objects.insert(recv, HostObj::Intent { component: None, extras: Vec::new() });
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
                    Some(HostObj::Intent { component, extras }) => (component.clone(), extras.clone()),
                    _ => (None, Vec::new()),
                };
                vm.fw.objects.insert(recv, HostObj::Intent { component: c, extras: e });
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
                let b = vm.heap.alloc_instance("Landroid/os/Bundle;".to_string(), Vec::new())?;
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
        ("getAction", "()Ljava/lang/String;") => Some(intent_meta(vm, recv).action.clone().unwrap_or(Value::Null)),
        ("setData", "(Landroid/net/Uri;)Landroid/content/Intent;") => {
            let m = intent_meta(vm, recv);
            m.data = Some(a0);
            m.mime = None; // AOSP: setData clears the type
            this
        }
        ("getData", "()Landroid/net/Uri;") => Some(intent_meta(vm, recv).data.clone().unwrap_or(Value::Null)),
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
        ("getType", "()Ljava/lang/String;") => Some(intent_meta(vm, recv).mime.clone().unwrap_or(Value::Null)),
        ("setPackage", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).package = Some(a0);
            this
        }
        ("getPackage", "()Ljava/lang/String;") => Some(intent_meta(vm, recv).package.clone().unwrap_or(Value::Null)),
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
        ("addCategory", "(Ljava/lang/String;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).categories.push(a0);
            this
        }
        ("setComponent", "(Landroid/content/ComponentName;)Landroid/content/Intent;") => {
            intent_meta(vm, recv).component_obj = Some(a0);
            this
        }
        ("getComponent", "()Landroid/content/ComponentName;") => {
            Some(intent_meta(vm, recv).component_obj.clone().unwrap_or(Value::Null))
        }
        ("setClassName", "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;")
        | ("setClassName", "(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;") => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_parsing() {
        assert_eq!(params_of("(Ljava/lang/String;I)V"), vec!["Ljava/lang/String;", "I"]);
        assert_eq!(params_of("(Ljava/lang/String;[Ljava/lang/String;)V").len(), 2);
        assert_eq!(params_of("()V").len(), 0);
        assert_eq!(ret_of("(Ljava/lang/String;)J"), "J");
        assert!(matches!(typed_default("Z"), Value::Int(0)));
        assert!(matches!(typed_default("Ljava/lang/String;"), Value::Null));
    }
}
