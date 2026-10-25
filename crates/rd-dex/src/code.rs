//! code_item — instructions (decode + encode round-trip), tries e catch handlers.
//!
//! Toda a stream é percorrida com avanço obrigatório (guard anti-loop): qualquer
//! instrução truncada/overrun vira erro tipado, nunca pânico. Os code units
//! brutos ficam preservados (`insns`) junto do modelo decodificado (Lei 2).

use crate::error::{RdError, RdResult};
use crate::opcode::{info, Format, PayloadKind};
use crate::read;

/// Uma instrução Dalvik decodificada (opcode + formato + operandos tipados).
#[derive(Debug, Clone, PartialEq)]
pub struct Insn {
    /// byte de opcode bruto (0x00–0xFF).
    pub opcode: u8,
    pub fmt: Format,
    /// tamanho em code units.
    pub size: u16,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// 10x — sem operandos (nop, return-void, unused).
    Plain,
    /// 12x — vA, vB.
    Regs(u8, u8),
    /// 11n — vA, #+B (4 bits com sinal).
    RegLit4(u8, i8),
    /// 11x — vAA.
    Reg(u8),
    /// 10t — branch de 8 bits com sinal.
    Branch8(i8),
    /// 20t — branch de 16 bits com sinal.
    Branch16(i16),
    /// 22x/32x — vAA, vBBBB.
    RegReg16(u8, u16),
    /// 21t — vAA, +BBBB.
    RegBranch16(u8, i16),
    /// 21s — vAA, #+BBBB.
    RegLit16(u8, i16),
    /// 21h — vAA, raw de 16 bits (shift 16/48 aplicado no rendering).
    RegHigh16(u8, u16),
    /// 21c — vAA, index@BBBB.
    RegIndex(u8, u16),
    /// 23x — vAA, vBB, vCC.
    RegRegReg(u8, u8, u8),
    /// 22b — vAA, vBB, #+CC (8 bits com sinal).
    RegRegLit8(u8, u8, i8),
    /// 22t — vA, vB, +CCCC.
    RegRegBranch16(u8, u8, i16),
    /// 22s — vA, vB, #+CCCC.
    RegRegLit16(u8, u8, i16),
    /// 22c — vA, vB, index@CCCC.
    RegRegIndex(u8, u8, u16),
    /// 30t — branch de 32 bits com sinal.
    Branch32(i32),
    /// 31i — vAA, #+BBBBBBBB.
    RegLit32(u8, i32),
    /// 31t — vAA, +BBBBBBBB.
    RegBranch32(u8, i32),
    /// 31c — vAA, index@BBBBBBBB.
    RegIndex32(u8, u32),
    /// 51l — vAA, #+BBBBBBBBBBBBBBBB.
    Lit64(u8, i64),
    /// 35c — invoke {regs…}, index@BBBB (count 1–5).
    Invoke35c { count: u8, idx: u16, regs: [u8; 5] },
    /// 3rc — invoke/range {vC .. vC+AA-1}, index@BBBB.
    InvokeRange3rc { count: u8, idx: u16, start: u16 },
    /// 45cc — invoke-polymorphic {regs…}, method@BBBB, proto@HHHH.
    Invoke45cc {
        count: u8,
        idx: u16,
        regs: [u8; 5],
        proto: u16,
    },
    /// 4rcc — invoke-polymorphic/range {vC ..}, method@BBBB, proto@HHHH.
    InvokeRange4rcc {
        count: u8,
        idx: u16,
        start: u16,
        proto: u16,
    },
    /// payload alinhado (packed/sparse-switch, fill-array-data).
    Payload(Payload),
}

/// Pseudoinstruções (dados embutidos na stream de código).
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    /// ident 0x0100 — alvos relativos ao endereço da instrução packed-switch.
    PackedSwitch { first_key: i32, targets: Vec<i32> },
    /// ident 0x0200 — chaves ordenadas + alvos relativos ao sparse-switch.
    SparseSwitch { keys: Vec<i32>, targets: Vec<i32> },
    /// ident 0x0300 — bytes brutos (já com padding par; Lei 2). O
    /// `element_count` bruto é preservado: o padding da `data` não permite
    /// recuperá-lo (contagens ímpares viram par no encoding) — Lei 2.
    ArrayData {
        element_width: u16,
        element_count: u32,
        data: Vec<u8>,
    },
}

impl Payload {
    /// Tamanho total em code units. Cada `i32` (alvo/chave) ocupa 2 units.
    pub fn size_in_units(&self) -> u16 {
        match self {
            Payload::PackedSwitch { targets, .. } => (4 + 2 * targets.len()) as u16,
            Payload::SparseSwitch { keys, .. } => (2 + 4 * keys.len()) as u16,
            Payload::ArrayData { data, .. } => (4 + data.len() / 2) as u16,
        }
    }
}

fn push_i32(units: &mut Vec<u16>, v: i32) {
    units.push(v as u16);
    units.push((v >> 16) as u16);
}

