//! Binary decoder and validator for the S1 subset.
//!
//! Single pass per function: type checking (with the spec's polymorphic
//! stack for unreachable code) and lowering to [`Instr`] with every branch
//! target resolved to an instruction index and a static operand-stack height.
//! Nothing here trusts the input: each read is bounds-checked and each index
//! is range-checked.

use crate::limits::{self, MAX_MEMORY_PAGES, MAX_NAME_BYTES};
use crate::{ValType, Value, WasmError};

type Res<T> = Result<T, WasmError>;

fn perr(cause: impl Into<String>) -> WasmError {
    WasmError::new("PARSE_ERROR", cause)
}

fn invalid(cause: impl Into<String>) -> WasmError {
    WasmError::new("INVALID_FORMAT", cause)
}

fn unsupported(cause: impl Into<String>) -> WasmError {
    WasmError::new("NOT_IMPLEMENTED", cause)
        .with_suggestion("the S1 WASM subset is i32/i64 MVP only (ADR 013 §4)")
}

fn over_limit(cause: impl Into<String>) -> WasmError {
    WasmError::new("LIMIT_EXCEEDED", cause)
}

/// Bounds-checked byte reader.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub(crate) fn eof(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn byte(&mut self) -> Res<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| perr("unexpected end of input"))?;
        self.pos += 1;
        Ok(b)
    }

    pub(crate) fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        if n > self.remaining() {
            return Err(perr(format!(
                "declared length {n} exceeds the remaining input"
            )));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn uleb(&mut self, bits: u32) -> Res<u64> {
        let mut result: u64 = 0;
        let mut shift: u32 = 0;
        loop {
            if shift >= bits {
                return Err(perr("LEB128 value is too long"));
            }
            let b = self.byte()?;
            let payload = u64::from(b & 0x7f);
            let avail = bits - shift;
            if avail < 7 && (payload >> avail) != 0 {
                return Err(perr("LEB128 value does not fit its type"));
            }
            result |= payload << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
        }
    }

    fn sleb(&mut self, bits: u32) -> Res<i64> {
        let mut result: i64 = 0;
        let mut shift: u32 = 0;
        loop {
            if shift >= bits {
                return Err(perr("LEB128 value is too long"));
            }
            let b = self.byte()?;
            let payload = i64::from(b & 0x7f);
            result |= payload << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && (b & 0x40) != 0 {
                    result |= -1i64 << shift;
                }
                // Unused high bits of the last byte must be a sign extension.
                let used_before = shift - 7;
                let avail = bits - used_before;
                if avail < 7 {
                    let sign = (payload >> (avail - 1)) & 1;
                    let high = payload >> avail;
                    let all_ones = (1i64 << (7 - avail)) - 1;
                    let ok = if sign == 1 {
                        high == all_ones
                    } else {
                        high == 0
                    };
                    if !ok {
                        return Err(perr("LEB128 value does not fit its type"));
                    }
                }
                return Ok(result);
            }
        }
    }

    pub(crate) fn u32(&mut self) -> Res<u32> {
        Ok(self.uleb(32)? as u32)
    }

    pub(crate) fn i32(&mut self) -> Res<i32> {
        Ok(self.sleb(32)? as i32)
    }

    pub(crate) fn i64(&mut self) -> Res<i64> {
        self.sleb(64)
    }

    /// Element count. Every element takes at least one byte, so a count larger
    /// than the remaining input is malformed and rejected before allocating.
    pub(crate) fn count(&mut self) -> Res<usize> {
        let n = self.u32()? as usize;
        if n > self.remaining() {
            return Err(perr(format!(
                "element count {n} exceeds the remaining input"
            )));
        }
        Ok(n)
    }

    fn name(&mut self) -> Res<String> {
        let n = self.u32()? as usize;
        if n > MAX_NAME_BYTES {
            return Err(over_limit(format!(
                "name of {n} bytes exceeds {MAX_NAME_BYTES}"
            )));
        }
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).map_err(|_| perr("name is not valid UTF-8"))
    }

    pub(crate) fn valtype(&mut self) -> Res<ValType> {
        match self.byte()? {
            0x7f => Ok(ValType::I32),
            0x7e => Ok(ValType::I64),
            0x7d | 0x7c => Err(unsupported(
                "floating-point value types are outside the S1 subset",
            )),
            0x7b => Err(unsupported("SIMD value types are outside the S1 subset")),
            _ => Err(perr("invalid value type")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Global {
    pub ty: ValType,
    pub mutable: bool,
    pub init: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Memory {
    pub min: u32,
    pub max: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportKind {
    Func(u32),
    Memory,
    Global(u32),
}

#[derive(Debug, Clone)]
pub(crate) struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Debug, Clone)]
pub(crate) struct Data {
    pub offset: u32,
    pub bytes: Vec<u8>,
}

/// A branch destination. `pc` is an index into the function's code; `arity`
/// is the number of values carried; `height` is the operand-stack depth,
/// relative to the frame's operand base, that the label expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub pc: usize,
    pub arity: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Instr {
    Unreachable,
    Br(Target),
    BrIf(Target),
    /// Last entry is the default target.
    BrTable(Box<[Target]>),
    /// Unconditional jump (the `else` skip).
    Jump(usize),
    /// Jump to the target when the popped i32 is zero.
    If(usize),
    /// Returns the top `nresults` values of the current frame.
    Return,
    Call(u32),
    Drop,
    Select,
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    Load {
        op: u8,
        offset: u32,
    },
    Store {
        op: u8,
        offset: u32,
    },
    MemorySize,
    MemoryGrow,
    I32Const(i32),
    I64Const(i64),
    /// Numeric operator; the opcode selects the semantics.
    Num(u8),
}

#[derive(Debug, Clone)]
pub(crate) struct Func {
    pub type_idx: u32,
    pub nparams: usize,
    pub nresults: usize,
    /// Params followed by declared locals.
    pub locals: Vec<ValType>,
    pub code: Vec<Instr>,
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleDef {
    pub types: Vec<FuncType>,
    pub funcs: Vec<Func>,
    pub globals: Vec<Global>,
    pub memory: Option<Memory>,
    pub exports: Vec<Export>,
    pub start: Option<u32>,
    pub data: Vec<Data>,
}

/// Section ids in the order the spec requires (custom sections are free-floating).
const SEC_TYPE: u8 = 1;
const SEC_IMPORT: u8 = 2;
const SEC_FUNCTION: u8 = 3;
const SEC_TABLE: u8 = 4;
const SEC_MEMORY: u8 = 5;
const SEC_GLOBAL: u8 = 6;
const SEC_EXPORT: u8 = 7;
const SEC_START: u8 = 8;
const SEC_ELEMENT: u8 = 9;
const SEC_CODE: u8 = 10;
const SEC_DATA: u8 = 11;

pub(crate) fn parse_module(bytes: &[u8]) -> Res<ModuleDef> {
    if bytes.len() > limits::MAX_MODULE_BYTES {
        return Err(over_limit(format!(
            "module of {} bytes exceeds the limit of {}",
            bytes.len(),
            limits::MAX_MODULE_BYTES
        )));
    }
    let mut r = Reader::new(bytes);
    if r.take(4)
        .map_err(|_| perr("module too short for the header"))?
        != b"\0asm"
    {
        return Err(perr("bad magic number (expected \\0asm)"));
    }
    if r.take(4)? != [1, 0, 0, 0] {
        return Err(perr("unsupported WebAssembly version (only 1 is accepted)"));
    }

    let mut def = ModuleDef {
        types: Vec::new(),
        funcs: Vec::new(),
        globals: Vec::new(),
        memory: None,
        exports: Vec::new(),
        start: None,
        data: Vec::new(),
    };
    let mut func_types: Vec<u32> = Vec::new();
    let mut seen_code = false;
    let mut last_id: u8 = 0;

    while !r.eof() {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let payload = r.take(size)?;
        if id == 0 {
            continue; // custom sections are skipped; their contents are not trusted
        }
        if id > SEC_DATA {
            return Err(perr(format!("unknown section id {id}")));
        }
        if id <= last_id {
            return Err(perr(format!("section {id} is out of order or duplicated")));
        }
        last_id = id;
        let mut s = Reader::new(payload);
        match id {
            SEC_TYPE => {
                for _ in 0..s.count()? {
                    if s.byte()? != 0x60 {
                        return Err(perr("type entry must start with 0x60"));
                    }
                    let np = s.count()?;
                    let mut params = Vec::with_capacity(np);
                    for _ in 0..np {
                        params.push(s.valtype()?);
                    }
                    let nr = s.count()?;
                    if nr > 1 {
                        return Err(unsupported(
                            "multi-value function types are outside the S1 subset",
                        ));
                    }
                    let mut results = Vec::with_capacity(nr);
                    for _ in 0..nr {
                        results.push(s.valtype()?);
                    }
                    def.types.push(FuncType { params, results });
                }
            }
            SEC_IMPORT => {
                if s.count()? > 0 {
                    return Err(unsupported(
                        "module imports are outside the S1 subset: the host bridge is a fixed contract, not a wasm import",
                    ));
                }
            }
            SEC_FUNCTION => {
                for _ in 0..s.count()? {
                    let t = s.u32()?;
                    if t as usize >= def.types.len() {
                        return Err(invalid("function references an unknown type"));
                    }
                    func_types.push(t);
                }
            }
            SEC_TABLE => {
                if s.count()? > 0 {
                    return Err(unsupported(
                        "tables (call_indirect) are outside the S1 subset",
                    ));
                }
            }
            SEC_MEMORY => {
                let n = s.count()?;
                if n > 1 {
                    return Err(unsupported("multiple memories are outside the S1 subset"));
                }
                if n == 1 {
                    def.memory = Some(read_limits(&mut s)?);
                }
            }
            SEC_GLOBAL => {
                for _ in 0..s.count()? {
                    let ty = s.valtype()?;
                    let mutable = match s.byte()? {
                        0 => false,
                        1 => true,
                        _ => return Err(perr("invalid global mutability flag")),
                    };
                    let init = read_const(&mut s, ty)?;
                    def.globals.push(Global { ty, mutable, init });
                }
            }
            SEC_EXPORT => {
                for _ in 0..s.count()? {
                    let name = s.name()?;
                    let kind = s.byte()?;
                    let idx = s.u32()?;
                    let kind = match kind {
                        0 => ExportKind::Func(idx),
                        2 => ExportKind::Memory,
                        3 => ExportKind::Global(idx),
                        1 => return Err(unsupported("table exports are outside the S1 subset")),
                        _ => return Err(perr("invalid export kind")),
                    };
                    if def.exports.iter().any(|e| e.name == name) {
                        return Err(invalid(format!("duplicate export name `{name}`")));
                    }
                    def.exports.push(Export { name, kind });
                }
            }
            SEC_START => {
                def.start = Some(s.u32()?);
            }
            SEC_ELEMENT => {
                if s.count()? > 0 {
                    return Err(unsupported("element segments are outside the S1 subset"));
                }
            }
            SEC_CODE => {
                seen_code = true;
                let n = s.count()?;
                if n != func_types.len() {
                    return Err(invalid(format!(
                        "code section has {n} bodies but function section declares {}",
                        func_types.len()
                    )));
                }
                for i in 0..n {
                    let body_size = s.u32()? as usize;
                    let body = s.take(body_size)?;
                    let f = decode_func(&def, &func_types, func_types[i], body)?;
                    def.funcs.push(f);
                }
            }
            SEC_DATA => {
                for _ in 0..s.count()? {
                    let flag = s.u32()?;
                    if flag != 0 {
                        return Err(unsupported(
                            "passive or explicit-memory data segments are outside the S1 subset",
                        ));
                    }
                    if def.memory.is_none() {
                        return Err(invalid("data segment without a memory"));
                    }
                    let offset = read_const(&mut s, ValType::I32)?;
                    let offset = match offset {
                        Value::I32(v) => v as u32,
                        _ => unreachable!("read_const checked the type"),
                    };
                    let len = s.u32()? as usize;
                    let bytes = s.take(len)?.to_vec();
                    def.data.push(Data { offset, bytes });
                }
            }
            _ => unreachable!("ids are checked above"),
        }
        if !s.eof() {
            return Err(perr(format!("section {id} has trailing bytes")));
        }
    }

    if !seen_code && !func_types.is_empty() {
        return Err(invalid("function section without a code section"));
    }
    if let Some(s) = def.start {
        let Some(&t) = func_types.get(s as usize) else {
            return Err(invalid("start function index out of range"));
        };
        let ft = &def.types[t as usize];
        if !ft.params.is_empty() || !ft.results.is_empty() {
            return Err(invalid("start function must have type [] -> []"));
        }
    }
    for e in &def.exports {
        match e.kind {
            ExportKind::Func(i) if i as usize >= def.funcs.len() => {
                return Err(invalid("export references an unknown function"))
            }
            ExportKind::Global(i) if i as usize >= def.globals.len() => {
                return Err(invalid("export references an unknown global"))
            }
            ExportKind::Memory if def.memory.is_none() => {
                return Err(invalid("export references a missing memory"))
            }
            _ => {}
        }
    }
    Ok(def)
}

fn read_limits(s: &mut Reader<'_>) -> Res<Memory> {
    let flag = s.byte()?;
    let min = s.u32()?;
    let max = match flag {
        0 => None,
        1 => Some(s.u32()?),
        _ => return Err(perr("invalid limits flag")),
    };
    if min > MAX_MEMORY_PAGES {
        return Err(over_limit(format!(
            "memory minimum of {min} pages exceeds the limit of {MAX_MEMORY_PAGES} (16 MiB)"
        )));
    }
    if let Some(m) = max {
        if m > MAX_MEMORY_PAGES {
            return Err(over_limit(format!(
                "memory maximum of {m} pages exceeds the limit of {MAX_MEMORY_PAGES} (16 MiB)"
            )));
        }
        if m < min {
            return Err(invalid("memory maximum is below its minimum"));
        }
    }
    Ok(Memory { min, max })
}

/// Constant expression: `i32.const` / `i64.const` followed by `end`.
fn read_const(s: &mut Reader<'_>, ty: ValType) -> Res<Value> {
    let v = match (s.byte()?, ty) {
        (0x41, ValType::I32) => Value::I32(s.i32()?),
        (0x42, ValType::I64) => Value::I64(s.i64()?),
        (0x23, _) => {
            return Err(unsupported(
                "global.get in constant expressions needs imports, outside the S1 subset",
            ))
        }
        _ => {
            return Err(invalid(
                "constant expression does not match the declared type",
            ))
        }
    };
    if s.byte()? != 0x0b {
        return Err(invalid("constant expression must end with `end`"));
    }
    Ok(v)
}

/// Element type and natural alignment (log2) of a memory access opcode.
fn mem_access(op: u8) -> Option<(u8, ValType, bool, bool)> {
    // (size in bytes, value type, sign-extending load, is_store)
    Some(match op {
        0x28 => (4, ValType::I32, false, false),
        0x29 => (8, ValType::I64, false, false),
        0x2c => (1, ValType::I32, true, false),
        0x2d => (1, ValType::I32, false, false),
        0x2e => (2, ValType::I32, true, false),
        0x2f => (2, ValType::I32, false, false),
        0x30 => (1, ValType::I64, true, false),
        0x31 => (1, ValType::I64, false, false),
        0x32 => (2, ValType::I64, true, false),
        0x33 => (2, ValType::I64, false, false),
        0x34 => (4, ValType::I64, true, false),
        0x35 => (4, ValType::I64, false, false),
        0x36 => (4, ValType::I32, false, true),
        0x37 => (8, ValType::I64, false, true),
        0x3a => (1, ValType::I32, false, true),
        0x3b => (2, ValType::I32, false, true),
        0x3c => (1, ValType::I64, false, true),
        0x3d => (2, ValType::I64, false, true),
        0x3e => (4, ValType::I64, false, true),
        _ => return None,
    })
}

/// Signature of a numeric operator: (operands bottom-to-top, result).
fn num_sig(op: u8) -> Option<(&'static [ValType], ValType)> {
    use ValType::{I32, I64};
    Some(match op {
        0x45 => (&[I32], I32),
        0x46..=0x4f => (&[I32, I32], I32),
        0x50 => (&[I64], I32),
        0x51..=0x5a => (&[I64, I64], I32),
        0x67..=0x69 => (&[I32], I32),
        0x6a..=0x78 => (&[I32, I32], I32),
        0x79..=0x7b => (&[I64], I64),
        0x7c..=0x8a => (&[I64, I64], I64),
        0xa7 => (&[I64], I32),
        0xac | 0xad => (&[I32], I64),
        0xc0 | 0xc1 => (&[I32], I32),
        0xc2..=0xc4 => (&[I64], I64),
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Block,
    Loop,
    If,
    Func,
}

struct Ctl {
    kind: Kind,
    /// Types produced at `end` (fallthrough).
    end_types: Vec<ValType>,
    /// Types carried by a branch to this label (empty for loops in MVP).
    label_types: Vec<ValType>,
    /// Operand-stack height at entry.
    height: usize,
    unreachable: bool,
    /// Loop start (branch target for loops).
    loop_pc: usize,
    /// Forward branch slots to fix when this label ends: (instr index, slot).
    patches: Vec<(usize, usize)>,
    /// `If` instruction to fix (to the else start, or to the end).
    if_idx: Option<usize>,
    else_seen: bool,
}

struct Body<'m> {
    def: &'m ModuleDef,
    /// Type index of every function (imports none), known before any body.
    func_types: &'m [u32],
    /// Params followed by declared locals.
    locals: Vec<ValType>,
    results: Vec<ValType>,
    vals: Vec<Option<ValType>>,
    ctls: Vec<Ctl>,
    out: Vec<Instr>,
}

fn decode_func(def: &ModuleDef, func_types: &[u32], type_idx: u32, body: &[u8]) -> Res<Func> {
    let ft = &def.types[type_idx as usize];
    let mut r = Reader::new(body);
    let ngroups = r.count()?;
    let mut locals: Vec<ValType> = ft.params.clone();
    let mut declared: u64 = 0;
    for _ in 0..ngroups {
        let n = r.u32()? as u64;
        declared += n;
        if declared > limits::MAX_LOCALS {
            return Err(over_limit(format!(
                "function declares more than {} locals",
                limits::MAX_LOCALS
            )));
        }
        let t = r.valtype()?;
        for _ in 0..n {
            locals.push(t);
        }
    }
    let nparams = ft.params.len();
    let mut b = Body {
        def,
        func_types,
        locals,
        results: ft.results.clone(),
        vals: Vec::new(),
        ctls: vec![Ctl {
            kind: Kind::Func,
            end_types: ft.results.clone(),
            label_types: ft.results.clone(),
            height: 0,
            unreachable: false,
            loop_pc: 0,
            patches: Vec::new(),
            if_idx: None,
            else_seen: false,
        }],
        out: Vec::new(),
    };
    b.decode(&mut r)?;
    Ok(Func {
        type_idx,
        nparams,
        nresults: ft.results.len(),
        locals: b.locals,
        code: b.out,
    })
}

fn block_type(r: &mut Reader<'_>) -> Res<Vec<ValType>> {
    match r.sleb(33)? {
        -64 => Ok(Vec::new()),
        -1 => Ok(vec![ValType::I32]),
        -2 => Ok(vec![ValType::I64]),
        -3 | -4 => Err(unsupported(
            "floating-point block types are outside the S1 subset",
        )),
        v if v >= 0 => Err(unsupported(
            "type-indexed block types (multi-value) are outside the S1 subset",
        )),
        _ => Err(perr("invalid block type")),
    }
}

fn patch_slot(ins: &mut Instr, slot: usize, pc: usize) {
    match ins {
        Instr::Br(t) | Instr::BrIf(t) => t.pc = pc,
        Instr::BrTable(ts) => ts[slot].pc = pc,
        Instr::Jump(p) | Instr::If(p) => *p = pc,
        _ => {}
    }
}

impl<'m> Body<'m> {
    fn decode(&mut self, r: &mut Reader<'_>) -> Res<()> {
        while !self.ctls.is_empty() {
            let op = r.byte()?;
            self.step(op, r)?;
        }
        if !r.eof() {
            return Err(perr("trailing bytes after the end of the function body"));
        }
        Ok(())
    }

    fn step(&mut self, op: u8, r: &mut Reader<'_>) -> Res<()> {
        match op {
            0x00 => {
                self.emit(Instr::Unreachable);
                self.set_unreachable();
            }
            0x01 => {} // nop
            0x02 => {
                let end_types = block_type(r)?;
                self.push_ctl(Kind::Block, end_types.clone(), end_types, None);
            }
            0x03 => {
                let end_types = block_type(r)?;
                self.push_ctl(Kind::Loop, end_types, Vec::new(), None);
                let top = self.ctls.len() - 1;
                self.ctls[top].loop_pc = self.out.len();
            }
            0x04 => {
                let end_types = block_type(r)?;
                self.pop_expect(ValType::I32)?;
                let idx = self.emit(Instr::If(0));
                self.push_ctl(Kind::If, end_types.clone(), end_types, Some(idx));
            }
            0x05 => self.op_else()?,
            0x0b => self.op_end()?,
            0x0c => {
                let depth = r.u32()? as usize;
                let t = self.branch_target(depth)?;
                let types = self.label_types(depth)?;
                self.pop_many(&types)?;
                let idx = self.emit(Instr::Br(t));
                self.record_patch(depth, idx, 0);
                self.set_unreachable();
            }
            0x0d => {
                let depth = r.u32()? as usize;
                self.pop_expect(ValType::I32)?;
                let t = self.branch_target(depth)?;
                let types = self.label_types(depth)?;
                let popped = self.pop_many(&types)?;
                self.push_raw(&popped);
                let idx = self.emit(Instr::BrIf(t));
                self.record_patch(depth, idx, 0);
            }
            0x0e => {
                let n = r.count()?;
                let mut depths = Vec::with_capacity(n + 1);
                for _ in 0..n {
                    depths.push(r.u32()? as usize);
                }
                depths.push(r.u32()? as usize);
                let default_depth = *depths.last().expect("pushed above");
                let arity = self.label_types(default_depth)?.len();
                self.pop_expect(ValType::I32)?;
                let mut targets = Vec::with_capacity(depths.len());
                for &d in &depths {
                    let types = self.label_types(d)?;
                    if types.len() != arity {
                        return Err(invalid("br_table targets have different arities"));
                    }
                    let popped = self.pop_many(&types)?;
                    self.push_raw(&popped);
                    targets.push(self.branch_target(d)?);
                }
                // The values must be popped once for the whole table.
                let default_types = self.label_types(default_depth)?;
                self.pop_many(&default_types)?;
                let idx = self.emit(Instr::BrTable(targets.into_boxed_slice()));
                for (slot, &d) in depths.iter().enumerate() {
                    self.record_patch(d, idx, slot);
                }
                self.set_unreachable();
            }
            0x0f => {
                let results = self.results.clone();
                self.pop_many(&results)?;
                self.emit(Instr::Return);
                self.set_unreachable();
            }
            0x10 => {
                let f = r.u32()?;
                let Some(&t) = self.func_types.get(f as usize) else {
                    return Err(invalid("call to an unknown function"));
                };
                let ft = self.def.types[t as usize].clone();
                self.pop_many(&ft.params)?;
                self.push_types(&ft.results);
                self.emit(Instr::Call(f));
            }
            0x11 => return Err(unsupported("call_indirect is outside the S1 subset (no tables)")),
            0x1a => {
                self.pop()?;
                self.emit(Instr::Drop);
            }
            0x1b => {
                self.pop_expect(ValType::I32)?;
                let a = self.pop()?;
                let b = self.pop()?;
                if let (Some(x), Some(y)) = (a, b) {
                    if x != y {
                        return Err(invalid("select operands have different types"));
                    }
                }
                self.vals.push(a.or(b));
                self.emit(Instr::Select);
            }
            0x20 => {
                let i = r.u32()?;
                let t = self.local(i)?;
                self.vals.push(Some(t));
                self.emit(Instr::LocalGet(i));
            }
            0x21 => {
                let i = r.u32()?;
                let t = self.local(i)?;
                self.pop_expect(t)?;
                self.emit(Instr::LocalSet(i));
            }
            0x22 => {
                let i = r.u32()?;
                let t = self.local(i)?;
                self.pop_expect(t)?;
                self.vals.push(Some(t));
                self.emit(Instr::LocalTee(i));
            }
            0x23 => {
                let i = r.u32()?;
                let g = *self.global(i)?;
                self.vals.push(Some(g.ty));
                self.emit(Instr::GlobalGet(i));
            }
            0x24 => {
                let i = r.u32()?;
                let g = *self.global(i)?;
                if !g.mutable {
                    return Err(invalid("global.set on an immutable global"));
                }
                self.pop_expect(g.ty)?;
                self.emit(Instr::GlobalSet(i));
            }
            0x3f | 0x40 => {
                if self.def.memory.is_none() {
                    return Err(invalid("memory instruction without a memory"));
                }
                if r.byte()? != 0 {
                    return Err(perr("memory index must be 0"));
                }
                if op == 0x40 {
                    self.pop_expect(ValType::I32)?;
                    self.emit(Instr::MemoryGrow);
                } else {
                    self.emit(Instr::MemorySize);
                }
                self.vals.push(Some(ValType::I32));
            }
            0x41 => {
                let v = r.i32()?;
                self.vals.push(Some(ValType::I32));
                self.emit(Instr::I32Const(v));
            }
            0x42 => {
                let v = r.i64()?;
                self.vals.push(Some(ValType::I64));
                self.emit(Instr::I64Const(v));
            }
            _ if mem_access(op).is_some() => {
                let (size, ty, _signed, is_store) = mem_access(op).expect("checked");
                if self.def.memory.is_none() {
                    return Err(invalid("memory instruction without a memory"));
                }
                let align = r.u32()?;
                let offset = r.u32()?;
                let natural = size.trailing_zeros();
                if align > natural {
                    return Err(invalid("memory alignment exceeds the natural alignment"));
                }
                if is_store {
                    self.pop_expect(ty)?;
                    self.pop_expect(ValType::I32)?;
                    self.emit(Instr::Store { op, offset });
                } else {
                    self.pop_expect(ValType::I32)?;
                    self.vals.push(Some(ty));
                    self.emit(Instr::Load { op, offset });
                }
            }
            _ if num_sig(op).is_some() => {
                let (ins, res) = num_sig(op).expect("checked");
                self.pop_many(ins)?;
                self.vals.push(Some(res));
                self.emit(Instr::Num(op));
            }
            0x43 | 0x44 => return Err(unsupported("floating-point constants are outside the S1 subset")),
            0xfc => return Err(unsupported("0xFC-prefixed instructions (saturating conversions, bulk memory) are outside the S1 subset")),
            0xfd | 0xfe => return Err(unsupported("SIMD and atomic instructions are outside the S1 subset")),
            0x06..=0x0a | 0x12..=0x19 | 0x1c..=0x1f | 0x25..=0x27 | 0xc5..=0xfb | 0xff => {
                return Err(perr(format!("reserved opcode 0x{op:02x}")))
            }
            _ => {
                return Err(unsupported(format!(
                    "opcode 0x{op:02x} is outside the S1 subset (ADR 013 §4)"
                )))
            }
        }
        Ok(())
    }

    fn emit(&mut self, ins: Instr) -> usize {
        self.out.push(ins);
        self.out.len() - 1
    }

    fn push_ctl(
        &mut self,
        kind: Kind,
        end_types: Vec<ValType>,
        label_types: Vec<ValType>,
        if_idx: Option<usize>,
    ) {
        let height = self.vals.len();
        let loop_pc = self.out.len();
        self.ctls.push(Ctl {
            kind,
            end_types,
            label_types,
            height,
            unreachable: false,
            loop_pc,
            patches: Vec::new(),
            if_idx,
            else_seen: false,
        });
    }

    fn set_unreachable(&mut self) {
        let c = self.ctls.last_mut().expect("open block");
        self.vals.truncate(c.height);
        c.unreachable = true;
    }

    fn pop(&mut self) -> Res<Option<ValType>> {
        let (height, unreachable) = {
            let c = self.ctls.last().ok_or_else(|| invalid("no open block"))?;
            (c.height, c.unreachable)
        };
        if self.vals.len() == height {
            if unreachable {
                return Ok(None);
            }
            return Err(invalid("operand stack underflow"));
        }
        match self.vals.pop() {
            Some(v) => Ok(v),
            None => Err(invalid("operand stack underflow")),
        }
    }

    fn pop_expect(&mut self, t: ValType) -> Res<()> {
        match self.pop()? {
            None => Ok(()),
            Some(g) if g == t => Ok(()),
            Some(_) => Err(invalid("operand type mismatch")),
        }
    }

    /// Pops `types` (in declaration order) and returns what was actually popped.
    fn pop_many(&mut self, types: &[ValType]) -> Res<Vec<Option<ValType>>> {
        let mut got = Vec::with_capacity(types.len());
        for t in types.iter().rev() {
            let g = self.pop()?;
            if let Some(x) = g {
                if x != *t {
                    return Err(invalid("operand type mismatch"));
                }
            }
            got.push(g);
        }
        got.reverse();
        Ok(got)
    }

    fn push_raw(&mut self, vs: &[Option<ValType>]) {
        self.vals.extend_from_slice(vs);
    }

    fn push_types(&mut self, types: &[ValType]) {
        for t in types {
            self.vals.push(Some(*t));
        }
    }

    fn label_types(&self, depth: usize) -> Res<Vec<ValType>> {
        let n = self.ctls.len();
        if depth >= n {
            return Err(invalid("branch depth exceeds the open blocks"));
        }
        Ok(self.ctls[n - 1 - depth].label_types.clone())
    }

    /// Target for a label. Forward labels get a placeholder; `record_patch`
    /// fills it in when the label ends. Loops are known now.
    fn branch_target(&self, depth: usize) -> Res<Target> {
        let n = self.ctls.len();
        if depth >= n {
            return Err(invalid("branch depth exceeds the open blocks"));
        }
        let c = &self.ctls[n - 1 - depth];
        let arity = c.label_types.len() as u32;
        let height = c.height as u32;
        let pc = if c.kind == Kind::Loop { c.loop_pc } else { 0 };
        Ok(Target { pc, arity, height })
    }

    fn record_patch(&mut self, depth: usize, instr: usize, slot: usize) {
        let n = self.ctls.len();
        let c = &mut self.ctls[n - 1 - depth];
        if c.kind != Kind::Loop {
            c.patches.push((instr, slot));
        }
    }

    fn local(&self, i: u32) -> Res<ValType> {
        self.locals
            .get(i as usize)
            .copied()
            .ok_or_else(|| invalid("local index out of range"))
    }

    fn global(&self, i: u32) -> Res<&Global> {
        self.def
            .globals
            .get(i as usize)
            .ok_or_else(|| invalid("global index out of range"))
    }

    fn op_else(&mut self) -> Res<()> {
        let (kind, else_seen, end_types, height) = {
            let c = self.ctls.last().ok_or_else(|| invalid("else without if"))?;
            (c.kind, c.else_seen, c.end_types.clone(), c.height)
        };
        if kind != Kind::If || else_seen {
            return Err(invalid("else without a matching if"));
        }
        self.pop_many(&end_types)?;
        if self.vals.len() != height {
            return Err(invalid("stack height mismatch at else"));
        }
        let jump = self.emit(Instr::Jump(0));
        let else_start = self.out.len();
        let c = self.ctls.last_mut().expect("checked");
        c.patches.push((jump, 0));
        c.else_seen = true;
        c.unreachable = false;
        let if_idx = c.if_idx.expect("if has its instruction");
        self.vals.truncate(height);
        if let Instr::If(p) = &mut self.out[if_idx] {
            *p = else_start;
        }
        Ok(())
    }

    fn op_end(&mut self) -> Res<()> {
        let (end_types, height) = {
            let c = self
                .ctls
                .last()
                .ok_or_else(|| invalid("end without a block"))?;
            (c.end_types.clone(), c.height)
        };
        self.pop_many(&end_types)?;
        if self.vals.len() != height {
            return Err(invalid("stack height mismatch at end of block"));
        }
        let c = self.ctls.pop().expect("checked");
        if c.kind == Kind::If && !c.else_seen {
            if !c.end_types.is_empty() {
                return Err(invalid("if without else must not produce values"));
            }
            if let Some(i) = c.if_idx {
                let end = self.out.len();
                patch_slot(&mut self.out[i], 0, end);
            }
        }
        let end = self.out.len();
        for (instr, slot) in c.patches {
            patch_slot(&mut self.out[instr], slot, end);
        }
        if c.kind == Kind::Func {
            self.emit(Instr::Return);
        }
        self.push_types(&c.end_types);
        Ok(())
    }
}
