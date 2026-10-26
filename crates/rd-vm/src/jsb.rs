//! Full `StringBuilder` / `StringBuffer` surface.
//!
//! **State.** Both classes use the original `buf` heap field. Complete text
//! uses a heap String; raw/unpaired UTF-16 uses a managed char[] in that same
//! field. No separate builder store exists. Indices are UTF-16 code units.
//!
//! **Construction and append:**
//! - Constructors: `()`, `(int)`, `(String)`, `(CharSequence)`.
//! - `append` for every overload: primitives, `String`, `Object`,
//!   `CharSequence[,int,int]`, `char[][,int,int]`, `StringBuffer`;
//!   `appendCodePoint`.
//!
//! **Editing:** `insert(int, X)`, `delete`, `deleteCharAt`, `replace`,
//! `reverse` (surrogate-aware), `setLength`, `setCharAt`.
//!
//! **Reading:**
//! - `charAt`, `codePointAt`, `indexOf`/`lastIndexOf` (with an optional
//!   index), `substring`, `subSequence`, `getChars`.
//! - `length`, `isEmpty`, `capacity`, `ensureCapacity`, `trimToSize`.
//! - `compareTo`, `toString`, `chars` (not modelled).
//!
//! **Return types.** Any method whose declared return is the builder type,
//! `Appendable`, `AbstractStringBuilder` or `CharSequence`-chaining returns
//! the receiver, so covariant bridge signatures work.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
#[cfg(not(feature = "std"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

const SB: &str = "Ljava/lang/StringBuilder;";
const SBUF: &str = "Ljava/lang/StringBuffer;";

/// Exact descriptor surface; the implementation's name dispatch is never a
/// license for arbitrary overloads or primitive return types.
pub fn method_metadata(
    class: &str,
    name: &str,
    sig: &str,
) -> Option<crate::framework_host::HostMethodKind> {
    use crate::framework_host::HostMethodKind::{Constructor, Instance};
    if !matches!(class, SB | SBUF) {
        return None;
    }
    let ps = crate::engine::parse_param_types(sig)?;
    let ret = sig.rsplit_once(')')?.1;
    if name == "<init>" {
        return matches!(
            sig,
            "()V" | "(I)V" | "(Ljava/lang/String;)V" | "(Ljava/lang/CharSequence;)V"
        )
        .then_some(Constructor);
    }
    let chain = ret == class
        || matches!(
            ret,
            "Ljava/lang/Appendable;" | "Ljava/lang/AbstractStringBuilder;"
        );
    let scalar = |p: &str| {
        matches!(
            p,
            "Z" | "C"
                | "I"
                | "J"
                | "F"
                | "D"
                | "Ljava/lang/Object;"
                | "Ljava/lang/String;"
                | "Ljava/lang/StringBuffer;"
                | "Ljava/lang/CharSequence;"
                | "[C"
        )
    };
    let implemented = match name {
        "append" if chain => match ps.as_slice() {
            [p] => scalar(p),
            [p, a, b] => {
                matches!(p.as_str(), "[C" | "Ljava/lang/CharSequence;") && a == "I" && b == "I"
            }
            _ => false,
        },
        "insert" if chain => match ps.as_slice() {
            [i, p] => i == "I" && scalar(p),
            [i, p, a, b] => {
                i == "I"
                    && matches!(p.as_str(), "[C" | "Ljava/lang/CharSequence;")
                    && a == "I"
                    && b == "I"
            }
            _ => false,
        },
        "appendCodePoint" | "deleteCharAt" if chain => ps == ["I"],
        "delete" if chain => ps == ["I", "I"],
        "replace" if chain => ps == ["I", "I", "Ljava/lang/String;"],
        "reverse" if chain => ps.is_empty(),
        "length" | "capacity" => sig == "()I",
        "isEmpty" => sig == "()Z",
        "charAt" => sig == "(I)C",
        "codePointAt" => sig == "(I)I",
        "setLength" | "ensureCapacity" => sig == "(I)V",
        "trimToSize" => sig == "()V",
        "setCharAt" => sig == "(IC)V",
        "indexOf" | "lastIndexOf" => {
            matches!(sig, "(Ljava/lang/String;)I" | "(Ljava/lang/String;I)I")
        }
        "substring" => matches!(sig, "(I)Ljava/lang/String;" | "(II)Ljava/lang/String;"),
        "subSequence" => sig == "(II)Ljava/lang/CharSequence;",
        "getChars" => sig == "(II[CI)V",
        "toString" => sig == "()Ljava/lang/String;",
        "compareTo" => ps == [class] && ret == "I",
        _ => false,
    };
    implemented.then_some(Instance)
}

