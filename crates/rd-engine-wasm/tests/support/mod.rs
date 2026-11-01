//! Tiny binary assembler for the tests: builds `.wasm` bytes by hand so the
//! tests exercise the real decoder, with no external toolchain (ADR 013 §9).
#![allow(dead_code)]

pub fn leb_u(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

pub fn leb_s(mut v: i64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        let done = (v == 0 && b & 0x40 == 0) || (v == -1 && b & 0x40 != 0);
        if done {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

pub fn name(s: &str) -> Vec<u8> {
    let mut v = leb_u(s.len() as u64);
    v.extend_from_slice(s.as_bytes());
    v
}

pub fn vec_of(items: &[Vec<u8>]) -> Vec<u8> {
    let mut v = leb_u(items.len() as u64);
    for i in items {
        v.extend_from_slice(i);
    }
    v
}

pub fn section(id: u8, payload: Vec<u8>) -> Vec<u8> {
    let mut v = vec![id];
    v.extend(leb_u(payload.len() as u64));
    v.extend(payload);
    v
}

pub const I32: u8 = 0x7f;
pub const I64: u8 = 0x7e;

/// Function type: params and results (MVP: at most one result).
pub fn functype(params: &[u8], results: &[u8]) -> Vec<u8> {
    let mut v = vec![0x60];
    v.extend(leb_u(params.len() as u64));
    v.extend_from_slice(params);
    v.extend(leb_u(results.len() as u64));
    v.extend_from_slice(results);
    v
}

pub struct FuncDef {
    pub type_idx: u32,
    /// (count, valtype) declarations.
    pub locals: Vec<(u32, u8)>,
    /// Instruction bytes, WITHOUT the final `end` (added here).
    pub body: Vec<u8>,
}

pub struct Global {
    pub ty: u8,
    pub mutable: bool,
    /// Constant expression bytes, WITHOUT `end`.
    pub init: Vec<u8>,
}

#[derive(Default)]
pub struct ModuleBuilder {
    pub types: Vec<Vec<u8>>,
    pub funcs: Vec<FuncDef>,
    pub memory: Option<(u32, Option<u32>)>,
    pub globals: Vec<Global>,
    /// (name, kind 0=func 2=memory 3=global, index)
    pub exports: Vec<(String, u8, u32)>,
    /// (offset, bytes)
    pub data: Vec<(u32, Vec<u8>)>,
    pub start: Option<u32>,
}

impl ModuleBuilder {
    pub fn build(&self) -> Vec<u8> {
        let mut out = b"\0asm".to_vec();
        out.extend_from_slice(&[1, 0, 0, 0]);
        if !self.types.is_empty() {
            out.extend(section(1, vec_of(&self.types)));
        }
        if !self.funcs.is_empty() {
            let items: Vec<Vec<u8>> = self
                .funcs
                .iter()
                .map(|f| leb_u(u64::from(f.type_idx)))
                .collect();
            out.extend(section(3, vec_of(&items)));
        }
        if let Some((min, max)) = self.memory {
            let mut lim = Vec::new();
            match max {
                Some(m) => {
                    lim.push(1);
                    lim.extend(leb_u(u64::from(min)));
                    lim.extend(leb_u(u64::from(m)));
                }
                None => {
                    lim.push(0);
                    lim.extend(leb_u(u64::from(min)));
                }
            }
            out.extend(section(5, vec_of(&[lim])));
        }
        if !self.globals.is_empty() {
            let items: Vec<Vec<u8>> = self
                .globals
                .iter()
                .map(|g| {
                    let mut v = vec![g.ty, u8::from(g.mutable)];
                    v.extend_from_slice(&g.init);
                    v.push(0x0b);
                    v
                })
                .collect();
            out.extend(section(6, vec_of(&items)));
        }
        if !self.exports.is_empty() {
            let items: Vec<Vec<u8>> = self
                .exports
                .iter()
                .map(|(n, k, i)| {
                    let mut v = name(n);
                    v.push(*k);
                    v.extend(leb_u(u64::from(*i)));
                    v
                })
                .collect();
            out.extend(section(7, vec_of(&items)));
        }
        if let Some(s) = self.start {
            out.extend(section(8, leb_u(u64::from(s))));
        }
        if !self.funcs.is_empty() {
            let items: Vec<Vec<u8>> = self
                .funcs
                .iter()
                .map(|f| {
                    let mut body = Vec::new();
                    body.extend(vec_of(
                        &f.locals
                            .iter()
                            .map(|(n, t)| {
                                let mut v = leb_u(u64::from(*n));
                                v.push(*t);
                                v
                            })
                            .collect::<Vec<_>>(),
                    ));
                    body.extend_from_slice(&f.body);
                    body.push(0x0b);
                    let mut v = leb_u(body.len() as u64);
                    v.extend(body);
                    v
                })
                .collect();
            out.extend(section(10, vec_of(&items)));
        }
        if !self.data.is_empty() {
            let items: Vec<Vec<u8>> = self
                .data
                .iter()
                .map(|(off, bytes)| {
                    let mut v = vec![0u8];
                    v.push(0x41);
                    v.extend(leb_s(i64::from(*off)));
                    v.push(0x0b);
                    v.extend(leb_u(bytes.len() as u64));
                    v.extend_from_slice(bytes);
                    v
                })
                .collect();
            out.extend(section(11, vec_of(&items)));
        }
        out
    }
}

// Instruction helpers (MVP encodings).
pub fn local_get(i: u32) -> Vec<u8> {
    let mut v = vec![0x20];
    v.extend(leb_u(u64::from(i)));
    v
}
pub fn local_set(i: u32) -> Vec<u8> {
    let mut v = vec![0x21];
    v.extend(leb_u(u64::from(i)));
    v
}
pub fn i32_const(x: i32) -> Vec<u8> {
    let mut v = vec![0x41];
    v.extend(leb_s(i64::from(x)));
    v
}
pub fn i64_const(x: i64) -> Vec<u8> {
    let mut v = vec![0x42];
    v.extend(leb_s(x));
    v
}
pub fn call(f: u32) -> Vec<u8> {
    let mut v = vec![0x10];
    v.extend(leb_u(u64::from(f)));
    v
}
pub fn br(depth: u32) -> Vec<u8> {
    let mut v = vec![0x0c];
    v.extend(leb_u(u64::from(depth)));
    v
}
pub fn br_if(depth: u32) -> Vec<u8> {
    let mut v = vec![0x0d];
    v.extend(leb_u(u64::from(depth)));
    v
}
pub fn global_get(i: u32) -> Vec<u8> {
    let mut v = vec![0x23];
    v.extend(leb_u(u64::from(i)));
    v
}
pub fn global_set(i: u32) -> Vec<u8> {
    let mut v = vec![0x24];
    v.extend(leb_u(u64::from(i)));
    v
}
/// Memory access with natural alignment `align` (log2) and `offset`.
pub fn mem(op: u8, align: u32, offset: u32) -> Vec<u8> {
    let mut v = vec![op];
    v.extend(leb_u(u64::from(align)));
    v.extend(leb_u(u64::from(offset)));
    v
}
