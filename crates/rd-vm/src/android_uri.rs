//! `android.net.Uri` and `Uri.Builder` with AOSP semantics.
//!
//! **Storage.** A Uri is its encoded string, kept in `M8State::uris` (the
//! store the m8 Intent/ContentResolver code already uses). Parts are parsed
//! on demand per RFC 3986 the way `android.net.Uri.StringUri` does:
//! - Scheme: before the first `:`, if no `/ ? #` comes first.
//! - Opaque URIs have no `/` after the scheme (`mailto:x`, `tel:1`).
//! - Hierarchical URIs: `//authority`, path, `?query`, `#fragment`.
//!
//! **Uri getters:**
//! - Decoded and `getEncoded*` forms of `SchemeSpecificPart`, `Authority`,
//!   `Path`, `Query` and `Fragment`.
//! - `getUserInfo`, `getHost`, `getPort`.
//! - `getPathSegments` and `getLastPathSegment`.
//! - Query parameters, with `+` decoded as space (as AOSP does):
//!   `getQueryParameter`, `getQueryParameters`, `getQueryParameterNames`
//!   (insertion-ordered), `getBooleanQueryParameter`.
//!
//! **Uri predicates and the rest:** `isHierarchical`/`isOpaque`/
//! `isAbsolute`/`isRelative`, `normalizeScheme`, `buildUpon`, `equals`,
//! `hashCode`, `compareTo`, `toString`.
//!
//! **Statics:** `parse`, `fromFile`, `fromParts`, `encode(s[, allow])`,
//! `decode`, `withAppendedPath`.
//!
//! **Builder:** `scheme`, `authority`/`encodedAuthority`, `path`/
//! `encodedPath`, `appendPath`/`appendEncodedPath`, `query`/`encodedQuery`,
//! `appendQueryParameter`, `clearQuery`, `fragment`/`encodedFragment`,
//! `opaquePart`/`encodedOpaquePart`, `build`, `toString`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

pub const URI: &str = "Landroid/net/Uri;";
const BUILDER: &str = "Landroid/net/Uri$Builder;";

#[derive(Default, Clone)]
struct B {
    scheme: Option<String>,
    opaque: Option<String>,
    authority: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

thread_local! {
    static BUILDERS: RefCell<HashMap<ObjRef, B>> = RefCell::new(HashMap::new());
}

/// GC: drop state of collected objects (slots are reused).
pub(crate) fn gc_purge(alive: &dyn Fn(ObjRef) -> bool) {
    BUILDERS.with(|m| m.borrow_mut().retain(|k, _| alive(*k)));
}

/// Issue #230: fronteira de `Engine` — builders são por `ObjRef` de um
/// heap que deixou de existir quando o `Engine` anterior morreu.
pub(crate) fn reset_all() {
    BUILDERS.with(|m| m.borrow_mut().clear());
}

/// Encoded parts of a Uri string.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Parts {
    pub scheme: Option<String>,
    pub ssp: String,
    pub authority: Option<String>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
    pub opaque: bool,
}

pub(crate) fn parse_parts(s: &str) -> Parts {
    let (body, fragment) = match s.find('#') {
        Some(i) => (&s[..i], Some(s[i + 1..].to_string())),
        None => (s, None),
    };
    let colon = body
        .find(':')
        .filter(|&c| !body[..c].contains(['/', '?', '#']));
    let scheme = colon.map(|c| body[..c].to_string());
    let ssp = match colon {
        Some(c) => &body[c + 1..],
        None => body,
    };
    let opaque = scheme.is_some() && !ssp.starts_with('/');
    let mut p = Parts {
        scheme,
        ssp: ssp.to_string(),
        fragment,
        opaque,
        ..Default::default()
    };
    if opaque {
        return p;
    }
    let (hier, query) = match ssp.find('?') {
        Some(i) => (&ssp[..i], Some(ssp[i + 1..].to_string())),
        None => (ssp, None),
    };
    p.query = query;
    if let Some(rest) = hier.strip_prefix("//") {
        let end = rest.find('/').unwrap_or(rest.len());
        p.authority = Some(rest[..end].to_string());
        p.path = rest[end..].to_string();
    } else {
        p.path = hier.to_string();
    }
    p
}

