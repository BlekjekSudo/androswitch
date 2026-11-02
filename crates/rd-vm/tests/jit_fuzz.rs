#![cfg(feature = "jit")]
//! Deterministic hostile-IR and differential Dalvik bytecode fuzzer for `MethodJit`.
//!
//! Exercises thousands of seeded adversarial IR graphs, multi-block CFG/loop/
//! `long`/`switch`/`div-rem` programs, transactional primitive-array kernels
//! (including aliasing, `null`, negative/OOB indices, and mid-loop `VM_FUEL`
//! rollbacks), and corrupted instruction streams against the reference
//! interpreter. Override `RD_JIT_FUZZ_SCALE` (default `1`) for extended runs.

mod common;

use common::*;
use rd_jni::jit::{
    ArrElem32, BinOp32, BinOp64, IntCond, JitBlock, JitExecResult, JitInst, JitProgram, JitTerm,
    MethodJit, ParamKind, RetKind, ShiftOp64, UnOp32, UnOp64,
};
use rd_vm::engine::{Engine, VmConfig};
use rd_vm::err::VmExit;
use rd_vm::heap::ElemKind;
use rd_vm::Value;

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn usize(&mut self, bound: usize) -> usize {
        if bound <= 1 {
            0
        } else {
            (self.next_u64() as usize) % bound
        }
    }
    fn bool(&mut self) -> bool {
        (self.next_u64() & 1) != 0
    }
    fn edge_i32(&mut self) -> i32 {
        const EDGES: &[i32] = &[
            0,
            1,
            -1,
            2,
            -2,
            7,
            -8,
            31,
            32,
            63,
            64,
            127,
            -128,
            32767,
            -32768,
            65535,
            i32::MAX,
            i32::MIN,
            i32::MIN + 1,
        ];
        if self.usize(4) < 3 {
            EDGES[self.usize(EDGES.len())]
        } else {
            self.next_u64() as i32
        }
    }
    fn edge_i64(&mut self) -> i64 {
        const EDGES: &[i64] = &[
            0,
            1,
            -1,
            63,
            64,
            -64,
            i32::MAX as i64,
            i32::MIN as i64,
            u32::MAX as i64,
            i64::MAX,
            i64::MIN,
            i64::MIN + 1,
        ];
        if self.usize(4) < 3 {
            EDGES[self.usize(EDGES.len())]
        } else {
            self.next_u64() as i64
        }
    }
}