impl Insn {
    /// Re-encodifica a instrução em code units (round-trip exato).
    pub fn encode(&self) -> Vec<u16> {
        let op = self.opcode as u16;
        let byte1 = |a: u16, b: u16| (b << 12) | (a << 8);
        let n4 = |r: u8| (r & 0xF) as u16;
        let mut units: Vec<u16> = Vec::with_capacity(self.size as usize);
        match &self.kind {
            Kind::Plain => units.push(op),
            Kind::Regs(a, b) => units.push(op | byte1(*a as u16, *b as u16)),
            Kind::RegLit4(a, b) => units.push(op | byte1(*a as u16, (*b & 0xF) as u16)),
            Kind::Reg(a) => units.push(op | ((*a as u16) << 8)),
            Kind::Branch8(off) => units.push(op | (((*off as u16) & 0xFF) << 8)),
            Kind::Branch16(off) => {
                units.push(op);
                units.push(*off as u16);
            }
            // 22x e 32x compartilham a forma de encode: vAA em 8 bits (byte1)
            // + vBBBB de 16 bits — a distinção fica no `fmt` (Lei 2).
            Kind::RegReg16(a, b) => {
                units.push(op | ((*a as u16) << 8));
                units.push(*b);
            }
            Kind::RegBranch16(a, off) => {
                units.push(op | ((*a as u16) << 8));
                units.push(*off as u16);
            }
            Kind::RegLit16(a, lit) => {
                units.push(op | ((*a as u16) << 8));
                units.push(*lit as u16);
            }
            Kind::RegHigh16(a, raw) => {
                units.push(op | ((*a as u16) << 8));
                units.push(*raw);
            }
            Kind::RegIndex(a, idx) => {
                units.push(op | ((*a as u16) << 8));
                units.push(*idx);
            }
            Kind::RegRegReg(a, b, c) => {
                units.push(op | ((*a as u16) << 8));
                units.push(((*c as u16) << 8) | *b as u16);
            }
            Kind::RegRegLit8(a, b, lit) => {
                units.push(op | ((*a as u16) << 8));
                units.push((((*lit as u8) as u16) << 8) | (*b as u16));
            }
            Kind::RegRegBranch16(a, b, off) => {
                units.push(op | byte1(*a as u16, *b as u16));
                units.push(*off as u16);
            }
            Kind::RegRegLit16(a, b, lit) => {
                units.push(op | byte1(*a as u16, *b as u16));
                units.push(*lit as u16);
            }
            Kind::RegRegIndex(a, b, idx) => {
                units.push(op | byte1(*a as u16, *b as u16));
                units.push(*idx);
            }
            Kind::Branch32(off) => {
                units.push(op);
                push_i32(&mut units, *off);
            }
            Kind::RegLit32(a, lit) => {
                units.push(op | ((*a as u16) << 8));
                push_i32(&mut units, *lit);
            }
            Kind::RegBranch32(a, off) => {
                units.push(op | ((*a as u16) << 8));
                push_i32(&mut units, *off);
            }
            Kind::RegIndex32(a, idx) => {
                units.push(op | ((*a as u16) << 8));
                push_i32(&mut units, *idx as i32);
            }
            Kind::Lit64(a, lit) => {
                units.push(op | ((*a as u16) << 8));
                push_i32(&mut units, *lit as i32);
                push_i32(&mut units, (*lit >> 32) as i32);
            }
            Kind::Invoke35c { count, idx, regs } => {
                units.push(op | ((*count as u16 & 0xF) << 12) | (n4(regs[4]) << 8));
                units.push(*idx);
                units.push(
                    (n4(regs[3]) << 12) | (n4(regs[2]) << 8) | (n4(regs[1]) << 4) | n4(regs[0]),
                );
            }
            Kind::InvokeRange3rc { count, idx, start } => {
                units.push(op | ((*count as u16) << 8));
                units.push(*idx);
                units.push(*start);
            }
            Kind::Invoke45cc {
                count,
                idx,
                regs,
                proto,
            } => {
                units.push(op | ((*count as u16 & 0xF) << 12) | (n4(regs[4]) << 8));
                units.push(*idx);
                units.push(
                    (n4(regs[3]) << 12) | (n4(regs[2]) << 8) | (n4(regs[1]) << 4) | n4(regs[0]),
                );
                units.push(*proto);
            }
            Kind::InvokeRange4rcc {
                count,
                idx,
                start,
                proto,
            } => {
                units.push(op | ((*count as u16) << 8));
                units.push(*idx);
                units.push(*start);
                units.push(*proto);
            }
            Kind::Payload(p) => encode_payload(p, &mut units),
        }
        units
    }

    /// Endereço absoluto (code units) do alvo de branch, relativo ao início da instrução.
    pub fn branch_target(&self, at: usize) -> Option<usize> {
        let off: i64 = match &self.kind {
            Kind::Branch8(o) => *o as i64,
            Kind::Branch16(o) => *o as i64,
            Kind::RegBranch16(_, o) => *o as i64,
            Kind::Branch32(o) => *o as i64,
            Kind::RegBranch32(_, o) => *o as i64,
            _ => return None,
        };
        let t = at as i64 + off;
        usize::try_from(t).ok()
    }
}

fn encode_payload(p: &Payload, units: &mut Vec<u16>) {
    match p {
        Payload::PackedSwitch { first_key, targets } => {
            units.push(0x0100);
            units.push(targets.len() as u16);
            push_i32(units, *first_key);
            for t in targets {
                push_i32(units, *t);
            }
        }
        Payload::SparseSwitch { keys, targets } => {
            units.push(0x0200);
            units.push(keys.len() as u16);
            for k in keys.iter().chain(targets.iter()) {
                push_i32(units, *k);
            }
        }
        Payload::ArrayData {
            element_width,
            element_count,
            data,
        } => {
            units.push(0x0300);
            units.push(*element_width);
            push_i32(units, *element_count as i32);
            for pair in data.chunks(2) {
                let hi = pair.get(1).copied().unwrap_or(0);
                units.push(u16::from_le_bytes([pair[0], hi]));
            }
        }
    }
}

