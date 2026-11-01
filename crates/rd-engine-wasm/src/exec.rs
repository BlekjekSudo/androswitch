//! Interpreter for validated modules.
//!
//! Iterative (no host recursion), so call depth is a plain counter. Every
//! executed instruction costs one unit of fuel. Operand types are guaranteed by
//! validation; the `internal` paths below exist only to keep the interpreter
//! panic-free if that guarantee is ever broken, and report a `TRAP`.

use crate::decode::{Instr, ModuleDef, Target};
use crate::limits::{
    MAX_CALL_DEPTH, MAX_MEMORY_PAGES, MAX_TOTAL_LOCALS, MAX_VALUE_STACK, PAGE_SIZE,
};
use crate::{ValType, Value, WasmError};

type Res<T> = Result<T, WasmError>;

#[derive(Debug)]
pub(crate) struct State {
    pub memory: Vec<u8>,
    /// Upper bound for `memory.grow`, in pages.
    pub max_pages: u32,
    pub globals: Vec<Value>,
}

#[derive(Clone, Copy)]
struct Frame {
    func: usize,
    pc: usize,
    /// Index of this frame's first local in the shared locals vector.
    lbase: usize,
    /// Operand-stack height at frame entry (after arguments were moved out).
    sbase: usize,
}

fn trap(cause: impl Into<String>) -> WasmError {
    WasmError::new("TRAP", cause)
}

fn internal() -> WasmError {
    WasmError::new("TRAP", "internal: operand types disagree after validation")
}

fn push(stack: &mut Vec<Value>, v: Value) -> Res<()> {
    if stack.len() >= MAX_VALUE_STACK {
        return Err(WasmError::new(
            "STACK_EXHAUSTED",
            "operand stack limit reached",
        ));
    }
    stack.push(v);
    Ok(())
}

fn pop(stack: &mut Vec<Value>) -> Res<Value> {
    stack.pop().ok_or_else(internal)
}

fn pop_i32(stack: &mut Vec<Value>) -> Res<i32> {
    match pop(stack)? {
        Value::I32(v) => Ok(v),
        _ => Err(internal()),
    }
}

fn pop_i64(stack: &mut Vec<Value>) -> Res<i64> {
    match pop(stack)? {
        Value::I64(v) => Ok(v),
        _ => Err(internal()),
    }
}

/// Moves the target's carried values down to its label height.
fn branch(stack: &mut Vec<Value>, sbase: usize, t: Target) -> Res<()> {
    let n = t.arity as usize;
    let from = stack.len().checked_sub(n).ok_or_else(internal)?;
    let carried = stack.split_off(from);
    stack.truncate(sbase + t.height as usize);
    stack.extend(carried);
    Ok(())
}

fn i32_bin(op: u8, a: i32, b: i32) -> Res<i32> {
    let (ua, ub) = (a as u32, b as u32);
    Ok(match op {
        0x6a => a.wrapping_add(b),
        0x6b => a.wrapping_sub(b),
        0x6c => a.wrapping_mul(b),
        0x6d => {
            if b == 0 {
                return Err(trap("integer divide by zero"));
            }
            if a == i32::MIN && b == -1 {
                return Err(trap("integer overflow"));
            }
            a / b
        }
        0x6e => {
            if ub == 0 {
                return Err(trap("integer divide by zero"));
            }
            (ua / ub) as i32
        }
        0x6f => {
            if b == 0 {
                return Err(trap("integer divide by zero"));
            }
            a.wrapping_rem(b)
        }
        0x70 => {
            if ub == 0 {
                return Err(trap("integer divide by zero"));
            }
            (ua % ub) as i32
        }
        0x71 => a & b,
        0x72 => a | b,
        0x73 => a ^ b,
        0x74 => a.wrapping_shl(ub),
        0x75 => a.wrapping_shr(ub),
        0x76 => ua.wrapping_shr(ub) as i32,
        0x77 => ua.rotate_left(ub & 31) as i32,
        0x78 => ua.rotate_right(ub & 31) as i32,
        _ => return Err(internal()),
    })
}

