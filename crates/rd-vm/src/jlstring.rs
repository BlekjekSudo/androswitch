//! `java.lang.String` methods beyond the core set in intrinsics.rs.
//!
//! Heap strings are Rust UTF-8, but Java indices are UTF-16 code units, so
//! every index-taking method converts through UTF-16. Covered here:
//! - **Search:** `indexOf`/`lastIndexOf` (char or string, with `fromIndex`),
//!   `startsWith(prefix, offset)`, `regionMatches`, `contains`,
//!   `contentEquals`.
//! - **Comparison:** `compareTo` (UTF-16 lexicographic, like the JDK),
//!   `compareToIgnoreCase`, `equalsIgnoreCase`.
//! - **Transforms:** `toLowerCase`/`toUpperCase` (with and without Locale;
//!   ROOT rules), `trim` (≤ U+0020), `strip`, `isBlank`, `repeat`,
//!   `subSequence`, `replace(CharSequence, CharSequence)`, `codePointAt`.
//! - **Regex** (`regex` crate): `split(regex[, limit])` with the JDK
//!   trailing-empty and limit rules, `replaceAll`, `replaceFirst`, `matches`.
//!   Java `$n` group references are honoured. A construct the engine lacks
//!   (backreferences, lookaround) raises `PatternSyntaxException`; the
//!   result is never silently wrong.
//! - **Bytes:** `getBytes(charsetName|Charset)` and the `String(byte[], …)`
//!   / `String(int[] codePoints, …)` constructors. UTF-8 is the default;
//!   ISO-8859-1 and US-ASCII are honoured by name.
//! - **Statics:** `String.format` (a `java.util.Formatter` subset: flags
//!   `-#+ 0,(`, width, precision, conversions `s S d x X o f e E g c b B h
//!   n %`, explicit indices `%1$s`; Locale-aware decimal and grouping
//!   separators), `String.join(CharSequence, Iterable)`.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;

const STRING: &str = "Ljava/lang/String;";

fn exc(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg))
}
fn npe(msg: &str) -> VmExit {
    exc("Ljava/lang/NullPointerException;", msg)
}
fn new_str(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, s)?))
}

/// Rejects host-side `String` allocations that would exceed `vm.heap.budget()`
/// before allocating multi-gigabyte buffers in the global Rust allocator (#270).
fn check_string_budget(vm: &Engine, byte_len: Option<usize>) -> Result<usize, VmExit> {
    let budget = vm.heap.budget();
    let used = vm.heap.used();
    let oom = |req: usize| {
        exc(
            "Ljava/lang/OutOfMemoryError;",
            format!("alocação de {req} bytes excede o heap de {budget} bytes"),
        )
    };
    let Some(len) = byte_len else {
        return Err(oom(usize::MAX));
    };
    let Some(heap_cost) = len.checked_mul(2).and_then(|b| b.checked_add(32)) else {
        return Err(oom(usize::MAX));
    };
    if used
        .checked_add(heap_cost)
        .map_or(true, |total| total > budget)
    {
        return Err(oom(heap_cost));
    }
    Ok(len)
}
fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn from_u16(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

/// A CharSequence argument as a Rust string (String directly; anything else
/// through its `toString()`).
fn charseq(vm: &mut Engine, v: &Value) -> Result<String, VmExit> {
    match v {
        Value::Obj(r) => match vm.heap.as_str(*r) {
            Ok(s) => Ok(s.to_string()),
            Err(_) => crate::intrinsics::object_to_string(vm, *r),
        },
        _ => Err(npe("CharSequence argument is null")),
    }
}

fn index_of(hay: &[u16], needle: &[u16], from: i32) -> i32 {
    let from = from.max(0) as usize;
    if needle.is_empty() {
        return from.min(hay.len()) as i32;
    }
    if needle.len() > hay.len() {
        return -1;
    }
    (from..=hay.len() - needle.len())
        .find(|&i| &hay[i..i + needle.len()] == needle)
        .map_or(-1, |i| i as i32)
}

fn last_index_of(hay: &[u16], needle: &[u16], from: i32) -> i32 {
    if from < 0 || needle.len() > hay.len() {
        return -1;
    }
    let start = (from as usize).min(hay.len() - needle.len());
    (0..=start)
        .rev()
        .find(|&i| &hay[i..i + needle.len()] == needle)
        .map_or(-1, |i| i as i32)
}

fn char_units(c: i32) -> Vec<u16> {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut b = [0u16; 2];
            ch.encode_utf16(&mut b).to_vec()
        }
        None => vec![c as u16],
    }
}

/// Shared Pattern/String engine: flags, lookaround, replacement and split
/// semantics must agree between java.lang.String and java.util.regex.
fn compile(pattern: &str) -> Result<fancy_regex::Regex, VmExit> {
    crate::jregex::compile(pattern, 0)
}

pub(crate) fn java_split(
    s: &str,
    re: &fancy_regex::Regex,
    limit: i32,
) -> Result<Vec<String>, VmExit> {
    crate::jregex::split(re, s, limit)
}