/// code_item completo (modelo + bytes brutos).
#[derive(Debug, Clone)]
pub struct CodeItem {
    /// offset bruto do code_item no dex (Lei 2).
    pub offset: usize,
    pub registers_size: u16,
    pub ins_size: u16,
    pub outs_size: u16,
    pub tries_size: u16,
    pub debug_info_off: u32,
    /// code units brutos (Lei 2).
    pub insns: Vec<u16>,
    /// instruções decodificadas, em ordem de endereço: (addr, insn).
    pub instructions: Vec<(usize, Insn)>,
    pub tries: Vec<TryItem>,
    /// handlers decodificados (mesma ordem dos tries).
    pub handlers: Vec<CatchHandler>,
    /// offset bruto do encoded_catch_handler_list (Lei 2).
    pub handlers_offset: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct TryItem {
    pub start_addr: u32,
    pub insn_count: u16,
    /// offset relativo ao início do handler list.
    pub handler_off: u16,
}

#[derive(Debug, Clone, Default)]
pub struct CatchHandler {
    /// pares (type_idx, endereço do handler).
    pub typed: Vec<(u32, u32)>,
    /// endereço do catchall (se size <= 0).
    pub catch_all: Option<u32>,
}

impl CatchHandler {
    pub fn total_entries(&self) -> usize {
        self.typed.len() + usize::from(self.catch_all.is_some())
    }
}

impl CodeItem {
    /// Parse de um code_item com decodificação completa das instruções.
    pub fn parse(data: &[u8], off: u32) -> RdResult<CodeItem> {
        let off = off as usize;
        if off
            .checked_add(16)
            .map(|end| end > data.len())
            .unwrap_or(true)
        {
            return Err(RdError::parse(format!(
                "code_item: header truncado @ 0x{off:x}"
            )));
        }
        let registers_size = read::u16_at(data, off)?;
        let ins_size = read::u16_at(data, off + 2)?;
        let outs_size = read::u16_at(data, off + 4)?;
        let tries_size = read::u16_at(data, off + 6)?;
        let debug_info_off = read::u32_at(data, off + 8)?;
        let insns_size = read::u32_at(data, off + 12)? as usize;

        // issue #20: ins_size > registers_size é estruturalmente impossível
        // (os registradores de entrada vivem no topo do vetor) — sem este
        // check, o disassembler subtrai em wrap e produz nomes pNNNN absurdos
        if ins_size > registers_size {
            return Err(RdError::invalid_format(format!(
                "code_item: ins_size ({ins_size}) > registers_size ({registers_size})"
            )));
        }

        let insns_off = off + 16;
        if insns_size.checked_mul(2).is_none() {
            return Err(RdError::parse("code_item: insns_size overflow"));
        }
        let insns_end = insns_off
            .checked_add(insns_size * 2)
            .ok_or_else(|| RdError::parse("code_item: insns overrun"))?;
        if insns_end > data.len() {
            return Err(RdError::parse(format!(
                "code_item: insns ({insns_size} units) além dos dados @ 0x{off:x}"
            )));
        }
        let mut insns = Vec::with_capacity(insns_size);
        for i in 0..insns_size {
            insns.push(read::u16_at(data, insns_off + i * 2)?);
        }

        // decodificação — avanço obrigatório, overrun tipado
        let instructions = decode_all(&insns)?;

        // issue #17: valida UMA vez por code_item que todos os índices de
        // registrador das instruções decodificadas cabem em registers_size
        // (incluindo o +1 dos pares wide) — sem isto, um DEX malformado de
        // ~200 bytes pânica/aborta o interpretador em regs[a]
        validate_registers(&instructions, registers_size)?;

        // tries (alinhados a 4 bytes; padding de 1 unit se insns_size ímpar)
        let mut tries = Vec::new();
        let mut handlers = Vec::new();
        let mut handlers_offset = 0usize;
        if tries_size > 0 {
            let tries_off = insns_end + usize::from(insns_size % 2 == 1) * 2;
            let tries_end = tries_off
                .checked_add(tries_size as usize * 8)
                .ok_or_else(|| RdError::parse("code_item: tries overrun"))?;
            if tries_end > data.len() {
                return Err(RdError::parse(format!(
                    "code_item: {} tries além dos dados @ 0x{tries_off:x}",
                    tries_size
                )));
            }
            for i in 0..tries_size as usize {
                let base = tries_off + i * 8;
                tries.push(TryItem {
                    start_addr: read::u32_at(data, base)?,
                    insn_count: read::u16_at(data, base + 4)?,
                    handler_off: read::u16_at(data, base + 6)?,
                });
            }
            // handler list logo após os tries
            handlers_offset = tries_end;
            let mut r = read::Reader::at(data, handlers_offset)?;
            // spec ("encoded_catch_handler_list"): size é ULEB128 (não u32) e
            // handler_off é relativo ao INÍCIO da lista (incluindo o campo size).
            let _list_size = r.uleb128()?;
            let base = handlers_offset;
            const MAX_METHOD_CATCH_CLAUSES: usize = 16_384;
            let clause_budget = data
                .len()
                .saturating_mul(2)
                .clamp(1024, MAX_METHOD_CATCH_CLAUSES);
            let mut total_clauses = 0usize;
            let mut handler_cache: std::collections::BTreeMap<u16, CatchHandler> =
                std::collections::BTreeMap::new();
            for t in &tries {
                let h = if let Some(cached) = handler_cache.get(&t.handler_off) {
                    cached.clone()
                } else {
                    let handler_abs = base
                        .checked_add(t.handler_off as usize)
                        .ok_or_else(|| RdError::parse("code_item: handler_off overflow"))?;
                    let parsed = parse_catch_handler(data, handler_abs)?;
                    handler_cache.insert(t.handler_off, parsed.clone());
                    parsed
                };
                let clauses = h
                    .typed
                    .len()
                    .saturating_add(usize::from(h.catch_all.is_some()))
                    .max(1);
                total_clauses = total_clauses.saturating_add(clauses);
                if total_clauses > clause_budget {
                    return Err(RdError::parse(format!(
                        "code_item: cláusulas catch acumuladas ({total_clauses}) excedem orçamento ({clause_budget}) @ 0x{off:x}"
                    )));
                }
                handlers.push(h);
            }
            // valida tries dentro da stream
            for t in &tries {
                let end = t.start_addr as usize + t.insn_count as usize;
                if end > insns.len() {
                    return Err(RdError::invalid_format(format!(
                        "code_item: try [0x{:x}, +{}) excede a stream ({} units)",
                        t.start_addr,
                        t.insn_count,
                        insns.len()
                    )));
                }
            }
        }

        Ok(CodeItem {
            offset: off,
            registers_size,
            ins_size,
            outs_size,
            tries_size,
            debug_info_off,
            insns,
            instructions,
            tries,
            handlers,
            handlers_offset,
        })
    }

    /// Re-encodifica todos os code units a partir do modelo decodificado.
    /// Usado no teste de round-trip (re-encode → bytes idênticos).
    pub fn reencode(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.insns.len());
        for (_, insn) in &self.instructions {
            out.extend(insn.encode());
        }
        out
    }
}

fn decode_all(insns: &[u16]) -> RdResult<Vec<(usize, Insn)>> {
    let mut out = Vec::new();
    let mut addr = 0usize;
    while addr < insns.len() {
        let (insn, size) = decode_at(insns, addr)?;
        out.push((addr, insn));
        // avanço obrigatório (guard anti-loop)
        if size == 0 {
            return Err(RdError::parse(format!(
                "insn: tamanho 0 no endereço {addr} — stream malformada"
            )));
        }
        addr += size as usize;
    }
    if addr != insns.len() {
        return Err(RdError::parse(format!(
            "insn: última instrução excede a stream ({addr} > {})",
            insns.len()
        )));
    }
    Ok(out)
}

