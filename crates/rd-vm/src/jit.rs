//! Complete, deterministic Cranelift integer/long/control-flow/array JIT (ADR 007).
//!
//! Lowers verified Dalvik methods — including multi-block CFGs, backward loops,
//! `packed-switch`/`sparse-switch`, 64-bit `long` arithmetic and shifts,
//! non-zero literal division/remainder, and primitive array kernels — into
//! `rd_jni::jit::MethodJit` while preserving exact per-instruction fuel
//! accounting, zero `unsafe` in `rd-vm`, and instant interpreter fallback for
//! unsupported or exception-throwing paths.

use std::collections::{BTreeSet, HashMap, VecDeque};

use rd_dex::code::{CodeItem, Insn, Kind, Payload};
use rd_jni::jit::{
    ArrElem32, BinOp32, BinOp64, IntCond, JitBlock, JitExecResult, JitInst, JitProgram, JitTerm,
    MethodJit, ParamKind, RetKind, ShiftOp64, UnOp32, UnOp64,
};

use crate::engine::{Engine, ACC_STATIC};
use crate::err::{self, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::Value;

const LEAF_WARMUP_CALLS: u8 = 32;
const MAX_METHOD_INSNS: usize = 2048;
const MAX_METHOD_REGS: usize = 256;
const MAX_CACHE_ENTRIES: usize = 8192;
const MAX_ARRAY_ELEMS: usize = 65_536;
const MAX_STACK_ARGS: usize = 32;
const MAX_JIT_ARRAY_HANDLES: usize = 16;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct JitStats {
    pub enabled: bool,
    pub compiled: usize,
    pub native_calls: u64,
    pub unsupported: usize,
    pub compile_errors: usize,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy)]
struct ParamSlot {
    slot_offset: usize,
    kind: ParamKind,
    arr_desc: Option<&'static str>,
}

struct CompiledPlan {
    handle: usize,
    ret: RetKind,
    params: Vec<ParamSlot>,
    ins_size: usize,
    has_array_params: bool,
    has_bailout_ops: bool,
    has_aput: bool,
}

enum Entry {
    Warming(u8),
    Unsupported,
    Compiled(CompiledPlan),
}

#[derive(Clone)]
struct StaticIcSlot {
    class: String,
    method: String,
    sig: String,
    key: u64,
}

#[derive(Default)]
pub(crate) struct JitState {
    compiler: Option<MethodJit>,
    cache: HashMap<u64, Entry>,
    static_ic_mru: Option<StaticIcSlot>,
    static_ic_map: HashMap<(String, String, String), u64>,
    pub stats: JitStats,
}