fn string_array(vm: &mut Engine, items: Vec<String>) -> Result<Value, VmExit> {
    let mut elems = Vec::with_capacity(items.len());
    for it in items {
        elems.push(new_str(vm, it)?);
    }
    let arr = vm.heap.alloc_array(ElemKind::Obj, elems.len(), STRING)?;
    let slot = vm
        .heap
        .array_elems_mut(arr)
        .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
    slot.clear();
    slot.extend(elems);
    Ok(Value::Obj(arr))
}

pub(crate) fn byte_array(vm: &mut Engine, bytes: &[u8]) -> Result<Value, VmExit> {
    let arr = vm.heap.alloc_array(ElemKind::Int, bytes.len(), "[B")?;
    let slot = vm
        .heap
        .array_elems_mut(arr)
        .map_err(|e| VmExit::from(crate::err::vm_error("INVALID_FORMAT", e)))?;
    slot.clear();
    slot.extend(bytes.iter().map(|b| Value::Int(*b as i8 as i32)));
    Ok(Value::Obj(arr))
}

fn int_elems(vm: &mut Engine, v: &Value) -> Result<Vec<i32>, VmExit> {
    let Value::Obj(r) = v else {
        return Err(npe("array is null"));
    };
    let elems = vm
        .heap
        .array_elems_mut(*r)
        .map_err(|e| VmExit::from(crate::err::vm_error("VM_TYPE_ERROR", e)))?;
    Ok(elems.iter().map(|e| e.as_int().unwrap_or(0)).collect())
}

pub(crate) fn charset_name(vm: &mut Engine, v: Option<&Value>) -> Result<String, VmExit> {
    Ok(match v {
        Some(Value::Obj(r)) => match vm.heap.as_str(*r) {
            Ok(s) => s.to_string(),
            // host Charset (jtext_format) keeps its name in `rd_name`
            Err(_) => match vm.heap.get_field(*r, "rd_name") {
                Ok(Value::Obj(n)) => vm.heap.as_str(n).map(str::to_string).unwrap_or_default(),
                _ => crate::intrinsics::object_to_string(vm, *r)?,
            },
        },
        _ => "UTF-8".to_string(),
    }
    .to_ascii_uppercase())
}

pub(crate) fn encode(s: &str, cs: &str) -> Vec<u8> {
    match cs {
        "ISO-8859-1" | "LATIN1" | "ISO8859_1" => s
            .chars()
            .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
            .collect(),
        "US-ASCII" | "ASCII" => s
            .chars()
            .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
            .collect(),
        "UTF-16" | "UTF_16" | "UTF-16BE" | "UTF_16BE" | "X-UTF-16BE" | "UNICODEBIGUNMARKED" => {
            s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
        }
        "UTF-16LE" | "UTF_16LE" | "X-UTF-16LE" | "UNICODELITTLEUNMARKED" => {
            s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
        }
        _ => s.as_bytes().to_vec(),
    }
}

fn decode_utf16_units(b: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(b.len() / 2);
    let chunks = b.chunks_exact(2);
    let has_rem = !chunks.remainder().is_empty();
    for pair in chunks {
        let u = if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        };
        units.push(u);
    }
    let mut out: String = char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect();
    if has_rem {
        out.push('\u{FFFD}');
    }
    out
}

pub(crate) fn decode(b: &[u8], cs: &str) -> String {
    match cs {
        "ISO-8859-1" | "LATIN1" | "ISO8859_1" => b.iter().map(|&x| x as char).collect(),
        "US-ASCII" | "ASCII" => b
            .iter()
            .map(|&x| if x < 128 { x as char } else { '\u{FFFD}' })
            .collect(),
        "UTF-16" | "UTF_16" => {
            if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
                decode_utf16_units(rest, false)
            } else if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
                decode_utf16_units(rest, true)
            } else {
                decode_utf16_units(b, false)
            }
        }
        "UTF-16BE" | "UTF_16BE" | "X-UTF-16BE" | "UNICODEBIGUNMARKED" => {
            decode_utf16_units(b, false)
        }
        "UTF-16LE" | "UTF_16LE" | "X-UTF-16LE" | "UNICODELITTLEUNMARKED" => {
            decode_utf16_units(b, true)
        }
        _ => String::from_utf8_lossy(b).into_owned(),
    }
}

fn string_matches(input: &str, pat: &str) -> Result<bool, VmExit> {
    // Validate raw pattern syntax and 64 KiB cap first.
    compile(pat)?;
    let body = crate::jregex::translate(pat, 0);
    let build = |s: &str| -> Result<fancy_regex::Regex, Box<fancy_regex::Error>> {
        fancy_regex::RegexBuilder::new(s)
            .backtrack_limit(100_000)
            .delegate_size_limit(2 << 20)
            .delegate_dfa_size_limit(2 << 20)
            .build()
            .map_err(Box::new)
    };
    let full = build(&format!(r"\A(?:{body})\z"))
        .or_else(|_| build(&format!("\\A(?:{body}\n)\\z")))
        .map_err(|e| {
            exc(
                "Ljava/util/regex/PatternSyntaxException;",
                format!("{pat}: not supported by RustyDroid's regex engine ({e})"),
            )
        })?;
    full.is_match(input).map_err(crate::jregex::runtime_error)
}

pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class != STRING
        && !(mref_class == "Ljava/lang/CharSequence;" && vm.heap.as_str(recv).is_ok())
    {
        return Ok(None);
    }
    if name == "<init>" {
        return string_ctor(vm, sig, recv, args);
    }
    // Do not clone the receiver for core String methods handled later in
    // intrinsics.rs (charAt/length/hashCode/substring/...). This gate is
    // deliberately name-only and over-inclusive for overloads shared with
    // the core dispatcher; keep it in sync with the match below.
    if !matches!(
        name,
        "codePointAt"
            | "compareTo"
            | "compareToIgnoreCase"
            | "contains"
            | "contentEquals"
            | "equalsIgnoreCase"
            | "indexOf"
            | "lastIndexOf"
            | "startsWith"
            | "regionMatches"
            | "subSequence"
            | "toLowerCase"
            | "toUpperCase"
            | "trim"
            | "strip"
            | "stripLeading"
            | "stripTrailing"
            | "isBlank"
            | "repeat"
            | "replace"
            | "split"
            | "replaceAll"
            | "replaceFirst"
            | "matches"
            | "getBytes"
    ) {
        return Ok(None);
    }
    let Ok(this) = vm.heap.as_str(recv).map(str::to_string) else {
        return Ok(None);
    };
    let int = |i: usize| args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let b = |x: bool| Value::Int(x as i32);
    Ok(Some(match (name, sig) {
        ("codePointAt", "(I)I") => {
            let u = u16s(&this);
            let i = int(0);
            if i < 0 || i as usize >= u.len() {
                return Err(exc(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("index {i}"),
                ));
            }
            let i = i as usize;
            let cp = char::decode_utf16(u[i..].iter().copied())
                .next()
                .and_then(|r| r.ok())
                .map_or(u[i] as i32, |c| c as i32);
            Value::Int(cp)
        }
        ("compareTo", "(Ljava/lang/String;)I") | ("compareTo", "(Ljava/lang/Object;)I") => {
            let o = charseq(vm, &args[0])?;
            let (a, c) = (u16s(&this), u16s(&o));
            let r = a
                .iter()
                .zip(&c)
                .find(|(x, y)| x != y)
                .map_or(a.len() as i32 - c.len() as i32, |(x, y)| {
                    *x as i32 - *y as i32
                });
            Value::Int(r)
        }
        ("compareToIgnoreCase", "(Ljava/lang/String;)I") => {
            let o = charseq(vm, &args[0])?;
            let (a, c) = (u16s(&this.to_lowercase()), u16s(&o.to_lowercase()));
            let r = a
                .iter()
                .zip(&c)
                .find(|(x, y)| x != y)
                .map_or(a.len() as i32 - c.len() as i32, |(x, y)| {
                    *x as i32 - *y as i32
                });
            Value::Int(r)
        }
        ("contains", "(Ljava/lang/CharSequence;)Z") => b(this.contains(&charseq(vm, &args[0])?)),
        ("contentEquals", "(Ljava/lang/CharSequence;)Z")
        | ("contentEquals", "(Ljava/lang/StringBuffer;)Z") => b(this == charseq(vm, &args[0])?),
        ("equalsIgnoreCase", "(Ljava/lang/String;)Z") => match &args[0] {
            Value::Obj(_) => {
                let o = charseq(vm, &args[0])?;
                b(this.chars().count() == o.chars().count()
                    && this
                        .chars()
                        .zip(o.chars())
                        .all(|(x, y)| x == y || x.to_lowercase().eq(y.to_lowercase())))
            }
            _ => b(false),
        },
        ("indexOf", "(I)I") => Value::Int(index_of(&u16s(&this), &char_units(int(0)), 0)),
        ("indexOf", "(II)I") => Value::Int(index_of(&u16s(&this), &char_units(int(0)), int(1))),
        ("indexOf", "(Ljava/lang/String;I)I") => {
            let n = charseq(vm, &args[0])?;
            Value::Int(index_of(&u16s(&this), &u16s(&n), int(1)))
        }
        ("lastIndexOf", "(I)I") => {
            let u = u16s(&this);
            Value::Int(last_index_of(&u, &char_units(int(0)), u.len() as i32))
        }
        ("lastIndexOf", "(II)I") => {
            Value::Int(last_index_of(&u16s(&this), &char_units(int(0)), int(1)))
        }
        ("lastIndexOf", "(Ljava/lang/String;)I") => {
            let n = charseq(vm, &args[0])?;
            let u = u16s(&this);
            Value::Int(last_index_of(&u, &u16s(&n), u.len() as i32))
        }
        ("lastIndexOf", "(Ljava/lang/String;I)I") => {
            let n = charseq(vm, &args[0])?;
            Value::Int(last_index_of(&u16s(&this), &u16s(&n), int(1)))
        }
        ("startsWith", "(Ljava/lang/String;I)Z") => {
            let p = u16s(&charseq(vm, &args[0])?);
            let u = u16s(&this);
            let off = int(1);
            b(usize::try_from(off)
                .ok()
                .and_then(|o| o.checked_add(p.len()).map(|end| (o, end)))
                .is_some_and(|(o, end)| end <= u.len() && u[o..end] == p[..]))
        }
        ("regionMatches", "(ILjava/lang/String;II)Z")
        | ("regionMatches", "(ZILjava/lang/String;II)Z") => {
            let (ic, base) = if sig.starts_with("(Z") {
                (int(0) != 0, 1)
            } else {
                (false, 0)
            };
            let to = int(base);
            let other = charseq(vm, &args[base + 1])?;
            let oo = int(base + 2);
            let len = int(base + 3);
            let (a, o) = (u16s(&this), u16s(&other));
            if to < 0
                || oo < 0
                || to as i64 + len as i64 > a.len() as i64
                || oo as i64 + len as i64 > o.len() as i64
            {
                b(false)
            } else if len <= 0 {
                b(true)
            } else {
                let (to_u, oo_u, len_u) = (to as usize, oo as usize, len as usize);
                let x = from_u16(&a[to_u..to_u + len_u]);
                let y = from_u16(&o[oo_u..oo_u + len_u]);
                b(if ic {
                    x.to_lowercase() == y.to_lowercase()
                } else {
                    x == y
                })
            }
        }
        ("subSequence", "(II)Ljava/lang/CharSequence;") => {
            let u = u16s(&this);
            let (s, e) = (int(0), int(1));
            if s < 0 || e < s || e as usize > u.len() {
                return Err(exc(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("begin {s}, end {e}, length {}", u.len()),
                ));
            }
            new_str(vm, from_u16(&u[s as usize..e as usize]))?
        }
        ("toLowerCase", "()Ljava/lang/String;")
        | ("toLowerCase", "(Ljava/util/Locale;)Ljava/lang/String;") => {
            if this.chars().all(|c| !c.is_uppercase()) {
                Value::Obj(recv)
            } else {
                new_str(vm, this.to_lowercase())?
            }
        }
        ("toUpperCase", "()Ljava/lang/String;")
        | ("toUpperCase", "(Ljava/util/Locale;)Ljava/lang/String;") => {
            if this.chars().all(|c| !c.is_lowercase()) {
                Value::Obj(recv)
            } else {
                new_str(vm, this.to_uppercase())?
            }
        }
        ("trim", "()Ljava/lang/String;") => {
            let t = this.trim_matches(|c: char| c <= ' ');
            if t.len() == this.len() {
                Value::Obj(recv)
            } else {
                new_str(vm, t.to_string())?
            }
        }
        ("strip", "()Ljava/lang/String;") => new_str(vm, this.trim().to_string())?,
        ("stripLeading", "()Ljava/lang/String;") => new_str(vm, this.trim_start().to_string())?,
        ("stripTrailing", "()Ljava/lang/String;") => new_str(vm, this.trim_end().to_string())?,
        ("isBlank", "()Z") => b(this.trim().is_empty()),
        ("repeat", "(I)Ljava/lang/String;") => {
            let n = int(0);
            if n < 0 {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("count is negative: {n}"),
                ));
            }
            if n == 0 || this.is_empty() {
                new_str(vm, String::new())?
            } else if n == 1 {
                Value::Obj(recv)
            } else {
                check_string_budget(vm, this.len().checked_mul(n as usize))?;
                new_str(vm, this.repeat(n as usize))?
            }
        }
        ("replace", "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;") => {
            let (t, r) = (charseq(vm, &args[0])?, charseq(vm, &args[1])?);
            if t.is_empty() {
                // JDK: empty target inserts the replacement between every char
                let n_chars = this.chars().count();
                let est = n_chars
                    .checked_add(1)
                    .and_then(|k| k.checked_mul(r.len()))
                    .and_then(|b| b.checked_add(this.len()));
                check_string_budget(vm, est)?;
                let mut out = r.clone();
                for c in this.chars() {
                    out.push(c);
                    out.push_str(&r);
                }
                new_str(vm, out)?
            } else {
                new_str(vm, this.replace(&t, &r))?
            }
        }
        ("split", "(Ljava/lang/String;)[Ljava/lang/String;")
        | ("split", "(Ljava/lang/String;I)[Ljava/lang/String;") => {
            let pat = charseq(vm, &args[0])?;
            let limit = if args.len() > 1 { int(1) } else { 0 };
            let re = compile(&pat)?;
            let parts = java_split(&this, &re, limit)?;
            string_array(vm, parts)?
        }
        ("replaceAll", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;") => {
            let (p, r) = (charseq(vm, &args[0])?, charseq(vm, &args[1])?);
            let out = crate::jregex::replace(&compile(&p)?, &this, &r, true)?;
            new_str(vm, out)?
        }
        ("replaceFirst", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;") => {
            let (p, r) = (charseq(vm, &args[0])?, charseq(vm, &args[1])?);
            let out = crate::jregex::replace(&compile(&p)?, &this, &r, false)?;
            new_str(vm, out)?
        }
        ("matches", "(Ljava/lang/String;)Z") => {
            let p = charseq(vm, &args[0])?;
            b(string_matches(&this, &p)?)
        }
        ("getBytes", "()[B") => byte_array(vm, this.as_bytes())?,
        ("getBytes", "(Ljava/lang/String;)[B") | ("getBytes", "(Ljava/nio/charset/Charset;)[B") => {
            let cs = charset_name(vm, args.first())?;
            byte_array(vm, &encode(&this, &cs))?
        }
        _ => return Ok(None),
    }))
}

