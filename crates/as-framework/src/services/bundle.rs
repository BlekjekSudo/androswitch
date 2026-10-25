//! In-process Bundle values and Intent extras share exact Android value types.
//! Parcel/Binder transport is outside this store and remains unsupported.
use super::*;

const BUNDLE: &str = "Landroid/os/Bundle;";
const INTENT: &str = "Landroid/content/Intent;";
const TYPES: [(&str, &str); 19] = [
    ("Boolean", "Z"),
    ("Byte", "B"),
    ("Char", "C"),
    ("Short", "S"),
    ("Int", "I"),
    ("Long", "J"),
    ("Float", "F"),
    ("Double", "D"),
    ("String", "Ljava/lang/String;"),
    ("Bundle", BUNDLE),
    ("BooleanArray", "[Z"),
    ("ByteArray", "[B"),
    ("CharArray", "[C"),
    ("ShortArray", "[S"),
    ("IntArray", "[I"),
    ("LongArray", "[J"),
    ("FloatArray", "[F"),
    ("DoubleArray", "[D"),
    ("StringArray", "[Ljava/lang/String;"),
];

fn key(vm: &Engine, value: &Value) -> Result<Option<String>> {
    let key = value
        .as_ref()?
        .map(|r| vm.heap.as_str(r).map(ToString::to_string))
        .transpose()?;
    if key.as_ref().is_some_and(|s| s.len() > 4096) {
        return Err(error("BUNDLE_BUDGET", "4096 bytes per Bundle key"));
    }
    Ok(key)
}
fn zero(kind: &str) -> Value {
    if kind.starts_with('[') {
        return Value::Null;
    }
    match kind {
        "J" => Value::Long(0),
        "F" => Value::Float(0.),
        "D" => Value::Double(0.),
        "Ljava/lang/String;" | BUNDLE => Value::Null,
        _ => Value::Int(0),
    }
}

pub(super) fn call(
    android: &Android,
    vm: &mut Engine,
    name: &str,
    sig: &str,
    r: ObjRef,
    args: &[Value],
) -> Result<Option<Value>> {
    let class = vm.heap.class_of(r)?;
    let intent = vm.cp.is_subtype(class, INTENT);
    if !intent && !vm.cp.is_subtype(class, BUNDLE) {
        return Ok(None);
    }
    for (suffix, kind) in TYPES {
        let put_name = if intent {
            "putExtra".to_string()
        } else {
            format!("put{suffix}")
        };
        let put_return = if intent { INTENT } else { "V" };
        if name == put_name && sig == format!("(Ljava/lang/String;{kind}){put_return}") {
            let key = key(vm, &args[0])?;
            let value = args[1].clone(); // Android.call already applied the exact descriptor.
            if kind == "Ljava/lang/String;" {
                if let Some(reference) = value.as_ref()? {
                    vm.heap.as_str(reference)?;
                }
            }
            if kind == BUNDLE {
                if let Some(reference) = value.as_ref()? {
                    if !vm.cp.is_subtype(vm.heap.class_of(reference)?, BUNDLE) {
                        return Err(Throwable::new(
                            "Ljava/lang/ClassCastException;",
                            "Bundle value",
                        )
                        .into());
                    }
                }
            }
            if kind.starts_with('[') {
                if let Some(reference) = value.as_ref()? {
                    if !vm.cp.is_subtype(vm.heap.class_of(reference)?, kind) {
                        return Err(Throwable::new(
                            "Ljava/lang/ClassCastException;",
                            "Bundle array value",
                        )
                        .into());
                    }
                }
            }
            let mut state = android.state.borrow_mut();
            let entries = state.bundles.entry(r).or_default();
            if entries.len() >= 1024 && !entries.contains_key(&key) {
                return Err(error("BUNDLE_BUDGET", "1024 entries per Bundle/Intent"));
            }
            entries.insert(key, BundleEntry { kind, value });
            return Ok(Some(if intent { Value::Obj(r) } else { Value::Null }));
        }
        let get_name = if intent {
            format!("get{suffix}Extra")
        } else {
            format!("get{suffix}")
        };
        let string = kind == "Ljava/lang/String;";
        let with_default = sig == format!("(Ljava/lang/String;{kind}){kind}")
            && (!intent || !string)
            && kind != BUNDLE
            && !kind.starts_with('[');
        let without_default = sig == format!("(Ljava/lang/String;){kind}")
            && (!intent || string || kind == BUNDLE || kind.starts_with('['));
        if name != get_name || !(with_default || without_default) {
            continue;
        }
        let key = key(vm, &args[0])?;
        let state = android.state.borrow();
        let value = state
            .bundles
            .get(&r)
            .and_then(|b| b.get(&key))
            .filter(|e| e.kind == kind);
        // Like BaseBundle's boxed casts, mismatched types use the selected
        // default. They must never reinterpret an int as boolean/float, etc.
        let value = value.filter(|e| !string || e.value.as_ref().ok().flatten().is_some());
        return Ok(Some(value.map_or_else(
            || {
                if with_default {
                    args[1].clone()
                } else {
                    zero(kind)
                }
            },
            |entry| entry.value.clone(),
        )));
    }
    let value = match (name, sig, intent) {
        ("size", "()I", false) => Value::Int(
            android
                .state
                .borrow()
                .bundles
                .get(&r)
                .map_or(0, |b| b.len()) as i32,
        ),
        ("isEmpty", "()Z", false) => Value::Int(i32::from(
            android
                .state
                .borrow()
                .bundles
                .get(&r)
                .is_none_or(|b| b.is_empty()),
        )),
        ("containsKey", "(Ljava/lang/String;)Z", false)
        | ("hasExtra", "(Ljava/lang/String;)Z", true) => {
            let key = key(vm, &args[0])?;
            Value::Int(i32::from(
                android
                    .state
                    .borrow()
                    .bundles
                    .get(&r)
                    .is_some_and(|b| b.contains_key(&key)),
            ))
        }
        ("remove", "(Ljava/lang/String;)V", false)
        | ("removeExtra", "(Ljava/lang/String;)V", true) => {
            let key = key(vm, &args[0])?;
            if let Some(entries) = android.state.borrow_mut().bundles.get_mut(&r) {
                entries.remove(&key);
            }
            Value::Null
        }
        ("clear", "()V", false) => {
            if let Some(entries) = android.state.borrow_mut().bundles.get_mut(&r) {
                entries.clear();
            }
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