impl JitState {
    pub fn enable(&mut self) -> Result<(), String> {
        if self.compiler.is_none() {
            self.compiler = Some(MethodJit::new()?);
        }
        self.stats.enabled = true;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn run(
        &mut self,
        key: u64,
        code: &CodeItem,
        proto: &str,
        args: &[Value],
        fuel_remaining: u64,
    ) -> Option<i32> {
        if !self.stats.enabled || fuel_remaining == 0 {
            return None;
        }
        if self.cache.len() >= MAX_CACHE_ENTRIES && !self.cache.contains_key(&key) {
            return None;
        }
        if !matches!(
            self.cache.get(&key),
            Some(Entry::Compiled(_) | Entry::Unsupported)
        ) {
            let threshold = if has_backward_branch(code) {
                1
            } else {
                LEAF_WARMUP_CALLS
            };
            let count = match self.cache.entry(key).or_insert(Entry::Warming(0)) {
                Entry::Warming(n) => {
                    *n = n.saturating_add(1);
                    *n
                }
                _ => return None,
            };
            if count < threshold {
                return None;
            }
            let Some((prog, mut plan)) = lower_method(code, proto, true) else {
                self.cache.insert(key, Entry::Unsupported);
                self.stats.unsupported += 1;
                return None;
            };
            let handle = match self.compiler.as_mut()?.compile(&prog) {
                Ok(h) => h,
                Err(e) => {
                    self.stats.compile_errors += 1;
                    self.stats.last_error = Some(e);
                    self.cache.insert(key, Entry::Unsupported);
                    return None;
                }
            };
            plan.handle = handle;
            self.cache.insert(key, Entry::Compiled(plan));
            self.stats.compiled += 1;
        }
        let Entry::Compiled(plan) = self.cache.get(&key)? else {
            return None;
        };
        if plan.ret != RetKind::Int32 || plan.has_array_params || args.len() != plan.params.len() {
            return None;
        }
        let mut raw_args = [0u64; MAX_STACK_ARGS];
        for (i, (slot, val)) in plan.params.iter().zip(args.iter()).enumerate() {
            raw_args[i] = match (slot.kind, val) {
                (ParamKind::Int32, Value::Int(v)) => u64::from(*v as u32),
                (ParamKind::Int64, Value::Long(v)) => *v as u64,
                _ => return None,
            };
        }
        match self.compiler.as_ref()?.execute(
            plan.handle,
            &raw_args[..args.len()],
            &mut [],
            &mut [],
            fuel_remaining,
        ) {
            Ok(JitExecResult::Returned { bits, .. }) => {
                self.stats.native_calls += 1;
                Some(bits as u32 as i32)
            }
            _ => None,
        }
    }

    pub(crate) fn is_compiled(&self, key: u64) -> bool {
        matches!(self.cache.get(&key), Some(Entry::Compiled(_)))
    }

    pub(crate) fn bind_static_ic(&mut self, class: &str, method: &str, sig: &str, key: u64) {
        if let Some(mru) = &self.static_ic_mru {
            if mru.key == key && mru.class == class && mru.method == method && mru.sig == sig {
                return;
            }
        }
        let slot = StaticIcSlot {
            class: class.to_string(),
            method: method.to_string(),
            sig: sig.to_string(),
            key,
        };
        if self.static_ic_map.len() < MAX_CACHE_ENTRIES {
            self.static_ic_map.insert(
                (slot.class.clone(), slot.method.clone(), slot.sig.clone()),
                key,
            );
        }
        self.static_ic_mru = Some(slot);
    }

    fn lookup_static_ic(&self, class: &str, method: &str, sig: &str) -> Option<u64> {
        if let Some(mru) = &self.static_ic_mru {
            if mru.class == class && mru.method == method && mru.sig == sig {
                return Some(mru.key);
            }
        }
        self.static_ic_map
            .get(&(class.to_string(), method.to_string(), sig.to_string()))
            .copied()
    }
}

fn format_fuel_error(vm: &Engine, top_label: Option<&str>) -> VmExit {
    let mut stack = String::from("; pilha no esgotamento (innermost primeiro):");
    let mut shown = 0usize;
    if let Some(lbl) = top_label {
        stack.push_str("\n  at ");
        stack.push_str(lbl);
        shown += 1;
    }
    for l in vm
        .frame_labels
        .iter()
        .rev()
        .take(24usize.saturating_sub(shown))
    {
        stack.push_str("\n  at ");
        stack.push_str(l);
        shown += 1;
    }
    let total = vm.frame_labels.len() + usize::from(top_label.is_some());
    if total > shown {
        stack.push_str(&format!("\n  ... +{} frames", total - shown));
    }
    err::with_suggestion(
        err::vm_error(
            "VM_FUEL",
            format!("execução excedeu {} instruções{stack}", vm.config.fuel),
        ),
        "aumente --fuel ou revise o método executado",
    )
    .into()
}

/// Zero-allocation inline-cached fast path for `Engine::invoke_static` once a
/// static DEX method has already been resolved, class-initialized, and compiled.
pub(crate) fn try_invoke_static_fast(
    vm: &mut Engine,
    class: &str,
    method: &str,
    sig: &str,
    args: &[Value],
) -> Option<Result<Value, VmExit>> {
    if !vm.jit.stats.enabled || vm.depth >= vm.config.max_depth {
        return None;
    }
    let fuel_rem = vm.fuel_remaining();
    if fuel_rem == 0 {
        return None;
    }
    let key = vm.jit.lookup_static_ic(class, method, sig)?;
    // Scalar-only fast path (no heap array copy needed — pure stack `[u64; 32]`).
    let (handle, ret_kind, has_bailout_ops) = {
        let Some(Entry::Compiled(plan)) = vm.jit.cache.get(&key) else {
            return None;
        };
        if plan.has_array_params || plan.params.len() != args.len() {
            return None;
        }
        (plan.handle, plan.ret, plan.has_bailout_ops)
    };
    vm.coverage.insert(key);

    let mut raw_args = [0u64; MAX_STACK_ARGS];
    let mut ref_table: [ObjRef; MAX_STACK_ARGS] = [0; MAX_STACK_ARGS];
    let mut ref_len = 0usize;
    {
        let Some(Entry::Compiled(plan)) = vm.jit.cache.get(&key) else {
            return None;
        };
        for (idx, (ps, val)) in plan.params.iter().zip(args.iter()).enumerate() {
            raw_args[idx] = match (ps.kind, val) {
                (ParamKind::Int32, Value::Int(v)) => u64::from(*v as u32),
                (ParamKind::Int64, Value::Long(v)) => *v as u64,
                (ParamKind::Ref, Value::Null) => 0,
                (ParamKind::Ref, Value::Obj(r)) => {
                    let h = intern_obj_handle(&mut ref_table, &mut ref_len, *r);
                    u64::from(h)
                }
                _ => return None,
            };
        }
    }

    let exec_res = vm.jit.compiler.as_ref()?.execute(
        handle,
        &raw_args[..args.len()],
        &mut [],
        &mut [],
        fuel_rem,
    );
    match exec_res {
        Ok(JitExecResult::Returned {
            bits,
            fuel_consumed,
        }) => {
            vm.jit.stats.native_calls += 1;
            vm.fuel_used = vm.fuel_used.saturating_add(fuel_consumed);
            Some(Ok(decode_return(ret_kind, bits)))
        }
        Ok(JitExecResult::FuelExhausted) if !has_bailout_ops => {
            vm.fuel_used = vm
                .fuel_used
                .max(vm.fuel_window_start.saturating_add(vm.config.fuel))
                .saturating_add(1);
            let label = format!("{class}->{method}{sig}");
            Some(Err(format_fuel_error(vm, Some(&label))))
        }
        Ok(JitExecResult::FuelExhausted | JitExecResult::Bailout) => None,
        Err(e) => {
            vm.jit.stats.last_error = Some(e);
            None
        }
    }
}

/// Main JIT hook invoked from `Engine::call` for all static & instance DEX calls.
pub(crate) fn try_exec_method(
    vm: &mut Engine,
    dex_idx: usize,
    m: &rd_dex::methods::EncodedMethod,
    key: u64,
    code: &CodeItem,
    ins: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.jit.stats.enabled {
        return Ok(None);
    }
    let fuel_rem = vm.fuel_remaining();
    if fuel_rem == 0 {
        return Ok(None);
    }
    if vm.jit.cache.len() >= MAX_CACHE_ENTRIES && !vm.jit.cache.contains_key(&key) {
        return Ok(None);
    }

    if !matches!(vm.jit.cache.get(&key), Some(Entry::Compiled(_))) {
        let should_compile = match vm.jit.cache.entry(key).or_insert(Entry::Warming(0)) {
            Entry::Unsupported => return Ok(None),
            Entry::Compiled(_) => false,
            Entry::Warming(n) => {
                *n = n.saturating_add(1);
                // Methods containing a backward branch (real loop) compile on
                // their first call so cold heavy kernels (CRC32/hashes/sorts)
                // accelerate immediately; straight-line methods warm up at 32.
                let threshold = if has_backward_branch(code) {
                    1
                } else {
                    LEAF_WARMUP_CALLS
                };
                *n >= threshold
            }
        };
        if !should_compile {
            return Ok(None);
        }
        let mref = vm.cp.method_ref(dex_idx, m.method_idx)?;
        let is_static = m.access_flags & ACC_STATIC != 0;
        let Some((prog, plan_template)) = lower_method(code, &mref.proto, is_static) else {
            vm.jit.cache.insert(key, Entry::Unsupported);
            vm.jit.stats.unsupported += 1;
            return Ok(None);
        };
        let Some(compiler) = vm.jit.compiler.as_mut() else {
            return Ok(None);
        };
        let handle = match compiler.compile(&prog) {
            Ok(h) => h,
            Err(e) => {
                vm.jit.stats.compile_errors += 1;
                vm.jit.stats.last_error = Some(e);
                vm.jit.cache.insert(key, Entry::Unsupported);
                return Ok(None);
            }
        };
        let mut plan = plan_template;
        plan.handle = handle;
        vm.jit.cache.insert(key, Entry::Compiled(plan));
        vm.jit.stats.compiled += 1;
    }

    let (handle, ret_kind, arity, ins_size, has_array_params, has_bailout_ops, has_aput) = {
        let Some(Entry::Compiled(plan)) = vm.jit.cache.get(&key) else {
            return Ok(None);
        };
        (
            plan.handle,
            plan.ret,
            plan.params.len(),
            plan.ins_size,
            plan.has_array_params,
            plan.has_bailout_ops,
            plan.has_aput,
        )
    };
    if ins.len() != ins_size {
        return Ok(None);
    }

    let mut raw_args = [0u64; MAX_STACK_ARGS];
    if !has_array_params {
        let mut ref_table: [ObjRef; MAX_STACK_ARGS] = [0; MAX_STACK_ARGS];
        let mut ref_len = 0usize;
        let Some(Entry::Compiled(plan)) = vm.jit.cache.get(&key) else {
            return Ok(None);
        };
        for (idx, ps) in plan.params.iter().enumerate() {
            let v = &ins[ps.slot_offset];
            raw_args[idx] = match (ps.kind, v) {
                (ParamKind::Int32, Value::Int(i)) => u64::from(*i as u32),
                (ParamKind::Int64, Value::Long(l)) => {
                    if !matches!(ins.get(ps.slot_offset + 1), Some(Value::WideHi)) {
                        return Ok(None);
                    }
                    *l as u64
                }
                (ParamKind::Ref, Value::Null) => 0,
                (ParamKind::Ref, Value::Obj(r)) => {
                    u64::from(intern_obj_handle(&mut ref_table, &mut ref_len, *r))
                }
                _ => return Ok(None),
            };
        }
        let Some(compiler) = vm.jit.compiler.as_ref() else {
            return Ok(None);
        };
        match compiler.execute(handle, &raw_args[..arity], &mut [], &mut [], fuel_rem) {
            Ok(JitExecResult::Returned {
                bits,
                fuel_consumed,
            }) => {
                vm.jit.stats.native_calls += 1;
                vm.fuel_used = vm.fuel_used.saturating_add(fuel_consumed);
                return Ok(Some(decode_return(ret_kind, bits)));
            }
            Ok(JitExecResult::FuelExhausted) if !has_bailout_ops => {
                vm.fuel_used = vm
                    .fuel_used
                    .max(vm.fuel_window_start.saturating_add(vm.config.fuel))
                    .saturating_add(1);
                let label = vm.method_label(dex_idx, m);
                return Err(format_fuel_error(vm, Some(&label)));
            }
            Ok(JitExecResult::FuelExhausted | JitExecResult::Bailout) => return Ok(None),
            Err(e) => {
                vm.jit.stats.last_error = Some(e);
                return Ok(None);
            }
        }
    }

    // Transactional primitive array marshaling with unified `ObjRef` identity
    // across `ArrI32`, `ArrI64`, and `Ref` (#249):
    // Pass 1 assigns handles `1..=arr_ref_len` (`<= MAX_JIT_ARRAY_HANDLES`) to
    // array parameters so `slices_i32` / `slices_i64` stay densely indexed by
    // `handle - 1` without exceeding Cranelift's 16-handle table.
    // Pass 2 interns `ParamKind::Ref` parameters into the SAME `ref_table`,
    // reusing `1..=arr_ref_len` when an `Object` aliases an array parameter
    // (`same(arr, arr) == true`) and assigning `arr_ref_len + 1..` otherwise
    // (`eq(int[], long[]) == false`).
    let mut scratch_i32: Vec<Option<(ObjRef, Vec<i32>)>> = Vec::new();
    let mut scratch_i64: Vec<Option<(ObjRef, Vec<i64>)>> = Vec::new();
    let mut has_i32_handle = false;
    let mut has_i64_handle = false;
    let mut ref_table: [ObjRef; MAX_STACK_ARGS] = [0; MAX_STACK_ARGS];
    let mut ref_len = 0usize;
    {
        let Some(Entry::Compiled(plan)) = vm.jit.cache.get(&key) else {
            return Ok(None);
        };
        for (idx, ps) in plan.params.iter().enumerate() {
            let v = &ins[ps.slot_offset];
            raw_args[idx] = match (ps.kind, v) {
                (ParamKind::Int32, Value::Int(i)) => u64::from(*i as u32),
                (ParamKind::Int64, Value::Long(l)) => {
                    if !matches!(ins.get(ps.slot_offset + 1), Some(Value::WideHi)) {
                        return Ok(None);
                    }
                    *l as u64
                }
                (ParamKind::ArrI32 | ParamKind::ArrI64 | ParamKind::Ref, Value::Null) => 0,
                (ParamKind::Ref, Value::Obj(_)) => 0,
                (ParamKind::ArrI32, Value::Obj(r)) => {
                    if ps.arr_desc.is_none() || vm.heap.class_of(*r).ok() != ps.arr_desc {
                        return Ok(None);
                    }
                    let h = intern_obj_handle(&mut ref_table, &mut ref_len, *r) as usize;
                    if h == 0 || h > MAX_JIT_ARRAY_HANDLES {
                        return Ok(None);
                    }
                    if scratch_i32.len() < h {
                        scratch_i32.resize_with(h, || None);
                    }
                    if scratch_i32[h - 1].is_none() {
                        let Ok((elems, ElemKind::Int)) = vm.heap.as_array_elems(*r) else {
                            return Ok(None);
                        };
                        if elems.len() > MAX_ARRAY_ELEMS {
                            return Ok(None);
                        }
                        let mut buf = Vec::with_capacity(elems.len());
                        for el in elems {
                            let Value::Int(iv) = el else {
                                return Ok(None);
                            };
                            buf.push(*iv);
                        }
                        scratch_i32[h - 1] = Some((*r, buf));
                    }
                    has_i32_handle = true;
                    h as u64
                }
                (ParamKind::ArrI64, Value::Obj(r)) => {
                    if ps.arr_desc != Some("[J") || vm.heap.class_of(*r).ok() != Some("[J") {
                        return Ok(None);
                    }
                    let h = intern_obj_handle(&mut ref_table, &mut ref_len, *r) as usize;
                    if h == 0 || h > MAX_JIT_ARRAY_HANDLES {
                        return Ok(None);
                    }
                    if scratch_i64.len() < h {
                        scratch_i64.resize_with(h, || None);
                    }
                    if scratch_i64[h - 1].is_none() {
                        let Ok((elems, ElemKind::Long)) = vm.heap.as_array_elems(*r) else {
                            return Ok(None);
                        };
                        if elems.len() > MAX_ARRAY_ELEMS {
                            return Ok(None);
                        }
                        let mut buf = Vec::with_capacity(elems.len());
                        for el in elems {
                            let Value::Long(lv) = el else {
                                return Ok(None);
                            };
                            buf.push(*lv);
                        }
                        scratch_i64[h - 1] = Some((*r, buf));
                    }
                    has_i64_handle = true;
                    h as u64
                }
                _ => return Ok(None),
            };
        }
        for (idx, ps) in plan.params.iter().enumerate() {
            if ps.kind == ParamKind::Ref {
                if let Value::Obj(r) = &ins[ps.slot_offset] {
                    raw_args[idx] = u64::from(intern_obj_handle(&mut ref_table, &mut ref_len, *r));
                }
            }
        }
    }

    let exec_res = {
        let mut slices_i32: Vec<&mut [i32]> = if has_i32_handle {
            scratch_i32
                .iter_mut()
                .map(|slot| match slot {
                    Some((_, v)) => v.as_mut_slice(),
                    None => &mut [][..],
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut slices_i64: Vec<&mut [i64]> = if has_i64_handle {
            scratch_i64
                .iter_mut()
                .map(|slot| match slot {
                    Some((_, v)) => v.as_mut_slice(),
                    None => &mut [][..],
                })
                .collect()
        } else {
            Vec::new()
        };
        let Some(compiler) = vm.jit.compiler.as_ref() else {
            return Ok(None);
        };
        compiler.execute(
            handle,
            &raw_args[..arity],
            &mut slices_i32,
            &mut slices_i64,
            fuel_rem,
        )
    };

    match exec_res {
        Ok(JitExecResult::Returned {
            bits,
            fuel_consumed,
        }) => {
            if has_aput {
                for (obj_ref, vals) in scratch_i32.into_iter().flatten() {
                    let elems = vm.heap.array_elems_mut(obj_ref)?;
                    for (dst, src) in elems.iter_mut().zip(vals) {
                        *dst = Value::Int(src);
                    }
                }
                for (obj_ref, vals) in scratch_i64.into_iter().flatten() {
                    let elems = vm.heap.array_elems_mut(obj_ref)?;
                    for (dst, src) in elems.iter_mut().zip(vals) {
                        *dst = Value::Long(src);
                    }
                }
            }
            vm.jit.stats.native_calls += 1;
            vm.fuel_used = vm.fuel_used.saturating_add(fuel_consumed);
            Ok(Some(decode_return(ret_kind, bits)))
        }
        Ok(JitExecResult::FuelExhausted | JitExecResult::Bailout) => {
            // Uncommitted scratch buffers drop cleanly; `Engine::call` replays
            // in the interpreter to materialize exact `NPE` / `AIOOBE` / `VM_FUEL`.
            Ok(None)
        }
        Err(e) => {
            vm.jit.stats.last_error = Some(e);
            Ok(None)
        }
    }
}

fn intern_obj_handle(table: &mut [ObjRef; MAX_STACK_ARGS], len: &mut usize, r: ObjRef) -> u32 {
    for (idx, existing) in table[..*len].iter().enumerate() {
        if *existing == r {
            return (idx + 1) as u32;
        }
    }
    let idx = *len;
    if idx < MAX_STACK_ARGS {
        table[idx] = r;
        *len += 1;
    }
    (idx + 1) as u32
}

fn decode_return(ret: RetKind, bits: u64) -> Value {
    match ret {
        RetKind::Void => Value::Null,
        RetKind::Int32 => Value::Int(bits as u32 as i32),
        RetKind::Int64 => Value::Long(bits as i64),
    }
}

fn has_backward_branch(code: &CodeItem) -> bool {
    for (pc, insn) in &code.instructions {
        let off: Option<i64> = match (&insn.kind, insn.opcode) {
            (Kind::Branch8(o), 0x28) => Some(i64::from(*o)),
            (Kind::Branch16(o), 0x29) => Some(i64::from(*o)),
            (Kind::Branch32(o), 0x2a) => Some(i64::from(*o)),
            (Kind::RegRegBranch16(_, _, o), 0x32..=0x37) => Some(i64::from(*o)),
            (Kind::RegBranch16(_, o), 0x38..=0x3d) => Some(i64::from(*o)),
            _ => None,
        };
        if let Some(d) = off {
            if d <= 0 && (*pc as i64) + d >= 0 {
                return true;
            }
        }
    }
    false
}

// ── Dalvik Bytecode -> Verified Multi-Block `JitProgram` Lowering ───────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum VmSlot {
    Uninit,
    Zero,
    Int32,
    LongLo,
    LongHi,
    Ref,
    ArrI32(ArrElem32),
    ArrI64,
}

impl VmSlot {
    fn merge(self, other: Self) -> Self {
        if self == other {
            return self;
        }
        match (self, other) {
            (VmSlot::Zero, VmSlot::Int32) | (VmSlot::Int32, VmSlot::Zero) => VmSlot::Int32,
            (VmSlot::Zero, VmSlot::ArrI32(e)) | (VmSlot::ArrI32(e), VmSlot::Zero) => {
                VmSlot::ArrI32(e)
            }
            (VmSlot::Zero, VmSlot::ArrI64) | (VmSlot::ArrI64, VmSlot::Zero) => VmSlot::ArrI64,
            (VmSlot::Zero, VmSlot::Ref) | (VmSlot::Ref, VmSlot::Zero) => VmSlot::Ref,
            (
                VmSlot::Ref | VmSlot::ArrI32(_) | VmSlot::ArrI64,
                VmSlot::Ref | VmSlot::ArrI32(_) | VmSlot::ArrI64,
            ) => VmSlot::Ref,
            _ => VmSlot::Uninit,
        }
    }
}

fn vm_write_single(state: &mut [VmSlot], dst: usize, kind: VmSlot) -> Option<()> {
    if dst >= state.len() {
        return None;
    }
    if dst > 0 && state[dst - 1] == VmSlot::LongLo {
        state[dst - 1] = VmSlot::Uninit;
    }
    if state[dst] == VmSlot::LongLo && dst + 1 < state.len() && state[dst + 1] == VmSlot::LongHi {
        state[dst + 1] = VmSlot::Uninit;
    }
    state[dst] = kind;
    Some(())
}

fn vm_write_wide(state: &mut [VmSlot], dst: usize) -> Option<()> {
    if dst + 1 >= state.len() {
        return None;
    }
    if dst > 0 && state[dst - 1] == VmSlot::LongLo {
        state[dst - 1] = VmSlot::Uninit;
    }
    if state[dst + 1] == VmSlot::LongLo && dst + 2 < state.len() && state[dst + 2] == VmSlot::LongHi
    {
        state[dst + 2] = VmSlot::Uninit;
    }
    state[dst] = VmSlot::LongLo;
    state[dst + 1] = VmSlot::LongHi;
    Some(())
}

type ProtoParam = (ParamKind, Option<&'static str>);

fn parse_proto(proto: &str, is_static: bool) -> Option<(Vec<ProtoParam>, RetKind, usize)> {
    let (params_str, ret_str) = proto.strip_prefix('(')?.split_once(')')?;
    let mut kinds = Vec::new();
    let mut slots = 0usize;
    if !is_static {
        kinds.push((ParamKind::Ref, None));
        slots += 1;
    }
    let bytes = params_str.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'Z' | b'B' | b'S' | b'C' | b'I' => {
                kinds.push((ParamKind::Int32, None));
                slots += 1;
                i += 1;
            }
            b'J' => {
                kinds.push((ParamKind::Int64, None));
                slots += 2;
                i += 1;
            }
            b'F' | b'D' => return None,
            b'L' => {
                let end = params_str[i..].find(';')?;
                kinds.push((ParamKind::Ref, None));
                slots += 1;
                i += end + 1;
            }
            b'[' => {
                let start = i;
                while i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                }
                if i >= bytes.len() {
                    return None;
                }
                if bytes[i] == b'L' {
                    let end = params_str[i..].find(';')?;
                    i += end + 1;
                } else {
                    i += 1;
                }
                let desc = &params_str[start..i];
                let pk = match desc {
                    "[Z" => (ParamKind::ArrI32, Some("[Z")),
                    "[B" => (ParamKind::ArrI32, Some("[B")),
                    "[S" => (ParamKind::ArrI32, Some("[S")),
                    "[C" => (ParamKind::ArrI32, Some("[C")),
                    "[I" => (ParamKind::ArrI32, Some("[I")),
                    "[J" => (ParamKind::ArrI64, Some("[J")),
                    _ => (ParamKind::Ref, None),
                };
                kinds.push(pk);
                slots += 1;
            }
            _ => return None,
        }
    }
    let ret = match ret_str {
        "V" => RetKind::Void,
        "Z" | "B" | "S" | "C" | "I" => RetKind::Int32,
        "J" => RetKind::Int64,
        _ => return None,
    };
    Some((kinds, ret, slots))
}

fn lookup_insn_idx(insns: &[(usize, Insn)], cur_pc: usize, rel: i64) -> Option<usize> {
    let target = (cur_pc as i64).checked_add(rel)?;
    if target < 0 {
        return None;
    }
    let idx = insns
        .binary_search_by_key(&(target as usize), |(pc, _)| *pc)
        .ok()?;
    if matches!(insns[idx].1.kind, Kind::Payload(_)) {
        return None;
    }
    Some(idx)
}

fn lookup_payload(insns: &[(usize, Insn)], cur_pc: usize, rel: i64) -> Option<&Payload> {
    let target = (cur_pc as i64).checked_add(rel)?;
    if target < 0 {
        return None;
    }
    let idx = insns
        .binary_search_by_key(&(target as usize), |(pc, _)| *pc)
        .ok()?;
    match &insns[idx].1.kind {
        Kind::Payload(p) => Some(p),
        _ => None,
    }
}

fn bin32_op(op: u8) -> Option<BinOp32> {
    Some(match op {
        0x90 => BinOp32::Add,
        0x91 => BinOp32::Sub,
        0x92 => BinOp32::Mul,
        0x95 => BinOp32::And,
        0x96 => BinOp32::Or,
        0x97 => BinOp32::Xor,
        0x98 => BinOp32::Shl,
        0x99 => BinOp32::Shr,
        0x9a => BinOp32::Ushr,
        _ => return None,
    })
}

fn bin64_op(op: u8) -> Option<BinOp64> {
    Some(match op {
        0x9b => BinOp64::Add,
        0x9c => BinOp64::Sub,
        0x9d => BinOp64::Mul,
        0xa0 => BinOp64::And,
        0xa1 => BinOp64::Or,
        0xa2 => BinOp64::Xor,
        _ => return None,
    })
}

fn shift64_op(op: u8) -> Option<ShiftOp64> {
    Some(match op {
        0xa3 => ShiftOp64::Shl,
        0xa4 => ShiftOp64::Shr,
        0xa5 => ShiftOp64::Ushr,
        _ => return None,
    })
}

fn is_terminator(op: u8) -> bool {
    matches!(op, 0x0e..=0x10 | 0x28..=0x2c | 0x32..=0x3d)
}

#[allow(clippy::too_many_lines)]
fn lower_method(
    code: &CodeItem,
    proto: &str,
    is_static: bool,
) -> Option<(JitProgram, CompiledPlan)> {
    let nregs = code.registers_size as usize;
    let ins_size = code.ins_size as usize;
    if !code.tries.is_empty()
        || nregs == 0
        || nregs > MAX_METHOD_REGS
        || ins_size > nregs
        || code.instructions.is_empty()
        || code.instructions.len() > MAX_METHOD_INSNS
    {
        return None;
    }
    let (param_kinds, ret_kind, counted_slots) = parse_proto(proto, is_static)?;
    if counted_slots != ins_size || param_kinds.len() > MAX_STACK_ARGS {
        return None;
    }
    if matches!(code.instructions[0].1.kind, Kind::Payload(_)) {
        return None;
    }

    let first_param_reg = nregs - ins_size;
    let mut jit_params = Vec::with_capacity(param_kinds.len());
    let mut plan_params = Vec::with_capacity(param_kinds.len());
    let mut entry_slots = vec![VmSlot::Uninit; nregs];
    let mut has_array_params = false;
    let mut cur_slot = 0usize;
    for (kind, arr_desc) in param_kinds {
        let reg = first_param_reg + cur_slot;
        jit_params.push((reg, kind));
        plan_params.push(ParamSlot {
            slot_offset: cur_slot,
            kind,
            arr_desc,
        });
        match kind {
            ParamKind::Int32 => {
                entry_slots[reg] = VmSlot::Int32;
                cur_slot += 1;
            }
            ParamKind::Int64 => {
                entry_slots[reg] = VmSlot::LongLo;
                entry_slots[reg + 1] = VmSlot::LongHi;
                cur_slot += 2;
            }
            ParamKind::Ref => {
                entry_slots[reg] = VmSlot::Ref;
                cur_slot += 1;
            }
            ParamKind::ArrI32 => {
                let elem = match arr_desc? {
                    "[Z" => ArrElem32::Bool,
                    "[B" => ArrElem32::Byte,
                    "[S" => ArrElem32::Short,
                    "[C" => ArrElem32::Char,
                    "[I" => ArrElem32::Int,
                    _ => return None,
                };
                entry_slots[reg] = VmSlot::ArrI32(elem);
                has_array_params = true;
                cur_slot += 1;
            }
            ParamKind::ArrI64 => {
                entry_slots[reg] = VmSlot::ArrI64;
                has_array_params = true;
                cur_slot += 1;
            }
        }
    }

    // Step 1: Discover basic-block leader instruction indices across `code.instructions`.
    let n_insns = code.instructions.len();
    let mut leaders = BTreeSet::new();
    leaders.insert(0usize);
    for (idx, (pc, insn)) in code.instructions.iter().enumerate() {
        if matches!(insn.kind, Kind::Payload(_)) {
            leaders.insert(idx);
            continue;
        }
        match (&insn.kind, insn.opcode) {
            (Kind::Plain, 0x0e) | (Kind::Reg(_), 0x0f | 0x10) => {
                if idx + 1 < n_insns {
                    leaders.insert(idx + 1);
                }
            }
            (Kind::Branch8(o), 0x28) => {
                leaders.insert(lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?);
                if idx + 1 < n_insns {
                    leaders.insert(idx + 1);
                }
            }
            (Kind::Branch16(o), 0x29) => {
                leaders.insert(lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?);
                if idx + 1 < n_insns {
                    leaders.insert(idx + 1);
                }
            }
            (Kind::Branch32(o), 0x2a) => {
                leaders.insert(lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?);
                if idx + 1 < n_insns {
                    leaders.insert(idx + 1);
                }
            }
            (Kind::RegRegBranch16(_, _, o), 0x32..=0x37)
            | (Kind::RegBranch16(_, o), 0x38..=0x3d) => {
                leaders.insert(lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?);
                if idx + 1 >= n_insns
                    || matches!(code.instructions[idx + 1].1.kind, Kind::Payload(_))
                {
                    return None;
                }
                leaders.insert(idx + 1);
            }
            (Kind::RegBranch32(_, o), 0x2b | 0x2c) => {
                let payload = lookup_payload(&code.instructions, *pc, i64::from(*o))?;
                match (insn.opcode, payload) {
                    (0x2b, Payload::PackedSwitch { targets, .. })
                    | (0x2c, Payload::SparseSwitch { targets, .. }) => {
                        for &t in targets {
                            leaders.insert(lookup_insn_idx(&code.instructions, *pc, i64::from(t))?);
                        }
                    }
                    _ => return None,
                }
                if idx + 1 >= n_insns
                    || matches!(code.instructions[idx + 1].1.kind, Kind::Payload(_))
                {
                    return None;
                }
                leaders.insert(idx + 1);
            }
            _ => {}
        }
    }

    let leader_vec: Vec<usize> = leaders.into_iter().collect();
    let mut start_to_raw_bb = HashMap::with_capacity(leader_vec.len());
    let mut raw_ranges = Vec::with_capacity(leader_vec.len());
    for (bb_i, &start) in leader_vec.iter().enumerate() {
        let end = leader_vec.get(bb_i + 1).copied().unwrap_or(n_insns);
        start_to_raw_bb.insert(start, bb_i);
        raw_ranges.push((start, end));
    }

    // Step 2: BFS reachability + monotone type propagation from raw block 0.
    // Unreachable trailing switch payloads / dead code blocks are pruned cleanly.
    let raw_succs = |raw_bb: usize| -> Option<Vec<usize>> {
        let (start, end) = raw_ranges[raw_bb];
        if start >= end || matches!(code.instructions[start].1.kind, Kind::Payload(_)) {
            return None;
        }
        for (_, insn) in &code.instructions[start..end - 1] {
            if matches!(insn.kind, Kind::Payload(_)) || is_terminator(insn.opcode) {
                return None;
            }
        }
        let last_idx = end - 1;
        let (pc, last) = &code.instructions[last_idx];
        if matches!(last.kind, Kind::Payload(_)) {
            return None;
        }
        let mut succs = Vec::new();
        match (&last.kind, last.opcode) {
            (Kind::Plain, 0x0e) | (Kind::Reg(_), 0x0f | 0x10) => {}
            (Kind::Branch8(o), 0x28) => {
                let t = lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?;
                succs.push(*start_to_raw_bb.get(&t)?);
            }
            (Kind::Branch16(o), 0x29) => {
                let t = lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?;
                succs.push(*start_to_raw_bb.get(&t)?);
            }
            (Kind::Branch32(o), 0x2a) => {
                let t = lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?;
                succs.push(*start_to_raw_bb.get(&t)?);
            }
            (Kind::RegRegBranch16(_, _, o), 0x32..=0x37)
            | (Kind::RegBranch16(_, o), 0x38..=0x3d) => {
                let t = lookup_insn_idx(&code.instructions, *pc, i64::from(*o))?;
                succs.push(*start_to_raw_bb.get(&t)?);
                succs.push(*start_to_raw_bb.get(&(last_idx + 1))?);
            }
            (Kind::RegBranch32(_, o), 0x2b | 0x2c) => {
                let payload = lookup_payload(&code.instructions, *pc, i64::from(*o))?;
                match (last.opcode, payload) {
                    (0x2b, Payload::PackedSwitch { targets, .. })
                    | (0x2c, Payload::SparseSwitch { targets, .. }) => {
                        for &rel in targets {
                            let t = lookup_insn_idx(&code.instructions, *pc, i64::from(rel))?;
                            succs.push(*start_to_raw_bb.get(&t)?);
                        }
                    }
                    _ => return None,
                }
                succs.push(*start_to_raw_bb.get(&(last_idx + 1))?);
            }
            _ => {
                // Fallthrough into next contiguous block.
                if last_idx + 1 >= n_insns {
                    return None;
                }
                succs.push(*start_to_raw_bb.get(&(last_idx + 1))?);
            }
        }
        Some(succs)
    };

    let mut reachable_order = Vec::new();
    let mut raw_to_dense: HashMap<usize, usize> = HashMap::new();
    let mut bfs = VecDeque::new();
    raw_to_dense.insert(0, 0);
    reachable_order.push(0usize);
    bfs.push_back(0usize);
    while let Some(rbb) = bfs.pop_front() {
        for s in raw_succs(rbb)? {
            if let std::collections::hash_map::Entry::Vacant(e) = raw_to_dense.entry(s) {
                let dense = reachable_order.len();
                if dense >= 256 {
                    return None;
                }
                e.insert(dense);
                reachable_order.push(s);
                bfs.push_back(s);
            }
        }
    }

    let dense_cnt = reachable_order.len();
    let mut in_states: Vec<Option<Vec<VmSlot>>> = vec![None; dense_cnt];
    in_states[0] = Some(entry_slots);
    let mut work = VecDeque::new();
    work.push_back(0usize);

    let step_insn_types = |state: &mut [VmSlot], insn: &Insn| -> Option<()> {
        match (&insn.kind, insn.opcode) {
            (Kind::Plain, 0x00 | 0x0e) | (Kind::Reg(_), 0x0f | 0x10) => Some(()),
            (Kind::Branch8(_), 0x28)
            | (Kind::Branch16(_), 0x29)
            | (Kind::Branch32(_), 0x2a)
            | (Kind::RegRegBranch16(_, _, _), 0x32..=0x37)
            | (Kind::RegBranch16(_, _), 0x38..=0x3d)
            | (Kind::RegBranch32(_, _), 0x2b | 0x2c) => Some(()),
            (Kind::Regs(a, b), 0x01) => {
                let s = *state.get(*b as usize)?;
                vm_write_single(state, *a as usize, s)
            }
            (Kind::RegReg16(a, b), 0x02 | 0x03) => {
                let s = *state.get(*b as usize)?;
                vm_write_single(state, *a as usize, s)
            }
            (Kind::Regs(a, _), 0x04) => vm_write_wide(state, *a as usize),
            (Kind::RegReg16(a, _), 0x05 | 0x06) => vm_write_wide(state, *a as usize),
            (Kind::Regs(a, b), 0x07) => {
                let s = *state.get(*b as usize)?;
                vm_write_single(state, *a as usize, s)
            }
            (Kind::RegReg16(a, b), 0x08 | 0x09) => {
                let s = *state.get(*b as usize)?;
                vm_write_single(state, *a as usize, s)
            }
            (Kind::RegLit4(a, v), 0x12) => vm_write_single(
                state,
                *a as usize,
                if *v == 0 { VmSlot::Zero } else { VmSlot::Int32 },
            ),
            (Kind::RegLit16(a, v), 0x13) => vm_write_single(
                state,
                *a as usize,
                if *v == 0 { VmSlot::Zero } else { VmSlot::Int32 },
            ),
            (Kind::RegLit32(a, v), 0x14) => vm_write_single(
                state,
                *a as usize,
                if *v == 0 { VmSlot::Zero } else { VmSlot::Int32 },
            ),
            (Kind::RegHigh16(a, v), 0x15) => vm_write_single(
                state,
                *a as usize,
                if *v == 0 { VmSlot::Zero } else { VmSlot::Int32 },
            ),
            (Kind::RegLit16(a, _), 0x16)
            | (Kind::RegLit32(a, _), 0x17)
            | (Kind::Lit64(a, _), 0x18)
            | (Kind::RegHigh16(a, _), 0x19) => vm_write_wide(state, *a as usize),
            (Kind::Regs(a, _), 0x21) => vm_write_single(state, *a as usize, VmSlot::Int32),
            (Kind::RegRegReg(a, _, _), 0x31) => vm_write_single(state, *a as usize, VmSlot::Int32),
            (Kind::RegRegReg(a, _, _), 0x44 | 0x47 | 0x48 | 0x49 | 0x4a) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            (Kind::RegRegReg(a, _, _), 0x45) => vm_write_wide(state, *a as usize),
            (Kind::RegRegReg(_, _, _), 0x4b | 0x4c | 0x4e | 0x4f | 0x50 | 0x51) => Some(()),
            (Kind::Regs(a, _), 0x7b | 0x7c | 0x84 | 0x8d | 0x8e | 0x8f) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            (Kind::Regs(a, _), 0x7d | 0x7e | 0x81) => vm_write_wide(state, *a as usize),
            (Kind::RegRegReg(a, _, _), 0x90..=0x92 | 0x95..=0x9a) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            (Kind::RegRegReg(a, _, _), 0x9b..=0x9d | 0xa0..=0xa5) => {
                vm_write_wide(state, *a as usize)
            }
            (Kind::Regs(a, _), 0xb0..=0xb2 | 0xb5..=0xba) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            (Kind::Regs(a, _), 0xbb..=0xbd | 0xc0..=0xc5) => vm_write_wide(state, *a as usize),
            (Kind::RegRegLit16(a, _, _), 0xd0..=0xd7) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            (Kind::RegRegLit8(a, _, _), 0xd8..=0xe2) => {
                vm_write_single(state, *a as usize, VmSlot::Int32)
            }
            _ => None,
        }
    };

    while let Some(dbb) = work.pop_front() {
        let mut st = in_states[dbb].clone()?;
        let rbb = reachable_order[dbb];
        let (start, end) = raw_ranges[rbb];
        for (_, insn) in &code.instructions[start..end] {
            step_insn_types(&mut st, insn)?;
        }
        for raw_succ in raw_succs(rbb)? {
            let dsucc = *raw_to_dense.get(&raw_succ)?;
            match &mut in_states[dsucc] {
                None => {
                    in_states[dsucc] = Some(st.clone());
                    work.push_back(dsucc);
                }
                Some(existing) => {
                    let mut changed = false;
                    for (slot, &inc) in existing.iter_mut().zip(st.iter()) {
                        let m = slot.merge(inc);
                        if m != *slot {
                            *slot = m;
                            changed = true;
                        }
                    }
                    if changed {
                        work.push_back(dsucc);
                    }
                }
            }
        }
    }

    // Step 3: Emit `JitBlock`s using resolved register types & dense block IDs.
    let mut blocks = Vec::with_capacity(dense_cnt);
    let mut has_bailout_ops = false;
    let mut has_aput = false;
    let map_target = |cur_pc: usize, rel: i64| -> Option<usize> {
        let idx = lookup_insn_idx(&code.instructions, cur_pc, rel)?;
        let rbb = *start_to_raw_bb.get(&idx)?;
        raw_to_dense.get(&rbb).copied()
    };

    for (dbb, &rbb) in reachable_order.iter().enumerate() {
        let (start, end) = raw_ranges[rbb];
        let mut st = in_states[dbb].clone()?;
        let mut insts = Vec::with_capacity(end - start);
        let dalvik_insns = (end - start) as u32;
        let mut term_opt: Option<JitTerm> = None;

        for (pos, (pc, insn)) in code.instructions[start..end].iter().enumerate() {
            let is_last = start + pos == end - 1;
            match (&insn.kind, insn.opcode) {
                (Kind::Plain, 0x00) => {}
                (Kind::Regs(a, b), 0x01) => {
                    if matches!(
                        st.get(*b as usize),
                        Some(VmSlot::Ref | VmSlot::ArrI32(_) | VmSlot::ArrI64)
                    ) {
                        insts.push(JitInst::MoveRef {
                            dst: *a as usize,
                            src: *b as usize,
                        });
                    } else {
                        insts.push(JitInst::Move32 {
                            dst: *a as usize,
                            src: *b as usize,
                        });
                    }
                }
                (Kind::RegReg16(a, b), 0x02 | 0x03) => {
                    if matches!(
                        st.get(*b as usize),
                        Some(VmSlot::Ref | VmSlot::ArrI32(_) | VmSlot::ArrI64)
                    ) {
                        insts.push(JitInst::MoveRef {
                            dst: *a as usize,
                            src: *b as usize,
                        });
                    } else {
                        insts.push(JitInst::Move32 {
                            dst: *a as usize,
                            src: *b as usize,
                        });
                    }
                }
                (Kind::Regs(a, b), 0x04) => insts.push(JitInst::Move64 {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::RegReg16(a, b), 0x05 | 0x06) => insts.push(JitInst::Move64 {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::Regs(a, b), 0x07) => insts.push(JitInst::MoveRef {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::RegReg16(a, b), 0x08 | 0x09) => insts.push(JitInst::MoveRef {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::Plain, 0x0e) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::ReturnVoid);
                }
                (Kind::Reg(a), 0x0f) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::Return32 { src: *a as usize });
                }
                (Kind::Reg(a), 0x10) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::Return64 { src: *a as usize });
                }
                (Kind::RegLit4(a, v), 0x12) => insts.push(JitInst::Const32 {
                    dst: *a as usize,
                    value: i32::from(*v),
                }),
                (Kind::RegLit16(a, v), 0x13) => insts.push(JitInst::Const32 {
                    dst: *a as usize,
                    value: i32::from(*v),
                }),
                (Kind::RegLit32(a, v), 0x14) => insts.push(JitInst::Const32 {
                    dst: *a as usize,
                    value: *v,
                }),
                (Kind::RegHigh16(a, v), 0x15) => insts.push(JitInst::Const32 {
                    dst: *a as usize,
                    value: (u32::from(*v) << 16) as i32,
                }),
                (Kind::RegLit16(a, v), 0x16) => insts.push(JitInst::Const64 {
                    dst: *a as usize,
                    value: i64::from(*v),
                }),
                (Kind::RegLit32(a, v), 0x17) => insts.push(JitInst::Const64 {
                    dst: *a as usize,
                    value: i64::from(*v),
                }),
                (Kind::Lit64(a, v), 0x18) => insts.push(JitInst::Const64 {
                    dst: *a as usize,
                    value: *v,
                }),
                (Kind::RegHigh16(a, v), 0x19) => insts.push(JitInst::Const64 {
                    dst: *a as usize,
                    value: (u64::from(*v) << 48) as i64,
                }),
                (Kind::Regs(a, b), 0x21) => {
                    let is_i64 = match st.get(*b as usize)? {
                        VmSlot::ArrI64 => true,
                        VmSlot::ArrI32(_) | VmSlot::Zero => false,
                        _ => return None,
                    };
                    has_bailout_ops = true;
                    insts.push(JitInst::ArrayLen {
                        dst: *a as usize,
                        arr: *b as usize,
                        is_i64,
                    });
                }
                (Kind::Branch8(o), 0x28) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::Jump {
                        target: map_target(*pc, i64::from(*o))?,
                    });
                }
                (Kind::Branch16(o), 0x29) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::Jump {
                        target: map_target(*pc, i64::from(*o))?,
                    });
                }
                (Kind::Branch32(o), 0x2a) => {
                    if !is_last {
                        return None;
                    }
                    term_opt = Some(JitTerm::Jump {
                        target: map_target(*pc, i64::from(*o))?,
                    });
                }
                (Kind::RegBranch32(a, o), 0x2b) => {
                    if !is_last {
                        return None;
                    }
                    let Payload::PackedSwitch { first_key, targets } =
                        lookup_payload(&code.instructions, *pc, i64::from(*o))?
                    else {
                        return None;
                    };
                    let mut mapped = Vec::with_capacity(targets.len());
                    for &rel in targets {
                        mapped.push(map_target(*pc, i64::from(rel))?);
                    }
                    let def_raw = *start_to_raw_bb.get(&end)?;
                    let default_bb = *raw_to_dense.get(&def_raw)?;
                    term_opt = Some(JitTerm::PackedSwitch {
                        src: *a as usize,
                        first_key: *first_key,
                        targets: mapped,
                        default_bb,
                    });
                }
                (Kind::RegBranch32(a, o), 0x2c) => {
                    if !is_last {
                        return None;
                    }
                    let Payload::SparseSwitch { keys, targets } =
                        lookup_payload(&code.instructions, *pc, i64::from(*o))?
                    else {
                        return None;
                    };
                    if keys.len() != targets.len() {
                        return None;
                    }
                    let mut cases = Vec::with_capacity(keys.len());
                    for (&k, &rel) in keys.iter().zip(targets.iter()) {
                        cases.push((k, map_target(*pc, i64::from(rel))?));
                    }
                    let def_raw = *start_to_raw_bb.get(&end)?;
                    let default_bb = *raw_to_dense.get(&def_raw)?;
                    term_opt = Some(JitTerm::SparseSwitch {
                        src: *a as usize,
                        cases,
                        default_bb,
                    });
                }
                (Kind::RegRegReg(a, b, c), 0x31) => insts.push(JitInst::CmpLong {
                    dst: *a as usize,
                    lhs: *b as usize,
                    rhs: *c as usize,
                }),
                (Kind::RegRegBranch16(a, b, o), 0x32 | 0x33) => {
                    if !is_last {
                        return None;
                    }
                    let then_bb = map_target(*pc, i64::from(*o))?;
                    let else_bb = *raw_to_dense.get(start_to_raw_bb.get(&end)?)?;
                    term_opt = Some(JitTerm::BranchEq {
                        eq: insn.opcode == 0x32,
                        lhs: *a as usize,
                        rhs: *b as usize,
                        then_bb,
                        else_bb,
                    });
                }
                (Kind::RegRegBranch16(a, b, o), 0x34..=0x37) => {
                    if !is_last {
                        return None;
                    }
                    let cond = match insn.opcode {
                        0x34 => IntCond::Lt,
                        0x35 => IntCond::Ge,
                        0x36 => IntCond::Gt,
                        _ => IntCond::Le,
                    };
                    let then_bb = map_target(*pc, i64::from(*o))?;
                    let else_bb = *raw_to_dense.get(start_to_raw_bb.get(&end)?)?;
                    term_opt = Some(JitTerm::BranchCmp32 {
                        cond,
                        lhs: *a as usize,
                        rhs: *b as usize,
                        then_bb,
                        else_bb,
                    });
                }
                (Kind::RegBranch16(a, o), 0x38 | 0x39) => {
                    if !is_last {
                        return None;
                    }
                    let then_bb = map_target(*pc, i64::from(*o))?;
                    let else_bb = *raw_to_dense.get(start_to_raw_bb.get(&end)?)?;
                    term_opt = Some(JitTerm::BranchZero {
                        eq: insn.opcode == 0x38,
                        src: *a as usize,
                        then_bb,
                        else_bb,
                    });
                }
                (Kind::RegBranch16(a, o), 0x3a..=0x3d) => {
                    if !is_last {
                        return None;
                    }
                    let cond = match insn.opcode {
                        0x3a => IntCond::Lt,
                        0x3b => IntCond::Ge,
                        0x3c => IntCond::Gt,
                        _ => IntCond::Le,
                    };
                    let then_bb = map_target(*pc, i64::from(*o))?;
                    let else_bb = *raw_to_dense.get(start_to_raw_bb.get(&end)?)?;
                    term_opt = Some(JitTerm::BranchCmpZero32 {
                        cond,
                        src: *a as usize,
                        then_bb,
                        else_bb,
                    });
                }
                (Kind::RegRegReg(a, b, c), 0x44 | 0x47 | 0x48 | 0x49 | 0x4a) => {
                    if !matches!(st.get(*b as usize), Some(VmSlot::ArrI32(_) | VmSlot::Zero)) {
                        return None;
                    }
                    has_bailout_ops = true;
                    insts.push(JitInst::ArrayGet32 {
                        dst: *a as usize,
                        arr: *b as usize,
                        idx: *c as usize,
                    });
                }
                (Kind::RegRegReg(a, b, c), 0x45) => {
                    if !matches!(st.get(*b as usize), Some(VmSlot::ArrI64 | VmSlot::Zero)) {
                        return None;
                    }
                    has_bailout_ops = true;
                    insts.push(JitInst::ArrayGet64 {
                        dst: *a as usize,
                        arr: *b as usize,
                        idx: *c as usize,
                    });
                }
                (Kind::RegRegReg(a, b, c), 0x4b | 0x4e | 0x4f | 0x50 | 0x51) => {
                    let elem = match insn.opcode {
                        0x4b => ArrElem32::Int,
                        0x4e => ArrElem32::Bool,
                        0x4f => ArrElem32::Byte,
                        0x50 => ArrElem32::Char,
                        _ => ArrElem32::Short,
                    };
                    match st.get(*b as usize)? {
                        VmSlot::Zero | VmSlot::ArrI32(ArrElem32::Int) => {}
                        VmSlot::ArrI32(slot_elem) if *slot_elem == elem => {}
                        _ => return None,
                    }
                    has_bailout_ops = true;
                    has_aput = true;
                    insts.push(JitInst::ArrayPut32 {
                        src: *a as usize,
                        arr: *b as usize,
                        idx: *c as usize,
                        elem,
                    });
                }
                (Kind::RegRegReg(a, b, c), 0x4c) => {
                    if !matches!(st.get(*b as usize), Some(VmSlot::ArrI64 | VmSlot::Zero)) {
                        return None;
                    }
                    has_bailout_ops = true;
                    has_aput = true;
                    insts.push(JitInst::ArrayPut64 {
                        src: *a as usize,
                        arr: *b as usize,
                        idx: *c as usize,
                    });
                }
                (Kind::Regs(a, b), 0x7b | 0x7c | 0x8d | 0x8e | 0x8f) => {
                    let op = match insn.opcode {
                        0x7b => UnOp32::Neg,
                        0x7c => UnOp32::Not,
                        0x8d => UnOp32::Byte,
                        0x8e => UnOp32::Char,
                        _ => UnOp32::Short,
                    };
                    insts.push(JitInst::Unary32 {
                        dst: *a as usize,
                        src: *b as usize,
                        op,
                    });
                }
                (Kind::Regs(a, b), 0x7d | 0x7e) => {
                    let op = if insn.opcode == 0x7d {
                        UnOp64::Neg
                    } else {
                        UnOp64::Not
                    };
                    insts.push(JitInst::Unary64 {
                        dst: *a as usize,
                        src: *b as usize,
                        op,
                    });
                }
                (Kind::Regs(a, b), 0x81) => insts.push(JitInst::Int32ToInt64 {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::Regs(a, b), 0x84) => insts.push(JitInst::Int64ToInt32 {
                    dst: *a as usize,
                    src: *b as usize,
                }),
                (Kind::RegRegReg(a, b, c), 0x90..=0x9a) => insts.push(JitInst::Binary32 {
                    dst: *a as usize,
                    lhs: *b as usize,
                    rhs: *c as usize,
                    op: bin32_op(insn.opcode)?,
                }),
                (Kind::RegRegReg(a, b, c), 0x9b..=0xa2) => insts.push(JitInst::Binary64 {
                    dst: *a as usize,
                    lhs: *b as usize,
                    rhs: *c as usize,
                    op: bin64_op(insn.opcode)?,
                }),
                (Kind::RegRegReg(a, b, c), 0xa3..=0xa5) => insts.push(JitInst::Shift64 {
                    dst: *a as usize,
                    lhs: *b as usize,
                    amt: *c as usize,
                    op: shift64_op(insn.opcode)?,
                }),
                (Kind::Regs(a, b), 0xb0..=0xba) => insts.push(JitInst::Binary32 {
                    dst: *a as usize,
                    lhs: *a as usize,
                    rhs: *b as usize,
                    op: bin32_op(insn.opcode - 0x20)?,
                }),
                (Kind::Regs(a, b), 0xbb..=0xc2) => insts.push(JitInst::Binary64 {
                    dst: *a as usize,
                    lhs: *a as usize,
                    rhs: *b as usize,
                    op: bin64_op(insn.opcode - 0x20)?,
                }),
                (Kind::Regs(a, b), 0xc3..=0xc5) => insts.push(JitInst::Shift64 {
                    dst: *a as usize,
                    lhs: *a as usize,
                    amt: *b as usize,
                    op: shift64_op(insn.opcode - 0x20)?,
                }),
                (Kind::RegRegLit16(a, b, lit), 0xd3 | 0xd4) => {
                    if *lit == 0 {
                        return None;
                    }
                    insts.push(JitInst::DivRemLit32 {
                        dst: *a as usize,
                        src: *b as usize,
                        lit: i32::from(*lit),
                        is_rem: insn.opcode == 0xd4,
                    });
                }
                (Kind::RegRegLit16(a, b, lit), 0xd0..=0xd2 | 0xd5..=0xd7) => {
                    let (op, rev) = match insn.opcode {
                        0xd0 => (BinOp32::Add, false),
                        0xd1 => (BinOp32::Sub, true),
                        0xd2 => (BinOp32::Mul, false),
                        0xd5 => (BinOp32::And, false),
                        0xd6 => (BinOp32::Or, false),
                        _ => (BinOp32::Xor, false),
                    };
                    insts.push(JitInst::Binary32Imm {
                        dst: *a as usize,
                        src: *b as usize,
                        imm: i32::from(*lit),
                        op,
                        reverse_sub: rev,
                    });
                }
                (Kind::RegRegLit8(a, b, lit), 0xdb | 0xdc) => {
                    if *lit == 0 {
                        return None;
                    }
                    insts.push(JitInst::DivRemLit32 {
                        dst: *a as usize,
                        src: *b as usize,
                        lit: i32::from(*lit),
                        is_rem: insn.opcode == 0xdc,
                    });
                }
                (Kind::RegRegLit8(a, b, lit), 0xd8..=0xda | 0xdd..=0xe2) => {
                    let (op, rev) = match insn.opcode {
                        0xd8 => (BinOp32::Add, false),
                        0xd9 => (BinOp32::Sub, true),
                        0xda => (BinOp32::Mul, false),
                        0xdd => (BinOp32::And, false),
                        0xde => (BinOp32::Or, false),
                        0xdf => (BinOp32::Xor, false),
                        0xe0 => (BinOp32::Shl, false),
                        0xe1 => (BinOp32::Shr, false),
                        _ => (BinOp32::Ushr, false),
                    };
                    insts.push(JitInst::Binary32Imm {
                        dst: *a as usize,
                        src: *b as usize,
                        imm: i32::from(*lit),
                        op,
                        reverse_sub: rev,
                    });
                }
                _ => return None,
            }
            step_insn_types(&mut st, insn)?;
        }

        let term = match term_opt {
            Some(t) => t,
            None => {
                let next_raw = *start_to_raw_bb.get(&end)?;
                let next_dense = *raw_to_dense.get(&next_raw)?;
                JitTerm::Jump { target: next_dense }
            }
        };
        blocks.push(JitBlock {
            dalvik_insns,
            insts,
            term,
        });
    }

    let prog = JitProgram {
        registers: nregs,
        params: jit_params,
        ret: ret_kind,
        blocks,
    };
    prog.validate().ok()?;

    let plan = CompiledPlan {
        handle: 0,
        ret: ret_kind,
        params: plan_params,
        ins_size,
        has_array_params,
        has_bailout_ops,
        has_aput,
    };
    Some((prog, plan))
}