fn fuzz_scale() -> usize {
    std::env::var("RD_JIT_FUZZ_SCALE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(1)
}

fn op21t(op: u8, a: u8, off: i16) -> Vec<u16> {
    vec![(op as u16) | ((a as u16) << 8), off as u16]
}

#[test]
fn hostile_jit_ir_verifier_and_codegen_never_panic_or_corrupt_memory() {
    let total = 6_000 * fuzz_scale();
    let mut rng = Rng::new(0xA07_2026_1025);
    let mut jit = MethodJit::new().unwrap();
    let mut compiled_ok = 0usize;

    let bin32 = [
        BinOp32::Add,
        BinOp32::Sub,
        BinOp32::Mul,
        BinOp32::And,
        BinOp32::Or,
        BinOp32::Xor,
        BinOp32::Shl,
        BinOp32::Shr,
        BinOp32::Ushr,
    ];
    let bin64 = [
        BinOp64::Add,
        BinOp64::Sub,
        BinOp64::Mul,
        BinOp64::And,
        BinOp64::Or,
        BinOp64::Xor,
    ];
    let sh64 = [ShiftOp64::Shl, ShiftOp64::Shr, ShiftOp64::Ushr];
    let un32 = [
        UnOp32::Neg,
        UnOp32::Not,
        UnOp32::Byte,
        UnOp32::Short,
        UnOp32::Char,
    ];
    let conds = [
        IntCond::Eq,
        IntCond::Ne,
        IntCond::Lt,
        IntCond::Ge,
        IntCond::Gt,
        IntCond::Le,
    ];
    let elems = [
        ArrElem32::Int,
        ArrElem32::Bool,
        ArrElem32::Byte,
        ArrElem32::Char,
        ArrElem32::Short,
    ];

    for case_idx in 0..total {
        // Alternate between completely wild hostile IR and semi-structured IR
        // so both verifier rejections AND Cranelift codegen/execution paths
        // get pounded hard.
        let structured = case_idx % 2 == 0;
        let registers = if structured {
            6 + rng.usize(6)
        } else {
            rng.usize(270)
        };
        let ret = match rng.usize(3) {
            0 => RetKind::Void,
            1 => RetKind::Int32,
            _ => RetKind::Int64,
        };
        let mut params = Vec::new();
        if structured {
            params.push((registers - 4, ParamKind::Int32));
            params.push((registers - 3, ParamKind::ArrI32));
            params.push((registers - 2, ParamKind::Int64));
        } else {
            let pcount = rng.usize(8);
            for _ in 0..pcount {
                let kind = match rng.usize(5) {
                    0 => ParamKind::Int32,
                    1 => ParamKind::Int64,
                    2 => ParamKind::Ref,
                    3 => ParamKind::ArrI32,
                    _ => ParamKind::ArrI64,
                };
                params.push((rng.usize(registers.saturating_add(4)), kind));
            }
        }

        let n_blocks = if structured {
            1 + rng.usize(5)
        } else {
            rng.usize(8)
        };
        let reg_pick = |r: &mut Rng| -> usize {
            if structured {
                r.usize(registers)
            } else {
                r.usize(registers.saturating_add(3))
            }
        };
        let bb_pick = |r: &mut Rng| -> usize {
            if structured && n_blocks > 0 {
                r.usize(n_blocks)
            } else {
                r.usize(n_blocks.saturating_add(2))
            }
        };

        let mut blocks = Vec::with_capacity(n_blocks);
        for bb in 0..n_blocks {
            let mut insts = Vec::new();
            if structured && bb == 0 {
                // Seed deterministic valid lanes in block 0 so many structured
                // programs pass validation and exercise native execution!
                insts.push(JitInst::Const32 {
                    dst: 0,
                    value: rng.edge_i32(),
                });
                insts.push(JitInst::Const64 {
                    dst: 2,
                    value: rng.edge_i64(),
                });
            }
            let n_insts = rng.usize(8);
            for _ in 0..n_insts {
                let ins = match rng.usize(19) {
                    0 => JitInst::Const32 {
                        dst: reg_pick(&mut rng),
                        value: rng.edge_i32(),
                    },
                    1 => JitInst::Const64 {
                        dst: reg_pick(&mut rng),
                        value: rng.edge_i64(),
                    },
                    2 => JitInst::Move32 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                    },
                    3 => JitInst::Move64 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                    },
                    4 => JitInst::MoveRef {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                    },
                    5 => JitInst::Unary32 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                        op: un32[rng.usize(un32.len())],
                    },
                    6 => JitInst::Unary64 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                        op: if rng.bool() { UnOp64::Neg } else { UnOp64::Not },
                    },
                    7 => JitInst::Int32ToInt64 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                    },
                    8 => JitInst::Int64ToInt32 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                    },
                    9 => JitInst::Binary32 {
                        dst: reg_pick(&mut rng),
                        lhs: reg_pick(&mut rng),
                        rhs: reg_pick(&mut rng),
                        op: bin32[rng.usize(bin32.len())],
                    },
                    10 => JitInst::Binary32Imm {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                        imm: rng.edge_i32(),
                        op: bin32[rng.usize(bin32.len())],
                        reverse_sub: rng.bool(),
                    },
                    11 => JitInst::DivRemLit32 {
                        dst: reg_pick(&mut rng),
                        src: reg_pick(&mut rng),
                        lit: rng.edge_i32(),
                        is_rem: rng.bool(),
                    },
                    12 => JitInst::Binary64 {
                        dst: reg_pick(&mut rng),
                        lhs: reg_pick(&mut rng),
                        rhs: reg_pick(&mut rng),
                        op: bin64[rng.usize(bin64.len())],
                    },
                    13 => JitInst::Shift64 {
                        dst: reg_pick(&mut rng),
                        lhs: reg_pick(&mut rng),
                        amt: reg_pick(&mut rng),
                        op: sh64[rng.usize(sh64.len())],
                    },
                    14 => JitInst::CmpLong {
                        dst: reg_pick(&mut rng),
                        lhs: reg_pick(&mut rng),
                        rhs: reg_pick(&mut rng),
                    },
                    15 => JitInst::ArrayLen {
                        dst: reg_pick(&mut rng),
                        arr: if structured {
                            registers - 3
                        } else {
                            reg_pick(&mut rng)
                        },
                        is_i64: rng.bool(),
                    },
                    16 => JitInst::ArrayGet32 {
                        dst: reg_pick(&mut rng),
                        arr: if structured {
                            registers - 3
                        } else {
                            reg_pick(&mut rng)
                        },
                        idx: reg_pick(&mut rng),
                    },
                    17 => JitInst::ArrayPut32 {
                        src: reg_pick(&mut rng),
                        arr: if structured {
                            registers - 3
                        } else {
                            reg_pick(&mut rng)
                        },
                        idx: reg_pick(&mut rng),
                        elem: elems[rng.usize(elems.len())],
                    },
                    _ => JitInst::ArrayGet64 {
                        dst: reg_pick(&mut rng),
                        arr: reg_pick(&mut rng),
                        idx: reg_pick(&mut rng),
                    },
                };
                insts.push(ins);
            }

            let term = if structured && bb + 1 == n_blocks {
                match ret {
                    RetKind::Void => JitTerm::ReturnVoid,
                    RetKind::Int32 => JitTerm::Return32 { src: 0 },
                    RetKind::Int64 => JitTerm::Return64 { src: 2 },
                }
            } else {
                match rng.usize(9) {
                    0 => JitTerm::Jump {
                        target: if structured {
                            (bb + 1) % n_blocks
                        } else {
                            bb_pick(&mut rng)
                        },
                    },
                    1 => JitTerm::BranchEq {
                        eq: rng.bool(),
                        lhs: reg_pick(&mut rng),
                        rhs: reg_pick(&mut rng),
                        then_bb: bb_pick(&mut rng),
                        else_bb: bb_pick(&mut rng),
                    },
                    2 => JitTerm::BranchCmp32 {
                        cond: conds[rng.usize(conds.len())],
                        lhs: reg_pick(&mut rng),
                        rhs: reg_pick(&mut rng),
                        then_bb: bb_pick(&mut rng),
                        else_bb: bb_pick(&mut rng),
                    },
                    3 => JitTerm::BranchZero {
                        eq: rng.bool(),
                        src: reg_pick(&mut rng),
                        then_bb: bb_pick(&mut rng),
                        else_bb: bb_pick(&mut rng),
                    },
                    4 => JitTerm::BranchCmpZero32 {
                        cond: conds[rng.usize(conds.len())],
                        src: reg_pick(&mut rng),
                        then_bb: bb_pick(&mut rng),
                        else_bb: bb_pick(&mut rng),
                    },
                    5 => {
                        let len = rng.usize(6);
                        let mut targets = Vec::with_capacity(len);
                        for _ in 0..len {
                            targets.push(bb_pick(&mut rng));
                        }
                        JitTerm::PackedSwitch {
                            src: reg_pick(&mut rng),
                            first_key: rng.edge_i32(),
                            targets,
                            default_bb: bb_pick(&mut rng),
                        }
                    }
                    6 => {
                        let len = rng.usize(6);
                        let mut cases = Vec::with_capacity(len);
                        let mut key = -20i32;
                        for _ in 0..len {
                            if structured {
                                key = key.saturating_add(1 + rng.usize(9) as i32);
                                cases.push((key, bb_pick(&mut rng)));
                            } else {
                                cases.push((rng.edge_i32(), bb_pick(&mut rng)));
                            }
                        }
                        JitTerm::SparseSwitch {
                            src: reg_pick(&mut rng),
                            cases,
                            default_bb: bb_pick(&mut rng),
                        }
                    }
                    7 => JitTerm::ReturnVoid,
                    _ => {
                        if ret == RetKind::Int64 {
                            JitTerm::Return64 {
                                src: reg_pick(&mut rng),
                            }
                        } else {
                            JitTerm::Return32 {
                                src: reg_pick(&mut rng),
                            }
                        }
                    }
                }
            };

            let dalvik_insns = if structured {
                (insts.len() as u32).saturating_add(1)
            } else {
                rng.usize(5) as u32
            };
            blocks.push(JitBlock {
                dalvik_insns,
                insts,
                term,
            });
        }

        let prog = JitProgram {
            registers,
            params: params.clone(),
            ret,
            blocks,
        };
        if compiled_ok >= 3_500 {
            jit = MethodJit::new().unwrap();
            compiled_ok = 0;
        }
        if let Ok(h) = jit.compile(&prog) {
            compiled_ok += 1;
            // Execute with hostile arguments: null handles (0), out-of-range
            // handles (99), valid handles (1) to empty and small slices, and
            // tight fuel budgets!
            for trial in 0..3 {
                let mut a32_0: [i32; 0] = [];
                let mut a32_1 = [rng.edge_i32(), rng.edge_i32(), rng.edge_i32()];
                let mut a64_1 = [rng.edge_i64(), rng.edge_i64()];
                let mut args = Vec::with_capacity(params.len());
                for &(_, kind) in &params {
                    let v = match kind {
                        ParamKind::Int32 => rng.edge_i32() as u32 as u64,
                        ParamKind::Int64 => rng.edge_i64() as u64,
                        ParamKind::Ref => (trial as u64) * 5,
                        ParamKind::ArrI32 | ParamKind::ArrI64 => match trial {
                            0 => 0,  // null
                            1 => 1,  // valid slot 1
                            _ => 99, // bogus handle -> must bail out cleanly!
                        },
                    };
                    args.push(v);
                }
                let fuel = match trial {
                    0 => 0,
                    1 => 32,
                    _ => 256,
                };
                let res = if trial == 0 {
                    let mut s32: [&mut [i32]; 1] = [&mut a32_0];
                    jit.execute(h, &args, &mut s32, &mut [], fuel).unwrap()
                } else {
                    let mut s32: [&mut [i32]; 1] = [&mut a32_1];
                    let mut s64: [&mut [i64]; 1] = [&mut a64_1];
                    jit.execute(h, &args, &mut s32, &mut s64, fuel).unwrap()
                };
                if let JitExecResult::Returned { fuel_consumed, .. } = res {
                    assert!(fuel_consumed <= fuel);
                }
            }
        }
    }
}