fn sioobe(msg: String) -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/StringIndexOutOfBoundsException;",
        msg,
    ))
}

fn get(vm: &Engine, r: ObjRef) -> Vec<u16> {
    match vm.heap.get_field(r, "buf") {
        Ok(Value::Obj(s)) => match vm.heap.as_str(s) {
            Ok(text) => text.encode_utf16().collect(),
            Err(_) if vm.heap.class_of(s).is_ok_and(|c| c == "[C") => vm
                .heap
                .as_array_elems(s)
                .map(|(values, _)| {
                    values
                        .iter()
                        .map(|v| v.as_int().unwrap_or(0) as u16)
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}
fn set(vm: &mut Engine, r: ObjRef, u: &[u16]) -> Result<(), VmExit> {
    let s = match String::from_utf16(u) {
        // Keep the inherited String allocation failure contract. The new raw
        // backing below uses the array allocator's catchable Java OOM path.
        Ok(text) => vm.heap.alloc_string(text)?,
        Err(_) => {
            // Java permits an incomplete surrogate while a buffer is being
            // edited. Charge the real managed Value-array storage before any
            // reserve, and let ordinary heap edges/barriers retain it.
            let chars = vm
                .heap
                .alloc_array_zeroed(crate::heap::ElemKind::Int, u.len(), "[C")
                .map_err(crate::err::oom_throwable)?;
            for (slot, unit) in vm.heap.array_elems_mut(chars)?.iter_mut().zip(u) {
                *slot = Value::Int(i32::from(*unit));
            }
            chars
        }
    };
    crate::err::put_field_result(vm.heap.put_field(r, "buf", Value::Obj(s)))
}
fn string_text(units: &[u16]) -> Result<String, VmExit> {
    String::from_utf16(units).map_err(|_| {
        crate::err::vm_error(
            "API_UNAVAILABLE",
            "an isolated surrogate String requires the UTF-16 String heap",
        )
        .into()
    })
}
fn int(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}

fn text_of(vm: &mut Engine, v: Option<&Value>) -> Result<Vec<u16>, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => {
            let c = vm.heap.class_of(*r).unwrap_or("").to_string();
            if c == SB || c == SBUF {
                get(vm, *r)
            } else if let Ok(s) = vm.heap.as_str(*r) {
                s.encode_utf16().collect()
            } else {
                crate::intrinsics::object_to_string(vm, *r)?
                    .encode_utf16()
                    .collect()
            }
        }
        _ => "null".encode_utf16().collect(),
    })
}

fn chars_of(vm: &Engine, v: Option<&Value>) -> Result<Vec<u16>, VmExit> {
    match v {
        Some(Value::Obj(a)) => Ok(vm
            .heap
            .as_array_elems(*a)
            .map(|(e, _)| e.iter().map(|x| x.as_int().unwrap_or(0) as u16).collect())
            .unwrap_or_default()),
        _ => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "char[] is null",
        ))),
    }
}

/// Text appended/inserted for the value whose parameter type is `p`.
fn value_text(vm: &mut Engine, p: &str, v: Option<&Value>) -> Result<Vec<u16>, VmExit> {
    let s = match (p, v) {
        ("Z", Some(v)) => (v.as_int().unwrap_or(0) != 0).to_string(),
        ("C", Some(v)) => return Ok(vec![v.as_int().unwrap_or(0) as u16]),
        ("I" | "B" | "S", Some(v)) => v.as_int().unwrap_or(0).to_string(),
        ("J", Some(v)) => v.as_long().unwrap_or(0).to_string(),
        ("F", Some(v)) => crate::repr::java_float(v.as_float().unwrap_or(0.0)),
        ("D", Some(v)) => crate::repr::java_double(v.as_double().unwrap_or(0.0)),
        ("[C", v) => return chars_of(vm, v),
        (_, v) => return text_of(vm, v),
    };
    Ok(s.encode_utf16().collect())
}

