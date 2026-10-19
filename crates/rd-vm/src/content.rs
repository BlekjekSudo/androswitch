//! `ContentResolver` observers, `Settings.Global/System/Secure`,
//! `ContentObserver` and `android.net.Uri` accessors.
//!
//! - **Settings** are a real key → string table seeded with AOSP defaults:
//!   animation scales 1.0, `font_scale` 1.0, `time_12_24` "24",
//!   developer/adb/airplane off. `getX(cr, name, def)` returns the stored
//!   value, else `def`. `getX(cr, name)` throws
//!   `Settings.SettingNotFoundException` for an unknown key, as AOSP does.
//!   `putX` stores the value and notifies observers of
//!   `content://settings/<table>/<name>`.
//! - **`Context.getContentResolver()`** returns a singleton.
//!   `registerContentObserver(uri, descendants, observer)` records the
//!   observer, and `notifyChange(uri, …)` calls `onChange(selfChange, uri)`
//!   on matching observers. A prefix match is used when
//!   `notifyForDescendants` is set.
//! - **`ContentObserver.onChange(boolean, Uri)`** defaults to
//!   `onChange(boolean)`, as in AOSP.
//! - **Uri** strings live in `fw.m8.uris`. The accessors parse RFC 3986
//!   components: scheme, authority, path, query, fragment, query
//!   parameters (percent-decoded), path segments.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const RESOLVER: &str = "Landroid/content/ContentResolver;";
const OBSERVER: &str = "Landroid/database/ContentObserver;";
const URI: &str = "Landroid/net/Uri;";

#[derive(Debug, Default, Clone)]
pub(crate) struct ContentState {
    pub resolver: Option<ObjRef>,
    /// (uri string, notifyForDescendants, observer)
    pub observers: Vec<(String, bool, ObjRef)>,
    /// "global/animator_duration_scale" → "1.0"
    pub settings: std::collections::HashMap<String, String>,
}

fn default_setting(key: &str) -> Option<&'static str> {
    let name = key.rsplit('/').next().unwrap_or(key);
    Some(match name {
        "animator_duration_scale" | "transition_animation_scale" | "window_animation_scale" | "font_scale" => "1.0",
        "time_12_24" => "24",
        "auto_time" | "auto_time_zone" | "stay_on_while_plugged_in" => "1",
        "development_settings_enabled" | "adb_enabled" | "airplane_mode_on" | "always_finish_activities"
        | "accessibility_enabled" | "show_touches" | "wait_for_debugger" | "debug_app" => "0",
        "screen_brightness" => "128",
        "android_id" => "rustydroid0000001",
        _ => return None,
    })
}

fn table_of(class: &str) -> Option<&'static str> {
    Some(match class {
        "Landroid/provider/Settings$Global;" => "global",
        "Landroid/provider/Settings$System;" => "system",
        "Landroid/provider/Settings$Secure;" => "secure",
        _ => return None,
    })
}

fn sarg(vm: &Engine, args: &[Value], i: usize) -> Result<Option<String>, VmExit> {
    Ok(match args.get(i) {
        Some(Value::Obj(r)) => Some(vm.heap.as_str(*r)?.to_string()),
        _ => None,
    })
}

