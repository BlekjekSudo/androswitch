//! debug_info_item — máquina de estados completa do debug info Dalvik.
//!
//! Opcodes: 0x00 END_SEQUENCE, 0x01 ADVANCE_PC, 0x02 ADVANCE_LINE,
//! 0x03 START_LOCAL, 0x04 START_LOCAL_EXTENDED, 0x05 END_LOCAL,
//! 0x06 RESTART_LOCAL, 0x07 SET_PROLOGUE_END, 0x08 SET_EPILOGUE_BEGIN,
//! 0x09 SET_FILE, 0x0a+ especial: `adjusted = op - 0x0a`;
//! `addr += adjusted / 15`; `line += LINE_BASE(-4) + adjusted % 15`.

#[cfg(not(feature = "std"))]
use alloc::{format, vec, vec::Vec};

use crate::error::{RdError, RdResult};
use crate::read::Reader;

pub const DBG_END_SEQUENCE: u8 = 0x00;
pub const DBG_ADVANCE_PC: u8 = 0x01;
pub const DBG_ADVANCE_LINE: u8 = 0x02;
pub const DBG_START_LOCAL: u8 = 0x03;
pub const DBG_START_LOCAL_EXTENDED: u8 = 0x04;
pub const DBG_END_LOCAL: u8 = 0x05;
pub const DBG_RESTART_LOCAL: u8 = 0x06;
pub const DBG_SET_PROLOGUE_END: u8 = 0x07;
pub const DBG_SET_EPILOGUE_BEGIN: u8 = 0x08;
pub const DBG_SET_FILE: u8 = 0x09;
pub const LINE_BASE: i32 = -4;
pub const LINE_RANGE: u32 = 15;
pub const FIRST_SPECIAL: u8 = 0x0a;

#[derive(Debug, Clone, PartialEq)]
pub enum LocalEvent {
    Start {
        reg: u16,
        name: Option<u32>,
        typ: Option<u32>,
        sig: Option<u32>,
    },
    End {
        reg: u16,
    },
    Restart {
        reg: u16,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DebugEvent {
    Local(LocalEvent),
    SetPrologueEnd,
    SetEpilogueBegin,
    SetFile(Option<u32>),
}

/// Uma entrada posicional (estado após eventos no endereço `addr`).
#[derive(Debug, Clone, PartialEq)]
pub struct DebugEntry {
    pub addr: u32,
    pub line: u32,
    pub events: Vec<DebugEvent>,
}

/// debug_info_item parseado.
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    pub line_start: u32,
    /// nomes de parâmetros (uleb128p1; None = NO_INDEX).
    pub parameter_names: Vec<Option<u32>>,
    /// entradas posicionais (addr, line, eventos).
    pub entries: Vec<DebugEntry>,
    /// offset bruto (Lei 2).
    pub offset: usize,
}

impl DebugInfo {
    /// Mapa endereço → linha (última linha conhecida por addr).
    pub fn line_map(&self) -> Vec<(u32, u32)> {
        self.entries
            .iter()
            .filter(|e| !e.events.iter().any(|ev| matches!(ev, DebugEvent::Local(_))))
            .map(|e| (e.addr, e.line))
            .collect()
    }

    /// Eventos de variáveis locais por endereço.
    pub fn locals_at(&self) -> Vec<(u32, &LocalEvent)> {
        let mut out = Vec::new();
        for e in &self.entries {
            for ev in &e.events {
                if let DebugEvent::Local(l) = ev {
                    out.push((e.addr, l));
                }
            }
        }
        out
    }