fn i64_bin(op: u8, a: i64, b: i64) -> Res<i64> {
    let (ua, ub) = (a as u64, b as u64);
    Ok(match op {
        0x7c => a.wrapping_add(b),
        0x7d => a.wrapping_sub(b),
        0x7e => a.wrapping_mul(b),
        0x7f => {
            if b == 0 {
                return Err(trap("integer divide by zero"));
            }
            if a == i64::MIN && b == -1 {
                return Err(trap("integer overflow"));
            }
            a / b
        }
        0x80 => {
            if ub == 0 {
                return Err(trap("integer divide by zero"));
            }
            (ua / ub) as i64
        }
        0x81 => {
            if b == 0 {
                return Err(trap("integer divide by zero"));
            }
            a.wrapping_rem(b)
        }
        0x82 => {
            if ub == 0 {
                return Err(trap("integer divide by zero"));
            }
            (ua % ub) as i64
        }
        0x83 => a & b,
        0x84 => a | b,
        0x85 => a ^ b,
        0x86 => a.wrapping_shl(ub as u32),
        0x87 => a.wrapping_shr(ub as u32),
        0x88 => ua.wrapping_shr(ub as u32) as i64,
        0x89 => ua.rotate_left((ub & 63) as u32) as i64,
        0x8a => ua.rotate_right((ub & 63) as u32) as i64,
        _ => return Err(internal()),
    })
}

fn num(op: u8, stack: &mut Vec<Value>) -> Res<()> {
    let out = match op {
        0x45 => Value::I32((pop_i32(stack)? == 0) as i32),
        0x46..=0x4f => {
            let b = pop_i32(stack)?;
            let a = pop_i32(stack)?;
            let (ua, ub) = (a as u32, b as u32);
            let r = match op {
                0x46 => a == b,
                0x47 => a != b,
                0x48 => a < b,
                0x49 => ua < ub,
                0x4a => a > b,
                0x4b => ua > ub,
                0x4c => a <= b,
                0x4d => ua <= ub,
                0x4e => a >= b,
                _ => ua >= ub,
            };
            Value::I32(r as i32)
        }
        0x50 => Value::I32((pop_i64(stack)? == 0) as i32),
        0x51..=0x5a => {
            let b = pop_i64(stack)?;
            let a = pop_i64(stack)?;
            let (ua, ub) = (a as u64, b as u64);
            let r = match op {
                0x51 => a == b,
                0x52 => a != b,
                0x53 => a < b,
                0x54 => ua < ub,
                0x55 => a > b,
                0x56 => ua > ub,
                0x57 => a <= b,
                0x58 => ua <= ub,
                0x59 => a >= b,
                _ => ua >= ub,
            };
            Value::I32(r as i32)
        }
        0x67..=0x69 => {
            let a = pop_i32(stack)? as u32;
            let r = match op {
                0x67 => a.leading_zeros(),
                0x68 => a.trailing_zeros(),
                _ => a.count_ones(),
            };
            Value::I32(r as i32)
        }
        0x6a..=0x78 => {
            let b = pop_i32(stack)?;
            let a = pop_i32(stack)?;
            Value::I32(i32_bin(op, a, b)?)
        }
        0x79..=0x7b => {
            let a = pop_i64(stack)? as u64;
            let r = match op {
                0x79 => a.leading_zeros(),
                0x7a => a.trailing_zeros(),
                _ => a.count_ones(),
            };
            Value::I64(i64::from(r))
        }
        0x7c..=0x8a => {
            let b = pop_i64(stack)?;
            let a = pop_i64(stack)?;
            Value::I64(i64_bin(op, a, b)?)
        }
        0xa7 => Value::I32(pop_i64(stack)? as i32),
        0xac => Value::I64(i64::from(pop_i32(stack)?)),
        0xad => Value::I64(i64::from(pop_i32(stack)? as u32)),
        0xc0 => Value::I32(i32::from(pop_i32(stack)? as i8)),
        0xc1 => Value::I32(i32::from(pop_i32(stack)? as i16)),
        0xc2 => Value::I64(i64::from(pop_i64(stack)? as i8)),
        0xc3 => Value::I64(i64::from(pop_i64(stack)? as i16)),
        0xc4 => Value::I64(i64::from(pop_i64(stack)? as i32)),
        _ => return Err(internal()),
    };
    push(stack, out)
}