fn params(sig: &str) -> Vec<String> {
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

fn check_range(start: i32, end: i32, len: usize) -> Result<(usize, usize), VmExit> {
    if start < 0 || start > end || end as usize > len {
        return Err(sioobe(format!("start {start}, end {end}, length {len}")));
    }
    Ok((start as usize, end as usize))
}

fn find(hay: &[u16], needle: &[u16], from: i32) -> i32 {
    let from = from.max(0) as usize;
    if needle.is_empty() {
        return from.min(hay.len()) as i32;
    }
    (from..=hay.len().saturating_sub(needle.len()))
        .find(|&i| i + needle.len() <= hay.len() && hay[i..i + needle.len()] == *needle)
        .map(|i| i as i32)
        .unwrap_or(-1)
}
fn rfind(hay: &[u16], needle: &[u16], from: i32) -> i32 {
    if from < 0 || needle.len() > hay.len() {
        return -1;
    }
    let start = (from as usize).min(hay.len() - needle.len());
    (0..=start)
        .rev()
        .find(|&i| hay[i..i + needle.len()] == *needle)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

pub fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if recv_class != SB && recv_class != SBUF {
        return Ok(None);
    }
    let ret = sig.rsplit(')').next().unwrap_or("");
    let chain = matches!(
        ret,
        "Ljava/lang/StringBuilder;"
            | "Ljava/lang/StringBuffer;"
            | "Ljava/lang/Appendable;"
            | "Ljava/lang/AbstractStringBuilder;"
    );
    let ps = params(sig);
    let mut u = if name == "<init>" {
        Vec::new()
    } else {
        get(vm, recv)
    };
    let len = u.len();
    let out: Value = match name {
        "<init>" => {
            let init = match ps.first().map(String::as_str) {
                None => Vec::new(),
                Some("I") => {
                    let cap = int(args, 0);
                    if cap < 0 {
                        return Err(VmExit::Exception(Throwable::new(
                            "Ljava/lang/NegativeArraySizeException;",
                            format!("{cap}"),
                        )));
                    }
                    Vec::new()
                }
                Some(_) => match args.first() {
                    Some(Value::Obj(_)) => text_of(vm, args.first())?,
                    _ => {
                        return Err(VmExit::Exception(Throwable::new(
                            "Ljava/lang/NullPointerException;",
                            "StringBuilder(null)",
                        )))
                    }
                },
            };
            set(vm, recv, &init)?;
            return Ok(Some(Value::Null));
        }
        "append" => {
            let add = match ps.as_slice() {
                [p, i, j] if p == "[C" && i == "I" && j == "I" => {
                    let all = chars_of(vm, args.first())?;
                    let (o, n) = (int(args, 1), int(args, 2));
                    if o < 0 || n < 0 || (o as usize).saturating_add(n as usize) > all.len() {
                        return Err(VmExit::Exception(Throwable::new(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            format!("offset {o}, count {n}, length {}", all.len()),
                        )));
                    }
                    all[o as usize..o as usize + n as usize].to_vec()
                }
                [_, i, j] if i == "I" && j == "I" => {
                    let all = text_of(vm, args.first())?;
                    let (a, b) =
                        check_range(int(args, 1), int(args, 2), all.len()).map_err(|_| {
                            VmExit::Exception(Throwable::new(
                                "Ljava/lang/IndexOutOfBoundsException;",
                                format!(
                                    "start {}, end {}, length {}",
                                    int(args, 1),
                                    int(args, 2),
                                    all.len()
                                ),
                            ))
                        })?;
                    all[a..b].to_vec()
                }
                [p] => value_text(vm, p, args.first())?,
                _ => return Ok(None),
            };
            u.extend(add);
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "appendCodePoint" => {
            let cp = int(args, 0);
            if !(0..=0x10ffff).contains(&cp) {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "Not a valid Unicode code point",
                )));
            }
            if cp < 0x10000 {
                u.push(cp as u16);
            } else {
                let cp = cp as u32 - 0x10000;
                u.push((0xd800 + (cp >> 10)) as u16);
                u.push((0xdc00 + (cp & 0x3ff)) as u16);
            }
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "insert" => {
            let at = int(args, 0);
            if at < 0 || at as usize > len {
                return Err(sioobe(format!("offset {at}, length {len}")));
            }
            let add = match ps.as_slice() {
                [_, p] => value_text(vm, p, args.get(1))?,
                [_, p, _, _] if p == "[C" => {
                    let all = chars_of(vm, args.get(1))?;
                    let (o, n) = (int(args, 2), int(args, 3));
                    if o < 0 || n < 0 || (o as usize).saturating_add(n as usize) > all.len() {
                        return Err(sioobe(format!("offset {o}, len {n}")));
                    }
                    all[o as usize..o as usize + n as usize].to_vec()
                }
                [_, _, _, _] => {
                    let all = text_of(vm, args.get(1))?;
                    let (a, b) = check_range(int(args, 2), int(args, 3), all.len())?;
                    all[a..b].to_vec()
                }
                _ => return Ok(None),
            };
            u.splice(at as usize..at as usize, add);
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "delete" => {
            let start = int(args, 0);
            let end = int(args, 1).min(len as i32);
            if start < 0 || start > end {
                return Err(sioobe(format!("start {start}, end {end}, length {len}")));
            }
            u.drain(start as usize..end as usize);
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "deleteCharAt" => {
            let i = int(args, 0);
            if i < 0 || i as usize >= len {
                return Err(sioobe(format!("index {i},length {len}")));
            }
            u.remove(i as usize);
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "replace" => {
            let start = int(args, 0);
            let end = int(args, 1).min(len as i32);
            if start < 0 || start as usize > len || start > end {
                return Err(sioobe(format!("start {start}, end {end}, length {len}")));
            }
            let add = text_of(vm, args.get(2))?;
            u.splice(start as usize..end as usize, add);
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "reverse" => {
            u.reverse();
            let mut at = 0;
            while at + 1 < u.len() {
                if (0xdc00..=0xdfff).contains(&u[at]) && (0xd800..=0xdbff).contains(&u[at + 1]) {
                    u.swap(at, at + 1);
                    at += 2;
                } else {
                    at += 1;
                }
            }
            set(vm, recv, &u)?;
            Value::Obj(recv)
        }
        "setLength" => {
            let n = int(args, 0);
            if n < 0 {
                return Err(sioobe(format!("String index out of range: {n}")));
            }
            let target = n as usize;
            if target > len {
                let avail = vm.heap.budget().saturating_sub(vm.heap.used());
                let needed = 32usize.saturating_add(target);
                if needed > avail || target.saturating_mul(2) > vm.heap.budget() {
                    return Err(VmExit::Exception(Throwable::new(
                        "Ljava/lang/OutOfMemoryError;",
                        format!(
                            "StringBuilder.setLength({n}) exceeds heap budget ({avail} B remaining)"
                        ),
                    )));
                }
            }
            u.resize(target, 0);
            set(vm, recv, &u)?;
            Value::Null
        }
        "setCharAt" => {
            let i = int(args, 0);
            if i < 0 || i as usize >= len {
                return Err(sioobe(format!("index {i},length {len}")));
            }
            u[i as usize] = int(args, 1) as u16;
            set(vm, recv, &u)?;
            Value::Null
        }
        "charAt" => {
            let i = int(args, 0);
            if i < 0 || i as usize >= len {
                return Err(sioobe(format!("index {i},length {len}")));
            }
            Value::Int(u[i as usize] as i32)
        }
        "codePointAt" => {
            let i = int(args, 0);
            if i < 0 || i as usize >= len {
                return Err(sioobe(format!("index {i},length {len}")));
            }
            let at = i as usize;
            let high = u[at];
            Value::Int(
                if (0xd800..=0xdbff).contains(&high)
                    && u.get(at + 1).is_some_and(|u| (0xdc00..=0xdfff).contains(u))
                {
                    0x10000 + ((i32::from(high) - 0xd800) << 10) + i32::from(u[at + 1]) - 0xdc00
                } else {
                    i32::from(high)
                },
            )
        }
        "indexOf" | "lastIndexOf" => {
            let needle = text_of(vm, args.first())?;
            if name == "indexOf" {
                Value::Int(find(
                    &u,
                    &needle,
                    if args.len() > 1 { int(args, 1) } else { 0 },
                ))
            } else {
                Value::Int(rfind(
                    &u,
                    &needle,
                    if args.len() > 1 {
                        int(args, 1)
                    } else {
                        len as i32
                    },
                ))
            }
        }
        "substring" | "subSequence" => {
            let start = int(args, 0);
            let end = if args.len() > 1 {
                int(args, 1)
            } else {
                len as i32
            };
            let (a, b) = check_range(start, end, len)?;
            Value::Obj(crate::intrinsics::alloc_string(vm, string_text(&u[a..b])?)?)
        }
        "getChars" => {
            let (a, b) = check_range(int(args, 0), int(args, 1), len)?;
            let Some(Value::Obj(dst)) = args.get(2) else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "dst",
                )));
            };
            let off = int(args, 3);
            let elems = vm
                .heap
                .array_elems_mut(*dst)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
            if off < 0 || (off as usize).saturating_add(b - a) > elems.len() {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "getChars",
                )));
            }
            let off = off as usize;
            for (k, c) in u[a..b].iter().enumerate() {
                elems[off + k] = Value::Int(*c as i32);
            }
            Value::Null
        }
        "length" => Value::Int(len as i32),
        "isEmpty" => Value::Int((len == 0) as i32),
        "capacity" => Value::Int((len + 16) as i32),
        "ensureCapacity" | "trimToSize" => Value::Null,
        "toString" => Value::Obj(crate::intrinsics::alloc_string(vm, string_text(&u)?)?),
        "compareTo" => {
            let other = text_of(vm, args.first())?;
            Value::Int(match u.cmp(&other) {
                core::cmp::Ordering::Less => -1,
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 1,
            })
        }
        _ => return Ok(None),
    };
    Ok(Some(if chain && matches!(out, Value::Null) {
        Value::Obj(recv)
    } else {
        out
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;
    #[test]
    fn raw_utf16_growth_checks_real_array_charge_and_keeps_previous_buffer_on_oom() {
        let mut vm = Engine::new(
            vec![],
            crate::VmConfig {
                heap_budget: 2048,
                ..Default::default()
            },
        );
        let builder = vm.heap.alloc_instance(SB.into(), vec![]).unwrap();
        vm.invoke_instance(builder, "<init>", "()V", &[]).unwrap();
        vm.invoke_instance(
            builder,
            "append",
            "(C)Ljava/lang/StringBuilder;",
            &[Value::Int(0xd83d)],
        )
        .unwrap();
        let input = vm.heap.alloc_string("x".repeat(128)).unwrap();
        let used = vm.heap.used();
        let error = vm.invoke_instance(
            builder,
            "append",
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
            &[Value::Obj(input)],
        );
        assert!(
            matches!(error,Err(VmExit::Exception(ref e)) if e.class == "Ljava/lang/OutOfMemoryError;"),
            "{error:?}"
        );
        assert_eq!(vm.heap.used(), used);
        assert_eq!(
            vm.invoke_instance(builder, "length", "()I", &[]).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            vm.invoke_instance(builder, "charAt", "(I)C", &[Value::Int(0)])
                .unwrap(),
            Value::Int(0xd83d)
        );
    }
    #[test]
    fn sig_params() {
        assert_eq!(
            params("(ILjava/lang/CharSequence;II)Ljava/lang/StringBuilder;"),
            vec!["I", "Ljava/lang/CharSequence;", "I", "I"]
        );
        assert_eq!(params("([CII)V"), vec!["[C", "I", "I"]);
        assert!(params("()V").is_empty());
    }
    #[test]
    fn search() {
        let h: Vec<u16> = "abcabc".encode_utf16().collect();
        let n: Vec<u16> = "bc".encode_utf16().collect();
        assert_eq!(find(&h, &n, 0), 1);
        assert_eq!(find(&h, &n, 2), 4);
        assert_eq!(rfind(&h, &n, 6), 4);
        assert_eq!(rfind(&h, &n, 3), 1);
    }
}