const UNRESERVED: &str = "_-!.~'()*";

/// `Uri.encode(s, allow)`.
pub(crate) fn encode(s: &str, allow: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || UNRESERVED.contains(ch) || allow.contains(ch) {
            out.push(ch);
        } else {
            let mut b = [0u8; 4];
            for byte in ch.encode_utf8(&mut b).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `Uri.decode` (`plus` → also turn `+` into space, as for query values).
pub(crate) fn decode(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match (hex_nibble(b[i + 1]), hex_nibble(b[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push((hi << 4) | lo);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_pairs(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (decode(k, true), decode(v, true)),
            None => (decode(kv, true), String::new()),
        })
        .collect()
}

fn java_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

fn jstr(vm: &mut Engine, s: impl Into<String>) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s.into())?))
}
fn opt_str(vm: &mut Engine, s: Option<String>) -> Result<Value, VmExit> {
    match s {
        Some(s) => jstr(vm, s),
        None => Ok(Value::Null),
    }
}
fn sval(vm: &Engine, v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
    }
}

pub(crate) fn new_uri(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_instance(URI.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    vm.fw.m8.uris.insert(r, s);
    Ok(Value::Obj(r))
}

fn builder_from_parts(p: Parts) -> B {
    B {
        scheme: p.scheme,
        opaque: p.opaque.then(|| p.ssp.clone()),
        authority: p.authority,
        path: p.path,
        query: p.query,
        fragment: p.fragment,
    }
}

fn append_encoded_segment(b: &mut B, seg: &str) {
    if !b.path.ends_with('/') {
        b.path.push('/');
    }
    b.path.push_str(seg.strip_prefix('/').unwrap_or(seg));
    b.opaque = None;
}

fn build_string(b: &B) -> String {
    let mut s = String::new();
    if let Some(sc) = &b.scheme {
        s.push_str(sc);
        s.push(':');
    }
    if let Some(op) = &b.opaque {
        s.push_str(op);
    } else {
        if let Some(a) = &b.authority {
            s.push_str("//");
            s.push_str(a);
            if !b.path.is_empty() && !b.path.starts_with('/') {
                s.push('/');
            }
        }
        s.push_str(&b.path);
        if let Some(q) = &b.query {
            s.push('?');
            s.push_str(q);
        }
    }
    if let Some(f) = &b.fragment {
        s.push('#');
        s.push_str(f);
    }
    s
}

pub fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != URI {
        return Ok(None);
    }
    Ok(Some(match name {
        "parse" => {
            let Some(s) = sval(vm, args.first()) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "uriString",
                )));
            };
            new_uri(vm, s)?
        }
        "fromFile" => {
            let path = match args.first() {
                Some(Value::Obj(f)) => {
                    let p = vm.heap.get_field(*f, "rd_path").unwrap_or(Value::Null);
                    sval(vm, Some(&p)).unwrap_or_default()
                }
                _ => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "file",
                    )))
                }
            };
            new_uri(vm, format!("file://{}", encode(&path, "/")))?
        }
        "fromParts" => {
            let Some(scheme) = sval(vm, args.first()) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "scheme",
                )));
            };
            let Some(ssp) = sval(vm, args.get(1)) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "ssp",
                )));
            };
            let mut s = format!("{scheme}:{}", encode(&ssp, ""));
            if let Some(f) = sval(vm, args.get(2)) {
                s.push('#');
                s.push_str(&encode(&f, ""));
            }
            new_uri(vm, s)?
        }
        "encode" => match sval(vm, args.first()) {
            Some(s) => {
                let allow = sval(vm, args.get(1)).unwrap_or_default();
                jstr(vm, encode(&s, &allow))?
            }
            None => Value::Null,
        },
        "decode" => match sval(vm, args.first()) {
            Some(s) => jstr(vm, decode(&s, false))?,
            None => Value::Null,
        },
        "withAppendedPath" => {
            let Some(Value::Obj(u)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "baseUri",
                )));
            };
            let Some(base) = vm.fw.m8.uris.get(u).cloned() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "baseUri",
                )));
            };
            let Some(seg) = sval(vm, args.get(1)) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "pathSegment",
                )));
            };
            let mut b = builder_from_parts(parse_parts(&base));
            append_encoded_segment(&mut b, &seg);
            new_uri(vm, build_string(&b))?
        }
        _ => return Ok(None),
    }))
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class == BUILDER || mref_class == BUILDER {
        return builder(vm, name, recv, args);
    }
    if recv_class != URI && mref_class != URI {
        return Ok(None);
    }
    let Some(s) = vm.fw.m8.uris.get(&recv).cloned() else {
        return Ok(None);
    };
    let p = parse_parts(&s);
    Ok(Some(match name {
        "toString" => jstr(vm, s)?,
        "getScheme" => opt_str(vm, p.scheme)?,
        "getSchemeSpecificPart" => jstr(vm, decode(&p.ssp, false))?,
        "getEncodedSchemeSpecificPart" => jstr(vm, p.ssp)?,
        "getAuthority" => opt_str(vm, p.authority.map(|a| decode(&a, false)))?,
        "getEncodedAuthority" => opt_str(vm, p.authority)?,
        "getUserInfo" | "getEncodedUserInfo" => {
            let ui = p
                .authority
                .and_then(|a| a.rsplit_once('@').map(|(u, _)| u.to_string()));
            opt_str(
                vm,
                if name == "getUserInfo" {
                    ui.map(|u| decode(&u, false))
                } else {
                    ui
                },
            )?
        }
        "getHost" => {
            let host = p.authority.map(|a| {
                let a = a.rsplit_once('@').map(|(_, h)| h.to_string()).unwrap_or(a);
                if a.starts_with('[') {
                    a.split_once(']').map(|(h, _)| format!("{h}]")).unwrap_or(a)
                } else {
                    a.split(':').next().unwrap_or("").to_string()
                }
            });
            opt_str(vm, host.map(|h| decode(&h, false)))?
        }
        "getPort" => {
            let port = p.authority.as_deref().and_then(|a| {
                let a = a.rsplit_once('@').map(|(_, h)| h).unwrap_or(a);
                let a = a.rsplit_once(']').map(|(_, r)| r).unwrap_or(a);
                a.rsplit_once(':')
                    .and_then(|(_, port)| port.parse::<i32>().ok())
            });
            Value::Int(port.unwrap_or(-1))
        }
        "getPath" => {
            if p.opaque {
                Value::Null
            } else {
                jstr(vm, decode(&p.path, false))?
            }
        }
        "getEncodedPath" => {
            if p.opaque {
                Value::Null
            } else {
                jstr(vm, p.path)?
            }
        }
        "getPathSegments" | "getLastPathSegment" => {
            let segs: Vec<String> = p
                .path
                .split('/')
                .filter(|x| !x.is_empty())
                .map(|x| decode(x, false))
                .collect();
            if name == "getLastPathSegment" {
                opt_str(vm, segs.last().cloned())?
            } else {
                let mut vals = Vec::new();
                for x in segs {
                    vals.push(jstr(vm, x)?);
                }
                crate::collections::new_list(vm, "Ljava/util/ArrayList;", vals)?
            }
        }
        "getQuery" => opt_str(vm, p.query.map(|q| decode(&q, false)))?,
        "getEncodedQuery" => opt_str(vm, p.query)?,
        "getFragment" => opt_str(vm, p.fragment.map(|f| decode(&f, false)))?,
        "getEncodedFragment" => opt_str(vm, p.fragment)?,
        "getQueryParameter" | "getQueryParameters" | "getBooleanQueryParameter" => {
            if p.opaque {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/UnsupportedOperationException;",
                    "This isn't a hierarchical URI.",
                )));
            }
            let key = sval(vm, args.first()).ok_or_else(|| {
                VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", "key"))
            })?;
            let pairs = query_pairs(p.query.as_deref().unwrap_or(""));
            let vals: Vec<String> = pairs
                .into_iter()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .collect();
            match name {
                "getQueryParameter" => opt_str(vm, vals.first().cloned())?,
                "getBooleanQueryParameter" => {
                    let d = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
                    Value::Int(match vals.first() {
                        Some(v) => {
                            let v = v.to_ascii_lowercase();
                            (v != "false" && v != "0") as i32
                        }
                        None => d,
                    })
                }
                _ => {
                    let mut out = Vec::new();
                    for v in vals {
                        out.push(jstr(vm, v)?);
                    }
                    crate::collections::new_list(vm, "Ljava/util/ArrayList;", out)?
                }
            }
        }
        "getQueryParameterNames" => {
            if p.opaque {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/UnsupportedOperationException;",
                    "This isn't a hierarchical URI.",
                )));
            }
            let mut names: Vec<String> = Vec::new();
            for (k, _) in query_pairs(p.query.as_deref().unwrap_or("")) {
                if !names.contains(&k) {
                    names.push(k);
                }
            }
            let mut out = Vec::new();
            for n in names {
                out.push(jstr(vm, n)?);
            }
            crate::collections::new_list(vm, "Ljava/util/LinkedHashSet;", out)?
        }
        "isHierarchical" => Value::Int(!p.opaque as i32),
        "isOpaque" => Value::Int(p.opaque as i32),
        "isAbsolute" => Value::Int(p.scheme.is_some() as i32),
        "isRelative" => Value::Int(p.scheme.is_none() as i32),
        "normalizeScheme" => match p.scheme {
            Some(sc) if sc.chars().any(|c| c.is_ascii_uppercase()) => {
                new_uri(vm, format!("{}{}", sc.to_ascii_lowercase(), &s[sc.len()..]))?
            }
            _ => Value::Obj(recv),
        },
        "buildUpon" => {
            let b = builder_from_parts(p);
            let r = vm
                .heap
                .alloc_instance(BUILDER.to_string(), Vec::new())
                .map_err(crate::err::oom_throwable)?;
            BUILDERS.with(|m| m.borrow_mut().insert(r, b));
            Value::Obj(r)
        }
        "equals" => Value::Int(match args.first() {
            Some(Value::Obj(o)) => vm.fw.m8.uris.get(o).is_some_and(|x| *x == s),
            _ => false,
        } as i32),
        "hashCode" => Value::Int(java_hash(&s)),
        "compareTo" => {
            let o = match args.first() {
                Some(Value::Obj(o)) => vm.fw.m8.uris.get(o).cloned().unwrap_or_default(),
                _ => {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "compareTo(null)",
                    )))
                }
            };
            let a: Vec<u16> = s.encode_utf16().collect();
            let b: Vec<u16> = o.encode_utf16().collect();
            Value::Int(match a.cmp(&b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        "describeContents" => Value::Int(0),
        _ => return Ok(None),
    }))
}