/// Decodifica a instrução em `addr` (retorna insn + tamanho em units).
pub fn decode_at(insns: &[u16], addr: usize) -> RdResult<(Insn, u16)> {
    let unit0 = *insns
        .get(addr)
        .ok_or_else(|| RdError::parse(format!("insn: endereço {addr} fora da stream")))?;

    // payloads: identificador na palavra alta com opcode 0x00
    if let Some(kind) = PayloadKind::from_unit(unit0) {
        return decode_payload(insns, addr, kind);
    }

    let op = (unit0 & 0xFF) as u8;
    let o = info(op);
    let b1 = unit0 >> 8; // segundo byte
    let n4 = |v: u16| (v & 0xF) as u8;
    let s4 = |v: u16| {
        let n = (v & 0xF) as u8;
        if n & 0x8 != 0 {
            n as i8 - 16
        } else {
            n as i8
        }
    };
    let unit = |i: usize| -> RdResult<u16> {
        insns
            .get(addr + i)
            .copied()
            .ok_or_else(|| RdError::parse(format!("insn: truncada no unit {i} @ {addr}")))
    };

    let mk = |size: u16, kind: Kind| Insn {
        opcode: op,
        fmt: o.format,
        size,
        kind,
    };

    match o.format {
        Format::F10x => Ok((mk(1, Kind::Plain), 1)),
        Format::F12x => Ok((mk(1, Kind::Regs(n4(b1), n4(b1 >> 4))), 1)),
        Format::F11n => Ok((mk(1, Kind::RegLit4(n4(b1), s4(b1 >> 4))), 1)),
        Format::F11x => Ok((mk(1, Kind::Reg(b1 as u8)), 1)),
        Format::F10t => Ok((mk(1, Kind::Branch8(b1 as u8 as i8)), 1)),
        Format::F20t => {
            let u = unit(1)?;
            Ok((mk(2, Kind::Branch16(u as i16)), 2))
        }
        Format::F22x | Format::F32x => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegReg16(b1 as u8, u)), 2))
        }
        Format::F21t => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegBranch16(b1 as u8, u as i16)), 2))
        }
        Format::F21s => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegLit16(b1 as u8, u as i16)), 2))
        }
        Format::F21h => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegHigh16(b1 as u8, u)), 2))
        }
        Format::F21c => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegIndex(b1 as u8, u)), 2))
        }
        Format::F23x => {
            let u = unit(1)?;
            Ok((
                mk(
                    2,
                    Kind::RegRegReg(b1 as u8, (u & 0xFF) as u8, (u >> 8) as u8),
                ),
                2,
            ))
        }
        Format::F22b => {
            let u = unit(1)?;
            Ok((
                mk(
                    2,
                    Kind::RegRegLit8(b1 as u8, (u & 0xFF) as u8, (u >> 8) as u8 as i8),
                ),
                2,
            ))
        }
        Format::F22t => {
            let u = unit(1)?;
            Ok((
                mk(2, Kind::RegRegBranch16(n4(b1), n4(b1 >> 4), u as i16)),
                2,
            ))
        }
        Format::F22s => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegRegLit16(n4(b1), n4(b1 >> 4), u as i16)), 2))
        }
        Format::F22c => {
            let u = unit(1)?;
            Ok((mk(2, Kind::RegRegIndex(n4(b1), n4(b1 >> 4), u)), 2))
        }
        Format::F30t => {
            let lo = unit(1)?;
            let hi = unit(2)?;
            let off = (((hi as u32) << 16) | lo as u32) as i32;
            Ok((mk(3, Kind::Branch32(off)), 3))
        }
        Format::F31i | Format::F31t | Format::F31c => {
            let lo = unit(1)?;
            let hi = unit(2)?;
            let raw = ((hi as u32) << 16) | lo as u32;
            let kind = match o.format {
                Format::F31i => Kind::RegLit32(b1 as u8, raw as i32),
                Format::F31t => Kind::RegBranch32(b1 as u8, raw as i32),
                _ => Kind::RegIndex32(b1 as u8, raw),
            };
            Ok((mk(3, kind), 3))
        }
        Format::F35c => {
            let u1 = unit(1)?;
            let u2 = unit(2)?;
            let count = n4(b1 >> 4); // A é o nibble ALTO (G é o baixo)
                                     // tolerante a count 0: dexes reais (d8) emitem filled-new-array vazio;
                                     // > 5 não é representável no formato (só há 5 slots de reg)
            if count > 5 {
                return Err(RdError::invalid_format(format!(
                    "insn: invoke 35c com count {count} > 5 @ {addr}"
                )));
            }
            let mut regs = [0u8; 5];
            regs[0] = n4(u2);
            regs[1] = n4(u2 >> 4);
            regs[2] = n4(u2 >> 8);
            regs[3] = n4(u2 >> 12);
            regs[4] = n4(b1); // G (nibble baixo)
            Ok((
                mk(
                    3,
                    Kind::Invoke35c {
                        count,
                        idx: u1,
                        regs,
                    },
                ),
                3,
            ))
        }
        Format::F3rc => {
            let u1 = unit(1)?;
            let u2 = unit(2)?;
            Ok((
                mk(
                    3,
                    Kind::InvokeRange3rc {
                        count: b1 as u8,
                        idx: u1,
                        start: u2,
                    },
                ),
                3,
            ))
        }
        Format::F45cc => {
            let u1 = unit(1)?;
            let u2 = unit(2)?;
            let u3 = unit(3)?;
            let count = n4(b1 >> 4); // A é o nibble ALTO (G é o baixo)
                                     // tolerante a count 0 (mesma justificativa do 35c)
            if count > 5 {
                return Err(RdError::invalid_format(format!(
                    "insn: invoke-polymorphic com count {count} > 5 @ {addr}"
                )));
            }
            let mut regs = [0u8; 5];
            regs[0] = n4(u2);
            regs[1] = n4(u2 >> 4);
            regs[2] = n4(u2 >> 8);
            regs[3] = n4(u2 >> 12);
            regs[4] = n4(b1); // G (nibble baixo)
            Ok((
                mk(
                    4,
                    Kind::Invoke45cc {
                        count,
                        idx: u1,
                        regs,
                        proto: u3,
                    },
                ),
                4,
            ))
        }
        Format::F4rcc => {
            let u1 = unit(1)?;
            let u2 = unit(2)?;
            let u3 = unit(3)?;
            Ok((
                mk(
                    4,
                    Kind::InvokeRange4rcc {
                        count: b1 as u8,
                        idx: u1,
                        start: u2,
                        proto: u3,
                    },
                ),
                4,
            ))
        }
        Format::F51l => {
            let mut raw = [0u8; 8];
            for (u, o) in [(1usize, 0usize), (2, 2), (3, 4), (4, 6)] {
                let u16v = unit(u)?;
                raw[o] = (u16v & 0xFF) as u8;
                raw[o + 1] = (u16v >> 8) as u8;
            }
            Ok((mk(5, Kind::Lit64(b1 as u8, i64::from_le_bytes(raw))), 5))
        }
    }
}

