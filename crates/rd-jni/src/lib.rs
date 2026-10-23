//! JNI wire types, descriptors, mangling, modified UTF-8 and reference frames.
//! No raw host pointers: native references are opaque tokens (ADR 004).
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::String,
    vec,
    vec::Vec,
};
#[cfg(feature = "jit")]
pub mod jit;

use serde::{Deserialize, Serialize};
pub const MODULE_ID: &str = "rd-jni";
pub const MILESTONE: &str = "M9";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdError {
    pub code: String,
    pub cause: String,
    pub suggestion: Option<String>,
    pub module_id: String,
}
impl RdError {
    pub fn new(code: &str, cause: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            cause: cause.into(),
            suggestion: Some("see docs/m9.md".into()),
            module_id: MODULE_ID.into(),
        }
    }
    pub fn not_implemented() -> Self {
        Self::new("NOT_IMPLEMENTED", "JNI operation not supported")
    }
}
impl core::fmt::Display for RdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} [{}]: {}", self.code, self.module_id, self.cause)
    }
}
#[cfg(feature = "std")]
impl std::error::Error for RdError {}
pub type Result<T> = core::result::Result<T, RdError>;
pub fn not_implemented<T>() -> Result<T> {
    Err(RdError::not_implemented())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handle {
    Object(u32),
    Class(String),
    Method {
        class: String,
        name: String,
        sig: String,
        is_static: bool,
    },
    Field {
        class: String,
        name: String,
        sig: String,
        is_static: bool,
    },
}
#[derive(Debug)]
pub struct References {
    next: u64,
    values: BTreeMap<u64, Handle>,
    frames: Vec<BTreeSet<u64>>,
    globals: BTreeSet<u64>,
    pub pending: Option<(String, String)>,
}
impl Default for References {
    fn default() -> Self {
        Self {
            next: 1,
            values: BTreeMap::new(),
            frames: vec![BTreeSet::new()],
            globals: BTreeSet::new(),
            pending: None,
        }
    }
}
impl References {
    pub fn object_roots(&self) -> impl Iterator<Item = u32> + '_ {
        self.values.values().filter_map(|v| {
            if let Handle::Object(r) = v {
                Some(*r)
            } else {
                None
            }
        })
    }
    pub const MAX: usize = 65536;
    pub fn insert(&mut self, v: Handle) -> Result<u64> {
        if self.values.len() >= Self::MAX {
            return Err(RdError::new(
                "JNI_LOCAL_OVERFLOW",
                "reference budget exceeded",
            ));
        }
        let h = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| RdError::new("JNI_LOCAL_OVERFLOW", "handle overflow"))?;
        self.values.insert(h, v);
        self.frames.last_mut().unwrap().insert(h);
        Ok(h)
    }
    pub fn get(&self, h: u64) -> Result<&Handle> {
        self.values.get(&h).ok_or_else(|| {
            RdError::new(
                "JNI_INVALID_REFERENCE",
                format!("invalid/deleted JNI reference {h}"),
            )
        })
    }
    pub fn duplicate(&mut self, h: u64, global: bool) -> Result<u64> {
        if h == 0 {
            return Ok(0);
        }
        let v = self.get(h)?.clone();
        let n = self.insert(v)?;
        if global {
            self.frames.last_mut().unwrap().remove(&n);
            self.globals.insert(n);
        }
        Ok(n)
    }
    pub fn delete_local(&mut self, h: u64) {
        for frame in &mut self.frames {
            if frame.remove(&h) {
                self.values.remove(&h);
                break;
            }
        }
    }
    pub fn delete_global(&mut self, h: u64) {
        if self.globals.remove(&h) {
            self.values.remove(&h);
        }
    }
    pub fn push(&mut self) -> Result<()> {
        if self.frames.len() >= 512 {
            return Err(RdError::new("JNI_LOCAL_OVERFLOW", "too many local frames"));
        }
        self.frames.push(BTreeSet::new());
        Ok(())
    }
    pub fn pop(&mut self, result: u64) -> Result<u64> {
        if self.frames.len() <= 1 {
            return Err(RdError::new("JNI_INVALID_FRAME", "cannot pop root frame"));
        }
        let value = if result == 0 {
            None
        } else {
            Some(self.get(result)?.clone())
        };
        for h in self.frames.pop().unwrap() {
            self.values.remove(&h);
        }
        value.map(|v| self.insert(v)).unwrap_or(Ok(0))
    }
    pub fn same(&self, a: u64, b: u64) -> Result<bool> {
        if a == 0 || b == 0 {
            return Ok(a == b);
        }
        Ok(self.get(a)? == self.get(b)?)
    }
}
/// JNI names escape UTF-16 code units, not Unicode scalar values.
pub fn mangle(s: &str) -> String {
    let mut r = String::new();
    for c in s.encode_utf16() {
        match c {
            47 => r.push('_'),
            95 => r.push_str("_1"),
            59 => r.push_str("_2"),
            91 => r.push_str("_3"),
            48..=57 | 65..=90 | 97..=122 => r.push(char::from_u32(c as u32).unwrap()),
            _ => r.push_str(&format!("_0{c:04x}")),
        }
    }
    r
}
pub fn symbols(class: &str, name: &str, sig: &str) -> Result<[String; 2]> {
    let (args, _) = descriptor(sig)?;
    let _ = args;
    let c = class
        .strip_prefix('L')
        .and_then(|s| s.strip_suffix(';'))
        .ok_or_else(|| RdError::new("JNI_SIGNATURE", "invalid class descriptor"))?;
    let p = sig.find(')').unwrap();
    let short = format!("Java_{}_{}", mangle(c), mangle(name));
    Ok([short.clone(), format!("{short}__{}", mangle(&sig[1..p]))])
}
pub fn descriptor(sig: &str) -> Result<(Vec<String>, String)> {
    fn one(b: &[u8], p: &mut usize, void: bool) -> bool {
        let start = *p;
        while b.get(*p) == Some(&b'[') {
            *p += 1;
        }
        let array = *p != start;
        match b.get(*p) {
            Some(b'V') if void && !array => {
                *p += 1;
                true
            }
            Some(b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D') => {
                *p += 1;
                true
            }
            Some(b'L') => {
                *p += 1;
                let s = *p;
                while let Some(c) = b.get(*p) {
                    if *c == b';' {
                        let valid = *p > s;
                        *p += 1;
                        return valid;
                    }
                    if *c == b'[' || *c == b'(' || *c == b')' {
                        return false;
                    }
                    *p += 1;
                }
                false
            }
            _ => false,
        }
    }
    let b = sig.as_bytes();
    let mut p = 1;
    let mut args = Vec::new();
    if b.first() != Some(&b'(') {
        return Err(RdError::new("JNI_SIGNATURE", sig));
    }
    while b.get(p) != Some(&b')') {
        let start = p;
        if !one(b, &mut p, false) {
            return Err(RdError::new("JNI_SIGNATURE", sig));
        }
        args.push(sig[start..p].into());
    }
    p += 1;
    let start = p;
    if !one(b, &mut p, true) || p != b.len() {
        return Err(RdError::new("JNI_SIGNATURE", sig));
    }
    Ok((args, sig[start..].into()))
}
pub fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for u in s.encode_utf16() {
        if u == 0 {
            out.extend([0xc0, 0x80]);
        } else if u < 0x80 {
            out.push(u as u8);
        } else if u < 0x800 {
            out.extend([0xc0 | (u >> 6) as u8, 0x80 | (u & 63) as u8]);
        } else {
            out.extend([
                0xe0 | (u >> 12) as u8,
                0x80 | ((u >> 6) & 63) as u8,
                0x80 | (u & 63) as u8,
            ]);
        }
    }
    out
}
pub fn decode_modified_utf8(b: &[u8]) -> Result<String> {
    let mut u = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let (n, mut v) = if (1..128).contains(&c) {
            (1, c as u16)
        } else if c & 0xe0 == 0xc0 {
            (2, (c & 31) as u16)
        } else if c & 0xf0 == 0xe0 {
            (3, (c & 15) as u16)
        } else {
            return Err(RdError::new(
                "JNI_ENCODING",
                "invalid modified UTF-8 lead byte",
            ));
        };
        if i + n > b.len() {
            return Err(RdError::new("JNI_ENCODING", "truncated modified UTF-8"));
        }
        for x in &b[i + 1..i + n] {
            if x & 0xc0 != 0x80 {
                return Err(RdError::new("JNI_ENCODING", "invalid continuation"));
            }
            v = (v << 6) | u16::from(x & 63);
        }
        if n == 2 && v < 128 && v != 0
            || n == 3 && v < 0x800
            || n == 2 && v == 0 && b[i..i + 2] != [0xc0, 0x80]
        {
            return Err(RdError::new("JNI_ENCODING", "overlong modified UTF-8"));
        }
        u.push(v);
        i += n;
    }
    String::from_utf16(&u).map_err(|_| {
        RdError::new(
            "JNI_ENCODING",
            "unpaired UTF-16 surrogate cannot be represented by the VM string heap",
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mangling_and_overloads() {
        assert_eq!(
            symbols("Lorg/a_b/你好;", "n_é", "([ILjava/lang/String;)I").unwrap(),
            [
                "Java_org_a_1b__04f60_0597d_n_1_000e9",
                "Java_org_a_1b__04f60_0597d_n_1_000e9___3ILjava_lang_String_2"
            ]
        );
    }
    #[test]
    fn utf8_roundtrip() {
        for s in ["", "a\0b", "Olá 日本 🦀"] {
            assert_eq!(decode_modified_utf8(&encode_modified_utf8(s)).unwrap(), s);
        }
    }
    #[test]
    fn invalid_utf8() {
        for b in [
            vec![0],
            vec![0xf0, 0x90, 0x80, 0x80],
            vec![0xc1, 0x81],
            vec![0xed, 0xa0, 0x80],
            vec![0xc0],
        ] {
            assert!(decode_modified_utf8(&b).is_err());
        }
    }
    #[test]
    fn signatures() {
        assert_eq!(descriptor("(I[[Ljava/lang/String;D)[B").unwrap().0.len(), 3);
        for s in ["", "(V)V", "(I", "()[V", "(L;)V", "()VI"] {
            assert!(descriptor(s).is_err(), "{s}");
        }
    }
    #[test]
    fn local_global_frames() {
        let mut r = References::default();
        let a = r.insert(Handle::Object(42)).unwrap();
        let g = r.duplicate(a, true).unwrap();
        r.push().unwrap();
        let b = r.duplicate(a, false).unwrap();
        let c = r.pop(b).unwrap();
        assert!(r.get(b).is_err());
        assert!(r.same(c, g).unwrap());
        r.delete_local(a);
        r.delete_global(g);
        assert!(r.get(g).is_err());
        assert_eq!(r.get(c).unwrap(), &Handle::Object(42));
    }
}
