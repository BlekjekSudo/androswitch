//! Validated Cranelift integer & control-flow JIT compiler (ADR 007).
//!
//! Provides both the original straight-line `LeafJit` surface and the full
//! multi-block `MethodJit` supporting 32-bit/64-bit integer lanes, structured
//! and unstructured Dalvik control flow (`goto`, `if-*`, `packed-switch`,
//! `sparse-switch`), deterministic basic-block fuel accounting, and bounds- &
//! null-checked primitive array kernels via opaque table handles (never raw
//! guest pointers).

use std::collections::VecDeque;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types, AbiParam, Block, InstBuilder, JumpTableData, MemFlagsData, UserFuncName, Value,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// `MemFlags::trusted()` was removed in cranelift 0.137. Same meaning: notrap + aligned.
fn trusted_flags() -> MemFlagsData {
    MemFlagsData::new().with_notrap().with_aligned()
}

const MAX_REGISTERS: usize = 256;
const MAX_PARAMS: usize = 32;
const MAX_BLOCKS: usize = 256;
const MAX_TOTAL_OPS: usize = 4096;
const MAX_SWITCH_CASES: usize = 512;
const MAX_FUNCTIONS: usize = 4096;
const MAX_ARRAY_HANDLES: usize = 32;

const STATUS_OK: i64 = 0;
const STATUS_FUEL: i64 = 1;
const STATUS_BAILOUT: i64 = 2;

fn make_jit_module() -> Result<JITModule, String> {
    let builder = JITBuilder::with_flags(
        &[("opt_level", "speed")],
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| e.to_string())?;
    Ok(JITModule::new(builder))
}

// ── Original straight-line LeafJit surface (preserved 100% intact) ──────────

#[derive(Debug, Clone, Copy)]
pub enum Binary {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Ushr,
}

#[derive(Debug, Clone, Copy)]
pub enum Unary {
    Neg,
    Not,
    Byte,
    Short,
    Char,
}

#[derive(Debug, Clone)]
pub enum Op {
    Const {
        dst: usize,
        value: i32,
    },
    Move {
        dst: usize,
        src: usize,
    },
    Binary {
        dst: usize,
        lhs: usize,
        rhs: usize,
        op: Binary,
    },
    Unary {
        dst: usize,
        src: usize,
        op: Unary,
    },
}

#[derive(Debug, Clone)]
pub struct Leaf {
    pub registers: usize,
    pub parameters: usize,
    pub ops: Vec<Op>,
    pub result: usize,
}

impl Leaf {
    fn validate(&self) -> Result<(), String> {
        if self.registers == 0
            || self.registers > 512
            || self.parameters > self.registers
            || self.parameters > MAX_PARAMS
            || self.ops.len() > 128
            || self.result >= self.registers
        {
            return Err("JIT leaf exceeds register/argument/instruction limit".into());
        }
        let valid = |r: usize| r < self.registers;
        let mut initialized = vec![false; self.registers];
        initialized[self.registers - self.parameters..].fill(true);
        for op in &self.ops {
            let (dst, ok) = match *op {
                Op::Const { dst, .. } => (dst, valid(dst)),
                Op::Move { dst, src } | Op::Unary { dst, src, .. } => {
                    (dst, valid(dst) && valid(src) && initialized[src])
                }
                Op::Binary { dst, lhs, rhs, .. } => (
                    dst,
                    valid(dst) && valid(lhs) && valid(rhs) && initialized[lhs] && initialized[rhs],
                ),
            };
            if !ok {
                return Err("JIT invalid or uninitialized register".into());
            }
            initialized[dst] = true;
        }
        if !initialized[self.result] {
            return Err("JIT uninitialized result".into());
        }
        Ok(())
    }
}

/// Owns all code addresses; no address escapes this safe interface.
pub struct LeafJit {
    module: Option<JITModule>,
    functions: Vec<(usize, usize)>,
}

impl LeafJit {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            module: Some(make_jit_module()?),
            functions: Vec::new(),
        })
    }

    pub fn compile(&mut self, leaf: &Leaf) -> Result<usize, String> {
        leaf.validate()?;
        if self.functions.len() >= 256 {
            return Err("JIT code cache limit (256 leaves)".into());
        }
        let module = self.module.as_mut().ok_or("JIT module unavailable")?;
        let mut ctx = module.make_context();
        let ptr = module.target_config().pointer_type();
        ctx.func.signature.params.push(AbiParam::new(ptr));
        ctx.func.signature.returns.push(AbiParam::new(types::I32));
        let id = module
            .declare_function(
                &format!("rd_leaf_{}", self.functions.len()),
                Linkage::Local,
                &ctx.func.signature,
            )
            .map_err(|e| e.to_string())?;
        ctx.func.name = UserFuncName::user(0, id.as_u32());
        let mut fctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);
            let args = b.block_params(entry)[0];
            let zero = b.ins().iconst(types::I32, 0);
            let mut regs = vec![zero; leaf.registers];
            for i in 0..leaf.parameters {
                regs[leaf.registers - leaf.parameters + i] =
                    b.ins()
                        .load(types::I32, trusted_flags(), args, (i * 4) as i32);
            }
            for op in &leaf.ops {
                match *op {
                    Op::Const { dst, value } => {
                        regs[dst] = b.ins().iconst(types::I32, i64::from(value as u32));
                    }
                    Op::Move { dst, src } => regs[dst] = regs[src],
                    Op::Binary { dst, lhs, rhs, op } => {
                        let (x, mut y) = (regs[lhs], regs[rhs]);
                        if matches!(op, Binary::Shl | Binary::Shr | Binary::Ushr) {
                            y = b.ins().band_imm_s(y, 31);
                        }
                        regs[dst] = match op {
                            Binary::Add => b.ins().iadd(x, y),
                            Binary::Sub => b.ins().isub(x, y),
                            Binary::Mul => b.ins().imul(x, y),
                            Binary::And => b.ins().band(x, y),
                            Binary::Or => b.ins().bor(x, y),
                            Binary::Xor => b.ins().bxor(x, y),
                            Binary::Shl => b.ins().ishl(x, y),
                            Binary::Shr => b.ins().sshr(x, y),
                            Binary::Ushr => b.ins().ushr(x, y),
                        };
                    }
                    Op::Unary { dst, src, op } => {
                        let x = regs[src];
                        regs[dst] = match op {
                            Unary::Neg => b.ins().ineg(x),
                            Unary::Not => b.ins().bnot(x),
                            Unary::Byte => {
                                let v = b.ins().ireduce(types::I8, x);
                                b.ins().sextend(types::I32, v)
                            }
                            Unary::Short => {
                                let v = b.ins().ireduce(types::I16, x);
                                b.ins().sextend(types::I32, v)
                            }
                            Unary::Char => b.ins().band_imm_s(x, 65535),
                        };
                    }
                }
            }
            b.ins().return_(&[regs[leaf.result]]);
            b.finalize(module.target_config());
        }
        module
            .define_function(id, &mut ctx)
            .map_err(|e| format!("{e:?}"))?;
        module.clear_context(&mut ctx);
        module.finalize_definitions().map_err(|e| e.to_string())?;
        let address = module.get_finalized_function(id) as usize;
        let handle = self.functions.len();
        self.functions.push((address, leaf.parameters));
        Ok(handle)
    }

    /// Execute only a function generated and owned by this module.
    #[allow(unsafe_code)] // ADR 007: checked ABI, arity and code lifetime.
    pub fn execute(&self, handle: usize, args: &[i32]) -> Result<i32, String> {
        let (address, arity) = *self.functions.get(handle).ok_or("invalid JIT handle")?;
        if args.len() != arity {
            return Err("JIT argument count mismatch".into());
        }
        // SAFETY: compile emits precisely extern-C(pointer-to-i32)->i32;
        // loads are bounded by arity; the slice is live for the call; module
        // owns executable code and cannot drop while borrowed by this method.
        let f: extern "C" fn(*const i32) -> i32 = unsafe { std::mem::transmute(address) };
        Ok(f(args.as_ptr()))
    }
}