/// shifts de long têm o contador como INT (spec Dalvik) — o segundo
/// operando NÃO é par wide.
fn is_long_shift(name: &str) -> bool {
    name.starts_with("shl-long") || name.starts_with("shr-long") || name.starts_with("ushr-long")
}

/// Validação de registradores (issue #17): todo índice de registrador usado
/// pelas instruções decodificadas deve ser < registers_size; operações wide
/// (long/double) ocupam o par (v, v+1), então v+1 também precisa caber.
///
/// A wideness é derivada do nome do opcode (na spec Dalvik, wide ⇔ nome
/// contém "wide"/"long"/"double") — exatamente os opcodes cujos operandos
/// ocupam pares. Converte erros de índice em falha tipada no load, nunca
/// pânico/abort no interpretador.
fn validate_registers(instructions: &[(usize, Insn)], registers_size: u16) -> RdResult<()> {
    let nregs = registers_size as usize;
    let wide_name = |n: &str| n.contains("wide") || n.contains("-long") || n.contains("-double");
    let chk = |addr: usize, op: u8, r: u32, wide: bool| -> RdResult<()> {
        let over = if wide {
            r as usize >= nregs || r as usize + 1 >= nregs
        } else {
            r as usize >= nregs
        };
        if over {
            let how = if wide { " (par wide)" } else { "" };
            return Err(RdError::invalid_format(format!(
                "code_item: registrador v{r}{how} fora do intervalo \
                 (registers_size={registers_size}) @ insn 0x{addr:x} (op 0x{op:02x})"
            )));
        }
        Ok(())
    };

    for (addr, insn) in instructions {
        let (addr, op) = (*addr, insn.opcode);
        let name = info(op).name;
        match &insn.kind {
            Kind::Plain | Kind::Branch8(_) | Kind::Branch16(_) | Kind::Branch32(_) => {}
            Kind::Payload(_) => {} // pseudoinstrução: sem registradores
            Kind::Regs(a, b) => {
                // 12x: move-wide, neg/not-long e aritmética */2addr são wide
                // nos dois; conversões int-to-long têm só o destino wide;
                // shifts-long/2addr têm contador INT (não par) no segundo op
                let (dw, sw) = if wide_name(name) {
                    match name.split_once("-to-") {
                        Some((src, dst)) => (
                            matches!(dst, "long" | "double"),
                            matches!(src, "long" | "double"),
                        ),
                        None if is_long_shift(name) => (true, false),
                        None => (true, true),
                    }
                } else {
                    (false, false)
                };
                chk(addr, op, *a as u32, dw)?;
                chk(addr, op, *b as u32, sw)?;
            }
            Kind::RegLit4(a, _) => chk(addr, op, *a as u32, false)?, // const/4 narrow
            Kind::Reg(a) => chk(addr, op, *a as u32, wide_name(name))?, // return-wide, move-result-wide
            Kind::RegReg16(a, b) => {
                // 22x/32x: move-wide/from16 e move-wide/16 — ambos wide
                let w = wide_name(name);
                chk(addr, op, *a as u32, w)?;
                chk(addr, op, *b as u32, w)?;
            }
            Kind::RegBranch16(a, _)
            | Kind::RegBranch32(a, _)
            | Kind::RegLit16(a, _)
            | Kind::RegHigh16(a, _)
            | Kind::RegIndex(a, _)
            | Kind::RegIndex32(a, _)
            | Kind::RegLit32(a, _)
            | Kind::Lit64(a, _) => {
                chk(addr, op, *a as u32, wide_name(name))?; // família const-wide*
            }
            Kind::RegRegReg(a, b, c) => {
                // 23x: aget/aput-wide têm só A em par; cmp-long/cmp*-double
                // têm B,C wide e A narrow (cmp*-float é TODO narrow);
                // shifts-long têm o contador INT em C; demais long/double
                // aritmética é wide nos três
                if name == "aget-wide" || name == "aput-wide" {
                    chk(addr, op, *a as u32, true)?;
                    chk(addr, op, *b as u32, false)?;
                    chk(addr, op, *c as u32, false)?;
                } else if name.starts_with("cmp") {
                    let w = name.contains("long") || name.contains("double");
                    chk(addr, op, *a as u32, false)?;
                    chk(addr, op, *b as u32, w)?;
                    chk(addr, op, *c as u32, w)?;
                } else if is_long_shift(name) {
                    chk(addr, op, *a as u32, true)?;
                    chk(addr, op, *b as u32, true)?;
                    chk(addr, op, *c as u32, false)?;
                } else {
                    let w = wide_name(name);
                    chk(addr, op, *a as u32, w)?;
                    chk(addr, op, *b as u32, w)?;
                    chk(addr, op, *c as u32, w)?;
                }
            }
            Kind::RegRegLit8(a, b, _)
            | Kind::RegRegBranch16(a, b, _)
            | Kind::RegRegLit16(a, b, _)
            | Kind::RegRegIndex(a, b, _) => {
                // 22b/22t/22s/22c: int/bool/refs — iget-wide/iput-wide têm A wide
                chk(addr, op, *a as u32, wide_name(name))?;
                chk(addr, op, *b as u32, false)?;
            }
            Kind::Invoke35c { count, regs, .. } | Kind::Invoke45cc { count, regs, .. } => {
                for r in regs.iter().take((*count as usize).min(5)) {
                    chk(addr, op, *r as u32, false)?;
                }
            }
            Kind::InvokeRange3rc { count, start, .. }
            | Kind::InvokeRange4rcc { count, start, .. } => {
                chk(addr, op, *start as u32, false)?;
                if *count > 0 {
                    chk(addr, op, (*start as u32) + (*count as u32) - 1, false)?;
                }
            }
        }
    }
    Ok(())
}