fn assert_vm_outcome_eq(
    seed: usize,
    trial: usize,
    ires: &Result<Value, VmExit>,
    jres: &Result<Value, VmExit>,
    ifuel: u64,
    jfuel: u64,
) {
    match (ires, jres) {
        (Ok(iv), Ok(jv)) => {
            assert_eq!(jv, iv, "seed={seed} trial={trial}: return value mismatch");
            assert_eq!(
                jfuel, ifuel,
                "seed={seed} trial={trial}: fuel mismatch on Ok({iv:?})"
            );
        }
        (Err(VmExit::Exception(it)), Err(VmExit::Exception(jt))) => {
            assert_eq!(
                jt.class, it.class,
                "seed={seed} trial={trial}: exception class mismatch ({jt:?} vs {it:?})"
            );
        }
        (Err(VmExit::Error(ie)), Err(VmExit::Error(je))) => {
            assert_eq!(
                je.code, ie.code,
                "seed={seed} trial={trial}: error code mismatch ({je:?} vs {ie:?})"
            );
            if ie.code == "VM_FUEL" {
                assert_eq!(
                    jfuel, ifuel,
                    "seed={seed} trial={trial}: fuel mismatch on VM_FUEL"
                );
            }
        }
        _ => panic!(
            "seed={seed} trial={trial}: outcome kind mismatch: interp={ires:?} (fuel={ifuel}) vs jit={jres:?} (fuel={jfuel})"
        ),
    }
}