fn string_ctor(
    vm: &mut Engine,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let s = match sig {
        "([B)V" => {
            let b: Vec<u8> = int_elems(vm, &args[0])?.iter().map(|x| *x as u8).collect();
            decode(&b, "UTF-8")
        }
        "([BLjava/lang/String;)V" | "([BLjava/nio/charset/Charset;)V" => {
            let b: Vec<u8> = int_elems(vm, &args[0])?.iter().map(|x| *x as u8).collect();
            let cs = charset_name(vm, args.get(1))?;
            decode(&b, &cs)
        }
        "([BII)V" | "([BIILjava/lang/String;)V" | "([BIILjava/nio/charset/Charset;)V" => {
            let all = int_elems(vm, &args[0])?;
            let (o, n) = (args[1].as_int()?, args[2].as_int()?);
            let Some((start, end)) = usize::try_from(o)
                .ok()
                .zip(usize::try_from(n).ok())
                .and_then(|(s, len)| s.checked_add(len).map(|e| (s, e)))
                .filter(|&(_, end)| end <= all.len())
            else {
                return Err(exc(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("offset {o}, count {n}, length {}", all.len()),
                ));
            };
            let b: Vec<u8> = all[start..end].iter().map(|x| *x as u8).collect();
            let cs = charset_name(vm, args.get(3))?;
            decode(&b, &cs)
        }
        "([III)V" => {
            let all = int_elems(vm, &args[0])?;
            let (o, n) = (args[1].as_int()?, args[2].as_int()?);
            let Some((start, end)) = usize::try_from(o)
                .ok()
                .zip(usize::try_from(n).ok())
                .and_then(|(s, len)| s.checked_add(len).map(|e| (s, e)))
                .filter(|&(_, end)| end <= all.len())
            else {
                return Err(exc(
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("offset {o}, count {n}"),
                ));
            };
            let mut out = String::new();
            for cp in &all[start..end] {
                match char::from_u32(*cp as u32) {
                    Some(c) => out.push(c),
                    None => {
                        return Err(exc(
                            "Ljava/lang/IllegalArgumentException;",
                            format!("invalid code point {cp}"),
                        ))
                    }
                }
            }
            out
        }
        _ => return Ok(None),
    };
    vm.heap
        .initialize_string(recv, s)
        .map_err(crate::err::oom_throwable)?;
    Ok(Some(Value::Null))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if class != STRING {
        return Ok(None);
    }
    match (name, sig) {
        ("format", "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;") => {
            let f = charseq(vm, &args[0])?;
            let items = obj_array(vm, args.get(1))?;
            let s = java_format(vm, &f, &items, ('.', ','))?;
            Ok(Some(new_str(vm, s)?))
        }
        (
            "format",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        ) => {
            let seps = locale_seps(vm, args.first())?;
            let f = charseq(vm, &args[1])?;
            let items = obj_array(vm, args.get(2))?;
            let s = java_format(vm, &f, &items, seps)?;
            Ok(Some(new_str(vm, s)?))
        }
        ("join", "(Ljava/lang/CharSequence;Ljava/lang/Iterable;)Ljava/lang/String;") => {
            let d = charseq(vm, &args[0])?;
            let elems = match &args[1] {
                Value::Obj(r) => match vm.fw.get(*r) {
                    Some(HostObj::List { elems }) => elems.clone(),
                    _ => return Ok(None), // DEX Iterable: explicit gap
                },
                _ => return Err(npe("String.join(_, null)")),
            };
            let mut parts = Vec::new();
            for e in elems {
                parts.push(match e {
                    Value::Null => "null".to_string(),
                    v => charseq(vm, &v)?,
                });
            }
            Ok(Some(new_str(vm, parts.join(&d))?))
        }
        _ => Ok(None),
    }
}