fn new_str(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

pub(crate) fn new_uri(vm: &mut Engine, s: String) -> Result<ObjRef, VmExit> {
    let r = vm.heap.alloc_instance(URI.to_string(), Vec::new())?;
    vm.fw.m8.uris.insert(r, s);
    Ok(r)
}

fn lookup(vm: &Engine, key: &str) -> Option<String> {
    vm.content.settings.get(key).cloned().or_else(|| default_setting(key).map(str::to_string))
}

fn not_found(name: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Landroid/provider/Settings$SettingNotFoundException;", name.to_string()))
}

fn notify(vm: &mut Engine, uri: &str) -> Result<(), VmExit> {
    let targets: Vec<ObjRef> = vm
        .content
        .observers
        .iter()
        .filter(|(u, desc, _)| u == uri || (*desc && uri.starts_with(u.as_str())))
        .map(|(_, _, o)| *o)
        .collect();
    for o in targets {
        let uri_obj = new_uri(vm, uri.to_string())?;
        let class = vm.heap.class_of(o)?.to_string();
        if let Some((d, def, m)) = vm.cp.resolve_method(&class, "onChange", "(ZLandroid/net/Uri;)V") {
            vm.call(d, def, &m, vec![Value::Obj(o), Value::Int(0), Value::Obj(uri_obj)])?;
        } else if let Some((d, def, m)) = vm.cp.resolve_method(&class, "onChange", "(Z)V") {
            vm.call(d, def, &m, vec![Value::Obj(o), Value::Int(0)])?;
        }
    }
    Ok(())
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(table) = table_of(class) {
        if name == "getUriFor" {
            let n = sarg(vm, args, 0)?.unwrap_or_default();
            return Ok(Some(Value::Obj(new_uri(vm, format!("content://settings/{table}/{n}"))?)));
        }
        let n = sarg(vm, args, 1)?.unwrap_or_default();
        let key = format!("{table}/{n}");
        let has_default = args.len() >= 3;
        let ret = sig.rsplit_once(')').map_or("V", |(_, r)| r);
        if let Some(kind) = name.strip_prefix("get") {
            let raw = lookup(vm, &key);
            return Ok(Some(match (kind, raw) {
                ("String", v) => match v {
                    Some(s) => new_str(vm, s)?,
                    None => Value::Null,
                },
                (_, Some(s)) => match ret {
                    "F" => Value::Float(s.parse().map_err(|_| not_found(&n))?),
                    "J" => Value::Long(s.parse().map_err(|_| not_found(&n))?),
                    _ => Value::Int(s.parse::<f64>().map_err(|_| not_found(&n))? as i32),
                },
                (_, None) if has_default => args[2].clone(),
                (_, None) => return Err(not_found(&n)),
            }));
        }
        if name.starts_with("put") {
            let v = match args.get(2) {
                Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
                Some(Value::Float(f)) => f.to_string(),
                Some(Value::Long(l)) => l.to_string(),
                Some(Value::Int(i)) => i.to_string(),
                _ => String::new(),
            };
            vm.content.settings.insert(key, v);
            notify(vm, &format!("content://settings/{table}/{n}"))?;
            return Ok(Some(Value::Int(1)));
        }
        return Ok(None);
    }
    if class == URI {
        return Ok(match (name, sig) {
            ("fromParts", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;") => {
                let (s, ssp, frag) = (sarg(vm, args, 0)?.unwrap_or_default(), sarg(vm, args, 1)?.unwrap_or_default(), sarg(vm, args, 2)?);
                let mut u = format!("{s}:{}", encode(&ssp, ":/@"));
                if let Some(f) = frag {
                    u.push('#');
                    u.push_str(&encode(&f, ""));
                }
                Some(Value::Obj(new_uri(vm, u)?))
            }
            ("encode", "(Ljava/lang/String;)Ljava/lang/String;") => match sarg(vm, args, 0)? {
                Some(s) => Some(new_str(vm, encode(&s, ""))?),
                None => Some(Value::Null),
            },
            ("decode", "(Ljava/lang/String;)Ljava/lang/String;") => match sarg(vm, args, 0)? {
                Some(s) => Some(new_str(vm, decode(&s))?),
                None => Some(Value::Null),
            },
            _ => None,
        });
    }
    Ok(None)
}

fn encode(s: &str, allow: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || "_-!.~'()*".contains(c) || allow.contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Parts {
    scheme: Option<String>,
    ssp: String,
    authority: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

fn parse(u: &str) -> Parts {
    let (main, fragment) = match u.split_once('#') {
        Some((m, f)) => (m, Some(f.to_string())),
        None => (u, None),
    };
    let (scheme, rest) = match main.find(':') {
        Some(i) if main[..i].chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) && i > 0 => {
            (Some(main[..i].to_string()), &main[i + 1..])
        }
        _ => (None, main),
    };
    let ssp = rest.to_string();
    let (hier, query) = match rest.split_once('?') {
        Some((h, q)) => (h, Some(q.to_string())),
        None => (rest, None),
    };
    let (authority, path) = match hier.strip_prefix("//") {
        Some(h) => match h.find('/') {
            Some(i) => (Some(h[..i].to_string()), h[i..].to_string()),
            None => (Some(h.to_string()), String::new()),
        },
        None => (None, hier.to_string()),
    };
    Parts { scheme, ssp, authority, path, query, fragment }
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "getContentResolver" && sig == "()Landroid/content/ContentResolver;" {
        let r = match vm.content.resolver {
            Some(r) => r,
            None => {
                let r = vm.heap.alloc_instance(RESOLVER.to_string(), Vec::new())?;
                vm.content.resolver = Some(r);
                r
            }
        };
        return Ok(Some(Value::Obj(r)));
    }
    let uri_str = |vm: &Engine, v: Option<&Value>| -> Option<String> {
        match v {
            Some(Value::Obj(u)) => vm.fw.m8.uris.get(u).cloned(),
            _ => None,
        }
    };
    if mref_class == RESOLVER {
        return Ok(match (name, sig) {
            ("registerContentObserver", "(Landroid/net/Uri;ZLandroid/database/ContentObserver;)V") => {
                let u = uri_str(vm, args.first()).unwrap_or_default();
                let desc = matches!(args.get(1), Some(Value::Int(x)) if *x != 0);
                if let Some(Value::Obj(o)) = args.get(2) {
                    vm.content.observers.push((u, desc, *o));
                }
                Some(Value::Null)
            }
            ("unregisterContentObserver", "(Landroid/database/ContentObserver;)V") => {
                if let Some(Value::Obj(o)) = args.first() {
                    vm.content.observers.retain(|(_, _, x)| x != o);
                }
                Some(Value::Null)
            }
            ("notifyChange", s) if s.starts_with("(Landroid/net/Uri;") => {
                let u = uri_str(vm, args.first()).unwrap_or_default();
                notify(vm, &u)?;
                Some(Value::Null)
            }
            _ => None,
        });
    }
    if mref_class == OBSERVER {
        return Ok(match (name, sig) {
            ("<init>", "(Landroid/os/Handler;)V") => Some(Value::Null),
            ("deliverSelfNotifications", "()Z") => Some(Value::Int(0)),
            // AOSP: onChange(self, uri) → onChange(self)
            ("onChange", "(ZLandroid/net/Uri;)V") => {
                let class = vm.heap.class_of(recv)?.to_string();
                if let Some((d, def, m)) = vm.cp.resolve_method(&class, "onChange", "(Z)V") {
                    vm.call(d, def, &m, vec![Value::Obj(recv), args.first().cloned().unwrap_or(Value::Int(0))])?;
                }
                Some(Value::Null)
            }
            ("onChange", "(Z)V") => Some(Value::Null),
            _ => None,
        });
    }
    if mref_class == URI {
        let Some(u) = vm.fw.m8.uris.get(&recv).cloned() else { return Ok(None) };
        let p = parse(&u);
        let opt = |vm: &mut Engine, s: Option<String>| -> Result<Value, VmExit> {
            match s {
                Some(s) => new_str(vm, s),
                None => Ok(Value::Null),
            }
        };
        return Ok(Some(match (name, sig) {
            ("toString", "()Ljava/lang/String;") => new_str(vm, u)?,
            ("getScheme", "()Ljava/lang/String;") => opt(vm, p.scheme)?,
            ("getSchemeSpecificPart", "()Ljava/lang/String;") => new_str(vm, decode(&p.ssp))?,
            ("getAuthority", "()Ljava/lang/String;") => opt(vm, p.authority.map(|a| decode(&a)))?,
            ("getHost", "()Ljava/lang/String;") => {
                opt(vm, p.authority.map(|a| a.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_string()))?
            }
            ("getPath", "()Ljava/lang/String;") => opt(vm, if p.path.is_empty() && p.authority.is_none() && p.scheme.is_some() { None } else { Some(decode(&p.path)) })?,
            ("getQuery", "()Ljava/lang/String;") => opt(vm, p.query.map(|q| decode(&q)))?,
            ("getFragment", "()Ljava/lang/String;") => opt(vm, p.fragment.map(|f| decode(&f)))?,
            ("getLastPathSegment", "()Ljava/lang/String;") => {
                opt(vm, p.path.split('/').filter(|s| !s.is_empty()).next_back().map(decode))?
            }
            ("getQueryParameter", "(Ljava/lang/String;)Ljava/lang/String;") => {
                let k = sarg(vm, args, 0)?.unwrap_or_default();
                let v = p.query.as_deref().unwrap_or("").split('&').find_map(|kv| {
                    let (a, b) = kv.split_once('=').unwrap_or((kv, ""));
                    (decode(a) == k).then(|| decode(&b.replace('+', " ")))
                });
                opt(vm, v)?
            }
            ("getPathSegments", "()Ljava/util/List;") | ("getQueryParameters", "(Ljava/lang/String;)Ljava/util/List;") => {
                let items: Vec<String> = if name == "getPathSegments" {
                    p.path.split('/').filter(|s| !s.is_empty()).map(decode).collect()
                } else {
                    let k = sarg(vm, args, 0)?.unwrap_or_default();
                    p.query.as_deref().unwrap_or("").split('&').filter_map(|kv| {
                        let (a, b) = kv.split_once('=').unwrap_or((kv, ""));
                        (decode(a) == k).then(|| decode(&b.replace('+', " ")))
                    }).collect()
                };
                let mut elems = Vec::new();
                for s in items {
                    elems.push(new_str(vm, s)?);
                }
                let l = vm.heap.alloc_instance("Ljava/util/ArrayList;".to_string(), Vec::new())?;
                vm.fw.objects.insert(l, crate::framework::HostObj::List { elems });
                Value::Obj(l)
            }
            ("isAbsolute", "()Z") => Value::Int(p.scheme.is_some() as i32),
            ("isRelative", "()Z") => Value::Int(p.scheme.is_none() as i32),
            ("isHierarchical", "()Z") => Value::Int((p.scheme.is_none() || p.ssp.starts_with('/')) as i32),
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = uri_str(vm, args.first());
                Value::Int((other.as_deref() == Some(u.as_str())) as i32)
            }
            ("hashCode", "()I") => Value::Int(u.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))),
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_parts() {
        let p = parse("content://settings/global/font_scale?x=1#f");
        assert_eq!(p.scheme.as_deref(), Some("content"));
        assert_eq!(p.authority.as_deref(), Some("settings"));
        assert_eq!(p.path, "/global/font_scale");
        assert_eq!(p.query.as_deref(), Some("x=1"));
        assert_eq!(p.fragment.as_deref(), Some("f"));
        assert_eq!(decode("a%20b"), "a b");
        assert_eq!(encode("a b", ""), "a%20b");
        assert_eq!(default_setting("global/animator_duration_scale"), Some("1.0"));
    }
}