/// Synthesize a random multi-block Dalvik `(IJI)J` method combining:
/// - backward loop with fuel/iteration guard (`if-ge` / `goto`),
/// - forward conditional branch (`if-eq..if-le` / `if-eqz..if-lez`),
/// - 32-bit ALU, shifts, narrowing, `rsub-int`, literal `div/rem` (incl. -1, 0, MIN),
/// - 64-bit `long` `const-wide`, 3-reg & `/2addr` ALU, `shl/shr/ushr-long`, `cmp-long`,
///   `int-to-long` / `long-to-int`, and optional `packed-switch` payload.
fn gen_random_cfg_long_method(rng: &mut Rng) -> Vec<u16> {
    // Registers (10 total, 4 ins):
    //   v0, v1: 32-bit scratch
    //   v2..v3, v4..v5: 64-bit scratch pairs
    //   v6: param0 (int x)
    //   v7..v8: param1 (long y)
    //   v9: param2 (int limit)
    let mut header = Vec::new();
    header.extend(op11n(0x12, 0, 0)); // v0 = 0 (loop counter)
    header.extend(op21s(0x13, 1, (rng.edge_i32() & 0x7fff) as i16)); // v1 = seed32
    header.extend(op12x(0x04, 2, 7)); // move-wide v2, v7 (acc64 = y)
    header.extend(op51l(0x18, 4, rng.edge_i64())); // const-wide v4, C

    let mut body = Vec::new();
    let n_ops = 4 + rng.usize(8);
    for _ in 0..n_ops {
        match rng.usize(11) {
            0 => {
                // 32-bit binary op on v1, v6, v0
                let op = [0x90u8, 0x91, 0x92, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a][rng.usize(9)];
                let lhs = [0u8, 1, 6, 9][rng.usize(4)];
                let rhs = [0u8, 1, 6, 9][rng.usize(4)];
                body.extend(op23x(op, 1, lhs, rhs));
            }
            1 => {
                // 32-bit literal op (`add/rsub/mul/and/or/xor/shl/shr/ushr-int/lit8`)
                let op = [0xd8u8, 0xd9, 0xda, 0xdd, 0xde, 0xdf, 0xe0, 0xe1, 0xe2][rng.usize(9)];
                let src = [0u8, 1, 6, 9][rng.usize(4)];
                body.extend(op22b(op, 1, src, rng.edge_i32() as i8));
            }
            2 => {
                // Literal div/rem (occasionally 0 or -1 to test ArithmeticException & MIN/-1!)
                let op = if rng.bool() { 0xdb } else { 0xdc };
                let divs: [i8; 7] = [-1, 1, 2, -3, 7, -128, 0];
                let d = if rng.usize(8) == 0 {
                    0
                } else {
                    divs[rng.usize(6)]
                };
                body.extend(op22b(op, 1, 6, d));
            }
            3 => {
                // Narrowing / unary 32-bit (`neg`, `not`, `int-to-byte/char/short`)
                let op = [0x7bu8, 0x7c, 0x8d, 0x8e, 0x8f][rng.usize(5)];
                body.extend(op12x(op, 1, 1));
            }
            4 => {
                // 64-bit binary (`add/sub/mul/and/or/xor-long`)
                let op = [0x9bu8, 0x9c, 0x9d, 0xa0, 0xa1, 0xa2][rng.usize(6)];
                let rhs = [2u8, 4, 7][rng.usize(3)];
                body.extend(op23x(op, 2, 2, rhs));
            }
            5 => {
                // 64-bit shift (`shl/shr/ushr-long` with 32-bit shift count in v1 or v6!)
                let op = [0xa3u8, 0xa4, 0xa5][rng.usize(3)];
                let amt = if rng.bool() { 1 } else { 6 };
                body.extend(op23x(op, 2, 2, amt));
            }
            6 => {
                // `cmp-long v1, v2, v4`
                body.extend(op23x(0x31, 1, 2, 4));
            }
            7 => {
                // `int-to-long v4, v1` then `xor-long/2addr v2, v4`
                body.extend(op12x(0x81, 4, 1));
                body.extend(op12x(0xc2, 2, 4));
            }
            8 => {
                // `long-to-int v1, v2`
                body.extend(op12x(0x84, 1, 2));
            }
            9 => {
                // Forward short diamond: `if-<cond>z v1, +3` skipping 2 units
                let cond_op = 0x38u8 + (rng.usize(6) as u8);
                body.extend(op21t(cond_op, 1, 4)); // len 2 -> +4 jumps over next 2-unit insn
                body.extend(op22b(0xd8, 1, 1, 13)); // len 2
            }
            _ => {
                // Forward reg-reg compare diamond: `if-<cond> v1, v6, +3`
                let cond_op = 0x32u8 + (rng.usize(6) as u8);
                body.extend(op22t(cond_op, 1, 6, 3)); // len 2 -> +3 jumps over 1-unit insn
                body.extend(op12x(0x7d, 2, 2)); // neg-long v2, v2 (len 1)
            }
        }
    }

    // Assemble loop structure with exact unit offsets:
    //   header
    //   loop_top: if-ge v0, v9, exit_pc
    //   body
    //   add-int/lit8 v0, v0, 1 (len 2)
    //   goto/16 loop_top (len 2: op20t = [0x29, rel16])
    //   exit: int-to-long v4, v1 (len 1); add-long/2addr v2, v4 (len 1); return-wide v2 (len 1)
    let mut code = header;
    let loop_top = code.len();
    let exit_rel = 2 + body.len() + 2 + 2; // if-ge(2) + body + inc(2) + goto/16(2)
    code.extend(op22t(0x35, 0, 9, exit_rel as i16)); // if-ge v0, v9 -> exit
    code.extend(body);
    code.extend(op22b(0xd8, 0, 0, 1)); // v0 += 1
    let goto_pc = code.len();
    let back_rel = (loop_top as isize) - (goto_pc as isize);
    code.push(0x29); // goto/16
    code.push((back_rel as i16) as u16);
    code.extend(op12x(0x81, 4, 1)); // int-to-long v4, v1
    code.extend(op12x(0xbb, 2, 4)); // add-long/2addr v2, v4
    code.extend(op11x(0x10, 2)); // return-wide v2
    code
}