fn obj_array(vm: &mut Engine, v: Option<&Value>) -> Result<Vec<Value>, VmExit> {
    match v {
        Some(Value::Obj(r)) => Ok(vm
            .heap
            .array_elems_mut(*r)
            .map_err(|e| VmExit::from(crate::err::vm_error("VM_TYPE_ERROR", e)))?
            .clone()),
        _ => Ok(Vec::new()),
    }
}

/// (decimal separator, grouping separator) for a Locale argument.
fn locale_seps(vm: &mut Engine, v: Option<&Value>) -> Result<(char, char), VmExit> {
    let tag = match v {
        Some(Value::Obj(r)) => crate::intrinsics::object_to_string(vm, *r).unwrap_or_default(),
        _ => String::new(),
    };
    let lang = tag
        .split(['_', '-'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    Ok(match lang.as_str() {
        "pt" | "de" | "es" | "it" | "nl" | "id" | "tr" | "da" | "ro" | "el" => (',', '.'),
        "fr" | "ru" | "pl" | "cs" | "sv" | "nb" | "fi" | "uk" => (',', '\u{a0}'),
        _ => ('.', ','),
    })
}

enum Num {
    I(i128),
    F(f64),
}

fn as_num(vm: &Engine, v: &Value) -> Option<Num> {
    match v {
        Value::Int(i) => Some(Num::I(*i as i128)),
        Value::Long(l) => Some(Num::I(*l as i128)),
        Value::Float(f) => Some(Num::F(*f as f64)),
        Value::Double(d) => Some(Num::F(*d)),
        Value::Obj(r) => {
            let class = vm.heap.class_of(*r).ok()?;
            let k = crate::boxes::kind(class)?;
            let inner = vm.heap.get_field(*r, "value").ok()?;
            match (k, inner) {
                ('F' | 'D', Value::Float(f)) => Some(Num::F(f as f64)),
                ('F' | 'D', Value::Double(d)) => Some(Num::F(d)),
                (_, Value::Long(l)) => Some(Num::I(l as i128)),
                (_, Value::Int(i)) => Some(Num::I(i as i128)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn group(digits: &str, sep: char) -> String {
    let n = digits.len();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// `java.util.Formatter` subset (see the module docs).
pub(crate) fn java_format(
    vm: &mut Engine,
    f: &str,
    args: &[Value],
    seps: (char, char),
) -> Result<String, VmExit> {
    let (dec, grp) = seps;
    let mut out = String::new();
    let chars: Vec<char> = f.chars().collect();
    let mut i = 0;
    let mut next_arg = 0usize;
    let bad = |m: String| exc("Ljava/util/UnknownFormatConversionException;", m);
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        // [index$][flags][width][.precision]conversion
        let start = i;
        let mut num = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            num.push(chars[i]);
            i += 1;
        }
        let mut explicit: Option<usize> = None;
        if i < chars.len() && chars[i] == '$' && !num.is_empty() {
            let idx_1based = num
                .parse::<usize>()
                .ok()
                .filter(|&n| n >= 1)
                .ok_or_else(|| bad(format!("{num}$")))?;
            explicit = Some(idx_1based - 1);
            i += 1;
        } else {
            i = start;
        }
        let mut flags = String::new();
        while i < chars.len() && "-#+ 0,(".contains(chars[i]) {
            flags.push(chars[i]);
            i += 1;
        }
        let mut width_str = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            width_str.push(chars[i]);
            i += 1;
        }
        let mut prec_str: Option<String> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                p.push(chars[i]);
                i += 1;
            }
            prec_str = Some(p);
        }
        let Some(&conv) = chars.get(i) else {
            return Err(bad("%".into()));
        };
        i += 1;
        let width: usize = if width_str.is_empty() {
            0
        } else {
            match width_str.parse::<usize>() {
                Ok(w) => {
                    check_string_budget(vm, out.len().checked_add(w))?;
                    w
                }
                Err(_) => {
                    check_string_budget(vm, None)?;
                    0
                }
            }
        };
        let prec: Option<usize> = match &prec_str {
            None => None,
            Some(ps) if ps.is_empty() => Some(0),
            Some(ps) => match ps.parse::<usize>() {
                Ok(p) => {
                    if matches!(conv, 'f' | 'e' | 'E' | 'g' | 'G') {
                        check_string_budget(vm, out.len().checked_add(p))?;
                    }
                    Some(p)
                }
                Err(_) if matches!(conv, 'f' | 'e' | 'E' | 'g' | 'G') => {
                    check_string_budget(vm, None)?;
                    None
                }
                Err(_) => Some(usize::MAX),
            },
        };
        let mut take = || -> Result<Value, VmExit> {
            let idx = explicit.unwrap_or_else(|| {
                let k = next_arg;
                next_arg += 1;
                k
            });
            args.get(idx).cloned().ok_or_else(|| {
                exc(
                    "Ljava/util/MissingFormatArgumentException;",
                    format!("Format specifier '%{conv}'"),
                )
            })
        };
        let body: String = match conv {
            '%' => "%".into(),
            'n' => "\n".into(),
            's' | 'S' => {
                let v = take()?;
                let s = match &v {
                    Value::Null => "null".to_string(),
                    Value::Obj(_) => charseq(vm, &v)?,
                    Value::Int(x) => x.to_string(),
                    Value::Long(x) => x.to_string(),
                    Value::Float(x) => x.to_string(),
                    Value::Double(x) => x.to_string(),
                    _ => String::new(),
                };
                let s = match prec {
                    Some(p) => s.chars().take(p).collect(),
                    None => s,
                };
                if conv == 'S' {
                    s.to_uppercase()
                } else {
                    s
                }
            }
            'b' | 'B' => {
                let v = take()?;
                let t = match &v {
                    Value::Null => false,
                    Value::Obj(r) if vm.heap.class_of(*r).ok() == Some("Ljava/lang/Boolean;") => {
                        matches!(vm.heap.get_field(*r, "value"), Ok(Value::Int(x)) if x != 0)
                    }
                    _ => true,
                };
                let s = t.to_string();
                if conv == 'B' {
                    s.to_uppercase()
                } else {
                    s
                }
            }
            'c' => {
                let v = take()?;
                let cp = match as_num(vm, &v) {
                    Some(Num::I(x)) => x as u32,
                    _ => match &v {
                        Value::Obj(r) => match vm.heap.get_field(*r, "value") {
                            Ok(Value::Int(x)) => x as u32,
                            _ => 0,
                        },
                        _ => 0,
                    },
                };
                char::from_u32(cp).map(String::from).unwrap_or_default()
            }
            'h' | 'H' => {
                let v = take()?;
                let h = match &v {
                    Value::Null => "null".to_string(),
                    Value::Obj(r) => format!("{r:x}"),
                    _ => "0".to_string(),
                };
                if conv == 'H' {
                    h.to_uppercase()
                } else {
                    h
                }
            }
            'd' | 'x' | 'X' | 'o' => {
                let v = take()?;
                let n = match as_num(vm, &v) {
                    Some(Num::I(x)) => x,
                    _ if matches!(v, Value::Null) => {
                        let chunk = pad("null".into(), width, &flags);
                        check_string_budget(vm, out.len().checked_add(chunk.len()))?;
                        out.push_str(&chunk);
                        continue;
                    }
                    _ => {
                        return Err(exc(
                            "Ljava/util/IllegalFormatConversionException;",
                            format!("{conv} != {}", value_class(vm, &v)),
                        ))
                    }
                };
                match conv {
                    'd' => {
                        let neg = n < 0;
                        let mut digits = n.unsigned_abs().to_string();
                        if flags.contains(',') {
                            digits = group(&digits, grp);
                        }
                        sign_pad(digits, neg, width, &flags)
                    }
                    _ => {
                        // two's complement for negatives, at the arg width
                        let wide = matches!(v, Value::Long(_))
                            || matches!(&v, Value::Obj(r) if vm.heap.class_of(*r).ok() == Some("Ljava/lang/Long;"));
                        let u: u64 = if wide {
                            n as i64 as u64
                        } else {
                            n as i32 as u32 as u64
                        };
                        let mut s = match conv {
                            'x' => format!("{u:x}"),
                            'X' => format!("{u:X}"),
                            _ => format!("{u:o}"),
                        };
                        if flags.contains('#') {
                            s = match conv {
                                'o' => format!("0{s}"),
                                'X' => format!("0X{s}"),
                                _ => format!("0x{s}"),
                            };
                        }
                        if flags.contains('0') && s.len() < width {
                            s = format!("{}{s}", "0".repeat(width - s.len()));
                        }
                        s
                    }
                }
            }
            'f' | 'e' | 'E' | 'g' | 'G' => {
                let v = take()?;
                let x = match as_num(vm, &v) {
                    Some(Num::F(x)) => x,
                    Some(Num::I(_)) => {
                        return Err(exc(
                            "Ljava/util/IllegalFormatConversionException;",
                            format!("{conv} != {}", value_class(vm, &v)),
                        ))
                    }
                    None if matches!(v, Value::Null) => {
                        let chunk = pad("null".into(), width, &flags);
                        check_string_budget(vm, out.len().checked_add(chunk.len()))?;
                        out.push_str(&chunk);
                        continue;
                    }
                    None => {
                        return Err(exc(
                            "Ljava/util/IllegalFormatConversionException;",
                            format!("{conv} != {}", value_class(vm, &v)),
                        ))
                    }
                };
                if x.is_nan() || x.is_infinite() {
                    let s = if x.is_nan() {
                        "NaN".to_string()
                    } else if x > 0.0 {
                        "Infinity".into()
                    } else {
                        "-Infinity".into()
                    };
                    pad(s, width, &flags)
                } else {
                    let p = prec.unwrap_or(6);
                    let neg = x.is_sign_negative() && x != 0.0;
                    let a = x.abs();
                    let body = match conv {
                        'f' => {
                            let s = format!("{a:.p$}");
                            let (ip, fp) = s
                                .split_once('.')
                                .map_or((s.clone(), String::new()), |(a, b)| {
                                    (a.to_string(), b.to_string())
                                });
                            let ip = if flags.contains(',') {
                                group(&ip, grp)
                            } else {
                                ip
                            };
                            if fp.is_empty() {
                                ip
                            } else {
                                format!("{ip}{dec}{fp}")
                            }
                        }
                        'e' | 'E' => {
                            let s = format!("{a:.p$e}");
                            let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
                            let ev: i32 = e.parse().unwrap_or(0);
                            let m = m.replace('.', &dec.to_string());
                            let r =
                                format!("{m}e{}{:02}", if ev < 0 { '-' } else { '+' }, ev.abs());
                            if conv == 'E' {
                                r.to_uppercase()
                            } else {
                                r
                            }
                        }
                        _ => {
                            let p = if p == 0 { 1 } else { p };
                            let exp = if a == 0.0 {
                                0
                            } else {
                                a.log10().floor() as i32
                            };
                            if a != 0.0 && (exp < -4 || exp >= p as i32) {
                                let s = format!("{a:.*e}", p - 1);
                                let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
                                let ev: i32 = e.parse().unwrap_or(0);
                                format!(
                                    "{}e{}{:02}",
                                    m.replace('.', &dec.to_string()),
                                    if ev < 0 { '-' } else { '+' },
                                    ev.abs()
                                )
                            } else {
                                let decimals = (p as i32 - 1 - exp).max(0) as usize;
                                format!("{a:.decimals$}").replace('.', &dec.to_string())
                            }
                        }
                    };
                    sign_pad(body, neg, width, &flags)
                }
            }
            other => return Err(bad(other.to_string())),
        };
        let chunk = if matches!(conv, 'd' | 'f' | 'e' | 'E' | 'g' | 'G') {
            body // sign_pad already applied width
        } else {
            pad(body, width, &flags)
        };
        check_string_budget(vm, out.len().checked_add(chunk.len()))?;
        out.push_str(&chunk);
    }
    Ok(out)
}

fn value_class(vm: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => vm
            .heap
            .class_of(*r)
            .unwrap_or("?")
            .trim_start_matches('L')
            .trim_end_matches(';')
            .replace('/', "."),
        Value::Int(_) => "java.lang.Integer".into(),
        Value::Long(_) => "java.lang.Long".into(),
        Value::Float(_) => "java.lang.Float".into(),
        Value::Double(_) => "java.lang.Double".into(),
        _ => "null".into(),
    }
}

fn pad(s: String, width: usize, flags: &str) -> String {
    let n = s.chars().count();
    if n >= width {
        return s;
    }
    if flags.contains('-') {
        format!("{s}{}", " ".repeat(width - n))
    } else {
        format!("{}{s}", " ".repeat(width - n))
    }
}

fn sign_pad(digits: String, neg: bool, width: usize, flags: &str) -> String {
    let (pre, post) = if neg {
        if flags.contains('(') {
            ("(", ")")
        } else {
            ("-", "")
        }
    } else if flags.contains('+') {
        ("+", "")
    } else if flags.contains(' ') {
        (" ", "")
    } else {
        ("", "")
    };
    let len = pre.len() + digits.chars().count() + post.len();
    if len >= width {
        return format!("{pre}{digits}{post}");
    }
    let fill = width - len;
    if flags.contains('-') {
        format!("{pre}{digits}{post}{}", " ".repeat(fill))
    } else if flags.contains('0') {
        format!("{pre}{}{digits}{post}", "0".repeat(fill))
    } else {
        format!("{}{pre}{digits}{post}", " ".repeat(fill))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_search() {
        let h = u16s("a.b.c");
        assert_eq!(last_index_of(&h, &char_units('.' as i32), 4), 3);
        assert_eq!(last_index_of(&h, &char_units('.' as i32), 2), 1);
        assert_eq!(index_of(&h, &char_units('.' as i32), 2), 3);
        assert_eq!(index_of(&h, &u16s("x"), 0), -1);
    }

    #[test]
    fn split_follows_jdk() {
        let re = fancy_regex::Regex::new(",").unwrap();
        assert_eq!(java_split("a,b,,", &re, 0).unwrap(), vec!["a", "b"]);
        assert_eq!(
            java_split("a,b,,", &re, -1).unwrap(),
            vec!["a", "b", "", ""]
        );
        assert_eq!(java_split("a,b,c", &re, 2).unwrap(), vec!["a", "b,c"]);
        assert_eq!(java_split("", &re, 0).unwrap(), vec![""]);
        let ws = fancy_regex::Regex::new(r"\s+").unwrap();
        assert_eq!(java_split("x  y z", &ws, 0).unwrap(), vec!["x", "y", "z"]);
    }

    #[test]
    fn replacement_syntax() {
        let re = compile(r"(\d+)-(\d+)").unwrap();
        assert_eq!(
            crate::jregex::replace(&re, "1-2", "$2:$1", true).unwrap(),
            "2:1"
        );
        assert_eq!(
            crate::jregex::replace(&re, "1-2", r"\$", true).unwrap(),
            "$"
        );
    }

    #[test]
    fn grouping_and_padding() {
        assert_eq!(group("1234567", '.'), "1.234.567");
        assert_eq!(sign_pad("42".into(), true, 6, "0"), "-00042");
        assert_eq!(sign_pad("42".into(), false, 5, "-"), "42   ");
        assert_eq!(pad("ab".into(), 4, ""), "  ab");
    }
}