fn decode_payload(insns: &[u16], addr: usize, kind: PayloadKind) -> RdResult<(Insn, u16)> {
    let unit = |i: usize| -> RdResult<u16> {
        insns
            .get(addr + i)
            .copied()
            .ok_or_else(|| RdError::parse(format!("payload: truncado no unit {i} @ {addr}")))
    };
    let i32_at = |i: usize| -> RdResult<i32> {
        let lo = unit(i)? as u32;
        let hi = unit(i + 1)? as u32;
        Ok(((hi << 16) | lo) as i32)
    };

    let payload = match kind {
        PayloadKind::PackedSwitch => {
            let size = unit(1)? as usize;
            let first_key = i32_at(2)?;
            let mut targets = Vec::with_capacity(size.min(1 << 16));
            for i in 0..size {
                targets.push(i32_at(4 + i * 2)?);
            }
            Payload::PackedSwitch { first_key, targets }
        }
        PayloadKind::SparseSwitch => {
            let size = unit(1)? as usize;
            let mut keys = Vec::with_capacity(size.min(1 << 16));
            for i in 0..size {
                keys.push(i32_at(2 + i * 2)?);
            }
            let mut targets = Vec::with_capacity(size.min(1 << 16));
            for i in 0..size {
                targets.push(i32_at(2 + size * 2 + i * 2)?);
            }
            Payload::SparseSwitch { keys, targets }
        }
        PayloadKind::ArrayData => {
            let element_width = unit(1)?;
            let lo = unit(2)? as u32;
            let hi = unit(3)? as u32;
            let element_count = (hi << 16) | lo;
            // issue #44: total × padded em u64 — element_count=0x00010001 ×
            // width=0xFFFF → o produto (0xFFFFFFFF) cabe em u32, mas
            // div_ceil(2)*2 = 0x100000000 estoura u32: panic em debug, e em
            // release envolve para 0 → o guard "cabe no método" passava e o
            // decoder emitia Payload interno inconsistente (data: [])
            let total = (element_count as u64) * (element_width as u64);
            let padded = total.div_ceil(2) * 2;
            // issue #19: o payload precisa caber no fluxo de instruções
            // restante do método (header de 4 units + dados) — validar ANTES
            // do with_capacity: element_count é u32 do atacante e a alocação
            // de até 4 GiB abortava o processo (não é pânico capturável)
            let needed = 4u64 + padded / 2;
            let available = (insns.len().saturating_sub(addr)) as u64;
            if available < needed {
                return Err(RdError::invalid_format(format!(
                    "fill-array-data: payload de {padded} bytes excede o método \
                     ({available} units restantes @ {addr})"
                )));
            }
            let mut data = Vec::with_capacity(padded as usize);
            for i in 0..padded {
                let u = unit(4 + (i / 2) as usize)?;
                data.push(if i % 2 == 0 {
                    (u & 0xFF) as u8
                } else {
                    (u >> 8) as u8
                });
            }
            Payload::ArrayData {
                element_width,
                element_count,
                data,
            }
        }
    };

    let size = payload.size_in_units();
    // issue #36: size_in_units trunca para u16 — um packed-switch com ≥32766
    // alvos (só em stream malformada; dx/d8 nunca emitem) envolveria e
    // desalinharía toda a decodificação seguinte. Falha tipada antes.
    let units = match &payload {
        Payload::PackedSwitch { targets, .. } => 4 + 2 * targets.len(),
        Payload::SparseSwitch { keys, .. } => 2 + 4 * keys.len(),
        Payload::ArrayData { data, .. } => 4 + data.len() / 2,
    };
    if units > u16::MAX as usize {
        return Err(RdError::invalid_format(
            "payload: tamanho excede 65535 code units (stream malformada)",
        ));
    }
    Ok((
        Insn {
            opcode: 0,
            fmt: Format::F10x,
            size,
            kind: Kind::Payload(payload),
        },
        size,
    ))
}