#[test]
fn differential_fuzz_random_cfg_long_and_divrem_programs() {
    let n_methods = 350 * fuzz_scale();
    let mut rng = Rng::new(0xD1FF_CF60_2026);
    let mut b = DexBuilder::new();
    let c = b.class("LFuzzCfg;", "Ljava/lang/Object;");
    for m_idx in 0..n_methods {
        let words = gen_random_cfg_long_method(&mut rng);
        let code = b.code(10, 4, 0, words);
        b.direct(
            c,
            &format!("m{m_idx}"),
            "J",
            vec!["I", "J", "I"],
            ACC_PUBLIC | ACC_STATIC,
            Some(code),
        );
    }
    let dex_bytes = b.finish();

    for m_idx in 0..n_methods {
        let name = format!("m{m_idx}");
        for trial in 0..6 {
            let x = rng.edge_i32();
            let y = rng.edge_i64();
            // Include negative, 0, small loop counts, and fuel-exhausting counts!
            let limit = match trial {
                0 => 0,
                1 => 1,
                2 => 5,
                3 => 19,
                4 => -3,
                _ => 500, // exceeds tight fuel budget -> tests deterministic VM_FUEL!
            };
            let fuel = if trial == 5 { 90 } else { 50_000 };
            let cfg = VmConfig {
                fuel,
                ..VmConfig::default()
            };

            let dex1 = rd_dex::Dex::parse_unverified(dex_bytes.clone()).unwrap();
            let mut interp = Engine::new(vec![dex1], cfg.clone());
            let dex2 = rd_dex::Dex::parse_unverified(dex_bytes.clone()).unwrap();
            let mut jit = Engine::new(vec![dex2], cfg);
            jit.enable_jit().unwrap();

            let args = [Value::Int(x), Value::Long(y), Value::Int(limit)];
            let ires = interp.invoke_static("LFuzzCfg;", &name, "(IJI)J", &args);
            let jres = jit.invoke_static("LFuzzCfg;", &name, "(IJI)J", &args);
            assert_vm_outcome_eq(
                m_idx,
                trial,
                &ires,
                &jres,
                interp.fuel_used(),
                jit.fuel_used(),
            );
        }
    }
}

/// Synthesize a random array kernel `([I[II)I` exercising `array-length`,
/// `aget`, `aput`, aliasing (`a == b`), negative/OOB indices, `null`, and
/// transactional rollback on mid-loop exceptions or `VM_FUEL`.
fn gen_random_array_method(rng: &mut Rng) -> Vec<u16> {
    // Registers (7 total, 3 ins):
    //   v0: acc (int), v1: i (int), v2: scratch, v3: len
    //   v4: int[] a, v5: int[] b, v6: int step_limit
    let mut body = Vec::new();
    body.extend(op23x(0x44, 2, 4, 1)); // aget v2, v4, v1
    let mut_op = [0xd8u8, 0xd9, 0xda, 0xdd, 0xde, 0xdf][rng.usize(6)];
    body.extend(op22b(mut_op, 2, 2, (rng.edge_i32() as i8) | 1));
    body.extend(op23x(0x4b, 2, 4, 1)); // aput v2, v4, v1 (mutates `a[i]` BEFORE reading `b[idx2]`!)

    // Compute index into `b`: either `i` or `i + offset` (which may go OOB mid-loop!)
    let off = [-1i8, 0, 0, 1, 2][rng.usize(5)];
    body.extend(op22b(0xd8, 3, 1, off)); // v3 = i + off
    body.extend(op23x(0x44, 2, 5, 3)); // aget v2, v5, v3 (may throw AIOOBE or NPE after `a` was mutated!)
    body.extend(op12x(0xb0, 0, 2)); // acc += v2
    if rng.bool() {
        body.extend(op23x(0x4b, 0, 5, 3)); // aput acc, v5, v3
    }

    let mut code = Vec::new();
    code.extend(op11n(0x12, 0, 0)); // acc = 0
    code.extend(op11n(0x12, 1, 0)); // i = 0
    if rng.bool() {
        // Touch `a.length` up front (tests NPE on null `a`)
        code.extend(op12x(0x21, 0, 4)); // array-length v0, v4
    }
    let loop_top = code.len();
    let exit_rel = 2 + body.len() + 2 + 2;
    code.extend(op22t(0x35, 1, 6, exit_rel as i16)); // if-ge i, step_limit -> exit
    code.extend(body);
    code.extend(op22b(0xd8, 1, 1, 1)); // i += 1
    let goto_pc = code.len();
    let back_rel = (loop_top as isize) - (goto_pc as isize);
    code.push(0x29);
    code.push((back_rel as i16) as u16);
    code.extend(op11x(0x0f, 0)); // return acc
    code
}