fn builder(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        BUILDERS.with(|m| m.borrow_mut().insert(recv, B::default()));
        return Ok(Some(Value::Null));
    }
    let Some(mut b) = BUILDERS.with(|m| m.borrow().get(&recv).cloned()) else {
        return Ok(None);
    };
    let a0 = sval(vm, args.first());
    let mut ret_self = true;
    let out = match name {
        "scheme" => {
            b.scheme = a0;
            Value::Null
        }
        "authority" => {
            b.authority = a0.map(|a| encode(&a, "@:[]"));
            b.opaque = None;
            Value::Null
        }
        "encodedAuthority" => {
            b.authority = a0;
            b.opaque = None;
            Value::Null
        }
        "path" => {
            b.path = a0.map(|p| encode(&p, "/")).unwrap_or_default();
            b.opaque = None;
            Value::Null
        }
        "encodedPath" => {
            b.path = a0.unwrap_or_default();
            b.opaque = None;
            Value::Null
        }
        "appendPath" | "appendEncodedPath" => {
            let Some(seg) = a0 else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "newSegment",
                )));
            };
            let seg = if name == "appendPath" {
                encode(&seg, "")
            } else {
                seg
            };
            append_encoded_segment(&mut b, &seg);
            Value::Null
        }
        "query" => {
            b.query = a0.map(|q| encode(&q, "=&"));
            b.opaque = None;
            Value::Null
        }
        "encodedQuery" => {
            b.query = a0;
            b.opaque = None;
            Value::Null
        }
        "appendQueryParameter" => {
            let k = a0.unwrap_or_default();
            let v = sval(vm, args.get(1)).unwrap_or_else(|| "null".into());
            let pair = format!("{}={}", encode(&k, ""), encode(&v, ""));
            b.query = Some(match b.query.take().filter(|q| !q.is_empty()) {
                Some(q) => format!("{q}&{pair}"),
                None => pair,
            });
            b.opaque = None;
            Value::Null
        }
        "clearQuery" => {
            b.query = None;
            Value::Null
        }
        "fragment" => {
            b.fragment = a0.map(|f| encode(&f, ""));
            Value::Null
        }
        "encodedFragment" => {
            b.fragment = a0;
            Value::Null
        }
        "opaquePart" | "encodedOpaquePart" => {
            b.opaque = a0.map(|o| {
                if name == "opaquePart" {
                    encode(&o, "")
                } else {
                    o
                }
            });
            Value::Null
        }
        "build" => {
            ret_self = false;
            let s = build_string(&b);
            new_uri(vm, s)?
        }
        "toString" => {
            ret_self = false;
            jstr(vm, build_string(&b))?
        }
        _ => return Ok(None),
    };
    BUILDERS.with(|m| m.borrow_mut().insert(recv, b));
    Ok(Some(if ret_self { Value::Obj(recv) } else { out }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_hierarchical_and_opaque() {
        let p = parse_parts("https://user@ex.com:8080/a/b%20c?x=1&y=a+b#frag");
        assert_eq!(p.scheme.as_deref(), Some("https"));
        assert_eq!(p.authority.as_deref(), Some("user@ex.com:8080"));
        assert_eq!(p.path, "/a/b%20c");
        assert_eq!(p.query.as_deref(), Some("x=1&y=a+b"));
        assert_eq!(p.fragment.as_deref(), Some("frag"));
        assert!(!p.opaque);
        let o = parse_parts("mailto:a@b.c");
        assert!(o.opaque);
        assert_eq!(o.ssp, "a@b.c");
        let rel = parse_parts("android-app://androidx.navigation/lista/{id}");
        assert_eq!(rel.path, "/lista/{id}");
    }
    #[test]
    fn codec() {
        assert_eq!(encode("a b/é", ""), "a%20b%2F%C3%A9");
        assert_eq!(encode("a/b", "/"), "a/b");
        assert_eq!(decode("a%20b+c", false), "a b+c");
        assert_eq!(decode("a%20b+c", true), "a b c");
        assert_eq!(
            query_pairs("x=1&y=a+b&z"),
            vec![
                ("x".into(), "1".into()),
                ("y".into(), "a b".into()),
                ("z".into(), String::new())
            ]
        );
    }
    #[test]
    fn builder_string() {
        let b = B {
            scheme: Some("https".into()),
            authority: Some("ex.com".into()),
            path: "/a".into(),
            query: Some("q=1".into()),
            ..Default::default()
        };
        assert_eq!(build_string(&b), "https://ex.com/a?q=1");
    }
}