    /// Última SetFile vista (usado p/ comentários `.source` em debug extendido).
    pub fn last_file(&self) -> Option<u32> {
        for e in self.entries.iter().rev() {
            for ev in &e.events {
                if let DebugEvent::SetFile(f) = ev {
                    return *f;
                }
            }
        }
        None
    }
}

pub fn parse_debug_info(data: &[u8], off: u32) -> RdResult<DebugInfo> {
    let mut r = Reader::at(data, off as usize)?;
    let mut info = DebugInfo {
        offset: off as usize,
        ..Default::default()
    };
    info.line_start = r.uleb128()?;
    let params = r.uleb128()? as usize;
    // guard fuzz: cada nome tem ≥1 byte
    if params > r.remaining() {
        return Err(RdError::parse(format!(
            "debug_info: {params} parâmetros > {} bytes restantes",
            r.remaining()
        )));
    }
    for _ in 0..params {
        let idx = r.uleb128p1()?;
        info.parameter_names
            .push(if idx < 0 { None } else { Some(idx as u32) });
    }

    let mut addr: u32 = 0;
    let mut line: i64 = info.line_start as i64;
    let mut current: Option<DebugEntry> = None;

    // guard anti-loop: cada opcode consome ≥1 byte
    let mut steps = 0usize;
    let max_steps = r.remaining().max(1) * 2 + 16;

    while steps <= max_steps {
        steps += 1;
        let op = r.u8()?;
        if op == DBG_END_SEQUENCE {
            break;
        }
        match op {
            DBG_ADVANCE_PC => {
                addr = addr.wrapping_add(r.uleb128()?);
            }
            DBG_ADVANCE_LINE => {
                line += r.sleb128()? as i64;
            }
            DBG_START_LOCAL => {
                let reg = r.uleb128()? as u16;
                let name = r.uleb128p1()?;
                let typ = r.uleb128p1()?;
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::Local(LocalEvent::Start {
                        reg,
                        name: opt_idx(name),
                        typ: opt_idx(typ),
                        sig: None,
                    }),
                );
            }
            DBG_START_LOCAL_EXTENDED => {
                let reg = r.uleb128()? as u16;
                let name = r.uleb128p1()?;
                let typ = r.uleb128p1()?;
                let sig = r.uleb128p1()?;
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::Local(LocalEvent::Start {
                        reg,
                        name: opt_idx(name),
                        typ: opt_idx(typ),
                        sig: opt_idx(sig),
                    }),
                );
            }
            DBG_END_LOCAL => {
                let reg = r.uleb128()? as u16;
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::Local(LocalEvent::End { reg }),
                );
            }
            DBG_RESTART_LOCAL => {
                let reg = r.uleb128()? as u16;
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::Local(LocalEvent::Restart { reg }),
                );
            }
            DBG_SET_PROLOGUE_END => {
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::SetPrologueEnd,
                );
            }
            DBG_SET_EPILOGUE_BEGIN => {
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::SetEpilogueBegin,
                );
            }
            DBG_SET_FILE => {
                let name = r.uleb128p1()?;
                push_event(
                    &mut current,
                    &mut info.entries,
                    addr,
                    line,
                    DebugEvent::SetFile(opt_idx(name)),
                );
            }
            _ => {
                // opcode especial (≥ 0x0a)
                let adjusted = (op as u32).wrapping_sub(FIRST_SPECIAL as u32);
                addr = addr.wrapping_add(adjusted / LINE_RANGE);
                line += LINE_BASE as i64 + (adjusted % LINE_RANGE) as i64;
                set_position(&mut current, &mut info.entries, addr, line);
            }
        }
    }
    if steps > max_steps {
        return Err(RdError::parse(format!(
            "debug_info: sequência não terminada (guard de loop) @ 0x{off:x}"
        )));
    }
    if let Some(e) = current.take() {
        info.entries.push(e);
    }
    Ok(info)
}

fn opt_idx(v: i64) -> Option<u32> {
    if v < 0 {
        None
    } else {
        Some(v as u32)
    }
}

fn clamp_line(line: i64) -> u32 {
    line.clamp(0, u32::MAX as i64) as u32
}

/// Anexa um evento na posição corrente. Se a entrada corrente já está no
/// mesmo endereço, o evento entra nela; senão a entrada corrente é fechada
/// (vai para `entries`, nada se perde — Lei 2) e uma nova é aberta com o
/// evento. Sem pânico em nenhum caminho (Lei 1).
fn push_event(
    current: &mut Option<DebugEntry>,
    entries: &mut Vec<DebugEntry>,
    addr: u32,
    line: i64,
    ev: DebugEvent,
) {
    if matches!(&*current, Some(e) if e.addr == addr) {
        if let Some(e) = current {
            e.events.push(ev);
        }
        return;
    }
    if let Some(e) = current.take() {
        entries.push(e);
    }
    *current = Some(DebugEntry {
        addr,
        line: clamp_line(line),
        events: vec![ev],
    });
}