#[test]
fn differential_fuzz_random_array_aliasing_oob_npe_and_rollback_programs() {
    let n_methods = 200 * fuzz_scale();
    let mut rng = Rng::new(0xA22A_77F0_2026);
    let mut b = DexBuilder::new();
    let c = b.class("LFuzzArr;", "Ljava/lang/Object;");
    for m_idx in 0..n_methods {
        let words = gen_random_array_method(&mut rng);
        let code = b.code(7, 3, 0, words);
        b.direct(
            c,
            &format!("arr{m_idx}"),
            "I",
            vec!["[I", "[I", "I"],
            ACC_PUBLIC | ACC_STATIC,
            Some(code),
        );
    }
    let dex_bytes = b.finish();

    let alloc_arr = |vm: &mut Engine, vals: &[i32]| -> u32 {
        let r = vm
            .heap
            .alloc_array(ElemKind::Int, vals.len(), "[I")
            .unwrap();
        vm.heap
            .array_elems_mut(r)
            .unwrap()
            .extend(vals.iter().copied().map(Value::Int));
        r
    };

    for m_idx in 0..n_methods {
        let name = format!("arr{m_idx}");
        let seed_a = [
            rng.edge_i32(),
            rng.edge_i32(),
            rng.edge_i32(),
            rng.edge_i32(),
            rng.edge_i32(),
        ];
        let seed_b = [
            rng.edge_i32(),
            rng.edge_i32(),
            rng.edge_i32(),
            rng.edge_i32(),
        ];

        // 6 adversarial scenarios per method:
        //   0: distinct arrays, in-bounds limit=3
        //   1: distinct arrays, OOB mid-loop limit=5 (triggers AIOOBE after partial writes!)
        //   2: ALIASED same array `(a, a)`, limit=4
        //   3: `a = null` (NPE)
        //   4: `a` valid, `b = null`, limit=2 (NPE AFTER `a[0]` write -> tests rollback!)
        //   5: tight fuel=14 mid-loop VM_FUEL after partial writes!
        for trial in 0..6 {
            let fuel = if trial == 5 { 14 } else { 20_000 };
            let cfg = VmConfig {
                fuel,
                ..VmConfig::default()
            };
            let dex1 = rd_dex::Dex::parse_unverified(dex_bytes.clone()).unwrap();
            let mut interp = Engine::new(vec![dex1], cfg.clone());
            let dex2 = rd_dex::Dex::parse_unverified(dex_bytes.clone()).unwrap();
            let mut jit = Engine::new(vec![dex2], cfg);
            jit.enable_jit().unwrap();

            let ia = alloc_arr(&mut interp, &seed_a);
            let ib = alloc_arr(&mut interp, &seed_b);
            let ja = alloc_arr(&mut jit, &seed_a);
            let jb = alloc_arr(&mut jit, &seed_b);

            let (iargs, jargs) = match trial {
                0 => (
                    [Value::Obj(ia), Value::Obj(ib), Value::Int(3)],
                    [Value::Obj(ja), Value::Obj(jb), Value::Int(3)],
                ),
                1 => (
                    [Value::Obj(ia), Value::Obj(ib), Value::Int(5)],
                    [Value::Obj(ja), Value::Obj(jb), Value::Int(5)],
                ),
                2 => (
                    [Value::Obj(ia), Value::Obj(ia), Value::Int(4)],
                    [Value::Obj(ja), Value::Obj(ja), Value::Int(4)],
                ),
                3 => (
                    [Value::Null, Value::Obj(ib), Value::Int(2)],
                    [Value::Null, Value::Obj(jb), Value::Int(2)],
                ),
                4 => (
                    [Value::Obj(ia), Value::Null, Value::Int(2)],
                    [Value::Obj(ja), Value::Null, Value::Int(2)],
                ),
                _ => (
                    [Value::Obj(ia), Value::Obj(ib), Value::Int(5)],
                    [Value::Obj(ja), Value::Obj(jb), Value::Int(5)],
                ),
            };

            let ires = interp.invoke_static("LFuzzArr;", &name, "([I[II)I", &iargs);
            let jres = jit.invoke_static("LFuzzArr;", &name, "([I[II)I", &jargs);
            assert_vm_outcome_eq(
                m_idx,
                trial,
                &ires,
                &jres,
                interp.fuel_used(),
                jit.fuel_used(),
            );
            // Heap array contents MUST be bit-identical after commit OR rollback!
            assert_eq!(
                jit.heap.as_array_elems(ja).unwrap().0,
                interp.heap.as_array_elems(ia).unwrap().0,
                "arr{m_idx} trial={trial}: array `a` state diverged after execution!"
            );
            assert_eq!(
                jit.heap.as_array_elems(jb).unwrap().0,
                interp.heap.as_array_elems(ib).unwrap().0,
                "arr{m_idx} trial={trial}: array `b` state diverged after execution!"
            );
        }
    }
}