fn parse_catch_handler(data: &[u8], off: usize) -> RdResult<CatchHandler> {
    let mut r = read::Reader::at(data, off)?;
    let size = r.sleb128()?;
    let typed_count = size.unsigned_abs() as usize;
    // guard fuzz
    if typed_count > r.remaining() {
        return Err(RdError::parse(format!(
            "catch handler: {typed_count} entradas > {} bytes @ 0x{off:x}",
            r.remaining()
        )));
    }
    let mut h = CatchHandler::default();
    for _ in 0..typed_count {
        let type_idx = r.uleb128()?;
        let addr = r.uleb128()?;
        h.typed.push((type_idx, addr));
    }
    if size <= 0 {
        h.catch_all = Some(r.uleb128()?);
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_12x_move() {
        // move v1, v2 → byte1 = (B<<4)|A = 0x21 → unit 0x2101
        let insns = [0x2101u16];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 1);
        assert_eq!(i.opcode, 0x01);
        assert_eq!(i.kind, Kind::Regs(1, 2));
        assert_eq!(i.encode(), vec![0x2101]);
    }

    #[test]
    fn decodes_11n_const4_negative() {
        // const/4 v0, -1 → op 0x12, A=0, B=0xF → unit = (0xF<<12)|(0<<8)|0x12
        let insns = [0xF012u16];
        let (i, _) = decode_at(&insns, 0).unwrap();
        assert_eq!(i.kind, Kind::RegLit4(0, -1));
        assert_eq!(i.encode(), vec![0xF012]);
    }

    #[test]
    fn decodes_35c_invoke() {
        // invoke-virtual {v1, v2}, m@0x1234
        let insns = [0x206eu16, 0x1234, 0x0021];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 3);
        match i.kind {
            Kind::Invoke35c { count, idx, regs } => {
                assert_eq!(count, 2);
                assert_eq!(idx, 0x1234);
                assert_eq!(&regs[0..2], &[1, 2]);
            }
            other => panic!("kind errado: {other:?}"),
        }
        assert_eq!(i.encode(), vec![0x206e, 0x1234, 0x0021]);
    }

    #[test]
    fn decodes_51l_const_wide() {
        let insns = [0x0018u16, 0xCDEF, 0x89AB, 0x4567, 0x0123];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 5);
        assert_eq!(i.kind, Kind::Lit64(0, 0x0123_4567_89AB_CDEF));
        assert_eq!(i.encode(), insns.to_vec());
    }

    #[test]
    fn decodes_21h_high16() {
        // const/high16 v1, raw 0x1234 → op 0x15, A=1 → unit0 = 0x0115
        let insns = [0x0115u16, 0x1234];
        let (i, _) = decode_at(&insns, 0).unwrap();
        assert_eq!(i.kind, Kind::RegHigh16(1, 0x1234));
        assert_eq!(i.encode(), insns.to_vec());
    }

    #[test]
    fn decodes_packed_switch_payload() {
        // 1 alvo: first_key=5, target=-2
        let insns = [0x0100u16, 0x0001, 0x0005, 0x0000, 0xFFFE, 0xFFFF];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 6);
        match i.kind {
            Kind::Payload(Payload::PackedSwitch {
                first_key,
                ref targets,
            }) => {
                assert_eq!(first_key, 5);
                assert_eq!(*targets, vec![-2]);
            }
            other => panic!("kind errado: {other:?}"),
        }
        assert_eq!(i.encode(), insns.to_vec());
    }

    #[test]
    fn decodes_sparse_switch_payload() {
        let insns = [0x0200u16, 0x0001, 0x0001, 0x0000, 0x0008, 0x0000];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 6);
        match i.kind {
            Kind::Payload(Payload::SparseSwitch {
                ref keys,
                ref targets,
            }) => {
                assert_eq!(*keys, vec![1]);
                assert_eq!(*targets, vec![8]);
            }
            other => panic!("kind errado: {other:?}"),
        }
        assert_eq!(i.encode(), insns.to_vec());
    }

    #[test]
    fn decodes_fill_array_payload_with_padding() {
        // 3 elementos de 1 byte + padding — count bruto preservado (Lei 2)
        let insns = [0x0300u16, 0x0001, 0x0003, 0x0000, 0x0201, 0x0003];
        let (i, size) = decode_at(&insns, 0).unwrap();
        assert_eq!(size, 6);
        match i.kind {
            Kind::Payload(Payload::ArrayData {
                element_width,
                element_count,
                ref data,
            }) => {
                assert_eq!(element_width, 1);
                assert_eq!(element_count, 3);
                assert_eq!(*data, vec![0x01, 0x02, 0x03, 0x00]);
            }
            other => panic!("kind errado: {other:?}"),
        }
        assert_eq!(i.encode(), insns.to_vec());
    }

    #[test]
    fn truncated_instruction_is_typed_error() {
        let insns = [0x026eu16];
        assert!(decode_at(&insns, 0).is_err());
        assert!(decode_all(&[]).unwrap().is_empty());
        assert!(decode_all(&[0x026e, 0x1234]).is_err());
    }

    #[test]
    fn all_256_opcodes_decode_without_panic() {
        for op in 0..=255u16 {
            // stream de 5 units (suficiente p/ o maior formato, 51l). O byte1:
            // 0x00 p/ 10x (byte alto deve ser zero) e count=1 p/ os invokes;
            // 0x01 é inócuo nos demais formatos (A=1).
            let fmt = info(op as u8).format;
            let unit0 = match fmt {
                // byte alto é 00 por spec nesses formatos (o encode o descarta)
                Format::F10x | Format::F20t | Format::F30t => op,
                _ => op | 0x0100,
            };
            let insns = [unit0, 0x0000, 0x0000, 0x0000, 0x0000];
            let (i, size) =
                decode_at(&insns, 0).unwrap_or_else(|e| panic!("opcode 0x{op:02x} falhou: {e}"));
            assert_eq!(i.opcode as u16, op, "opcode 0x{op:02x}: byte baixo é o op");
            assert!(
                (1..=5).contains(&size),
                "0x{op:02x}: tamanho {size} fora de 1..=5"
            );
            // round-trip: re-encode == units consumidos
            assert_eq!(
                i.encode(),
                insns[..size as usize].to_vec(),
                "round-trip do opcode 0x{op:02x}"
            );
        }
    }

    #[test]
    fn branch_target_computation() {
        // goto -7: op 0x28, A=-7=0xF9 no byte alto → unit 0xF928
        let insns = [0xF928u16];
        let (i, _) = decode_at(&insns, 0).unwrap();
        assert_eq!(i.kind, Kind::Branch8(-7));
        assert_eq!(i.branch_target(10), Some(3));
        let back = Insn {
            opcode: 0x28,
            fmt: Format::F10t,
            size: 1,
            kind: Kind::Branch8(-1),
        };
        // alvo absoluto negativo não existe (return None), 1-1 = 0 existe
        assert_eq!(back.branch_target(0), None);
        assert_eq!(back.branch_target(1), Some(0));
    }

    fn build_code_item(insns: &[u16], tries: &[(u32, u16, u16)], handler_bytes: &[u8]) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&3u16.to_le_bytes()); // registers
        data.extend_from_slice(&1u16.to_le_bytes()); // ins
        data.extend_from_slice(&1u16.to_le_bytes()); // outs
        data.extend_from_slice(&(tries.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // debug off
        data.extend_from_slice(&(insns.len() as u32).to_le_bytes());
        for u in insns {
            data.extend_from_slice(&u.to_le_bytes());
        }
        if insns.len() % 2 == 1 {
            data.extend_from_slice(&[0, 0]); // padding
        }
        for (start, count, hoff) in tries {
            data.extend_from_slice(&start.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&hoff.to_le_bytes());
        }
        // spec: encoded_catch_handler_list.size é ULEB128
        data.push(tries.len() as u8);
        data.extend_from_slice(handler_bytes);
        data
    }

    #[test]
    fn code_item_roundtrip_and_catch() {
        let insns: Vec<u16> = vec![0x0000, 0xFE28, 0x000E]; // nop, goto -2, return-void
                                                            // handler_off=0 relativo ao início da lista (0x01 size + handler)
        let data = build_code_item(&insns, &[(0, 3, 1)], &[0x01, 0x02, 0x01]);
        let ci = CodeItem::parse(&data, 0).expect("code item");
        assert_eq!(ci.registers_size, 3);
        assert_eq!(ci.instructions.len(), 3);
        assert_eq!(ci.reencode(), insns);
        assert_eq!(ci.tries.len(), 1);
        assert_eq!(ci.handlers[0].typed, vec![(2, 1)]);
        assert!(ci.handlers[0].catch_all.is_none());
    }

    #[test]
    fn code_item_catch_all_and_multiple_typed() {
        // handler com 2 typed + catchall: sleb -2 = 0x7E; t3@a5, t7@a9, catchall@11
        let insns: Vec<u16> = vec![0x000E, 0x0000]; // par — sem padding
        let data = build_code_item(&insns, &[(0, 2, 1)], &[0x7E, 0x03, 0x05, 0x07, 0x09, 0x0B]);
        let ci = CodeItem::parse(&data, 0).expect("code item");
        let h = &ci.handlers[0];
        assert_eq!(h.typed, vec![(3, 5), (7, 9)]);
        assert_eq!(h.catch_all, Some(11));
    }

    #[test]
    fn try_beyond_stream_is_invalid() {
        let insns: Vec<u16> = vec![0x0e00];
        let data = build_code_item(&insns, &[(0, 50, 0)], &[0x01, 0x02, 0x01]);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
    }

    // issue #17: helper com registers_size/ins_size customizados
    fn build_code_item_regs(registers: u16, ins: u16, insns: &[u16]) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&registers.to_le_bytes());
        data.extend_from_slice(&ins.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes()); // outs
        data.extend_from_slice(&0u16.to_le_bytes()); // tries
        data.extend_from_slice(&0u32.to_le_bytes()); // debug off
        data.extend_from_slice(&(insns.len() as u32).to_le_bytes());
        for u in insns {
            data.extend_from_slice(&u.to_le_bytes());
        }
        data
    }

    #[test]
    fn register_index_beyond_registers_size_is_invalid() {
        // add-int v0, v1, v200 (23x: op 0x90) com registers_size=2 — PoC da issue #17
        // unit0 = op | (AA<<8); unit1 = BB | (CC<<8)
        let insns: Vec<u16> = vec![0x0090, 0xC801];
        let data = build_code_item_regs(2, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("registrador v200"));
    }

    #[test]
    fn wide_pair_at_last_register_is_invalid() {
        // const-wide/16 v0 (21s) com registers_size=1 — o par (v0,v1) não cabe
        // encode 21s: op | (AA<<8); const-wide/16 é op 0x16
        let insns: Vec<u16> = vec![0x0016, 0x0001];
        let data = build_code_item_regs(1, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("par wide"));
        // com registers_size=2 o mesmo código é válido
        let data = build_code_item_regs(2, 0, &insns);
        CodeItem::parse(&data, 0).expect("const-wide/16 v0 cabe em 2 registradores");
    }

    #[test]
    fn invoke_range_beyond_registers_size_is_invalid() {
        // invoke-virtual/range {v2..v3}, meth@0 (3rc) com registers_size=2
        // encode 3rc: op | (count<<8), idx, start — count=2, start=2
        let insns: Vec<u16> = vec![0x0274, 0x0000, 0x0202];
        let data = build_code_item_regs(2, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
    }

    #[test]
    fn conversion_wide_dest_at_boundary_is_invalid() {
        // int-to-long v0, v0 (12x) com registers_size=1: o destino wide (par v0+v1) não cabe
        // encode 12x: op | (B<<12) | (A<<8) — A=B=0 → só o opcode
        let insns: Vec<u16> = vec![0x81];
        let data = build_code_item_regs(1, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("par wide"));
    }

    #[test]
    fn long_shift_count_is_narrow_operand() {
        // regressão golden (shlJJ(JI)J, regs=3): shl-long/2addr v1, v2 —
        // A é par wide (v1,v2)... não: A wide (v1+v2 espera regs=3) e o
        // CONTADOR v2 é int — em 12x: A=1 wide (pair 1,2 ok com regs=3),
        // B=2 narrow (sem +1)
        let insns: Vec<u16> = vec![0xC3 | (2 << 12) | (1 << 8)]; // shl-long/2addr v1, v2
        let data = build_code_item_regs(3, 3, &insns);
        CodeItem::parse(&data, 0).expect("contador de shift é int, não par");
        // e o par wide do destino precisa caber: shl-long/2addr v2, v1 com regs=3 → pair (2,3) estoura
        let insns: Vec<u16> = vec![0xC3 | (1 << 12) | (2 << 8)];
        let data = build_code_item_regs(3, 3, &insns);
        assert!(CodeItem::parse(&data, 0).is_err());
    }

    #[test]
    fn cmp_float_is_all_narrow() {
        // regressão golden (ltFF(FF)Z, regs=2): cmpg-float v0, v0, v1
        let insns: Vec<u16> = vec![0x002E, 0x0100]; // cmpg-float vAA=0, vBB=0, vCC=1
        let data = build_code_item_regs(2, 2, &insns);
        CodeItem::parse(&data, 0).expect("cmpl/cmpg-float não usa pares wide");
    }

    #[test]
    fn ins_size_greater_than_registers_size_is_invalid() {
        // issue #20: registers_size=0, ins_size=1 — subtração do disasm wraparia
        let data = build_code_item_regs(0, 1, &[0x000E]);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("ins_size"));
    }

    #[test]
    fn fill_array_data_payload_beyond_method_is_invalid() {
        // issue #19: payload declarado de 2 GiB num método de 10 units
        // fill-array-data v0, +3 (31t: 3 units — op|AA + offset32)
        // payload @3: ident=0x0300, width=2, element_count=0x40000000
        // (u32 do atacante; sem u32 overflow no produto)
        let insns: Vec<u16> = vec![
            0x0026, 0x0003, 0x0000, 0x0300, 0x0002, 0x0000, 0x4000, 0x0000, 0x0000, 0x0000,
        ];
        let data = build_code_item_regs(4, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("fill-array-data"));
    }

    #[test]
    fn fill_array_data_padded_u32_wrap_is_typed_error() {
        // issue #44: count=0x00010001 × width=0xFFFF → produto 0xFFFFFFFF CABE
        // em u32, mas div_ceil(2)*2 = 0x100000000 estourava: panic "multiply
        // with overflow" em debug; em release envolvia para 0 → guard passava
        // e o decoder emitia Payload::ArrayData inconsistente (data: [])
        let insns: Vec<u16> = vec![
            0x0026, 0x0003, 0x0000, 0x0300, 0xFFFF, 0x0001, 0x0001, 0x0000, 0x0000, 0x0000,
        ];
        let data = build_code_item_regs(4, 0, &insns);
        let e = CodeItem::parse(&data, 0).unwrap_err();
        assert_eq!(e.code, "INVALID_FORMAT");
        assert!(e.cause.contains("fill-array-data"));
    }
}