/// Opcode especial define uma posição (addr, line). Se já existe entrada
/// corrente no mesmo endereço, só a linha é atualizada (os eventos locais
/// daquele endereço permanecem nela); senão fecha a anterior e abre nova
/// sem eventos.
fn set_position(
    current: &mut Option<DebugEntry>,
    entries: &mut Vec<DebugEntry>,
    addr: u32,
    line: i64,
) {
    if matches!(&*current, Some(e) if e.addr == addr) {
        if let Some(e) = current {
            e.line = clamp_line(line);
        }
        return;
    }
    if let Some(e) = current.take() {
        entries.push(e);
    }
    *current = Some(DebugEntry {
        addr,
        line: clamp_line(line),
        events: Vec::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uleb(v: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut v = v;
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }

    #[test]
    fn special_opcode_arithmetic() {
        // line_start=5, params=0; op 0x0a → addr+0, line+(-4+0)=1
        let mut d = Vec::new();
        d.extend(uleb(5));
        d.extend(uleb(0));
        d.push(0x0a);
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        assert_eq!(di.entries.len(), 1);
        assert_eq!(di.entries[0].addr, 0);
        assert_eq!(di.entries[0].line, 1);
    }

    #[test]
    fn special_opcode_advances_addr_and_line() {
        // adjusted = 0x0a - 0x0a = 0 → addr+0, line-4
        // adjusted = 0x1e - 0x0a = 20 → addr+1 (20/15), line -4+5=+1
        let mut d = Vec::new();
        d.extend(uleb(0));
        d.extend(uleb(0));
        d.push(0x1e);
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        assert_eq!(di.entries[0].addr, 1);
        assert_eq!(di.entries[0].line, 1);
    }

    #[test]
    fn locals_and_prologue_state_machine() {
        let mut d = Vec::new();
        d.extend(uleb(10)); // line_start
        d.extend(uleb(1)); // 1 parâmetro
        d.extend(uleb(3)); // nome idx 2 (uleb128p1)
        d.push(DBG_SET_PROLOGUE_END);
        // START_LOCAL reg=1 name=0x3 type=0x4
        d.push(DBG_START_LOCAL);
        d.extend(uleb(1));
        d.extend(uleb(4));
        d.extend(uleb(5));
        // special: avança
        d.push(0x0a);
        // END_LOCAL reg=1
        d.push(DBG_END_LOCAL);
        d.extend(uleb(1));
        d.push(DBG_END_SEQUENCE);

        let di = parse_debug_info(&d, 0).expect("debug");
        assert_eq!(di.parameter_names, vec![Some(2)]);
        let events: Vec<&DebugEvent> = di.entries.iter().flat_map(|e| e.events.iter()).collect();
        assert!(events.iter().any(|e| **e == DebugEvent::SetPrologueEnd));
        assert!(events.iter().any(|e| matches!(
            e,
            DebugEvent::Local(LocalEvent::Start {
                reg: 1,
                name: Some(3),
                typ: Some(4),
                sig: None
            })
        )));
        assert!(events
            .iter()
            .any(|e| matches!(e, DebugEvent::Local(LocalEvent::End { reg: 1 }))));
    }

    #[test]
    fn start_local_extended_with_signature() {
        let mut d = Vec::new();
        d.extend(uleb(1));
        d.extend(uleb(0));
        d.push(DBG_START_LOCAL_EXTENDED);
        d.extend(uleb(2)); // reg
        d.extend(uleb(1)); // name → idx 0
        d.extend(uleb(1)); // type → idx 0
        d.extend(uleb(1)); // sig → idx 0
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        assert!(matches!(
            di.entries[0].events[0],
            DebugEvent::Local(LocalEvent::Start {
                reg: 2,
                name: Some(0),
                typ: Some(0),
                sig: Some(0)
            })
        ));
    }

    #[test]
    fn set_file_and_restart() {
        let mut d = Vec::new();
        d.extend(uleb(1));
        d.extend(uleb(0));
        d.push(DBG_SET_FILE);
        d.extend(uleb(4)); // idx 3
        d.push(DBG_RESTART_LOCAL);
        d.extend(uleb(7));
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        assert_eq!(di.last_file(), Some(3));
        assert!(matches!(
            di.entries[0].events[1],
            DebugEvent::Local(LocalEvent::Restart { reg: 7 })
        ));
    }

    #[test]
    fn advance_pc_and_line() {
        let mut d = Vec::new();
        d.extend(uleb(1));
        d.extend(uleb(0));
        d.push(DBG_ADVANCE_PC);
        d.extend(uleb(9));
        d.push(DBG_ADVANCE_LINE);
        d.extend(uleb(0x7e)); // sleb128 → -2
        d.push(0x0a); // special
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        assert_eq!(di.entries[0].addr, 9);
        // line 1 + (-2) + (-4) = -5 → clampado a 0? Não: line_start=1; advance -2 → -1; special -4 → -5
        // clamp 0..u32::MAX → 0
        assert_eq!(di.entries[0].line, 0);
    }

    #[test]
    fn unterminated_sequence_hits_guard() {
        let mut d = Vec::new();
        d.extend(uleb(1));
        d.extend(uleb(0));
        // muitos ADVANCE_PC sem END_SEQUENCE — consome todos os bytes
        for _ in 0..64 {
            d.push(DBG_ADVANCE_PC);
            d.push(0x00);
        }
        // sem END_SEQUENCE → guarda de loop deve disparar
        assert!(parse_debug_info(&d, 0).is_err());
    }

    #[test]
    fn line_map_skips_local_only_entries() {
        let mut d = Vec::new();
        d.extend(uleb(7));
        d.extend(uleb(0));
        d.push(DBG_START_LOCAL);
        d.extend(uleb(1));
        d.extend(uleb(0));
        d.extend(uleb(0));
        d.push(0x0a); // entry @0 line 3
        d.push(DBG_END_SEQUENCE);
        let di = parse_debug_info(&d, 0).expect("debug");
        // a única entry tem eventos locais → line_map vazia
        assert!(di.line_map().is_empty());
        assert_eq!(di.locals_at().len(), 1);
    }
}