/// (size in bytes, result type, sign-extending load) for a memory opcode.
fn mem_shape(op: u8) -> Res<(usize, ValType, bool)> {
    Ok(match op {
        0x28 => (4, ValType::I32, false),
        0x29 => (8, ValType::I64, false),
        0x2c => (1, ValType::I32, true),
        0x2d => (1, ValType::I32, false),
        0x2e => (2, ValType::I32, true),
        0x2f => (2, ValType::I32, false),
        0x30 => (1, ValType::I64, true),
        0x31 => (1, ValType::I64, false),
        0x32 => (2, ValType::I64, true),
        0x33 => (2, ValType::I64, false),
        0x34 => (4, ValType::I64, true),
        0x35 => (4, ValType::I64, false),
        0x36 => (4, ValType::I32, false),
        0x37 => (8, ValType::I64, false),
        0x3a => (1, ValType::I32, false),
        0x3b => (2, ValType::I32, false),
        0x3c => (1, ValType::I64, false),
        0x3d => (2, ValType::I64, false),
        0x3e => (4, ValType::I64, false),
        _ => return Err(internal()),
    })
}

fn effective(stack: &mut Vec<Value>, offset: u32, size: usize, mem_len: usize) -> Res<usize> {
    let base = pop_i32(stack)? as u32 as u64;
    let ea = base + u64::from(offset);
    if ea + size as u64 > mem_len as u64 {
        return Err(trap("out of bounds memory access"));
    }
    Ok(ea as usize)
}