impl Drop for LeafJit {
    #[allow(unsafe_code)] // ADR 007: no code pointers escape the owning module.
    fn drop(&mut self) {
        if let Some(module) = self.module.take() {
            // SAFETY: no references to JIT functions are exposed; dropping
            // requires exclusive ownership, so no execute call can be active.
            unsafe {
                module.free_memory();
            }
        }
    }
}

// ── Complete RustyDroid Multi-Block Integer/Long/Switch/Array JIT ───────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Int32,
    Int64,
    Ref,
    ArrI32,
    ArrI64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetKind {
    Void,
    Int32,
    Int64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp32 {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Ushr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp64 {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftOp64 {
    Shl,
    Shr,
    Ushr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp32 {
    Neg,
    Not,
    Byte,
    Short,
    Char,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp64 {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntCond {
    Eq,
    Ne,
    Lt,
    Ge,
    Gt,
    Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrElem32 {
    Int,
    Bool,
    Byte,
    Char,
    Short,
}

#[derive(Debug, Clone)]
pub enum JitInst {
    Const32 {
        dst: usize,
        value: i32,
    },
    Const64 {
        dst: usize,
        value: i64,
    },
    Move32 {
        dst: usize,
        src: usize,
    },
    Move64 {
        dst: usize,
        src: usize,
    },
    MoveRef {
        dst: usize,
        src: usize,
    },
    Unary32 {
        dst: usize,
        src: usize,
        op: UnOp32,
    },
    Unary64 {
        dst: usize,
        src: usize,
        op: UnOp64,
    },
    Int32ToInt64 {
        dst: usize,
        src: usize,
    },
    Int64ToInt32 {
        dst: usize,
        src: usize,
    },
    Binary32 {
        dst: usize,
        lhs: usize,
        rhs: usize,
        op: BinOp32,
    },
    Binary32Imm {
        dst: usize,
        src: usize,
        imm: i32,
        op: BinOp32,
        reverse_sub: bool,
    },
    DivRemLit32 {
        dst: usize,
        src: usize,
        lit: i32,
        is_rem: bool,
    },
    Binary64 {
        dst: usize,
        lhs: usize,
        rhs: usize,
        op: BinOp64,
    },
    Shift64 {
        dst: usize,
        lhs: usize,
        amt: usize,
        op: ShiftOp64,
    },
    CmpLong {
        dst: usize,
        lhs: usize,
        rhs: usize,
    },
    ArrayLen {
        dst: usize,
        arr: usize,
        is_i64: bool,
    },
    ArrayGet32 {
        dst: usize,
        arr: usize,
        idx: usize,
    },
    ArrayGet64 {
        dst: usize,
        arr: usize,
        idx: usize,
    },
    ArrayPut32 {
        src: usize,
        arr: usize,
        idx: usize,
        elem: ArrElem32,
    },
    ArrayPut64 {
        src: usize,
        arr: usize,
        idx: usize,
    },
}

#[derive(Debug, Clone)]
pub enum JitTerm {
    Jump {
        target: usize,
    },
    BranchEq {
        eq: bool,
        lhs: usize,
        rhs: usize,
        then_bb: usize,
        else_bb: usize,
    },
    BranchCmp32 {
        cond: IntCond,
        lhs: usize,
        rhs: usize,
        then_bb: usize,
        else_bb: usize,
    },
    BranchZero {
        eq: bool,
        src: usize,
        then_bb: usize,
        else_bb: usize,
    },
    BranchCmpZero32 {
        cond: IntCond,
        src: usize,
        then_bb: usize,
        else_bb: usize,
    },
    PackedSwitch {
        src: usize,
        first_key: i32,
        targets: Vec<usize>,
        default_bb: usize,
    },
    SparseSwitch {
        src: usize,
        cases: Vec<(i32, usize)>,
        default_bb: usize,
    },
    ReturnVoid,
    Return32 {
        src: usize,
    },
    Return64 {
        src: usize,
    },
}

#[derive(Debug, Clone)]
pub struct JitBlock {
    pub dalvik_insns: u32,
    pub insts: Vec<JitInst>,
    pub term: JitTerm,
}

#[derive(Debug, Clone)]
pub struct JitProgram {
    pub registers: usize,
    pub params: Vec<(usize, ParamKind)>,
    pub ret: RetKind,
    pub blocks: Vec<JitBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitExecResult {
    Returned { bits: u64, fuel_consumed: u64 },
    FuelExhausted,
    Bailout,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Uninit,
    Zero,
    Int32,
    LongLo,
    LongHi,
    Ref,
    ArrI32,
    ArrI64,
}

impl Slot {
    fn is_i32(self) -> bool {
        matches!(self, Slot::Zero | Slot::Int32)
    }

    fn is_ref_like(self) -> bool {
        matches!(self, Slot::Zero | Slot::Ref | Slot::ArrI32 | Slot::ArrI64)
    }

    fn is_single_word_eq_compatible(self, other: Self) -> bool {
        // Matches interp.rs `values_ref_eq`: ints compare with ints, refs with
        // refs, and Zero (`const/4 0`) bridges both int 0 and null reference.
        (self.is_i32() && other.is_i32()) || (self.is_ref_like() && other.is_ref_like())
    }

    fn merge(self, other: Self) -> Self {
        if self == other {
            return self;
        }
        match (self, other) {
            (Slot::Zero, Slot::Int32) | (Slot::Int32, Slot::Zero) => Slot::Int32,
            (Slot::Zero, Slot::ArrI32) | (Slot::ArrI32, Slot::Zero) => Slot::ArrI32,
            (Slot::Zero, Slot::ArrI64) | (Slot::ArrI64, Slot::Zero) => Slot::ArrI64,
            (Slot::Zero, Slot::Ref) | (Slot::Ref, Slot::Zero) => Slot::Ref,
            (Slot::Ref | Slot::ArrI32 | Slot::ArrI64, Slot::Ref | Slot::ArrI32 | Slot::ArrI64) => {
                Slot::Ref
            }
            _ => Slot::Uninit,
        }
    }
}

fn write_single(state: &mut [Slot], dst: usize, kind: Slot) -> Result<(), String> {
    if dst >= state.len() {
        return Err("JIT register out of range".into());
    }
    if dst > 0 && state[dst - 1] == Slot::LongLo {
        state[dst - 1] = Slot::Uninit;
    }
    if state[dst] == Slot::LongLo && dst + 1 < state.len() && state[dst + 1] == Slot::LongHi {
        state[dst + 1] = Slot::Uninit;
    }
    state[dst] = kind;
    Ok(())
}

fn write_wide(state: &mut [Slot], dst: usize) -> Result<(), String> {
    if dst + 1 >= state.len() {
        return Err("JIT wide register pair out of range".into());
    }
    if dst > 0 && state[dst - 1] == Slot::LongLo {
        state[dst - 1] = Slot::Uninit;
    }
    if state[dst + 1] == Slot::LongLo && dst + 2 < state.len() && state[dst + 2] == Slot::LongHi {
        state[dst + 2] = Slot::Uninit;
    }
    state[dst] = Slot::LongLo;
    state[dst + 1] = Slot::LongHi;
    Ok(())
}

fn check_wide(state: &[Slot], src: usize) -> bool {
    src + 1 < state.len() && state[src] == Slot::LongLo && state[src + 1] == Slot::LongHi
}

fn check_i32(state: &[Slot], src: usize) -> bool {
    src < state.len() && state[src].is_i32()
}

fn check_arr_i32(state: &[Slot], src: usize) -> bool {
    src < state.len() && matches!(state[src], Slot::Zero | Slot::ArrI32)
}

fn check_arr_i64(state: &[Slot], src: usize) -> bool {
    src < state.len() && matches!(state[src], Slot::Zero | Slot::ArrI64)
}

impl JitProgram {
    pub fn validate(&self) -> Result<(), String> {
        if self.registers == 0
            || self.registers > MAX_REGISTERS
            || self.params.len() > MAX_PARAMS
            || self.blocks.is_empty()
            || self.blocks.len() > MAX_BLOCKS
        {
            return Err("JIT program exceeds structural limits".into());
        }
        let mut total_ops = 0usize;
        for blk in &self.blocks {
            if blk.dalvik_insns == 0 {
                return Err("JIT block must account for at least 1 Dalvik instruction".into());
            }
            total_ops = total_ops.saturating_add(blk.insts.len()).saturating_add(1);
            if total_ops > MAX_TOTAL_OPS {
                return Err("JIT program exceeds total instruction limit".into());
            }
        }

        let mut entry_state = vec![Slot::Uninit; self.registers];
        let mut occupied = vec![false; self.registers];
        for &(reg, kind) in &self.params {
            match kind {
                ParamKind::Int64 => {
                    if reg + 1 >= self.registers || occupied[reg] || occupied[reg + 1] {
                        return Err("JIT overlapping or out-of-bounds wide parameter".into());
                    }
                    occupied[reg] = true;
                    occupied[reg + 1] = true;
                    entry_state[reg] = Slot::LongLo;
                    entry_state[reg + 1] = Slot::LongHi;
                }
                ParamKind::Int32 | ParamKind::Ref | ParamKind::ArrI32 | ParamKind::ArrI64 => {
                    if reg >= self.registers || occupied[reg] {
                        return Err("JIT overlapping or out-of-bounds parameter".into());
                    }
                    occupied[reg] = true;
                    entry_state[reg] = match kind {
                        ParamKind::Int32 => Slot::Int32,
                        ParamKind::Ref => Slot::Ref,
                        ParamKind::ArrI32 => Slot::ArrI32,
                        ParamKind::ArrI64 => Slot::ArrI64,
                        ParamKind::Int64 => Slot::Uninit,
                    };
                }
            }
        }

        let nblocks = self.blocks.len();
        let mut in_states: Vec<Option<Vec<Slot>>> = vec![None; nblocks];
        in_states[0] = Some(entry_state);
        let mut queue = VecDeque::new();
        queue.push_back(0usize);

        let push_edge = |target: usize,
                         out_state: &[Slot],
                         in_states: &mut [Option<Vec<Slot>>],
                         queue: &mut VecDeque<usize>|
         -> Result<(), String> {
            if target >= nblocks {
                return Err("JIT branch target out of bounds".into());
            }
            match &mut in_states[target] {
                None => {
                    in_states[target] = Some(out_state.to_vec());
                    queue.push_back(target);
                }
                Some(existing) => {
                    let mut changed = false;
                    for (slot, &incoming) in existing.iter_mut().zip(out_state.iter()) {
                        let merged = slot.merge(incoming);
                        if merged != *slot {
                            *slot = merged;
                            changed = true;
                        }
                    }
                    if changed {
                        queue.push_back(target);
                    }
                }
            }
            Ok(())
        };

        while let Some(bb) = queue.pop_front() {
            let Some(mut state) = in_states[bb].clone() else {
                return Err("JIT internal unvisited block".into());
            };
            let blk = &self.blocks[bb];
            for inst in &blk.insts {
                match *inst {
                    JitInst::Const32 { dst, value } => {
                        let s = if value == 0 { Slot::Zero } else { Slot::Int32 };
                        write_single(&mut state, dst, s)?;
                    }
                    JitInst::Const64 { dst, .. } => {
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::Move32 { dst, src } => {
                        if !check_i32(&state, src) {
                            return Err("JIT Move32 from non-int register".into());
                        }
                        let s = state[src];
                        write_single(&mut state, dst, s)?;
                    }
                    JitInst::Move64 { dst, src } => {
                        if !check_wide(&state, src) {
                            return Err("JIT Move64 from non-wide register pair".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::MoveRef { dst, src } => {
                        if src >= state.len() || !state[src].is_ref_like() {
                            return Err("JIT MoveRef from non-ref register".into());
                        }
                        let s = state[src];
                        write_single(&mut state, dst, s)?;
                    }
                    JitInst::Unary32 { dst, src, .. } | JitInst::Binary32Imm { dst, src, .. } => {
                        if !check_i32(&state, src) {
                            return Err("JIT 32-bit op on non-int register".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::DivRemLit32 { dst, src, lit, .. } => {
                        if lit == 0 || !check_i32(&state, src) {
                            return Err("JIT literal div/rem by zero or non-int register".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::Unary64 { dst, src, .. } => {
                        if !check_wide(&state, src) {
                            return Err("JIT Unary64 on non-wide register".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::Int32ToInt64 { dst, src } => {
                        if !check_i32(&state, src) {
                            return Err("JIT int-to-long on non-int register".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::Int64ToInt32 { dst, src } => {
                        if !check_wide(&state, src) {
                            return Err("JIT long-to-int on non-wide register".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::Binary32 { dst, lhs, rhs, .. } => {
                        if !check_i32(&state, lhs) || !check_i32(&state, rhs) {
                            return Err("JIT Binary32 on non-int registers".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::Binary64 { dst, lhs, rhs, .. } => {
                        if !check_wide(&state, lhs) || !check_wide(&state, rhs) {
                            return Err("JIT Binary64 on non-wide registers".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::Shift64 { dst, lhs, amt, .. } => {
                        if !check_wide(&state, lhs) || !check_i32(&state, amt) {
                            return Err("JIT Shift64 expects wide lhs and int32 shift count".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::CmpLong { dst, lhs, rhs } => {
                        if !check_wide(&state, lhs) || !check_wide(&state, rhs) {
                            return Err("JIT CmpLong on non-wide registers".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::ArrayLen { dst, arr, is_i64 } => {
                        let ok = if is_i64 {
                            check_arr_i64(&state, arr)
                        } else {
                            check_arr_i32(&state, arr)
                        };
                        if !ok {
                            return Err("JIT ArrayLen on incompatible register".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::ArrayGet32 { dst, arr, idx } => {
                        if !check_arr_i32(&state, arr) || !check_i32(&state, idx) {
                            return Err("JIT ArrayGet32 on incompatible registers".into());
                        }
                        write_single(&mut state, dst, Slot::Int32)?;
                    }
                    JitInst::ArrayGet64 { dst, arr, idx } => {
                        if !check_arr_i64(&state, arr) || !check_i32(&state, idx) {
                            return Err("JIT ArrayGet64 on incompatible registers".into());
                        }
                        write_wide(&mut state, dst)?;
                    }
                    JitInst::ArrayPut32 { src, arr, idx, .. } => {
                        if !check_i32(&state, src)
                            || !check_arr_i32(&state, arr)
                            || !check_i32(&state, idx)
                        {
                            return Err("JIT ArrayPut32 on incompatible registers".into());
                        }
                    }
                    JitInst::ArrayPut64 { src, arr, idx } => {
                        if !check_wide(&state, src)
                            || !check_arr_i64(&state, arr)
                            || !check_i32(&state, idx)
                        {
                            return Err("JIT ArrayPut64 on incompatible registers".into());
                        }
                    }
                }
            }

            match &blk.term {
                JitTerm::Jump { target } => {
                    push_edge(*target, &state, &mut in_states, &mut queue)?;
                }
                JitTerm::BranchEq {
                    lhs,
                    rhs,
                    then_bb,
                    else_bb,
                    ..
                } => {
                    if *lhs >= state.len()
                        || *rhs >= state.len()
                        || !state[*lhs].is_single_word_eq_compatible(state[*rhs])
                    {
                        return Err("JIT BranchEq on incompatible registers".into());
                    }
                    push_edge(*then_bb, &state, &mut in_states, &mut queue)?;
                    push_edge(*else_bb, &state, &mut in_states, &mut queue)?;
                }
                JitTerm::BranchCmp32 {
                    lhs,
                    rhs,
                    then_bb,
                    else_bb,
                    ..
                } => {
                    if !check_i32(&state, *lhs) || !check_i32(&state, *rhs) {
                        return Err("JIT BranchCmp32 on non-int registers".into());
                    }
                    push_edge(*then_bb, &state, &mut in_states, &mut queue)?;
                    push_edge(*else_bb, &state, &mut in_states, &mut queue)?;
                }
                JitTerm::BranchZero {
                    src,
                    then_bb,
                    else_bb,
                    ..
                } => {
                    if *src >= state.len() || !(state[*src].is_i32() || state[*src].is_ref_like()) {
                        return Err("JIT BranchZero on invalid register".into());
                    }
                    push_edge(*then_bb, &state, &mut in_states, &mut queue)?;
                    push_edge(*else_bb, &state, &mut in_states, &mut queue)?;
                }
                JitTerm::BranchCmpZero32 {
                    src,
                    then_bb,
                    else_bb,
                    ..
                } => {
                    if !check_i32(&state, *src) {
                        return Err("JIT BranchCmpZero32 on non-int register".into());
                    }
                    push_edge(*then_bb, &state, &mut in_states, &mut queue)?;
                    push_edge(*else_bb, &state, &mut in_states, &mut queue)?;
                }
                JitTerm::PackedSwitch {
                    src,
                    targets,
                    default_bb,
                    ..
                } => {
                    if !check_i32(&state, *src)
                        || targets.is_empty()
                        || targets.len() > MAX_SWITCH_CASES
                    {
                        return Err("JIT PackedSwitch invalid operand or size".into());
                    }
                    push_edge(*default_bb, &state, &mut in_states, &mut queue)?;
                    for &t in targets {
                        push_edge(t, &state, &mut in_states, &mut queue)?;
                    }
                }
                JitTerm::SparseSwitch {
                    src,
                    cases,
                    default_bb,
                } => {
                    if !check_i32(&state, *src) || cases.len() > MAX_SWITCH_CASES {
                        return Err("JIT SparseSwitch invalid operand or size".into());
                    }
                    for w in cases.windows(2) {
                        if w[0].0 >= w[1].0 {
                            return Err("JIT SparseSwitch keys must be strictly ascending".into());
                        }
                    }
                    push_edge(*default_bb, &state, &mut in_states, &mut queue)?;
                    for &(_, t) in cases {
                        push_edge(t, &state, &mut in_states, &mut queue)?;
                    }
                }
                JitTerm::ReturnVoid => {
                    if self.ret != RetKind::Void {
                        return Err("JIT ReturnVoid mismatches signature".into());
                    }
                }
                JitTerm::Return32 { src } => {
                    if self.ret != RetKind::Int32 || !check_i32(&state, *src) {
                        return Err("JIT Return32 mismatches signature or register".into());
                    }
                }
                JitTerm::Return64 { src } => {
                    if self.ret != RetKind::Int64 || !check_wide(&state, *src) {
                        return Err("JIT Return64 mismatches signature or register".into());
                    }
                }
            }
        }

        // Every block in `self.blocks` must be reachable and type-verified so
        // Cranelift never lowers unverified dead blocks.
        if in_states.iter().any(Option::is_none) {
            return Err("JIT program contains unreachable unverified blocks".into());
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawArrayDesc {
    ptr: *mut u8,
    len: u64,
}

const METHOD_CTX_I32_WORDS: usize = 14;

#[derive(Clone, Copy)]
struct CompiledMethod {
    inner_handle: usize,
    arity: usize,
}

fn pack_ctx_u64(words: &mut [i32; METHOD_CTX_I32_WORDS], word_idx: usize, val: u64) {
    words[word_idx] = val as u32 as i32;
    words[word_idx + 1] = (val >> 32) as u32 as i32;
}

fn load_ctx_u64(b: &mut FunctionBuilder, ctx_ptr: Value, word_idx: i32) -> Value {
    let lo32 = b
        .ins()
        .load(types::I32, trusted_flags(), ctx_ptr, word_idx * 4);
    let hi32 = b
        .ins()
        .load(types::I32, trusted_flags(), ctx_ptr, (word_idx + 1) * 4);
    let lo64 = b.ins().uextend(types::I64, lo32);
    let hi64 = b.ins().uextend(types::I64, hi32);
    let hi_shifted = b.ins().ishl_imm_s(hi64, 32);
    b.ins().bor(lo64, hi_shifted)
}

fn u64_to_ptr(b: &mut FunctionBuilder, ptr_ty: types::Type, val64: Value) -> Value {
    if ptr_ty == types::I64 {
        val64
    } else {
        b.ins().ireduce(ptr_ty, val64)
    }
}

fn int_cc(c: IntCond) -> IntCC {
    match c {
        IntCond::Eq => IntCC::Equal,
        IntCond::Ne => IntCC::NotEqual,
        IntCond::Lt => IntCC::SignedLessThan,
        IntCond::Ge => IntCC::SignedGreaterThanOrEqual,
        IntCond::Gt => IntCC::SignedGreaterThan,
        IntCond::Le => IntCC::SignedLessThanOrEqual,
    }
}

fn emit_bin32(b: &mut FunctionBuilder, op: BinOp32, x: Value, mut y: Value) -> Value {
    if matches!(op, BinOp32::Shl | BinOp32::Shr | BinOp32::Ushr) {
        y = b.ins().band_imm_s(y, 31);
    }
    match op {
        BinOp32::Add => b.ins().iadd(x, y),
        BinOp32::Sub => b.ins().isub(x, y),
        BinOp32::Mul => b.ins().imul(x, y),
        BinOp32::And => b.ins().band(x, y),
        BinOp32::Or => b.ins().bor(x, y),
        BinOp32::Xor => b.ins().bxor(x, y),
        BinOp32::Shl => b.ins().ishl(x, y),
        BinOp32::Shr => b.ins().sshr(x, y),
        BinOp32::Ushr => b.ins().ushr(x, y),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_array_addr(
    b: &mut FunctionBuilder,
    ptr_ty: types::Type,
    handle_val: Value,
    idx_val: Value,
    table_ptr: Value,
    table_count: Value,
    elem_shift: u8,
    bailout_bb: Block,
) -> Value {
    // Handles are 1-based (`0` = Java null). Verify `1 <= h <= table_count`.
    let h64 = b.ins().uextend(types::I64, handle_val);
    let is_zero = b.ins().icmp_imm_s(IntCC::Equal, h64, 0);
    let too_high = b.ins().icmp(IntCC::UnsignedGreaterThan, h64, table_count);
    let bad_handle = b.ins().bor(is_zero, too_high);
    let ok_handle_bb = b.create_block();
    b.ins().brif(bad_handle, bailout_bb, &[], ok_handle_bb, &[]);
    b.seal_block(ok_handle_bb);
    b.switch_to_block(ok_handle_bb);

    // Load `RawArrayDesc { ptr, len }` at `(h - 1) * sizeof(RawArrayDesc)` (16 bytes on 64-bit).
    let slot_idx = b.ins().iadd_imm_s(h64, -1);
    let desc_byte_off = b.ins().ishl_imm_s(slot_idx, 4);
    let off_ptr = if ptr_ty == types::I64 {
        desc_byte_off
    } else {
        b.ins().ireduce(ptr_ty, desc_byte_off)
    };
    let desc_addr = b.ins().iadd(table_ptr, off_ptr);
    let base_ptr = b.ins().load(ptr_ty, trusted_flags(), desc_addr, 0);
    let arr_len64 = b.ins().load(
        types::I64,
        trusted_flags(),
        desc_addr,
        ptr_ty.bytes() as i32,
    );
    let arr_len32 = b.ins().ireduce(types::I32, arr_len64);

    // Single unsigned compare catches both `idx < 0` and `idx >= len`.
    let oob = b
        .ins()
        .icmp(IntCC::UnsignedGreaterThanOrEqual, idx_val, arr_len32);
    let ok_bounds_bb = b.create_block();
    b.ins().brif(oob, bailout_bb, &[], ok_bounds_bb, &[]);
    b.seal_block(ok_bounds_bb);
    b.switch_to_block(ok_bounds_bb);

    let idx_ptr = if ptr_ty == types::I64 {
        b.ins().uextend(types::I64, idx_val)
    } else {
        idx_val
    };
    let byte_off = b.ins().ishl_imm_s(idx_ptr, i64::from(elem_shift));
    b.ins().iadd(base_ptr, byte_off)
}

/// Owns all compiled method addresses via an inner `LeafJit` code module; no
/// code pointer escapes this safe API and no additional `unsafe` boundary is
/// introduced beyond ADR 007's single-pointer entrypoint.
pub struct MethodJit {
    inner: LeafJit,
    functions: Vec<CompiledMethod>,
}

impl MethodJit {
    pub fn new() -> Result<Self, String> {
        // RawArrayDesc layout assumption check (`ptr` + `u64` == 16 bytes on 64-bit hosts).
        if std::mem::size_of::<RawArrayDesc>() != 16 || std::mem::size_of::<usize>() != 8 {
            return Err("MethodJit requires a 64-bit host pointer target".into());
        }
        Ok(Self {
            inner: LeafJit::new()?,
            functions: Vec::new(),
        })
    }

    #[allow(clippy::too_many_lines)]
    pub fn compile(&mut self, prog: &JitProgram) -> Result<usize, String> {
        prog.validate()?;
        if self.functions.len() >= MAX_FUNCTIONS {
            return Err("JIT code cache limit (4096 methods)".into());
        }
        let module = self.inner.module.as_mut().ok_or("JIT module unavailable")?;
        let mut ctx = module.make_context();
        let ptr_ty = module.target_config().pointer_type();
        // Shares ADR 007's exact `extern "C" fn(*const i32) -> i32` signature
        // with `LeafJit`, reading a 14-word (`7 * u64`) stack context packet.
        ctx.func.signature.params.push(AbiParam::new(ptr_ty));
        ctx.func.signature.returns.push(AbiParam::new(types::I32));

        let id = module
            .declare_function(
                &format!("rd_jit_fn_{}", self.functions.len()),
                Linkage::Local,
                &ctx.func.signature,
            )
            .map_err(|e| e.to_string())?;
        ctx.func.name = UserFuncName::user(0, id.as_u32());

        let mut fctx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let prologue_bb = b.create_block();
            b.append_block_params_for_function_params(prologue_bb);

            let mut cl_blocks = Vec::with_capacity(prog.blocks.len());
            for _ in 0..prog.blocks.len() {
                cl_blocks.push(b.create_block());
            }
            let fuel_exit_bb = b.create_block();
            let bailout_exit_bb = b.create_block();

            // Declare SSA variables:
            //   0..registers               -> I32 / handle lane
            //   registers..2*registers     -> I64 wide lane
            //   2*registers                -> I64 accumulated fuel
            let mut var_i32 = Vec::with_capacity(prog.registers);
            let mut var_i64 = Vec::with_capacity(prog.registers);
            for _ in 0..prog.registers {
                // cranelift 0.137: declare_var devolve a Variable (sem índice manual).
                let v32 = b.declare_var(types::I32);
                let v64 = b.declare_var(types::I64);
                var_i32.push(v32);
                var_i64.push(v64);
            }
            let fuel_var = b.declare_var(types::I64);

            // Prologue: unpack the 14-word read-only context packet & initialize SSA slots.
            b.switch_to_block(prologue_bb);
            let p_ctx = b.block_params(prologue_bb)[0];
            let args_u64 = load_ctx_u64(&mut b, p_ctx, 0);
            let arr32_u64 = load_ctx_u64(&mut b, p_ctx, 2);
            let p_arr32_cnt = load_ctx_u64(&mut b, p_ctx, 4);
            let arr64_u64 = load_ctx_u64(&mut b, p_ctx, 6);
            let p_arr64_cnt = load_ctx_u64(&mut b, p_ctx, 8);
            let fuel_budget = load_ctx_u64(&mut b, p_ctx, 10);
            let out_io_u64 = load_ctx_u64(&mut b, p_ctx, 12);

            let p_args = u64_to_ptr(&mut b, ptr_ty, args_u64);
            let p_arr32 = u64_to_ptr(&mut b, ptr_ty, arr32_u64);
            let p_arr64 = u64_to_ptr(&mut b, ptr_ty, arr64_u64);
            let p_out_io = u64_to_ptr(&mut b, ptr_ty, out_io_u64);
            let zero32 = b.ins().iconst(types::I32, 0);
            let zero64 = b.ins().iconst(types::I64, 0);
            for r in 0..prog.registers {
                b.def_var(var_i32[r], zero32);
                b.def_var(var_i64[r], zero64);
            }
            b.def_var(fuel_var, zero64);

            for (arg_idx, &(reg, kind)) in prog.params.iter().enumerate() {
                let raw = b
                    .ins()
                    .load(types::I64, trusted_flags(), p_args, (arg_idx * 8) as i32);
                match kind {
                    ParamKind::Int64 => {
                        b.def_var(var_i64[reg], raw);
                    }
                    ParamKind::Int32 | ParamKind::Ref | ParamKind::ArrI32 | ParamKind::ArrI64 => {
                        let v32 = b.ins().ireduce(types::I32, raw);
                        b.def_var(var_i32[reg], v32);
                    }
                }
            }
            b.ins().jump(cl_blocks[0], &[]);
            b.seal_block(prologue_bb);

            // Emit each verified basic block with exact Dalvik fuel check at header.
            for (bb_idx, blk) in prog.blocks.iter().enumerate() {
                b.switch_to_block(cl_blocks[bb_idx]);
                let cur_fuel = b.use_var(fuel_var);
                let next_fuel = b.ins().iadd_imm_s(cur_fuel, i64::from(blk.dalvik_insns));
                let over = b
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThan, next_fuel, fuel_budget);
                let body_bb = b.create_block();
                b.ins().brif(over, fuel_exit_bb, &[], body_bb, &[]);
                b.seal_block(body_bb);
                b.switch_to_block(body_bb);
                b.def_var(fuel_var, next_fuel);

                for inst in &blk.insts {
                    match *inst {
                        JitInst::Const32 { dst, value } => {
                            let v = b.ins().iconst(types::I32, i64::from(value as u32));
                            b.def_var(var_i32[dst], v);
                        }
                        JitInst::Const64 { dst, value } => {
                            let v = b.ins().iconst(types::I64, value);
                            b.def_var(var_i64[dst], v);
                        }
                        JitInst::Move32 { dst, src } | JitInst::MoveRef { dst, src } => {
                            let v = b.use_var(var_i32[src]);
                            b.def_var(var_i32[dst], v);
                        }
                        JitInst::Move64 { dst, src } => {
                            let v = b.use_var(var_i64[src]);
                            b.def_var(var_i64[dst], v);
                        }
                        JitInst::Unary32 { dst, src, op } => {
                            let x = b.use_var(var_i32[src]);
                            let res = match op {
                                UnOp32::Neg => b.ins().ineg(x),
                                UnOp32::Not => b.ins().bnot(x),
                                UnOp32::Byte => {
                                    let t = b.ins().ireduce(types::I8, x);
                                    b.ins().sextend(types::I32, t)
                                }
                                UnOp32::Short => {
                                    let t = b.ins().ireduce(types::I16, x);
                                    b.ins().sextend(types::I32, t)
                                }
                                UnOp32::Char => b.ins().band_imm_s(x, 0xFFFF),
                            };
                            b.def_var(var_i32[dst], res);
                        }
                        JitInst::Unary64 { dst, src, op } => {
                            let x = b.use_var(var_i64[src]);
                            let res = match op {
                                UnOp64::Neg => b.ins().ineg(x),
                                UnOp64::Not => b.ins().bnot(x),
                            };
                            b.def_var(var_i64[dst], res);
                        }
                        JitInst::Int32ToInt64 { dst, src } => {
                            let x = b.use_var(var_i32[src]);
                            let v = b.ins().sextend(types::I64, x);
                            b.def_var(var_i64[dst], v);
                        }
                        JitInst::Int64ToInt32 { dst, src } => {
                            let x = b.use_var(var_i64[src]);
                            let v = b.ins().ireduce(types::I32, x);
                            b.def_var(var_i32[dst], v);
                        }
                        JitInst::Binary32 { dst, lhs, rhs, op } => {
                            let x = b.use_var(var_i32[lhs]);
                            let y = b.use_var(var_i32[rhs]);
                            let res = emit_bin32(&mut b, op, x, y);
                            b.def_var(var_i32[dst], res);
                        }
                        JitInst::Binary32Imm {
                            dst,
                            src,
                            imm,
                            op,
                            reverse_sub,
                        } => {
                            let x = b.use_var(var_i32[src]);
                            let k = b.ins().iconst(types::I32, i64::from(imm as u32));
                            let res = if reverse_sub {
                                b.ins().isub(k, x)
                            } else {
                                emit_bin32(&mut b, op, x, k)
                            };
                            b.def_var(var_i32[dst], res);
                        }
                        JitInst::DivRemLit32 {
                            dst,
                            src,
                            lit,
                            is_rem,
                        } => {
                            // `lit != 0` is guaranteed by `validate()`. Special-case `±1`
                            // so `i32::MIN / -1` wraps to `i32::MIN` and `i32::MIN % -1 == 0`
                            // without risking hardware `idiv` overflow trap (`#DE`).
                            let x = b.use_var(var_i32[src]);
                            let res = match (lit, is_rem) {
                                (1, false) => x,
                                (-1, false) => b.ins().ineg(x),
                                (1 | -1, true) => b.ins().iconst(types::I32, 0),
                                (d, false) => {
                                    let kv = b.ins().iconst(types::I32, i64::from(d as u32));
                                    b.ins().sdiv(x, kv)
                                }
                                (d, true) => {
                                    let kv = b.ins().iconst(types::I32, i64::from(d as u32));
                                    b.ins().srem(x, kv)
                                }
                            };
                            b.def_var(var_i32[dst], res);
                        }
                        JitInst::Binary64 { dst, lhs, rhs, op } => {
                            let x = b.use_var(var_i64[lhs]);
                            let y = b.use_var(var_i64[rhs]);
                            let res = match op {
                                BinOp64::Add => b.ins().iadd(x, y),
                                BinOp64::Sub => b.ins().isub(x, y),
                                BinOp64::Mul => b.ins().imul(x, y),
                                BinOp64::And => b.ins().band(x, y),
                                BinOp64::Or => b.ins().bor(x, y),
                                BinOp64::Xor => b.ins().bxor(x, y),
                            };
                            b.def_var(var_i64[dst], res);
                        }
                        JitInst::Shift64 { dst, lhs, amt, op } => {
                            let x = b.use_var(var_i64[lhs]);
                            let sh32 = b.use_var(var_i32[amt]);
                            let sh_masked = b.ins().band_imm_s(sh32, 63);
                            let res = match op {
                                ShiftOp64::Shl => b.ins().ishl(x, sh_masked),
                                ShiftOp64::Shr => b.ins().sshr(x, sh_masked),
                                ShiftOp64::Ushr => b.ins().ushr(x, sh_masked),
                            };
                            b.def_var(var_i64[dst], res);
                        }
                        JitInst::CmpLong { dst, lhs, rhs } => {
                            let x = b.use_var(var_i64[lhs]);
                            let y = b.use_var(var_i64[rhs]);
                            let gt = b.ins().icmp(IntCC::SignedGreaterThan, x, y);
                            let lt = b.ins().icmp(IntCC::SignedLessThan, x, y);
                            let gt32 = b.ins().uextend(types::I32, gt);
                            let lt32 = b.ins().uextend(types::I32, lt);
                            let res = b.ins().isub(gt32, lt32);
                            b.def_var(var_i32[dst], res);
                        }
                        JitInst::ArrayLen { dst, arr, is_i64 } => {
                            let h = b.use_var(var_i32[arr]);
                            let (tbl, cnt) = if is_i64 {
                                (p_arr64, p_arr64_cnt)
                            } else {
                                (p_arr32, p_arr32_cnt)
                            };
                            let h64 = b.ins().uextend(types::I64, h);
                            let is_zero = b.ins().icmp_imm_s(IntCC::Equal, h64, 0);
                            let too_high = b.ins().icmp(IntCC::UnsignedGreaterThan, h64, cnt);
                            let bad = b.ins().bor(is_zero, too_high);
                            let ok_bb = b.create_block();
                            b.ins().brif(bad, bailout_exit_bb, &[], ok_bb, &[]);
                            b.seal_block(ok_bb);
                            b.switch_to_block(ok_bb);

                            let slot_idx = b.ins().iadd_imm_s(h64, -1);
                            let desc_off = b.ins().ishl_imm_s(slot_idx, 4);
                            let desc_addr = b.ins().iadd(tbl, desc_off);
                            let len64 = b.ins().load(
                                types::I64,
                                trusted_flags(),
                                desc_addr,
                                ptr_ty.bytes() as i32,
                            );
                            let len32 = b.ins().ireduce(types::I32, len64);
                            b.def_var(var_i32[dst], len32);
                        }
                        JitInst::ArrayGet32 { dst, arr, idx } => {
                            let h = b.use_var(var_i32[arr]);
                            let iv = b.use_var(var_i32[idx]);
                            let addr = emit_array_addr(
                                &mut b,
                                ptr_ty,
                                h,
                                iv,
                                p_arr32,
                                p_arr32_cnt,
                                2,
                                bailout_exit_bb,
                            );
                            let val = b.ins().load(types::I32, trusted_flags(), addr, 0);
                            b.def_var(var_i32[dst], val);
                        }
                        JitInst::ArrayGet64 { dst, arr, idx } => {
                            let h = b.use_var(var_i32[arr]);
                            let iv = b.use_var(var_i32[idx]);
                            let addr = emit_array_addr(
                                &mut b,
                                ptr_ty,
                                h,
                                iv,
                                p_arr64,
                                p_arr64_cnt,
                                3,
                                bailout_exit_bb,
                            );
                            let val = b.ins().load(types::I64, trusted_flags(), addr, 0);
                            b.def_var(var_i64[dst], val);
                        }
                        JitInst::ArrayPut32 {
                            src,
                            arr,
                            idx,
                            elem,
                        } => {
                            let raw_v = b.use_var(var_i32[src]);
                            let h = b.use_var(var_i32[arr]);
                            let iv = b.use_var(var_i32[idx]);
                            let addr = emit_array_addr(
                                &mut b,
                                ptr_ty,
                                h,
                                iv,
                                p_arr32,
                                p_arr32_cnt,
                                2,
                                bailout_exit_bb,
                            );
                            let narrowed = match elem {
                                ArrElem32::Int => raw_v,
                                ArrElem32::Bool => {
                                    let nz = b.ins().icmp_imm_s(IntCC::NotEqual, raw_v, 0);
                                    b.ins().uextend(types::I32, nz)
                                }
                                ArrElem32::Byte => {
                                    let t = b.ins().ireduce(types::I8, raw_v);
                                    b.ins().sextend(types::I32, t)
                                }
                                ArrElem32::Char => b.ins().band_imm_s(raw_v, 0xFFFF),
                                ArrElem32::Short => {
                                    let t = b.ins().ireduce(types::I16, raw_v);
                                    b.ins().sextend(types::I32, t)
                                }
                            };
                            b.ins().store(trusted_flags(), narrowed, addr, 0);
                        }
                        JitInst::ArrayPut64 { src, arr, idx } => {
                            let val = b.use_var(var_i64[src]);
                            let h = b.use_var(var_i32[arr]);
                            let iv = b.use_var(var_i32[idx]);
                            let addr = emit_array_addr(
                                &mut b,
                                ptr_ty,
                                h,
                                iv,
                                p_arr64,
                                p_arr64_cnt,
                                3,
                                bailout_exit_bb,
                            );
                            b.ins().store(trusted_flags(), val, addr, 0);
                        }
                    }
                }

                match &blk.term {
                    JitTerm::Jump { target } => {
                        b.ins().jump(cl_blocks[*target], &[]);
                    }
                    JitTerm::BranchEq {
                        eq,
                        lhs,
                        rhs,
                        then_bb,
                        else_bb,
                    } => {
                        let x = b.use_var(var_i32[*lhs]);
                        let y = b.use_var(var_i32[*rhs]);
                        let cc = if *eq { IntCC::Equal } else { IntCC::NotEqual };
                        let c = b.ins().icmp(cc, x, y);
                        b.ins()
                            .brif(c, cl_blocks[*then_bb], &[], cl_blocks[*else_bb], &[]);
                    }
                    JitTerm::BranchCmp32 {
                        cond,
                        lhs,
                        rhs,
                        then_bb,
                        else_bb,
                    } => {
                        let x = b.use_var(var_i32[*lhs]);
                        let y = b.use_var(var_i32[*rhs]);
                        let c = b.ins().icmp(int_cc(*cond), x, y);
                        b.ins()
                            .brif(c, cl_blocks[*then_bb], &[], cl_blocks[*else_bb], &[]);
                    }
                    JitTerm::BranchZero {
                        eq,
                        src,
                        then_bb,
                        else_bb,
                    } => {
                        let x = b.use_var(var_i32[*src]);
                        let cc = if *eq { IntCC::Equal } else { IntCC::NotEqual };
                        let c = b.ins().icmp_imm_s(cc, x, 0);
                        b.ins()
                            .brif(c, cl_blocks[*then_bb], &[], cl_blocks[*else_bb], &[]);
                    }
                    JitTerm::BranchCmpZero32 {
                        cond,
                        src,
                        then_bb,
                        else_bb,
                    } => {
                        let x = b.use_var(var_i32[*src]);
                        let c = b.ins().icmp_imm_s(int_cc(*cond), x, 0);
                        b.ins()
                            .brif(c, cl_blocks[*then_bb], &[], cl_blocks[*else_bb], &[]);
                    }
                    JitTerm::PackedSwitch {
                        src,
                        first_key,
                        targets,
                        default_bb,
                    } => {
                        let key = b.use_var(var_i32[*src]);
                        let fk = b.ins().iconst(types::I32, i64::from(*first_key as u32));
                        let rel = b.ins().isub(key, fk);
                        let def_call = b.func.dfg.block_call(cl_blocks[*default_bb], &[]);
                        let mut calls = Vec::with_capacity(targets.len());
                        for &t in targets {
                            calls.push(b.func.dfg.block_call(cl_blocks[t], &[]));
                        }
                        let jt = b.create_jump_table(JumpTableData::new(def_call, &calls));
                        b.ins().br_table(rel, jt);
                    }
                    JitTerm::SparseSwitch {
                        src,
                        cases,
                        default_bb,
                    } => {
                        let key = b.use_var(var_i32[*src]);
                        let mut sw = Switch::new();
                        for &(k, t) in cases {
                            sw.set_entry(u128::from(k as u32), cl_blocks[t]);
                        }
                        sw.emit(&mut b, key, cl_blocks[*default_bb]);
                    }
                    JitTerm::ReturnVoid => {
                        let used = b.use_var(fuel_var);
                        b.ins().store(trusted_flags(), used, p_out_io, 0);
                        let ok = b.ins().iconst(types::I32, STATUS_OK);
                        b.ins().return_(&[ok]);
                    }
                    JitTerm::Return32 { src } => {
                        let used = b.use_var(fuel_var);
                        let v32 = b.use_var(var_i32[*src]);
                        let v64 = b.ins().uextend(types::I64, v32);
                        b.ins().store(trusted_flags(), used, p_out_io, 0);
                        b.ins().store(trusted_flags(), v64, p_out_io, 8);
                        let ok = b.ins().iconst(types::I32, STATUS_OK);
                        b.ins().return_(&[ok]);
                    }
                    JitTerm::Return64 { src } => {
                        let used = b.use_var(fuel_var);
                        let v64 = b.use_var(var_i64[*src]);
                        b.ins().store(trusted_flags(), used, p_out_io, 0);
                        b.ins().store(trusted_flags(), v64, p_out_io, 8);
                        let ok = b.ins().iconst(types::I32, STATUS_OK);
                        b.ins().return_(&[ok]);
                    }
                }
            }

            b.switch_to_block(fuel_exit_bb);
            let st_fuel = b.ins().iconst(types::I32, STATUS_FUEL);
            b.ins().return_(&[st_fuel]);

            b.switch_to_block(bailout_exit_bb);
            let st_bail = b.ins().iconst(types::I32, STATUS_BAILOUT);
            b.ins().return_(&[st_bail]);

            b.seal_all_blocks();
            b.finalize(module.target_config());
        }

        module
            .define_function(id, &mut ctx)
            .map_err(|e| format!("{e:?}"))?;
        module.clear_context(&mut ctx);
        module.finalize_definitions().map_err(|e| e.to_string())?;
        let address = module.get_finalized_function(id) as usize;
        let inner_handle = self.inner.functions.len();
        self.inner.functions.push((address, METHOD_CTX_I32_WORDS));
        let handle = self.functions.len();
        self.functions.push(CompiledMethod {
            inner_handle,
            arity: prog.params.len(),
        });
        Ok(handle)
    }

    /// Execute a verified, owned native method with stack-marshaled scalar args
    /// and safe Rust slice descriptors for primitive arrays, delegating to the
    /// shared ADR 007 single-pointer `LeafJit::execute` boundary.
    pub fn execute(
        &self,
        handle: usize,
        args: &[u64],
        arrays_i32: &mut [&mut [i32]],
        arrays_i64: &mut [&mut [i64]],
        fuel_budget: u64,
    ) -> Result<JitExecResult, String> {
        let meta = *self.functions.get(handle).ok_or("invalid JIT handle")?;
        if args.len() != meta.arity {
            return Err("JIT argument count mismatch".into());
        }
        if arrays_i32.len() > MAX_ARRAY_HANDLES || arrays_i64.len() > MAX_ARRAY_HANDLES {
            return Err("JIT array handle count exceeds limit".into());
        }
        let mut out_io = [0u64; 2];
        let mut ctx_words = [0i32; METHOD_CTX_I32_WORDS];
        pack_ctx_u64(&mut ctx_words, 0, args.as_ptr() as usize as u64);
        pack_ctx_u64(&mut ctx_words, 10, fuel_budget);
        pack_ctx_u64(&mut ctx_words, 12, out_io.as_mut_ptr() as usize as u64);

        let status = if arrays_i32.is_empty() && arrays_i64.is_empty() {
            self.inner.execute(meta.inner_handle, &ctx_words)?
        } else {
            let mut raw_i32 = [RawArrayDesc {
                ptr: std::ptr::null_mut(),
                len: 0,
            }; MAX_ARRAY_HANDLES];
            for (slot, slice) in raw_i32.iter_mut().zip(arrays_i32.iter_mut()) {
                slot.ptr = slice.as_mut_ptr().cast::<u8>();
                slot.len = slice.len() as u64;
            }
            let mut raw_i64 = [RawArrayDesc {
                ptr: std::ptr::null_mut(),
                len: 0,
            }; MAX_ARRAY_HANDLES];
            for (slot, slice) in raw_i64.iter_mut().zip(arrays_i64.iter_mut()) {
                slot.ptr = slice.as_mut_ptr().cast::<u8>();
                slot.len = slice.len() as u64;
            }
            pack_ctx_u64(&mut ctx_words, 2, raw_i32.as_ptr() as usize as u64);
            pack_ctx_u64(&mut ctx_words, 4, arrays_i32.len() as u64);
            pack_ctx_u64(&mut ctx_words, 6, raw_i64.as_ptr() as usize as u64);
            pack_ctx_u64(&mut ctx_words, 8, arrays_i64.len() as u64);
            self.inner.execute(meta.inner_handle, &ctx_words)?
        };
        match i64::from(status) {
            STATUS_OK => Ok(JitExecResult::Returned {
                bits: out_io[1],
                fuel_consumed: out_io[0],
            }),
            STATUS_FUEL => Ok(JitExecResult::FuelExhausted),
            STATUS_BAILOUT => Ok(JitExecResult::Bailout),
            other => Err(format!("unexpected JIT exit status {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_leaf_wraps_and_checks_arity() {
        let mut j = LeafJit::new().unwrap();
        let leaf = Leaf {
            registers: 3,
            parameters: 2,
            ops: vec![Op::Binary {
                dst: 0,
                lhs: 1,
                rhs: 2,
                op: Binary::Add,
            }],
            result: 0,
        };
        let h = j.compile(&leaf).unwrap();
        assert_eq!(j.execute(h, &[i32::MAX, 1]).unwrap(), i32::MIN);
        assert!(j.execute(h, &[1]).is_err());
        assert!(j.execute(999, &[1, 2]).is_err());
        let bad = Leaf { result: 5, ..leaf };
        assert!(j.compile(&bad).is_err());
    }

    #[test]
    fn method_jit_executes_cfg_loop_long_and_checked_array_bounds() {
        let mut jit = MethodJit::new().unwrap();
        // Sum `arr[0..n]` as i64:
        // v0 = acc (i64, slots 0..1), v2 = i (i32), v3 = arr ([I), v4 = n (i32)
        let prog = JitProgram {
            registers: 6,
            params: vec![(3, ParamKind::ArrI32), (4, ParamKind::Int32)],
            ret: RetKind::Int64,
            blocks: vec![
                JitBlock {
                    dalvik_insns: 3,
                    insts: vec![
                        JitInst::Const64 { dst: 0, value: 0 },
                        JitInst::Const32 { dst: 2, value: 0 },
                    ],
                    term: JitTerm::Jump { target: 1 },
                },
                JitBlock {
                    dalvik_insns: 1,
                    insts: vec![],
                    term: JitTerm::BranchCmp32 {
                        cond: IntCond::Ge,
                        lhs: 2,
                        rhs: 4,
                        then_bb: 3,
                        else_bb: 2,
                    },
                },
                JitBlock {
                    dalvik_insns: 5,
                    insts: vec![
                        JitInst::ArrayGet32 {
                            dst: 5,
                            arr: 3,
                            idx: 2,
                        },
                        JitInst::Int32ToInt64 { dst: 4, src: 5 },
                    ],
                    term: JitTerm::Return64 { src: 0 },
                },
                JitBlock {
                    dalvik_insns: 1,
                    insts: vec![],
                    term: JitTerm::Return64 { src: 0 },
                },
            ],
        };
        // Notice block 2 above clobbers v4 (`Int32ToInt64 { dst: 4 }`), which is fine since it returns;
        // now build a real loop without clobbering `n` in v4:
        assert!(jit.compile(&prog).is_ok());

        let loop_prog = JitProgram {
            registers: 8,
            params: vec![(5, ParamKind::ArrI32), (6, ParamKind::Int32)],
            ret: RetKind::Int64,
            blocks: vec![
                JitBlock {
                    dalvik_insns: 3,
                    insts: vec![
                        JitInst::Const64 { dst: 0, value: 0 },
                        JitInst::Const32 { dst: 2, value: 0 },
                    ],
                    term: JitTerm::Jump { target: 1 },
                },
                JitBlock {
                    dalvik_insns: 1,
                    insts: vec![],
                    term: JitTerm::BranchCmp32 {
                        cond: IntCond::Ge,
                        lhs: 2,
                        rhs: 6,
                        then_bb: 3,
                        else_bb: 2,
                    },
                },
                JitBlock {
                    dalvik_insns: 5,
                    insts: vec![
                        JitInst::ArrayGet32 {
                            dst: 7,
                            arr: 5,
                            idx: 2,
                        },
                        JitInst::Int32ToInt64 { dst: 3, src: 7 },
                        JitInst::Binary64 {
                            dst: 0,
                            lhs: 0,
                            rhs: 3,
                            op: BinOp64::Add,
                        },
                        JitInst::Binary32Imm {
                            dst: 2,
                            src: 2,
                            imm: 1,
                            op: BinOp32::Add,
                            reverse_sub: false,
                        },
                    ],
                    term: JitTerm::Jump { target: 1 },
                },
                JitBlock {
                    dalvik_insns: 1,
                    insts: vec![],
                    term: JitTerm::Return64 { src: 0 },
                },
            ],
        };
        let h = jit.compile(&loop_prog).unwrap();
        let mut data = [10i32, -20, 30, 40];
        let res = jit
            .execute(h, &[1, 4], &mut [&mut data[..]], &mut [], 1000)
            .unwrap();
        assert_eq!(
            res,
            JitExecResult::Returned {
                bits: 60u64,
                fuel_consumed: 3 + 4 * (1 + 5) + 1 + 1,
            }
        );

        // Out-of-bounds `n = 5 > data.len()` bails out safely without crashing.
        let oob = jit
            .execute(h, &[1, 5], &mut [&mut data[..]], &mut [], 1000)
            .unwrap();
        assert_eq!(oob, JitExecResult::Bailout);

        // Null handle `0` bails out safely on first dereference.
        let null_res = jit
            .execute(h, &[0, 1], &mut [&mut data[..]], &mut [], 1000)
            .unwrap();
        assert_eq!(null_res, JitExecResult::Bailout);

        // Tight fuel limit stops loop deterministically.
        let fuel_res = jit
            .execute(h, &[1, 4], &mut [&mut data[..]], &mut [], 10)
            .unwrap();
        assert_eq!(fuel_res, JitExecResult::FuelExhausted);
    }
}
