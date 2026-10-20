//! Opt-in hot integer leaf JIT. Unsupported methods stay in the interpreter.
use crate::Value;
use rd_dex::code::{CodeItem, Kind};
use rd_jni::jit::{Binary, Leaf, LeafJit, Op, Unary};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct JitStats {
    pub enabled: bool,
    pub compiled: usize,
    pub native_calls: u64,
    pub unsupported: usize,
    pub compile_errors: usize,
    pub last_error: Option<String>,
}
enum Entry {
    Warming(u8),
    Unsupported,
    Compiled(usize),
}
#[derive(Default)]
pub(crate) struct JitState {
    compiler: Option<LeafJit>,
    cache: HashMap<u64, Entry>,
    pub stats: JitStats,
}
impl JitState {
    pub fn enable(&mut self) -> Result<(), String> {
        if self.compiler.is_none() {
            self.compiler = Some(LeafJit::new()?);
        }
        self.stats.enabled = true;
        Ok(())
    }
    pub fn run(
        &mut self,
        key: u64,
        code: &CodeItem,
        proto: &str,
        args: &[Value],
        fuel_remaining: u64,
    ) -> Option<i32> {
        if !self.stats.enabled || code.instructions.len() as u64 > fuel_remaining {
            return None;
        }
        if self.cache.len() >= 8192 && !self.cache.contains_key(&key) {
            return None;
        }
        let entry = self.cache.entry(key).or_insert(Entry::Warming(0));
        match entry {
            Entry::Unsupported => return None,
            Entry::Warming(n) => {
                *n += 1;
                if *n < 32 {
                    return None;
                }
                let Some(leaf) = lower(code, proto) else {
                    *entry = Entry::Unsupported;
                    self.stats.unsupported += 1;
                    return None;
                };
                let h = match self.compiler.as_mut()?.compile(&leaf) {
                    Ok(h) => h,
                    Err(e) => {
                        self.stats.compile_errors += 1;
                        self.stats.last_error = Some(e);
                        *entry = Entry::Unsupported;
                        return None;
                    }
                };
                *entry = Entry::Compiled(h);
                self.stats.compiled += 1;
            }
            Entry::Compiled(_) => {}
        }
        let Entry::Compiled(h) = self.cache.get(&key)? else {
            return None;
        };
        let a: Option<Vec<i32>> = args
            .iter()
            .map(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
            .collect();
        match self.compiler.as_ref()?.execute(*h, &a?) {
            Ok(v) => {
                self.stats.native_calls += 1;
                Some(v)
            }
            Err(e) => {
                self.stats.last_error = Some(e);
                None
            }
        }
    }
}
fn binary(op: u8) -> Option<Binary> {
    Some(match op {
        0x90 => Binary::Add,
        0x91 => Binary::Sub,
        0x92 => Binary::Mul,
        0x95 => Binary::And,
        0x96 => Binary::Or,
        0x97 => Binary::Xor,
        0x98 => Binary::Shl,
        0x99 => Binary::Shr,
        0x9a => Binary::Ushr,
        _ => return None,
    })
}
fn lower(code: &CodeItem, proto: &str) -> Option<Leaf> {
    let (params, ret) = proto.strip_prefix('(')?.split_once(')')?;
    if !params.chars().all(|c| "ZBSCI".contains(c))
        || ret.len() != 1
        || !"ZBSCI".contains(ret)
        || !code.tries.is_empty()
        || code.instructions.len() > 128
    {
        return None;
    }
    if params.len() != code.ins_size as usize
        || code.registers_size == 0
        || code.registers_size > 256
    {
        return None;
    }
    let scratch = code.registers_size as usize;
    let mut leaf = Leaf {
        registers: scratch + 1,
        parameters: params.len(),
        ops: vec![],
        result: 0,
    };
    // Backend places args at the end, but Dalvik's last physical register is
    // scratch-1. Move args left before using the extra literal scratch slot.
    for i in 0..params.len() {
        leaf.ops.push(Op::Move {
            dst: scratch - params.len() + i,
            src: scratch + 1 - params.len() + i,
        });
    }
    let mut returned = false;
    for (_, insn) in &code.instructions {
        if returned {
            return None;
        }
        let valid = match &insn.kind {
            Kind::Reg(a)
            | Kind::RegLit4(a, _)
            | Kind::RegLit16(a, _)
            | Kind::RegLit32(a, _)
            | Kind::RegHigh16(a, _) => usize::from(*a) < scratch,
            Kind::Regs(a, b) => usize::from(*a) < scratch && usize::from(*b) < scratch,
            Kind::RegReg16(a, b) => usize::from(*a) < scratch && usize::from(*b) < scratch,
            Kind::RegRegReg(a, b, c) => [*a, *b, *c].iter().all(|r| usize::from(*r) < scratch),
            Kind::RegRegLit8(a, b, _) | Kind::RegRegLit16(a, b, _) => {
                usize::from(*a) < scratch && usize::from(*b) < scratch
            }
            Kind::Plain => true,
            _ => false,
        };
        if !valid {
            return None;
        }
        let op = insn.opcode;
        let operation = match (&insn.kind, op) {
            (Kind::Plain, 0x00) => continue,
            (Kind::Reg(r), 0x0f) => {
                leaf.result = *r as usize;
                returned = true;
                continue;
            }
            (Kind::Regs(a, b), 0x01) => Op::Move {
                dst: *a as usize,
                src: *b as usize,
            },
            (Kind::RegReg16(a, b), 0x02) => Op::Move {
                dst: *a as usize,
                src: *b as usize,
            },
            (Kind::RegLit4(a, v), 0x12) => Op::Const {
                dst: *a as usize,
                value: i32::from(*v),
            },
            (Kind::RegLit16(a, v), 0x13) => Op::Const {
                dst: *a as usize,
                value: i32::from(*v),
            },
            (Kind::RegLit32(a, v), 0x14) => Op::Const {
                dst: *a as usize,
                value: *v,
            },
            (Kind::RegHigh16(a, v), 0x15) => Op::Const {
                dst: *a as usize,
                value: i32::from(*v) << 16,
            },
            (Kind::Regs(a, b), 0x7b | 0x7c | 0x8d | 0x8e | 0x8f) => Op::Unary {
                dst: *a as usize,
                src: *b as usize,
                op: match op {
                    0x7b => Unary::Neg,
                    0x7c => Unary::Not,
                    0x8d => Unary::Byte,
                    0x8e => Unary::Char,
                    _ => Unary::Short,
                },
            },
            (Kind::RegRegReg(a, b, c), 0x90..=0x9a) => Op::Binary {
                dst: *a as usize,
                lhs: *b as usize,
                rhs: *c as usize,
                op: binary(op)?,
            },
            (Kind::Regs(a, b), 0xb0..=0xba) => Op::Binary {
                dst: *a as usize,
                lhs: *a as usize,
                rhs: *b as usize,
                op: binary(op - 0x20)?,
            },
            (Kind::RegRegLit8(a, b, v), 0xd8..=0xe2) => {
                leaf.ops.push(Op::Const {
                    dst: scratch,
                    value: i32::from(*v),
                });
                let base = match op {
                    0xd8 => 0x90,
                    0xd9 => 0x91,
                    0xda => 0x92,
                    0xdd => 0x95,
                    0xde => 0x96,
                    0xdf => 0x97,
                    0xe0 => 0x98,
                    0xe1 => 0x99,
                    0xe2 => 0x9a,
                    _ => return None,
                };
                let (lhs, rhs) = if op == 0xd9 {
                    (scratch, *b as usize)
                } else {
                    (*b as usize, scratch)
                };
                Op::Binary {
                    dst: *a as usize,
                    lhs,
                    rhs,
                    op: binary(base)?,
                }
            }
            (Kind::RegRegLit16(a, b, v), 0xd0..=0xd7) => {
                leaf.ops.push(Op::Const {
                    dst: scratch,
                    value: i32::from(*v),
                });
                let base = match op {
                    0xd0 => 0x90,
                    0xd1 => 0x91,
                    0xd2 => 0x92,
                    0xd5 => 0x95,
                    0xd6 => 0x96,
                    0xd7 => 0x97,
                    _ => return None,
                };
                let (lhs, rhs) = if op == 0xd1 {
                    (scratch, *b as usize)
                } else {
                    (*b as usize, scratch)
                };
                Op::Binary {
                    dst: *a as usize,
                    lhs,
                    rhs,
                    op: binary(base)?,
                }
            }
            _ => return None,
        };
        // Validate original operands against physical Dalvik registers; the
        // extra backend scratch must never make malformed bytecode valid.
        let valid = match operation {
            Op::Const { dst, .. } => dst < scratch,
            Op::Move { dst, src } | Op::Unary { dst, src, .. } => dst < scratch && src < scratch,
            Op::Binary { dst, lhs, rhs, .. } => dst < scratch && lhs <= scratch && rhs <= scratch,
        };
        if !valid {
            return None;
        }
        leaf.ops.push(operation);
    }
    if !returned || leaf.result >= scratch {
        return None;
    }
    Some(leaf)
}