/// Runs `func` with `args`. Returns the values it leaves on the stack.
pub(crate) fn invoke(
    module: &ModuleDef,
    st: &mut State,
    func: u32,
    args: &[Value],
    fuel: &mut u64,
) -> Res<Vec<Value>> {
    let mut stack: Vec<Value> = Vec::new();
    let mut locals: Vec<Value> = Vec::new();
    let mut callers: Vec<Frame> = Vec::new();

    let entry = module
        .funcs
        .get(func as usize)
        .ok_or_else(|| trap("entry function out of range"))?;
    for (i, t) in entry.locals.iter().enumerate() {
        locals.push(match args.get(i) {
            Some(v) if i < entry.nparams => *v,
            _ => Value::zero(*t),
        });
    }
    let mut cur = Frame {
        func: func as usize,
        pc: 0,
        lbase: 0,
        sbase: 0,
    };

    loop {
        let ins = &module.funcs[cur.func].code[cur.pc];
        cur.pc += 1;
        if *fuel == 0 {
            return Err(WasmError::new(
                "FUEL_EXHAUSTED",
                "fuel budget exhausted before the function returned",
            )
            .with_suggestion(
                "raise the fuel of this call, up to the 10M limit, or reduce the work",
            ));
        }
        *fuel -= 1;

        match ins {
            Instr::Unreachable => return Err(trap("unreachable executed")),
            Instr::Br(t) => {
                branch(&mut stack, cur.sbase, *t)?;
                cur.pc = t.pc;
            }
            Instr::BrIf(t) => {
                if pop_i32(&mut stack)? != 0 {
                    branch(&mut stack, cur.sbase, *t)?;
                    cur.pc = t.pc;
                }
            }
            Instr::BrTable(ts) => {
                let i = pop_i32(&mut stack)? as u32 as usize;
                let last = ts.len() - 1;
                let t = if i < last { ts[i] } else { ts[last] };
                branch(&mut stack, cur.sbase, t)?;
                cur.pc = t.pc;
            }
            Instr::Jump(p) => cur.pc = *p,
            Instr::If(p) => {
                if pop_i32(&mut stack)? == 0 {
                    cur.pc = *p;
                }
            }
            Instr::Return => {
                let n = module.funcs[cur.func].nresults;
                let from = stack.len().checked_sub(n).ok_or_else(internal)?;
                let carried = stack.split_off(from);
                stack.truncate(cur.sbase);
                stack.extend(carried);
                locals.truncate(cur.lbase);
                match callers.pop() {
                    Some(f) => cur = f,
                    None => return Ok(stack),
                }
            }
            Instr::Call(f) => {
                if callers.len() >= MAX_CALL_DEPTH {
                    return Err(WasmError::new(
                        "STACK_EXHAUSTED",
                        "call depth limit reached",
                    ));
                }
                let callee = &module.funcs[*f as usize];
                if locals.len() + callee.locals.len() > MAX_TOTAL_LOCALS {
                    return Err(WasmError::new(
                        "STACK_EXHAUSTED",
                        "locals budget of the call stack reached",
                    ));
                }
                let from = stack
                    .len()
                    .checked_sub(callee.nparams)
                    .ok_or_else(internal)?;
                let call_args = stack.split_off(from);
                let lbase = locals.len();
                locals.extend(call_args);
                for t in &callee.locals[callee.nparams..] {
                    locals.push(Value::zero(*t));
                }
                callers.push(cur);
                cur = Frame {
                    func: *f as usize,
                    pc: 0,
                    lbase,
                    sbase: stack.len(),
                };
            }
            Instr::Drop => {
                pop(&mut stack)?;
            }
            Instr::Select => {
                let c = pop_i32(&mut stack)?;
                let v2 = pop(&mut stack)?;
                let v1 = pop(&mut stack)?;
                push(&mut stack, if c != 0 { v1 } else { v2 })?;
            }
            Instr::LocalGet(i) => {
                let v = *locals.get(cur.lbase + *i as usize).ok_or_else(internal)?;
                push(&mut stack, v)?;
            }
            Instr::LocalSet(i) => {
                let v = pop(&mut stack)?;
                *locals
                    .get_mut(cur.lbase + *i as usize)
                    .ok_or_else(internal)? = v;
            }
            Instr::LocalTee(i) => {
                let v = *stack.last().ok_or_else(internal)?;
                *locals
                    .get_mut(cur.lbase + *i as usize)
                    .ok_or_else(internal)? = v;
            }
            Instr::GlobalGet(i) => {
                let v = *st.globals.get(*i as usize).ok_or_else(internal)?;
                push(&mut stack, v)?;
            }
            Instr::GlobalSet(i) => {
                let v = pop(&mut stack)?;
                *st.globals.get_mut(*i as usize).ok_or_else(internal)? = v;
            }
            Instr::Load { op, offset } => {
                let (size, ty, signed) = mem_shape(*op)?;
                let ea = effective(&mut stack, *offset, size, st.memory.len())?;
                let mut raw: u64 = 0;
                for k in 0..size {
                    raw |= u64::from(st.memory[ea + k]) << (8 * k);
                }
                let shift = 64 - 8 * size as u32;
                let v = if signed {
                    ((raw << shift) as i64) >> shift
                } else {
                    raw as i64
                };
                let v = match ty {
                    ValType::I32 => Value::I32(v as i32),
                    ValType::I64 => Value::I64(v),
                };
                push(&mut stack, v)?;
            }
            Instr::Store { op, offset } => {
                let (size, _, _) = mem_shape(*op)?;
                let v = pop(&mut stack)?;
                let raw: u64 = match v {
                    Value::I32(x) => x as u32 as u64,
                    Value::I64(x) => x as u64,
                };
                let ea = effective(&mut stack, *offset, size, st.memory.len())?;
                for k in 0..size {
                    st.memory[ea + k] = (raw >> (8 * k)) as u8;
                }
            }
            Instr::MemorySize => {
                let pages = st.memory.len() / PAGE_SIZE;
                push(&mut stack, Value::I32(pages as i32))?;
            }
            Instr::MemoryGrow => {
                let delta = pop_i32(&mut stack)? as u32;
                let old = st.memory.len() / PAGE_SIZE;
                let new = old as u64 + u64::from(delta);
                if new > u64::from(st.max_pages.min(MAX_MEMORY_PAGES)) {
                    push(&mut stack, Value::I32(-1))?;
                } else {
                    st.memory.resize(new as usize * PAGE_SIZE, 0);
                    push(&mut stack, Value::I32(old as i32))?;
                }
            }
            Instr::I32Const(v) => push(&mut stack, Value::I32(*v))?,
            Instr::I64Const(v) => push(&mut stack, Value::I64(*v))?,
            Instr::Num(op) => num(*op, &mut stack)?,
        }
    }
}
