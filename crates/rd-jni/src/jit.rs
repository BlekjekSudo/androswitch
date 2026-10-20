//! Narrow, validated Cranelift integer leaf compiler (ADR 007).
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlags, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

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
            || self.parameters > 32
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
        let builder = JITBuilder::new(cranelift_module::default_libcall_names())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            module: Some(JITModule::new(builder)),
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
                // Validation guarantees every load is within the checked args slice.
                regs[leaf.registers - leaf.parameters + i] =
                    b.ins()
                        .load(types::I32, MemFlags::trusted(), args, (i * 4) as i32);
            }
            for op in &leaf.ops {
                match *op {
                    Op::Const { dst, value } => {
                        regs[dst] = b.ins().iconst(types::I32, i64::from(value as u32))
                    }
                    Op::Move { dst, src } => regs[dst] = regs[src],
                    Op::Binary { dst, lhs, rhs, op } => {
                        let (x, mut y) = (regs[lhs], regs[rhs]);
                        if matches!(op, Binary::Shl | Binary::Shr | Binary::Ushr) {
                            y = b.ins().band_imm(y, 31);
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
                            Unary::Char => b.ins().band_imm(x, 65535),
                        };
                    }
                }
            }
            b.ins().return_(&[regs[leaf.result]]);
            b.finalize();
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
}