#[test]
fn differential_issue_249_cross_array_and_ref_identity_and_descriptor_guard() {
    let mut b = DexBuilder::new();
    let c = b.class("LIssue249;", "Ljava/lang/Object;");

    // 1. `static boolean eqIntLong(int[] a, long[] b) { return (Object) a == (Object) b; }`
    // Registers: v0 (scratch), v1 = p0 (int[]), v2 = p1 (long[])
    let eq_int_long = b.code(
        3,
        2,
        0,
        [
            op22t(0x32, 1, 2, 4), // if-eq v1, v2, +4 -> true
            op11n(0x12, 0, 0),    // const/4 v0, 0
            op11x(0x0f, 0),       // return v0
            op11n(0x12, 0, 1),    // const/4 v0, 1
            op11x(0x0f, 0),       // return v0
        ]
        .concat(),
    );
    b.direct(
        c,
        "eqIntLong",
        "Z",
        vec!["[I", "[J"],
        ACC_PUBLIC | ACC_STATIC,
        Some(eq_int_long),
    );

    // 2. `static int eqIntLongAndRead(int[] a, long[] b)`:
    // Uses BOTH `a[0]` and `b[0]` AND compares `a == b` so both scratch slices
    // (`slices_i32` and `slices_i64`) are actively indexed by their unified handles!
    // Registers: v0..v4 (scratch), v5 = p0 (int[]), v6 = p1 (long[])
    let eq_and_read = b.code(
        7,
        2,
        0,
        [
            op11n(0x12, 0, 0),    // v0 = 0 (idx)
            op23x(0x44, 1, 5, 0), // aget v1, v5, v0 (a[0])
            op23x(0x45, 2, 6, 0), // aget-wide v2..v3, v6, v0 (b[0])
            op12x(0x84, 4, 2),    // long-to-int v4, v2
            op23x(0x90, 1, 1, 4), // add-int v1, v1, v4
            op22t(0x32, 5, 6, 3), // if-eq v5, v6, +3 -> bogus branch
            op11x(0x0f, 1),       // return v1
            op21s(0x13, 1, -999), // const/16 v1, -999
            op11x(0x0f, 1),       // return v1
        ]
        .concat(),
    );
    b.direct(
        c,
        "eqIntLongAndRead",
        "I",
        vec!["[I", "[J"],
        ACC_PUBLIC | ACC_STATIC,
        Some(eq_and_read),
    );

    // 3. `static boolean sameObjArr(Object o, int[] a) { return o == a; }`
    let same_obj_arr = b.code(
        3,
        2,
        0,
        [
            op22t(0x32, 1, 2, 4), // if-eq v1, v2, +4
            op11n(0x12, 0, 0),
            op11x(0x0f, 0),
            op11n(0x12, 0, 1),
            op11x(0x0f, 0),
        ]
        .concat(),
    );
    b.direct(
        c,
        "sameObjArr",
        "Z",
        vec!["Ljava/lang/Object;", "[I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(same_obj_arr),
    );

    // 4. `static boolean sameArrObj(int[] a, Object o) { return a == o; }`
    let same_arr_obj = b.code(
        3,
        2,
        0,
        [
            op22t(0x32, 1, 2, 4),
            op11n(0x12, 0, 0),
            op11x(0x0f, 0),
            op11n(0x12, 0, 1),
            op11x(0x0f, 0),
        ]
        .concat(),
    );
    b.direct(
        c,
        "sameArrObj",
        "Z",
        vec!["[I", "Ljava/lang/Object;"],
        ACC_PUBLIC | ACC_STATIC,
        Some(same_arr_obj),
    );

    // 5. `static boolean sameObjLong(Object o, long[] b) { return o == b; }`
    let same_obj_long = b.code(
        3,
        2,
        0,
        [
            op22t(0x32, 1, 2, 4),
            op11n(0x12, 0, 0),
            op11x(0x0f, 0),
            op11n(0x12, 0, 1),
            op11x(0x0f, 0),
        ]
        .concat(),
    );
    b.direct(
        c,
        "sameObjLong",
        "Z",
        vec!["Ljava/lang/Object;", "[J"],
        ACC_PUBLIC | ACC_STATIC,
        Some(same_obj_long),
    );

    // 6. Unverified `aput` (0x4b, 32-bit int store) on `[B` parameter MUST be rejected
    // by `lower_method` (`VmSlot::ArrI32(Byte)` != `ArrElem32::Int`), and passing a
    // `[B` runtime object into `([I)I` MUST bail out in `try_execute` via `class_of`!
    let write_int_arr = b.code(
        4,
        1,
        0,
        [
            op11n(0x12, 0, 0),     // v0 = 0
            op21s(0x13, 1, 0x1ff), // v1 = 511
            op23x(0x4b, 1, 3, 0),  // aput v1, p0(v3), v0
            op23x(0x44, 2, 3, 0),  // aget v2, p0(v3), v0
            op11x(0x0f, 2),        // return v2
        ]
        .concat(),
    );
    b.direct(
        c,
        "writeIntArr",
        "I",
        vec!["[I"],
        ACC_PUBLIC | ACC_STATIC,
        Some(write_int_arr),
    );

    let unverified_aput_on_byte_arr = b.code(
        4,
        1,
        0,
        [
            op11n(0x12, 0, 0),
            op21s(0x13, 1, 0x1ff),
            op23x(0x4b, 1, 3, 0), // 0x4b `aput` on declared `[B`!
            op11x(0x0f, 1),
        ]
        .concat(),
    );
    b.direct(
        c,
        "unverifiedAputOnByte",
        "I",
        vec!["[B"],
        ACC_PUBLIC | ACC_STATIC,
        Some(unverified_aput_on_byte_arr),
    );

    let dex_bytes = b.finish();
    let cfg = VmConfig {
        fuel: 500_000,
        ..VmConfig::default()
    };
    let mut interp = Engine::new(
        vec![rd_dex::Dex::parse_unverified(dex_bytes.clone()).unwrap()],
        cfg.clone(),
    );
    let mut jit = Engine::new(vec![rd_dex::Dex::parse_unverified(dex_bytes).unwrap()], cfg);
    jit.enable_jit().unwrap();

    let alloc_i32 = |vm: &mut Engine, desc: &str, v: i32| -> u32 {
        let r = vm.heap.alloc_array(ElemKind::Int, 1, desc).unwrap();
        vm.heap.array_elems_mut(r).unwrap().push(Value::Int(v));
        r
    };
    let alloc_i64 = |vm: &mut Engine, v: i64| -> u32 {
        let r = vm.heap.alloc_array(ElemKind::Long, 1, "[J").unwrap();
        vm.heap.array_elems_mut(r).unwrap().push(Value::Long(v));
        r
    };

    let ia = alloc_i32(&mut interp, "[I", 40);
    let il = alloc_i64(&mut interp, 2);
    let io = interp.new_instance("Ljava/lang/Object;").unwrap();
    let ibyte = alloc_i32(&mut interp, "[B", 7);

    let ja = alloc_i32(&mut jit, "[I", 40);
    let jl = alloc_i64(&mut jit, 2);
    let jo = jit.new_instance("Ljava/lang/Object;").unwrap();
    let jbyte = alloc_i32(&mut jit, "[B", 7);

    type CallCase<'a> = (&'a str, &'a str, Vec<Value>, Vec<Value>, i32);

    // Run 40 iterations (> LEAF_WARMUP_CALLS = 32) so every method compiles & runs natively!
    for iter in 0..40 {
        let calls: [CallCase<'_>; 9] = [
            (
                "eqIntLong",
                "([I[J)Z",
                vec![Value::Obj(ia), Value::Obj(il)],
                vec![Value::Obj(ja), Value::Obj(jl)],
                0,
            ),
            (
                "eqIntLong",
                "([I[J)Z",
                vec![Value::Null, Value::Null],
                vec![Value::Null, Value::Null],
                1,
            ),
            (
                "eqIntLongAndRead",
                "([I[J)I",
                vec![Value::Obj(ia), Value::Obj(il)],
                vec![Value::Obj(ja), Value::Obj(jl)],
                42,
            ),
            (
                "sameObjArr",
                "(Ljava/lang/Object;[I)Z",
                vec![Value::Obj(ia), Value::Obj(ia)],
                vec![Value::Obj(ja), Value::Obj(ja)],
                1,
            ),
            (
                "sameObjArr",
                "(Ljava/lang/Object;[I)Z",
                vec![Value::Obj(io), Value::Obj(ia)],
                vec![Value::Obj(jo), Value::Obj(ja)],
                0,
            ),
            (
                "sameArrObj",
                "([ILjava/lang/Object;)Z",
                vec![Value::Obj(ia), Value::Obj(ia)],
                vec![Value::Obj(ja), Value::Obj(ja)],
                1,
            ),
            (
                "sameArrObj",
                "([ILjava/lang/Object;)Z",
                vec![Value::Obj(ia), Value::Obj(il)],
                vec![Value::Obj(ja), Value::Obj(jl)],
                0,
            ),
            (
                "sameObjLong",
                "(Ljava/lang/Object;[J)Z",
                vec![Value::Obj(il), Value::Obj(il)],
                vec![Value::Obj(jl), Value::Obj(jl)],
                1,
            ),
            (
                "sameObjLong",
                "(Ljava/lang/Object;[J)Z",
                vec![Value::Obj(ia), Value::Obj(il)],
                vec![Value::Obj(ja), Value::Obj(jl)],
                0,
            ),
        ];

        for (t_idx, (name, sig, iargs, jargs, expected)) in calls.into_iter().enumerate() {
            let ires = interp
                .invoke_static("LIssue249;", name, sig, &iargs)
                .unwrap();
            let jres = jit.invoke_static("LIssue249;", name, sig, &jargs).unwrap();
            assert_eq!(ires, Value::Int(expected), "iter={iter} case={t_idx}");
            assert_eq!(jres, ires, "iter={iter} case={t_idx} ({name})");
        }
    }
    assert!(
        jit.jit_stats().native_calls >= 5 * 7,
        "expected native Cranelift execution after warmup, stats={:?}",
        jit.jit_stats()
    );

    // Warm up `writeIntArr` on valid `[I`, then pass `[B` -> `try_execute` must reject
    // the mismatched runtime array descriptor instead of running compiled `[I` code!
    for _ in 0..35 {
        let _ = jit.invoke_static("LIssue249;", "writeIntArr", "([I)I", &[Value::Obj(ja)]);
        let _ = interp.invoke_static("LIssue249;", "writeIntArr", "([I)I", &[Value::Obj(ia)]);
    }
    let before_native = jit.jit_stats().native_calls;
    let j_mismatch = jit.invoke_static("LIssue249;", "writeIntArr", "([I)I", &[Value::Obj(jbyte)]);
    let i_mismatch =
        interp.invoke_static("LIssue249;", "writeIntArr", "([I)I", &[Value::Obj(ibyte)]);
    assert_eq!(
        jit.jit_stats().native_calls,
        before_native,
        "passing `[B` to `([I)I` must NOT execute compiled `[I` native code!"
    );
    assert_eq!(j_mismatch.ok(), i_mismatch.ok());

    // Unverified `0x4b aput` on `[B` must be rejected by `lower_method` (`unsupported += 1`).
    let before_unsup = jit.jit_stats().unsupported;
    for _ in 0..35 {
        let _ = jit.invoke_static(
            "LIssue249;",
            "unverifiedAputOnByte",
            "([B)I",
            &[Value::Obj(jbyte)],
        );
    }
    assert!(
        jit.jit_stats().unsupported > before_unsup,
        "unverified `aput` on `[B` must be rejected as Unsupported by `lower_method`"
    );
}
