//! Loop de interpretação Dalvik (M2 — opcodes núcleo).
//!
//! Executa sobre as instruções já decodificadas do rd-dex (`(addr, Insn)`,
//! ordenadas por endereço). Branch/switch resolvem alvo por busca binária;
//! payloads (switch/array-data) são lidos, nunca executados. O dispatch é
//! por OPCODE EXATO — as famílias lit16/lit8 NÃO mapeiam linearmente para a
//! 0x90..0xAF (rsub-int = 0xD1 é "lit - vB"), armadilha clássica (cf. #13).
//! Toda anomalia vira exceção Java capturável ou erro estruturado — Lei 1.

use std::sync::Arc;

use rd_dex::code::{CodeItem, Insn, Kind, Payload};

use crate::engine::{default_for, ACC_STATIC};
use crate::err::{Throwable, VmExit};
use crate::heap::ElemKind;
use crate::intrinsics;
use crate::value::Value;
use crate::{engine::Engine, err};

const NPE: &str = "Ljava/lang/NullPointerException;";
const ARITH: &str = "Ljava/lang/ArithmeticException;";
const AIOOBE: &str = "Ljava/lang/ArrayIndexOutOfBoundsException;";
const CCE: &str = "Ljava/lang/ClassCastException;";
const NASE: &str = "Ljava/lang/NegativeArraySizeException;";
const ASC: &str = "Ljava/lang/ArrayStoreException;";
const OOME: &str = "Ljava/lang/OutOfMemoryError;";

/// Executa um frame; `ins` já está em forma de registradores (wide = par).
/// Live call stack (innermost first, capped) appended to VM_FUEL so an agent
/// can see WHERE a guest spins instead of only that it did.
fn fuel_stack(vm: &Engine) -> String {
    let mut out = String::from("; pilha no esgotamento (innermost primeiro):");
    for l in vm.frame_labels.iter().rev().take(24) {
        out.push_str("\n  at ");
        out.push_str(l);
    }
    if vm.frame_labels.len() > 24 {
        out.push_str(&format!("\n  ... +{} frames", vm.frame_labels.len() - 24));
    }
    out
}

pub(crate) fn exec_frame(
    vm: &mut Engine,
    dex_idx: usize,
    frame_super: Option<&str>,
    code: Arc<CodeItem>,
    ins: Vec<Value>,
) -> Result<Value, VmExit> {
    let nregs = code.registers_size as usize;
    let ins_size = code.ins_size as usize;
    if ins.len() != ins_size || ins_size > nregs {
        return Err(err::vm_error(
            "INVALID_FORMAT",
            format!(
                "frame: {} argumentos para {} registradores de entrada (regs={nregs})",
                ins.len(),
                ins_size
            ),
        )
        .into());
    }
    let mut regs = vec![Value::Null; nregs];
    for (slot, v) in regs[nregs - ins_size..].iter_mut().zip(ins) {
        *slot = v;
    }
    let mut last_result = Value::Null;
    let mut pending_exc: Option<Throwable> = None;
    let mut pc: usize = 0;
    let mut next_idx: usize = 0;

    'main: loop {
        // fuel por instrução (contra loop infinito em código hostil)
        vm.fuel_used += 1;
        if vm.fuel_used.saturating_sub(vm.fuel_window_start) > vm.config.fuel {
            return Err(err::with_suggestion(
                err::vm_error(
                    "VM_FUEL",
                    format!(
                        "execução excedeu {} instruções{}",
                        vm.config.fuel,
                        fuel_stack(vm)
                    ),
                ),
                "aumente --fuel ou revise o método executado",
            )
            .into());
        }

        let cur_pc = pc;
        vm.last_pc = cur_pc;
        // Decoded instructions are sorted by address. Sequential execution
        // normally uses the next entry; branches and exception handlers still
        // use the same checked binary lookup, including identical bad-PC errors.
        let idx = lookup_next(&code.instructions, cur_pc, &mut next_idx)?;
        let insn = &code.instructions[idx].1;
        // Suspended callers must publish roots before any path that can
        // initialize a class or invoke guest code. Current roots are also
        // refreshed whenever allocation pressure requests collection.
        let may_call = matches!(insn.opcode, 0x22 | 0x60..=0x78);
        let collect = vm.gc_due();
        if may_call || collect {
            vm.publish_frame_roots(
                &regs,
                &last_result,
                pending_exc.as_ref().and_then(|e| e.obj),
            );
        }
        if collect {
            vm.maybe_gc();
        }
        pc += insn.size as usize;

        // captura exceção Java no frame atual (try/catch); sem handler → caller
        macro_rules! step {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(VmExit::Exception(t)) => {
                        match vm.enter_handler(dex_idx, &code, cur_pc, t.clone()) {
                            Ok(addr) => {
                                pending_exc = Some(t);
                                pc = addr;
                                continue 'main;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(e) => return Err(e),
                }
            };
        }
        // entra em handler de exceção; nenhum → propaga para o caller
        macro_rules! throw {
            ($t:expr) => {{
                let t: Throwable = $t;
                match vm.enter_handler(dex_idx, &code, cur_pc, t.clone()) {
                    Ok(addr) => {
                        pending_exc = Some(t);
                        pc = addr;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }};
        }
        macro_rules! throw_exc {
            ($class:expr, $msg:expr) => {
                throw!(vm.vm_exception($class, &$msg)?)
            };
        }

        match insn.opcode {
            // ── nop (0x00) — opcode REAL (format 10x, nenhum efeito):
            // padding executável e código ofuscado legítimo. Não cai no braço
            // unused (issue #22: tratá-lo como NOT_IMPLEMENTED matava a
            // execução de código Java 100% válido)
            0x00 => {}

            // ── move family (01..09) ────────────────────────────────────────
            0x01 | 0x07 => {
                let Kind::Regs(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = regs[*b as usize].clone();
            }
            0x02 | 0x03 | 0x08 | 0x09 => {
                let Kind::RegReg16(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = regs[*b as usize].clone();
            }
            0x04 => {
                // move-wide (12x): copia o par (valor + WideHi)
                let Kind::Regs(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = regs[*b as usize].clone();
                regs[*a as usize + 1] = Value::WideHi;
            }
            0x05 | 0x06 => {
                // move-wide/from16 (22x), move-wide/16 (32x)
                let Kind::RegReg16(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = regs[*b as usize].clone();
                regs[*a as usize + 1] = Value::WideHi;
            }

            // ── move-result / move-exception (0A..0D) ───────────────────────
            0x0A | 0x0C => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = last_result.clone();
            }
            0x0B => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = last_result.clone();
                regs[*a as usize + 1] = Value::WideHi;
            }
            0x0D => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                let Some(t) = pending_exc.take() else {
                    return Err(err::vm_error(
                        "INVALID_FORMAT",
                        "move-exception sem exceção pendente",
                    )
                    .into());
                };
                // issue #49: se o heap está esgotado, materializar o PRÓPRIO
                // OutOfMemoryError OOMa dentro do OOM (objeto + message não
                // cabem). JVM real pré-aloca o OOM; aqui o fallback é mover
                // Null e deixar o catch executar — a exceção original nunca é
                // substituída por um erro de materialização.
                let obj = match vm.materialize_throwable(&t) {
                    Ok(r) => Value::Obj(r),
                    Err(VmExit::Exception(_)) => Value::Null,
                    Err(e) => return Err(e),
                };
                regs[*a as usize] = obj;
            }

            // ── return family (0E..11) ──────────────────────────────────────
            0x0E => return Ok(Value::Null),
            0x0F..=0x11 => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                return Ok(regs[*a as usize].clone());
            }

            // ── const family (12..19) ─ tabela canônica validada vs baksmali
            0x12 => {
                let Kind::RegLit4(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(*v as i32);
            }
            0x13 => {
                let Kind::RegLit16(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(*v as i32);
            }
            0x14 => {
                // const (31i)
                let Kind::RegLit32(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(*v);
            }
            0x15 => {
                // const/high16 (21h)
                let Kind::RegHigh16(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(((*v as u32) << 16) as i32);
            }
            0x16 => {
                // const-wide/16 (21s)
                let Kind::RegLit16(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Long(*v as i64);
                regs[*a as usize + 1] = Value::WideHi;
            }
            0x17 => {
                // const-wide/32 (31i)
                let Kind::RegLit32(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Long(*v as i64);
                regs[*a as usize + 1] = Value::WideHi;
            }
            0x18 => {
                let Kind::Lit64(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Long(*v);
                regs[*a as usize + 1] = Value::WideHi;
            }
            0x19 => {
                // const-wide/high16 (21h)
                let Kind::RegHigh16(a, v) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Long((*v as i64) << 48);
                regs[*a as usize + 1] = Value::WideHi;
            }

            // ── const-string (1A: 21c com índice de 16 bits; 1B: jumbo 31c)
            0x1A => {
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let s = vm.cp.string(dex_idx, *i as u32);
                let r = intrinsics::alloc_string(vm, s)?;
                regs[*a as usize] = Value::Obj(r);
            }
            0x1B => {
                let Kind::RegIndex32(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let s = vm.cp.string(dex_idx, *i);
                let r = intrinsics::alloc_string(vm, s)?;
                regs[*a as usize] = Value::Obj(r);
            }

            0x1C => {
                // M3: Class host mínimo — objeto Ljava/lang/Class; com o field
                // "name" (descritor); Intent(Context, Class) e Class.getName
                // dependem disso. Primitivos continuam NOT_IMPLEMENTED.
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let desc = vm.cp.type_str(dex_idx, *i as u32);
                if !(desc.starts_with('L') || desc.starts_with('[')) {
                    return Err(err::not_implemented(format!(
                        "const-class {desc} (primitivos não têm Class host no M3)"
                    ))
                    .into());
                }
                // canonical Class object: `Foo.class == foo.getClass()` and
                // `Foo.class == Foo.class` (identity keys in AndroidX/Compose).
                // OOM surfaces as a catchable OutOfMemoryError (issue #49).
                let r = step!(vm.class_object_for(&desc));
                regs[*a as usize] = Value::Obj(r);
            }

            // monitor-*: single-thread no M2 — no-op deliberado COM referência
            // válida (monitores reais entram com threads, M3+); null → NPE
            // como na JVM (issue #37, notadamente monitor-exit)
            0x1D | 0x1E => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                if regs[*a as usize].as_ref()?.is_none() {
                    throw_exc!(
                        NPE,
                        format!(
                            "monitor-{} em null",
                            if insn.opcode == 0x1D { "enter" } else { "exit" }
                        )
                    );
                }
            }

            // ── check-cast (1F) / instance-of (20) ──────────────────────────
            0x1F => {
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let target = vm.cp.type_str(dex_idx, *i as u32);
                if let Some(r) = regs[*a as usize].as_ref()? {
                    let cls = vm.heap.class_of(r)?.to_string();
                    if cls != target && !vm.cp.is_subtype(&cls, &target) {
                        if std::env::var_os("RD_VM_TRACE_CAST").is_some() {
                            eprintln!(
                                "[cast] {} pc={cur_pc}: {cls} -> {target}",
                                vm.frame_labels
                                    .last()
                                    .map(String::as_str)
                                    .unwrap_or("<unknown>")
                            );
                        }
                        throw_exc!(CCE, format!("{cls} cannot be cast to {target}"));
                    }
                }
            }
            0x20 => {
                let Kind::RegRegIndex(a, b, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let target = vm.cp.type_str(dex_idx, *i as u32);
                let v = match regs[*b as usize].as_ref()? {
                    None => 0,
                    Some(r) => {
                        let cls = vm.heap.class_of(r)?.to_string();
                        (cls == target || vm.cp.is_subtype(&cls, &target)) as i32
                    }
                };
                regs[*a as usize] = Value::Int(v);
            }

            // ── arrays/objetos (21..26) ─────────────────────────────────────
            0x21 => {
                let Kind::Regs(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                match regs[*b as usize].as_ref()? {
                    None => throw_exc!(NPE, "array-length em null"),
                    Some(r) => {
                        let (elems, _) = vm.heap.as_array_elems(r)?;
                        regs[*a as usize] = Value::Int(elems.len() as i32);
                    }
                }
            }
            0x22 => {
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let class = vm.cp.type_str(dex_idx, *i as u32);
                // issue #25: criar instância é “ação ativa” (JLS 12.4.2) —
                // <clinit> da classe roda ANTES do construtor; sem isto, o
                // efeito do <clinit> só era observável no primeiro sget
                // issue #41: exceção de <clinit> (EIIE) é despachada na MESMA
                // frame (try/catch do método corrente) — `?` bypassava o handler
                step!(vm.ensure_initialized(&class));
                let r = step!(match vm.heap.alloc_instance(class.clone(), Vec::new()) {
                    Ok(r) => Ok(r),
                    Err(e) => Err(oom_exception(e)),
                });
                // M3: anexa estado host p/ classes de plataforma (Activity,
                // View/TextView/Button/LinearLayout, Handler, Intent, Bundle)
                step!(vm.attach_host_state(&class, r));
                regs[*a as usize] = Value::Obj(r);
            }
            0x23 => {
                let Kind::RegRegIndex(a, b, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let desc = vm.cp.type_str(dex_idx, *i as u32);
                let len = step!(match &regs[*b as usize] {
                    Value::Null => Err(VmExit::Exception(
                        vm.vm_exception(NPE, "new-array com tamanho em null")?,
                    )),
                    v => Ok(v.as_int()?),
                });
                if len < 0 {
                    throw_exc!(NASE, format!("tamanho de array negativo: {len}"));
                }
                let Some(elem) = ElemKind::from_array_desc(&desc) else {
                    return Err(err::not_implemented(format!("new-array {desc}")).into());
                };
                // issue #37: OOM vira OutOfMemoryError capturável (Throwable)
                let r = step!(new_filled_array(vm, elem, len as usize, &desc));
                regs[*a as usize] = Value::Obj(r);
            }
            0x24 | 0x25 => {
                // filled-new-array {regs}, type@BBBB (0x24) / range (0x25)
                let (count, src, type_idx) = match &insn.kind {
                    Kind::Invoke35c {
                        count,
                        idx,
                        regs: rr,
                    } => (
                        *count as usize,
                        rr[..*count as usize]
                            .iter()
                            .map(|r| regs[*r as usize].clone())
                            .collect::<Vec<_>>(),
                        *idx as u32,
                    ),
                    Kind::InvokeRange3rc { count, idx, start } => (
                        *count as usize,
                        regs[*start as usize..(*start as usize + *count as usize)].to_vec(),
                        *idx as u32,
                    ),
                    _ => return bad_kind(insn),
                };
                let desc = vm.cp.type_str(dex_idx, type_idx);
                let Some(elem) = ElemKind::from_array_desc(&desc) else {
                    return Err(err::not_implemented(format!("filled-new-array {desc}")).into());
                };
                let r = step!(new_filled_array(vm, elem, count, &desc));
                let elems = vm.heap.array_elems_mut(r)?;
                for (i, v) in src.into_iter().enumerate() {
                    elems[i] = v;
                }
                last_result = Value::Obj(r);
            }
            0x26 => {
                // fill-array-data vAA, +payload (payload em +offset)
                let Kind::RegBranch32(a, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let payload_addr = branch_target(cur_pc, *off as i64)?;
                let pidx = lookup(&code.instructions, payload_addr)?;
                let Kind::Payload(Payload::ArrayData {
                    element_width,
                    element_count,
                    data,
                }) = &code.instructions[pidx].1.kind
                else {
                    return Err(err::vm_error(
                        "INVALID_FORMAT",
                        format!(
                            "fill-array-data: payload não está em +{off} (addr {payload_addr})"
                        ),
                    )
                    .into());
                };
                let arr = match regs[*a as usize].as_ref()? {
                    None => throw_exc!(NPE, "fill-array-data em null"),
                    Some(r) => r,
                };
                fill_array(vm, arr, *element_width, *element_count, data)?;
            }

            // ── throw (27) ──────────────────────────────────────────────────
            0x27 => {
                let Kind::Reg(a) = &insn.kind else {
                    return bad_kind(insn);
                };
                let t = match regs[*a as usize].as_ref()? {
                    None => vm.vm_exception(NPE, "throw com null")?,
                    Some(r) => {
                        let cls = vm.heap.class_of(r)?.to_string();
                        let msg = vm.heap.get_field(r, "message").ok().and_then(|v| match v {
                            Value::Obj(m) => vm.heap.as_str(m).ok().map(str::to_string),
                            _ => None,
                        });
                        Throwable {
                            class: cls,
                            message: msg,
                            obj: Some(r),
                        }
                    }
                };
                throw!(t);
            }

            // ── goto (28..2A) ───────────────────────────────────────────────
            0x28 => {
                let Kind::Branch8(off) = &insn.kind else {
                    return bad_kind(insn);
                };
                pc = branch_target(cur_pc, *off as i64)?;
            }
            0x29 => {
                let Kind::Branch16(off) = &insn.kind else {
                    return bad_kind(insn);
                };
                pc = branch_target(cur_pc, *off as i64)?;
            }
            0x2A => {
                let Kind::Branch32(off) = &insn.kind else {
                    return bad_kind(insn);
                };
                pc = branch_target(cur_pc, *off as i64)?;
            }

            // ── switch (2B/2C) — payload em +offset; alvos relativos ao switch
            0x2B | 0x2C => {
                let Kind::RegBranch32(a, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let key = regs[*a as usize].as_int()?;
                let payload_addr = branch_target(cur_pc, *off as i64)?;
                let pidx = lookup(&code.instructions, payload_addr)?;
                pc = match &code.instructions[pidx].1.kind {
                    Kind::Payload(Payload::PackedSwitch { first_key, targets }) => {
                        let rel = key.wrapping_sub(*first_key);
                        if rel >= 0 && (rel as usize) < targets.len() {
                            branch_target(cur_pc, targets[rel as usize] as i64)?
                        } else {
                            pc
                        }
                    }
                    Kind::Payload(Payload::SparseSwitch { keys, targets }) => {
                        match keys.binary_search(&key) {
                            Ok(i) => branch_target(cur_pc, targets[i] as i64)?,
                            Err(_) => pc,
                        }
                    }
                    _ => {
                        return Err(err::vm_error(
                            "INVALID_FORMAT",
                            format!("switch: payload não está em +{off} (addr {payload_addr})"),
                        )
                        .into())
                    }
                };
            }

            // ── comparações float/double/long (2D..31) ──────────────────────
            0x2D => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(cmp_float(
                    regs[*b as usize].as_float()?,
                    regs[*c as usize].as_float()?,
                    -1,
                ));
            }
            0x2E => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(cmp_float(
                    regs[*b as usize].as_float()?,
                    regs[*c as usize].as_float()?,
                    1,
                ));
            }
            0x2F => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(cmp_double(
                    regs[*b as usize].as_double()?,
                    regs[*c as usize].as_double()?,
                    -1,
                ));
            }
            0x30 => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                regs[*a as usize] = Value::Int(cmp_double(
                    regs[*b as usize].as_double()?,
                    regs[*c as usize].as_double()?,
                    1,
                ));
            }
            0x31 => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                let (l, r) = (regs[*b as usize].as_long()?, regs[*c as usize].as_long()?);
                regs[*a as usize] = Value::Int(match l.cmp(&r) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                });
            }

            // ── if-test (32..37) — int E referência (if-eq/if-ne sobre objetos)
            0x32 | 0x33 => {
                let Kind::RegRegBranch16(a, b, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*a as usize].clone();
                let y = regs[*b as usize].clone();
                let eq = values_ref_eq(&x, &y)
                    .map_err(|m| VmExit::Error(err::vm_error("VM_TYPE_ERROR", m)))?;
                let hit = if insn.opcode == 0x32 { eq } else { !eq };
                if hit {
                    pc = branch_target(cur_pc, *off as i64)?;
                }
            }
            0x34..=0x37 => {
                let Kind::RegRegBranch16(a, b, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*a as usize].as_int()?;
                let y = regs[*b as usize].as_int()?;
                if cmp_op(insn.opcode - 0x34, x, y) {
                    pc = branch_target(cur_pc, *off as i64)?;
                }
            }
            // ── if-testz (38..3D) — int/boolean E referência (eqz/nez)
            0x38 | 0x39 => {
                let Kind::RegBranch16(a, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let hit = match regs[*a as usize].as_ref() {
                    Ok(r) => {
                        let eq0 = r.is_none();
                        if insn.opcode == 0x38 {
                            eq0
                        } else {
                            !eq0
                        }
                    }
                    Err(_) => {
                        let x = regs[*a as usize].as_int()?;
                        let eq0 = x == 0;
                        if insn.opcode == 0x38 {
                            eq0
                        } else {
                            !eq0
                        }
                    }
                };
                if hit {
                    pc = branch_target(cur_pc, *off as i64)?;
                }
            }
            0x3A..=0x3D => {
                let Kind::RegBranch16(a, off) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*a as usize].as_int()?;
                if x_match(insn.opcode - 0x3A, x) {
                    pc = branch_target(cur_pc, *off as i64)?;
                }
            }

            // ── aget (44..4A) / aput (4B..51) ───────────────────────────────
            0x44..=0x4A => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                let v = step!(aget(
                    vm,
                    insn.opcode,
                    regs[*b as usize].as_ref()?,
                    regs[*c as usize].as_int()?
                ));
                let wide = matches!(v, Value::Long(_) | Value::Double(_));
                regs[*a as usize] = v;
                if wide {
                    regs[*a as usize + 1] = Value::WideHi; // aget-wide
                }
            }
            0x4B..=0x51 => {
                // aput vAA, vBB, vCC: vAA=valor, vBB=array, vCC=índice
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                step!(aput(
                    vm,
                    insn.opcode,
                    regs[*b as usize].as_ref()?,
                    regs[*c as usize].as_int()?,
                    regs[*a as usize].clone(),
                ));
            }

            // ── iget (52..58) ───────────────────────────────────────────────
            0x52..=0x58 => {
                let Kind::RegRegIndex(a, b, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let (_class, fname, ftype) = vm.cp.field_ref(dex_idx, *i as u32)?;
                let r = step!(match regs[*b as usize].as_ref() {
                    Ok(None) => Err(VmExit::Exception(
                        vm.vm_exception(NPE, &format!("iget '{fname}' em null"))?,
                    )),
                    Ok(Some(r)) => Ok(r),
                    Err(e) => Err(e.into()),
                });
                let v = match vm.heap.get_field(r, &fname) {
                    Ok(v) => v,
                    Err(_) => default_for(&ftype), // campo nunca escrito → default
                };
                regs[*a as usize] = v;
                if matches!(regs[*a as usize], Value::Long(_) | Value::Double(_)) {
                    regs[*a as usize + 1] = Value::WideHi; // iget-wide
                }
            }
            // ── iput (59..5F) ───────────────────────────────────────────────
            0x59..=0x5F => {
                let Kind::RegRegIndex(a, b, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let (_class, fname, _ftype) = vm.cp.field_ref(dex_idx, *i as u32)?;
                let r = step!(match regs[*b as usize].as_ref() {
                    Ok(None) => Err(VmExit::Exception(
                        vm.vm_exception(NPE, &format!("iput '{fname}' em null"))?,
                    )),
                    Ok(Some(r)) => Ok(r),
                    Err(e) => Err(e.into()),
                });
                // issue #49: OOM de campo novo no iput é OutOfMemoryError
                // CAPTURÁVEL (antes: VM_TYPE_ERROR mentiroso e run-killer)
                let field_value = regs[*a as usize].clone();
                step!(crate::err::put_field_result(vm.heap.put_field(
                    r,
                    &fname,
                    field_value
                )));
            }

            // ── sget (60..66) / sput (67..6D) ───────────────────────────────
            0x60..=0x66 => {
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let (ref_class, fname, ftype) = vm.cp.field_ref(dex_idx, *i as u32)?;
                // issue #57: JVMS 5.4.3.2 — o slot static é da classe
                // DECLARANTE (sobe superclasses/interfaces; miss final =
                // NoSuchFieldError capturável). Antes: statics keyados pela
                // classe do ref — sget de campo herdado via ref da subclasse
                // lia default silenciosamente e sput criava slot divergente.
                let owner = step!(vm.resolve_static_owner(&ref_class, &fname));
                // issue #41: EIIE de <clinit> despacha na frame corrente
                // (JVMS 5.5: init é disparado no DECLARANTE — via ref da
                // subclasse NÃO inicializa a subclasse)
                step!(vm.ensure_initialized(&owner));
                // issues #43/#48: System.out/.err materializam um PrintStream
                // embutido na primeira leitura (println/print são intrínsecos;
                // saída vai para stdout/stderr do host, spec PrintStream: nunca lança)
                if owner == "Ljava/lang/System;" && (fname == "out" || fname == "err") {
                    let key = (owner.clone(), fname.clone());
                    if let std::collections::hash_map::Entry::Vacant(e) = vm.statics.entry(key) {
                        let fd = if fname == "err" { 2 } else { 1 };
                        let r = step!(vm
                            .heap
                            .alloc_instance(
                                "Ljava/io/PrintStream;".to_string(),
                                vec![("fd".to_string(), Value::Int(fd))],
                            )
                            .map_err(oom_exception));
                        e.insert(Value::Obj(r));
                    }
                }
                if owner == "Ljava/util/concurrent/TimeUnit;"
                    && !vm.statics.contains_key(&(owner.clone(), fname.clone()))
                    && matches!(
                        fname.as_str(),
                        "NANOSECONDS"
                            | "MICROSECONDS"
                            | "MILLISECONDS"
                            | "SECONDS"
                            | "MINUTES"
                            | "HOURS"
                            | "DAYS"
                    )
                {
                    let unit = step!(vm.time_unit_obj(&fname));
                    vm.statics
                        .insert((owner.clone(), fname.clone()), Value::Obj(unit));
                }
                // JDK constants not backed by any DEX (Boolean.TRUE/FALSE,
                // MAX_VALUE, NaN, ...) are materialized on first read.
                // Host-static resolution needs `&mut Engine`; keeping a map Entry
                // live across those calls would conflict with the mutable borrow.
                #[allow(clippy::map_entry)]
                if !vm.statics.contains_key(&(owner.clone(), fname.clone())) {
                    if let Some(c) = step!(crate::boxes::host_static(vm, &owner, &fname)) {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::jnormalizer::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::jbigdecimal::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::jtext_format::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::android_colorspace::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::android_env::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::android_build::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) = step!(crate::graphics::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) = step!(crate::parcel::host_static(vm, &owner, &fname)) {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::android_util::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::platform_base::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) =
                        step!(crate::collections::host_static(vm, &owner, &fname))
                    {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    } else if let Some(c) = step!(crate::jtime::host_static(vm, &owner, &fname)) {
                        vm.statics.insert((owner.clone(), fname.clone()), c);
                    }
                    #[cfg(feature = "virtual-devices")]
                    if !vm.statics.contains_key(&(owner.clone(), fname.clone())) {
                        if let Some(c) = step!(crate::m10::host_static(vm, &owner, &fname)) {
                            vm.statics.insert((owner.clone(), fname.clone()), c);
                        }
                    }
                }
                let v = vm
                    .statics
                    .get(&(owner, fname))
                    .cloned()
                    // issue #37: default POR TIPO declarado — Int(0) genérico
                    // quebrava sget-wide (Long lido como Int → VM_TYPE_ERROR)
                    .unwrap_or_else(|| default_for(&ftype));
                regs[*a as usize] = v;
                if matches!(regs[*a as usize], Value::Long(_) | Value::Double(_)) {
                    regs[*a as usize + 1] = Value::WideHi; // sget-wide
                }
            }
            0x67..=0x6D => {
                let Kind::RegIndex(a, i) = &insn.kind else {
                    return bad_kind(insn);
                };
                let (ref_class, fname, _ftype) = vm.cp.field_ref(dex_idx, *i as u32)?;
                // issue #57: grava no slot da classe DECLARANTE (JVMS 5.4.3.2)
                // — refs pela subclasse e pela superclasse vêem o MESMO valor.
                let owner = step!(vm.resolve_static_owner(&ref_class, &fname));
                // issue #41: EIIE de <clinit> despacha na frame corrente
                step!(vm.ensure_initialized(&owner));
                vm.statics.insert((owner, fname), regs[*a as usize].clone());
            }

            // ── invokes (6E..72, 74..78) ────────────────────────────────────
            0x6E..=0x72 | 0x74..=0x78 => {
                let (midx, slots) = match &insn.kind {
                    Kind::Invoke35c {
                        count,
                        idx,
                        regs: rr,
                    } => (
                        *idx as u32,
                        rr[..*count as usize]
                            .iter()
                            .map(|r| regs[*r as usize].clone())
                            .collect::<Vec<_>>(),
                    ),
                    Kind::InvokeRange3rc { count, idx, start } => {
                        let s = *start as usize;
                        let n = *count as usize;
                        (*idx as u32, regs[s..s + n].to_vec())
                    }
                    _ => return bad_kind(insn),
                };
                let invoke_op = insn.opcode;
                match do_invoke(vm, dex_idx, frame_super, invoke_op, midx, &slots) {
                    Ok(v) => last_result = v,
                    Err(VmExit::Exception(t)) => throw!(t),
                    Err(e) => return Err(e),
                }
            }

            // ── unop (7B..8F) ───────────────────────────────────────────────
            0x7B..=0x8F => {
                let Kind::Regs(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                let v = unop(insn.opcode, regs[*b as usize].clone())?;
                regs[*a as usize] = v;
                if matches!(regs[*a as usize], Value::Long(_) | Value::Double(_)) {
                    regs[*a as usize + 1] = Value::WideHi; // neg-long, not-long, *to-long/double
                }
            }

            // ── binop: 90..AF normal / B0..CF 2addr / D0..D7 lit16 / D8..E2 lit8
            0x90..=0xAF => {
                let Kind::RegRegReg(a, b, c) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*b as usize].clone();
                let y = regs[*c as usize].clone();
                let v = step!(binop(vm, insn.opcode, x, y));
                write_binop(&mut regs, *a, v);
            }
            0xB0..=0xCF => {
                let Kind::Regs(a, b) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*a as usize].clone();
                let y = regs[*b as usize].clone();
                // 2addr: mesmo grupo, offset -0x20
                let v = step!(binop(vm, insn.opcode - 0x20, x, y));
                write_binop(&mut regs, *a, v);
            }
            0xD0..=0xD7 => {
                // lit16: add, rsub, mul, div, rem, and, or, xor
                let Kind::RegRegLit16(a, b, lit) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*b as usize].clone();
                let v = match insn.opcode {
                    0xD0 => step!(binop(vm, 0x90, x, Value::Int(*lit as i32))), // add-int/lit16
                    0xD1 => step!(binop(vm, 0x91, Value::Int(*lit as i32), x)), // rsub-int (lit - vB)
                    0xD2 => step!(binop(vm, 0x92, x, Value::Int(*lit as i32))), // mul-int/lit16
                    0xD3 => step!(int_div_lit(vm, insn.opcode, x, *lit)),       // div-int/lit16
                    0xD4 => step!(int_rem_lit(vm, insn.opcode, x, *lit)),       // rem-int/lit16
                    0xD5 => step!(binop(vm, 0x95, x, Value::Int(*lit as i32))), // and
                    0xD6 => step!(binop(vm, 0x96, x, Value::Int(*lit as i32))), // or
                    op => step!(binop(vm, op_to_base(op), x, Value::Int(*lit as i32))), // xor
                };
                write_binop(&mut regs, *a, v);
            }
            0xD8..=0xE2 => {
                // lit8: add, rsub, mul, div, rem, and, or, xor, shl, shr, ushr
                let Kind::RegRegLit8(a, b, raw_lit) = &insn.kind else {
                    return bad_kind(insn);
                };
                let x = regs[*b as usize].clone();
                let lit = Value::Int(*raw_lit as i32);
                let v = match insn.opcode {
                    0xD8 => step!(binop(vm, 0x90, x, lit)), // add-int/lit8
                    0xD9 => step!(binop(vm, 0x91, lit, x)), // rsub-int/lit8
                    0xDA => step!(binop(vm, 0x92, x, lit)), // mul-int/lit8
                    0xDB => step!(int_div_lit(vm, insn.opcode, x, *raw_lit as i16)),
                    0xDC => step!(int_rem_lit(vm, insn.opcode, x, *raw_lit as i16)),
                    0xDD => step!(binop(vm, 0x95, x, lit)), // and
                    0xDE => step!(binop(vm, 0x96, x, lit)), // or
                    0xDF => step!(binop(vm, 0x97, x, lit)), // xor
                    0xE0 => step!(binop(vm, 0x98, x, lit)), // shl
                    0xE1 => step!(binop(vm, 0x99, x, lit)), // shr
                    op => step!(binop(vm, op_to_base(op), x, lit)), // ushr (0xE2)
                };
                write_binop(&mut regs, *a, v);
            }

            _ => {
                let info = rd_dex::opcode::info(insn.opcode);
                if info.is_unused() {
                    // opcodes unused nunca aparecem em código verifier-clean;
                    // tratados como nop (mesma política do disassembler)
                } else {
                    return Err(err::not_implemented(format!(
                        "opcode {} (0x{:02x})",
                        info.name, insn.opcode
                    ))
                    .into());
                }
            }
        }
    }
}

/// lit16/lit8 → opcode base do grupo binop 0x90..0xAF equivalente.
fn op_to_base(op: u8) -> u8 {
    match op {
        0xD0 | 0xD8 => 0x90, // add
        0xD1 | 0xD9 => 0x91, // rsub (chamador troca os operandos)
        0xD2 | 0xDA => 0x92, // mul
        0xD3 | 0xDB => 0x93, // div
        0xD4 | 0xDC => 0x94, // rem
        0xD5 | 0xDD => 0x95, // and
        0xD6 | 0xDE => 0x96, // or
        0xD7 | 0xDF => 0x97, // xor
        0xE0 => 0x98,        // shl
        0xE1 => 0x99,        // shr
        0xE2 => 0x9A,        // ushr
        _ => unreachable!("op_to_base só para lit16/lit8"),
    }
}

// ── helpers do loop ─────────────────────────────────────────────────────────

#[inline]
fn lookup_next(
    instructions: &[(usize, Insn)],
    addr: usize,
    next: &mut usize,
) -> Result<usize, VmExit> {
    let index = match instructions.get(*next) {
        Some((pc, _)) if *pc == addr => *next,
        _ => lookup(instructions, addr)?,
    };
    *next = index + 1;
    Ok(index)
}

fn lookup(instructions: &[(usize, Insn)], addr: usize) -> Result<usize, VmExit> {
    match instructions.binary_search_by_key(&addr, |(a, _)| *a) {
        Ok(i) => Ok(i),
        Err(_) => Err(err::vm_error(
            "INVALID_FORMAT",
            format!("alvo de branch 0x{addr:x} não é início de instrução"),
        )
        .into()),
    }
}

fn branch_target(cur_pc: usize, off: i64) -> Result<usize, VmExit> {
    let target = cur_pc as i64 + off;
    if target < 0 {
        return Err(err::vm_error(
            "INVALID_FORMAT",
            format!("branch para endereço negativo ({target})"),
        )
        .into());
    }
    Ok(target as usize)
}

fn bad_kind(insn: &Insn) -> Result<Value, VmExit> {
    Err(err::vm_error(
        "INVALID_FORMAT",
        format!(
            "formato de registrador inesperado para {} (0x{:02x})",
            rd_dex::opcode::info(insn.opcode).name,
            insn.opcode
        ),
    )
    .into())
}

/// issue #37: OOM do heap vira exceção Java CAPTURÁVEL (OutOfMemoryError é
/// um Throwable na JVM — `catch (OutOfMemoryError)` tem que funcionar),
/// não erro estruturado inalcançável pelo código do app.
fn oom_exception(e: crate::heap::OomError) -> VmExit {
    VmExit::Exception(Throwable::new(
        OOME,
        format!(
            "alocação de {} bytes excede o heap de {} bytes (piso E5)",
            e.requested, e.budget
        ),
    ))
}

fn new_filled_array(
    vm: &mut Engine,
    elem: ElemKind,
    len: usize,
    elem_class: &str,
) -> Result<crate::heap::ObjRef, VmExit> {
    let fill = match elem {
        ElemKind::Int => Value::Int(0),
        ElemKind::Long => Value::Long(0),
        ElemKind::Float => Value::Float(0.0),
        ElemKind::Double => Value::Double(0.0),
        ElemKind::Obj => Value::Null,
    };
    let r = vm
        .heap
        .alloc_array(elem, len, elem_class)
        .map_err(oom_exception)?;
    let elems = vm.heap.array_elems_mut(r)?;
    elems.extend(std::iter::repeat(fill).take(len));
    Ok(r)
}

/// if-eq/if-ne sobre referências: igualdade de identidade (Null == Null).
/// issue #37: a convenção interna Int(0)=null vale também na comparação mista
/// (`boolean isNull(String s){return s==null;}` via slot de arg Null).
fn values_ref_eq(a: &Value, b: &Value) -> Result<bool, String> {
    match (a, b) {
        (Value::Obj(x), Value::Obj(y)) => Ok(x == y),
        (Value::Null, Value::Null) => Ok(true),
        (Value::Null, Value::Obj(_)) | (Value::Obj(_), Value::Null) => Ok(false),
        (Value::Int(x), Value::Int(y)) => Ok(x == y),
        (Value::Long(x), Value::Long(y)) => Ok(x == y),
        (Value::Int(x), Value::Null) | (Value::Null, Value::Int(x)) => Ok(*x == 0),
        // issue #49: comparação mista int × objeto — Java nunca produz este
        // bytecode com int ≠ convenção null, e `false` segue a execução em vez
        // de matar o run com VM_TYPE_ERROR (Int(0)=null já cobre o par com Null)
        (Value::Int(_), Value::Obj(_)) | (Value::Obj(_), Value::Int(_)) => Ok(false),
        (x, y) => Err(format!(
            "if sobre tipos incompatíveis: {} vs {}",
            x.type_name(),
            y.type_name()
        )),
    }
}

fn cmp_op(which: u8, x: i32, y: i32) -> bool {
    match which {
        0 => x < y,  // if-lt
        1 => x >= y, // if-ge
        2 => x > y,  // if-gt
        3 => x <= y, // if-le
        _ => false,
    }
}

fn x_match(which: u8, x: i32) -> bool {
    match which {
        0 => x < 0,  // if-ltz
        1 => x >= 0, // if-gez
        2 => x > 0,  // if-gtz
        3 => x <= 0, // if-lez
        _ => false,
    }
}

fn cmp_float(a: f32, b: f32, nan: i32) -> i32 {
    if a.is_nan() || b.is_nan() {
        return nan;
    }
    match a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

fn cmp_double(a: f64, b: f64, nan: i32) -> i32 {
    if a.is_nan() || b.is_nan() {
        return nan;
    }
    match a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Escreve resultado de binop; wide (long/double) ocupa 2 slots.
fn write_binop(regs: &mut [Value], a: u8, v: Value) {
    let wide = matches!(v, Value::Long(_) | Value::Double(_));
    regs[a as usize] = v;
    if wide {
        regs[a as usize + 1] = Value::WideHi;
    }
}

#[allow(clippy::too_many_lines)]
fn unop(op: u8, x: Value) -> Result<Value, VmExit> {
    Ok(match op {
        0x7B => Value::Int(x.as_int()?.wrapping_neg()), // neg-int
        0x7C => Value::Int(!x.as_int()?),               // not-int
        0x7D => Value::Long(x.as_long()?.wrapping_neg()), // neg-long
        0x7E => Value::Long(!x.as_long()?),             // not-long
        0x7F => Value::Float(-x.as_float()?),           // neg-float
        0x80 => Value::Double(-x.as_double()?),         // neg-double
        0x81 => Value::Long(x.as_int()? as i64),        // int-to-long
        0x82 => Value::Float(x.as_int()? as f32),       // int-to-float
        0x83 => Value::Double(x.as_int()? as f64),      // int-to-double
        0x84 => Value::Int(x.as_long()? as i32),        // long-to-int
        0x85 => Value::Float(x.as_long()? as f32),      // long-to-float
        0x86 => Value::Double(x.as_long()? as f64),     // long-to-double
        // float/double → int/long: Java = NaN→0 + saturação; `as` do Rust
        // é exatamente isso (desde 1.45)
        0x87 => Value::Int(x.as_float()? as i32), // float-to-int
        0x88 => Value::Long(x.as_float()? as i64), // float-to-long
        0x89 => Value::Double(x.as_float()? as f64), // float-to-double
        0x8A => Value::Int(x.as_double()? as i32), // double-to-int
        0x8B => Value::Long(x.as_double()? as i64), // double-to-long
        0x8C => Value::Float(x.as_double()? as f32), // double-to-float
        0x8D => Value::Int(x.as_int()? as i8 as i32), // int-to-byte
        0x8E => Value::Int(x.as_int()? as u16 as i32), // int-to-char
        0x8F => Value::Int(x.as_int()? as i16 as i32), // int-to-short
        _ => return Err(err::not_implemented(format!("unop 0x{op:02x}")).into()),
    })
}

/// binop por OPCODE EXATO (0x90..0xAF — 2addr/lit já normalizados pelo caller).
#[allow(clippy::too_many_lines)]
fn binop(vm: &mut Engine, op: u8, x: Value, y: Value) -> Result<Value, VmExit> {
    Ok(match op {
        // int
        0x90 => Value::Int(x.as_int()?.wrapping_add(y.as_int()?)),
        0x91 => Value::Int(x.as_int()?.wrapping_sub(y.as_int()?)),
        0x92 => Value::Int(x.as_int()?.wrapping_mul(y.as_int()?)),
        0x93 => return int_div(x.as_int()?, y.as_int()?, vm),
        0x94 => return int_rem(x.as_int()?, y.as_int()?, vm),
        0x95 => Value::Int(x.as_int()? & y.as_int()?),
        0x96 => Value::Int(x.as_int()? | y.as_int()?),
        0x97 => Value::Int(x.as_int()? ^ y.as_int()?),
        0x98 => Value::Int(x.as_int()? << (y.as_int()? as u32 & 31)),
        0x99 => Value::Int(x.as_int()? >> (y.as_int()? as u32 & 31)),
        0x9A => Value::Int(((x.as_int()? as u32) >> (y.as_int()? as u32 & 31)) as i32),
        // long
        0x9B => Value::Long(x.as_long()?.wrapping_add(y.as_long()?)),
        0x9C => Value::Long(x.as_long()?.wrapping_sub(y.as_long()?)),
        0x9D => Value::Long(x.as_long()?.wrapping_mul(y.as_long()?)),
        0x9E => return long_div(x.as_long()?, y.as_long()?, vm),
        0x9F => return long_rem(x.as_long()?, y.as_long()?, vm),
        0xA0 => Value::Long(x.as_long()? & y.as_long()?),
        0xA1 => Value::Long(x.as_long()? | y.as_long()?),
        0xA2 => Value::Long(x.as_long()? ^ y.as_long()?),
        // shifts de long: o contador é um INT (spec Dalvik) — caso real pego
        // no APK da F-Droid (IntIntPair.getFirst-impl: shr-long/2addr com v0=int)
        0xA3 => Value::Long(x.as_long()? << (y.as_int()? as u64 & 63)),
        0xA4 => Value::Long(x.as_long()? >> (y.as_int()? as u64 & 63)),
        0xA5 => Value::Long(((x.as_long()? as u64) >> (y.as_int()? as u64 & 63)) as i64),
        // float
        0xA6 => Value::Float(x.as_float()? + y.as_float()?),
        0xA7 => Value::Float(x.as_float()? - y.as_float()?),
        0xA8 => Value::Float(x.as_float()? * y.as_float()?),
        0xA9 => Value::Float(x.as_float()? / y.as_float()?),
        0xAA => Value::Float(x.as_float()? % y.as_float()?),
        // double
        0xAB => Value::Double(x.as_double()? + y.as_double()?),
        0xAC => Value::Double(x.as_double()? - y.as_double()?),
        0xAD => Value::Double(x.as_double()? * y.as_double()?),
        0xAE => Value::Double(x.as_double()? / y.as_double()?),
        0xAF => Value::Double(x.as_double()? % y.as_double()?),
        _ => return Err(err::not_implemented(format!("binop 0x{op:02x}")).into()),
    })
}

/// div-int por literal: literal 0 → ArithmeticException (d8 emite const/0
/// apenas quando não pode provar; o caso precisa existir).
fn int_div_lit(vm: &mut Engine, op: u8, x: Value, lit: i16) -> Result<Value, VmExit> {
    match op {
        0xD3 | 0xDB => int_div(x.as_int()?, lit as i32, vm),
        _ => unreachable!("int_div_lit só para div"),
    }
}

fn int_rem_lit(vm: &mut Engine, op: u8, x: Value, lit: i16) -> Result<Value, VmExit> {
    match op {
        0xD4 | 0xDC => int_rem(x.as_int()?, lit as i32, vm),
        _ => unreachable!("int_rem_lit só para rem"),
    }
}

fn int_div(a: i32, b: i32, vm: &mut Engine) -> Result<Value, VmExit> {
    if b == 0 {
        return Err(VmExit::Exception(vm.vm_exception(ARITH, "divide by zero")?));
    }
    Ok(Value::Int(a.wrapping_div(b)))
}

fn int_rem(a: i32, b: i32, vm: &mut Engine) -> Result<Value, VmExit> {
    if b == 0 {
        return Err(VmExit::Exception(vm.vm_exception(ARITH, "divide by zero")?));
    }
    Ok(Value::Int(a.wrapping_rem(b)))
}

fn long_div(a: i64, b: i64, vm: &mut Engine) -> Result<Value, VmExit> {
    if b == 0 {
        return Err(VmExit::Exception(vm.vm_exception(ARITH, "divide by zero")?));
    }
    Ok(Value::Long(a.wrapping_div(b)))
}

fn long_rem(a: i64, b: i64, vm: &mut Engine) -> Result<Value, VmExit> {
    if b == 0 {
        return Err(VmExit::Exception(vm.vm_exception(ARITH, "divide by zero")?));
    }
    Ok(Value::Long(a.wrapping_rem(b)))
}

fn bounds_check(len: usize, idx: i32, what: &str) -> Result<(), VmExit> {
    if idx < 0 || idx as usize >= len {
        return Err(VmExit::Exception(Throwable::new(
            AIOOBE,
            format!("{what}: index {idx}, length {len}"),
        )));
    }
    Ok(())
}

fn aget(
    vm: &mut Engine,
    _op: u8,
    arr: Option<crate::heap::ObjRef>,
    idx: i32,
) -> Result<Value, VmExit> {
    let Some(r) = arr else {
        return Err(VmExit::Exception(vm.vm_exception(NPE, "aget em null")?));
    };
    let (elems, _kind) = vm.heap.as_array_elems(r)?;
    bounds_check(elems.len(), idx, "array get")?;
    Ok(elems[idx as usize].clone())
}

fn aput(
    vm: &mut Engine,
    op: u8,
    arr: Option<crate::heap::ObjRef>,
    idx: i32,
    v: Value,
) -> Result<(), VmExit> {
    let Some(r) = arr else {
        return Err(VmExit::Exception(vm.vm_exception(NPE, "aput em null")?));
    };
    {
        let (elems, _kind) = vm.heap.as_array_elems(r)?;
        bounds_check(elems.len(), idx, "array put")?;
    }
    // issue #37: store check para aput-object (0x4D) — ArrayStoreException
    // quando o valor não é subtipo do elemento declarado do array
    if op == 0x4D {
        let desc = vm.heap.class_of(r)?.to_string();
        if let Some(elem_cls) = desc.strip_prefix('[') {
            match &v {
                Value::Null | Value::Int(0) => {}
                Value::Obj(val) => {
                    let val_cls = vm.heap.class_of(*val)?.to_string();
                    if val_cls != elem_cls && !vm.cp.is_subtype(&val_cls, elem_cls) {
                        return Err(VmExit::Exception(vm.vm_exception(
                            ASC,
                            &format!("{val_cls} cannot be stored into {desc}"),
                        )?));
                    }
                }
                _ => {
                    return Err(VmExit::Exception(vm.vm_exception(
                        ASC,
                        &format!("primitivo não pode ser guardado em {desc}"),
                    )?));
                }
            }
        }
    }
    let slot = match op {
        // tabela validada vs baksmali (rd-dex opcode.rs): 0x4b aput (genérico),
        // 0x4c aput-wide, 0x4d aput-object, 0x4e aput-boolean, 0x4f byte,
        // 0x50 char, 0x51 short — o match antigo tratava 0x4d como float e
        // 0x4e como double (bug latente descoberto pelo store check)
        0x4C => Value::Long(v.as_long()?),             // aput-wide
        0x4E => Value::Int((v.as_int()? != 0) as i32), // aput-boolean
        0x4F => Value::Int(v.as_int()? as i8 as i32),  // aput-byte
        0x50 => Value::Int(v.as_int()? as u16 as i32), // aput-char
        0x51 => Value::Int(v.as_int()? as i16 as i32), // aput-short
        _ => v,                                        // aput / aput-object
    };
    let elems = vm.heap.array_elems_mut(r)?;
    elems[idx as usize] = slot;
    Ok(())
}

fn fill_array(
    vm: &mut Engine,
    arr: crate::heap::ObjRef,
    width: u16,
    count: u32,
    data: &[u8],
) -> Result<(), VmExit> {
    // issue #24: o kind vem do ARRAY (não do width) — d8/Kotlin emitem
    // fill-array-data para float[]/double[] rotineiramente; decodificar os
    // bits com from_bits (não como inteiros)
    let values: Vec<Value> = {
        let (elems, kind) = vm.heap.as_array_elems(arr)?;
        if kind == ElemKind::Obj {
            return Err(err::vm_error(
                "INVALID_FORMAT",
                "fill-array-data em array de objetos (spec proíbe)",
            )
            .into());
        }
        let width_ok = match kind {
            ElemKind::Int => matches!(width, 1 | 2 | 4),
            ElemKind::Long | ElemKind::Double => width == 8,
            ElemKind::Float => width == 4,
            ElemKind::Obj => false,
        };
        if !width_ok {
            return Err(err::vm_error(
                "INVALID_FORMAT",
                "fill-array-data: largura do payload não casa com o array",
            )
            .into());
        }
        if count as usize > elems.len() {
            return Err(VmExit::Exception(Throwable::new(
                AIOOBE,
                format!(
                    "fill-array-data: payload de {count} excede array de {}",
                    elems.len()
                ),
            )));
        }
        // issue #48: [C é SEM sinal — decodificar u16. O desc real do array
        // vem do elem_class guardado no HeapObj (class_of devolve "[I"
        // genérico para todos os integrais primitivos)
        let is_char_array = match vm.heap.get(arr)? {
            crate::heap::HeapObj::Array { elem_class, .. } => {
                elem_class == "C" || elem_class == "[C"
            }
            _ => false,
        };
        let mut values = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let off = i * width as usize;
            let Some(slice) = data.get(off..off + width as usize) else {
                return Err(
                    err::vm_error("INVALID_FORMAT", "fill-array-data: payload truncado").into(),
                );
            };
            values.push(match kind {
                ElemKind::Long => {
                    Value::Long(i64::from_le_bytes(slice.try_into().unwrap_or([0; 8])))
                }
                ElemKind::Float => Value::Float(f32::from_bits(u32::from_le_bytes(
                    slice.try_into().unwrap_or([0; 4]),
                ))),
                ElemKind::Double => Value::Double(f64::from_bits(u64::from_le_bytes(
                    slice.try_into().unwrap_or([0; 8]),
                ))),
                _ => match width {
                    1 => Value::Int(slice[0] as i8 as i32),
                    // issue #48: char[] decodifica SEM sinal (u16 — payload
                    // 0xFFFD é U+FFFD/65533, não −3); short[] continua i16
                    2 if is_char_array => {
                        Value::Int(u16::from_le_bytes([slice[0], slice[1]]) as i32)
                    }
                    2 => Value::Int(i16::from_le_bytes([slice[0], slice[1]]) as i32),
                    4 => Value::Int(i32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])),
                    _ => {
                        return Err(err::vm_error(
                            "INVALID_FORMAT",
                            "fill-array-data: combinação width/kind inválida",
                        )
                        .into())
                    }
                },
            });
        }
        values
    };
    let elems = vm.heap.array_elems_mut(arr)?;
    for (i, v) in values.into_iter().enumerate() {
        elems[i] = v;
    }
    Ok(())
}

// ── invokes ─────────────────────────────────────────────────────────────────

fn do_invoke(
    vm: &mut Engine,
    dex_idx: usize,
    frame_super: Option<&str>,
    invoke_op: u8,
    method_idx: u32,
    slots: &[Value],
) -> Result<Value, VmExit> {
    let mref = vm.cp.method_ref(dex_idx, method_idx)?;
    // Host dispatch (intrinsics, m8/m9, framework, native loader) receives
    // LOGICAL arguments: one Value per parameter. Register slots carry a
    // WideHi placeholder after each long/double, so before this a host
    // reading a parameter after a wide one saw WideHi. For example
    // AtomicLongFieldUpdater.compareAndSet(o, J, J) wrote 0 on every CAS,
    // which made kotlinx.coroutines' lock-free queue spin forever.
    // DEX callees still get the raw register form.
    let raw = slots;
    // Fast path (ADR 008 Stage 1b): most invokes carry no wide values, so
    // the register slots are already the logical arguments — no allocation.
    let logical: Vec<Value>;
    let slots: &[Value] = if slots.iter().any(|v| matches!(v, Value::WideHi)) {
        logical = slots
            .iter()
            .filter(|v| !matches!(v, Value::WideHi))
            .cloned()
            .collect();
        &logical
    } else {
        slots
    };
    // DEX opcodes (Dalvik spec): 6E virtual · 6F super · 70 direct ·
    // 71 static · 72 interface · 74 virtual/range · 75 super/range ·
    // 76 direct/range · 77 static/range · 78 interface/range.
    // M8 (37º gap, real no launch do primftpd): as variantes /range de
    // virtual e direct estavam TROCADAS (0x74 marcado como direct) — um
    // invoke-virtual/range despachava NÃO-virtualmente (classe declarada) e
    // um invoke-direct/range despachava VIRTUALMENTE (classe do receiver),
    // fazendo ctor Kotlin `super(...)` (FunctionReferenceImpl.<init> da
    // lambda de OnBackPressedDispatcher.addCallback) re-executar o próprio
    // <init> em recursão infinita até VM_STACK_OVERFLOW/abort nativo.
    let is_static = matches!(invoke_op, 0x71 | 0x77);
    let is_super = matches!(invoke_op, 0x6F | 0x75);
    let is_direct = matches!(invoke_op, 0x70 | 0x76);

    // static: intrínsecos de plataforma primeiro
    if is_static {
        if let Some(v) =
            intrinsics::call_native_loader(vm, &mref.class, &mref.name, &mref.proto, slots)?
        {
            return Ok(v);
        }
        #[cfg(feature = "advanced-components")]
        if let Some(v) = crate::m9::call_static(vm, &mref.class, &mref.name, &mref.proto, slots)? {
            return Ok(v);
        }
        #[cfg(feature = "virtual-devices")]
        if let Some(v) = crate::m10::call_static(vm, &mref.class, &mref.name, &mref.proto, slots)? {
            return Ok(v);
        }
        if let Some(v) =
            intrinsics::call_static_intrinsic(vm, &mref.class, &mref.name, &mref.proto, slots)?
        {
            return Ok(v);
        }
        // M8: subsistemas de compat real (OkHttp/WorkManager/Room/Uri…)
        if let Some(v) = crate::m8::call_m8_static(vm, &mref.class, &mref.name, &mref.proto, slots)?
        {
            return Ok(v);
        }
        // Apêndice D: shims host AndroidX (FileProvider, AppInitializer,
        // DataStore keys, MultiDex.install…) — `None` mantém o DEX real.
        if let Some(v) =
            crate::androidx::call_static(vm, &mref.class, &mref.name, &mref.proto, slots)?
        {
            return Ok(v);
        }
        // Apêndice E (Google Play Services / Firebase, modo "sem GMS"
        // honesto): só responde por classes GMS/Firebase/Billing, antes do
        // host `Landroid` genérico — `None` continua para DEX.
        if let Some(v) = crate::gms::call_static(vm, &mref.class, &mref.name, &mref.proto, slots)? {
            return Ok(v);
        }
        // M3: classes android/* são HOST (framework.rs) — nunca estão no DEX
        if mref.class.starts_with("Landroid/") {
            return match crate::framework::call_host_static(
                vm,
                &mref.class,
                &mref.name,
                &mref.proto,
                slots,
            )? {
                Some(v) => Ok(v),
                None => {
                    Err(
                        crate::classpath::unresolved_method(&mref.class, &mref.name, &mref.proto)
                            .into(),
                    )
                }
            };
        }
        vm.ensure_initialized(&mref.class)?;
        let Some((d2, def, m)) = vm.cp.resolve_method(&mref.class, &mref.name, &mref.proto) else {
            return Err(
                crate::classpath::unresolved_method(&mref.class, &mref.name, &mref.proto).into(),
            );
        };
        if m.access_flags & ACC_STATIC == 0 {
            return Err(err::vm_error(
                "INVALID_FORMAT",
                format!(
                    "invoke-static sobre método não-static {}",
                    vm.method_label(d2, &m)
                ),
            )
            .into());
        }
        return vm.call(d2, def, &m, raw.to_vec());
    }

    // não-static: receiver é o primeiro slot
    let Some(first) = slots.first() else {
        return Err(err::vm_error("INVALID_FORMAT", "invoke não-static sem receiver").into());
    };
    // java/lang/Object.<init> é no-op (raiz de toda hierarquia de usuário)
    if mref.class == "Ljava/lang/Object;" && mref.name == "<init>" {
        return Ok(Value::Null);
    }
    // issue #25: construtores são “ação ativa” (JLS 12.4.2) — normalmente já
    // inicializada pelo new-instance imediatamente anterior (no-op aqui)
    if is_direct && mref.name == "<init>" {
        vm.ensure_initialized(&mref.class)?;
    }
    let recv = match first.as_ref() {
        Ok(None) | Err(_) => {
            return Err(VmExit::Exception(vm.vm_exception(
                NPE,
                &format!(
                    "invoke em receiver null: {}->{}{} (opcode 0x{invoke_op:02x})",
                    mref.class, mref.name, mref.proto
                ),
            )?));
        }
        Ok(Some(r)) => r,
    };
    let recv_class = vm.heap.class_of(recv)?.to_string();

    // `invoke-super Object.equals` from an AndroidX base implementation
    // (Fragment.equals is a final identity wrapper) must execute Object's
    // identity semantics, not resolve the same DEX override recursively.
    if is_super
        && mref.class == "Ljava/lang/Object;"
        && mref.name == "equals"
        && mref.proto == "(Ljava/lang/Object;)Z"
    {
        let other = slots.get(1).and_then(|slot| slot.as_ref().ok()).flatten();
        return Ok(Value::Int((other == Some(recv)) as i32));
    }

    // intrínseco de instância (String/StringBuilder/Throwable/Object)
    if let Some(v) = intrinsics::call_instance_intrinsic_mref(
        vm,
        &recv_class,
        &mref.class,
        &mref.name,
        &mref.proto,
        recv,
        &slots[1..],
    )? {
        return Ok(v);
    }
    // Covariant bridge: `Appendable.append(..)Appendable` on a host
    // StringBuilder (or any host method whose declared return is a supertype
    // of the receiver) matches the intrinsic declared with the receiver type.
    if let Some((params, ret)) = mref.proto.rsplit_once(')') {
        if ret.starts_with('L')
            && ret != recv_class
            && vm.cp.find_class(&recv_class).is_none()
            && vm.cp.is_subtype(&recv_class, ret)
        {
            let bridged = format!("{params}){recv_class}");
            if let Some(v) = intrinsics::call_instance_intrinsic_mref(
                vm,
                &recv_class,
                &recv_class,
                &mref.name,
                &bridged,
                recv,
                &slots[1..],
            )? {
                return Ok(v);
            }
        }
    }

    #[cfg(feature = "advanced-components")]
    if let Some(v) =
        crate::m9::call_instance(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
    {
        return Ok(v);
    }

    #[cfg(feature = "virtual-devices")]
    if let Some(v) =
        crate::m10::call_instance(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
    {
        return Ok(v);
    }

    // M8: compat real — host sequestra só os métodos listados em m8.rs;
    // `None` cai para o DEX real da biblioteca (androidx/okhttp no APK).
    if let Some(v) =
        crate::m8::call_m8_instance(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
    {
        return Ok(v);
    }

    // Apêndice D: shims host AndroidX por módulo (lifecycle, livedata,
    // viewmodel, collection, datastore valores, work Data, navigation,
    // startup, biometria, futures) — `None` mantém o DEX real da lib.
    if let Some(v) =
        crate::androidx::call_instance(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
    {
        return Ok(v);
    }
    // Apêndice E: objetos seguidos pelo shim GMS/Firebase e classes
    // `Lcom/google/android/gms/`, `Lcom/google/firebase/`,
    // `Lcom/android/billingclient/` — `None` continua para o host/DEX.
    if let Some(v) =
        crate::gms::call_instance(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
    {
        return Ok(v);
    }

    // M3: framework host (Activity/View/TextView/Button/LinearLayout/
    // Handler/Intent/Bundle) — a classe DECLARANTE do method ref é android/*
    if mref.class.starts_with("Landroid/") {
        if !is_direct && !is_super {
            if let Some((d2, def, m)) = vm.cp.resolve_method(&recv_class, &mref.name, &mref.proto) {
                return vm.call(d2, def, &m, raw.to_vec());
            }
        }
        if let Some(value) =
            inherited_android_host(vm, &mref.class, &mref.name, &mref.proto, recv, &slots[1..])?
        {
            return Ok(value);
        }
        // Generic property storage is strictly last: it must not swallow
        // inherited View IDs, EditText text, or Activity/Context state.
        if let Some(value) = crate::view_props::fallback(
            vm,
            &mref.class,
            &mref.name,
            &mref.proto,
            recv,
            &slots[1..],
        )? {
            return Ok(value);
        }
        return Err(crate::classpath::unresolved_method_on(
            &mref.class,
            &recv_class,
            &mref.name,
            &mref.proto,
        )
        .into());
    }

    // classe inicial da resolução
    let start_class = if is_super {
        // Semântica DEX (Dalvik): invoke-super inicia a busca na SUPER da
        // classe que DECLARA o método em execução (defining class) — não em
        // `super(mref.class)`. Bug corrigido (M8, 36º gap): antes resolvia a
        // partir da super da classe referenciada, pulando um nível inteiro da
        // hierarquia — no launch real do primftpd isso fazia
        // FragmentActivity.onCreate ignorar androidx/activity/
        // ComponentActivity.onCreate (perdendo dispatchOnContextAvailable/
        // attachHost) e cair direto em androidx/core/app/ComponentActivity.
        frame_super.map_or_else(
            || {
                vm.cp
                    .superclass_of(&mref.class)
                    .unwrap_or_else(|| mref.class.clone())
            },
            |s| s.to_string(),
        )
    } else if is_direct {
        // direct (<init>/private): NÃO despacha virtualmente
        mref.class.clone()
    } else {
        recv_class.clone()
    };

    let Some((d2, def, m)) = vm.cp.resolve_method(&start_class, &mref.name, &mref.proto) else {
        // D8 may use an app subclass as the inherited Android method owner.
        // DEX overrides above always win; direct app constructors stay direct.
        if !is_direct || is_super {
            if let Some(value) = inherited_android_host(
                vm,
                &start_class,
                &mref.name,
                &mref.proto,
                recv,
                &slots[1..],
            )? {
                return Ok(value);
            }
        }
        return Err(
            crate::classpath::unresolved_method(&start_class, &mref.name, &mref.proto).into(),
        );
    };
    vm.call(d2, def, &m, raw.to_vec())
}

/// Resolve only real inherited Android hosts; never invent an app method.
pub(crate) fn inherited_android_host(
    vm: &mut Engine,
    start: &str,
    name: &str,
    sig: &str,
    recv: crate::heap::ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let mut ancestor = start.to_string();
    let mut property_fallback_class = None;
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..16 {
        if !visited.insert(ancestor.clone()) {
            break;
        }
        if ancestor.starts_with("Landroid/") {
            property_fallback_class.get_or_insert_with(|| ancestor.clone());
            #[cfg(feature = "advanced-components")]
            if let Some(value) = crate::m9::call_instance(vm, &ancestor, name, sig, recv, args)? {
                return Ok(Some(value));
            }
            #[cfg(feature = "virtual-devices")]
            if let Some(value) = crate::m10::call_instance(vm, &ancestor, name, sig, recv, args)? {
                return Ok(Some(value));
            }
            if let Some(value) = crate::m8::call_m8_instance(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) =
                crate::framework::call_host_instance(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) =
                crate::view_attach::instance_intrinsic(vm, &ancestor, name, sig, recv, args)?
            {
                return Ok(Some(value));
            }
            if let Some(value) = crate::platform_base::instance_intrinsic(&ancestor, name, sig) {
                return Ok(Some(value));
            }
        }
        let next = vm.cp.superclass_of(&ancestor).or_else(|| {
            crate::classpath::builtin_hierarchy(&ancestor).and_then(|chain| {
                chain
                    .iter()
                    .copied()
                    .find(|s| *s != ancestor)
                    .map(str::to_string)
            })
        });
        match next {
            Some(next) => ancestor = next,
            None => break,
        }
    }
    if let Some(class) = property_fallback_class {
        // Try generic properties only after every concrete Android host has had
        // a chance to handle the call (e.g. View.setId must not be shadowed by
        // a generic setId property on an intermediate EditText ancestor).
        if let Some(value) = crate::view_props::fallback(vm, &class, name, sig, recv, args)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// Renderização do resultado para humano/JSON do CLI (formato Java).
pub fn render_result(v: &Value, return_type: &str) -> String {
    match v {
        Value::Int(i) => match return_type {
            "Z" => format!("{}", *i != 0),
            "C" => char::from_u32(*i as u32).unwrap_or('\u{FFFD}').to_string(),
            _ => format!("{i}"),
        },
        Value::Long(l) => format!("{l}"),
        Value::Float(f) => crate::repr::java_float(*f),
        Value::Double(d) => crate::repr::java_double(*d),
        Value::Obj(r) => vm_obj_repr(r),
        Value::Null => "null".to_string(),
        Value::WideHi => "<wide-hi>".to_string(),
        Value::StrPlaceholder(_) => "<string>".to_string(),
    }
}

fn vm_obj_repr(r: &crate::heap::ObjRef) -> String {
    format!("<object #{r}>")
}

#[cfg(test)]
mod fetch_tests {
    use super::*;

    #[test]
    fn sequential_cursor_matches_binary_lookup_for_branches_handlers_and_bad_pcs() {
        let instructions: Vec<_> = [0, 1, 3, 5, 6, 18, 20, 100]
            .into_iter()
            .map(|pc| {
                (
                    pc,
                    Insn {
                        opcode: 0,
                        fmt: rd_dex::opcode::Format::F10x,
                        size: 1,
                        kind: Kind::Plain,
                    },
                )
            })
            .collect();
        for initial in [0, 1, 7, 8, usize::MAX] {
            let mut cursor = initial;
            for pc in [0, 1, 3, 18, 20, 100, 0, 5, 6, 2, 0, usize::MAX, 100, 101] {
                let expected = lookup(&instructions, pc);
                let previous = cursor;
                let actual = lookup_next(&instructions, pc, &mut cursor);
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                match actual {
                    Ok(i) => assert_eq!(cursor, i + 1),
                    Err(_) => assert_eq!(cursor, previous, "failed fetch must not advance"),
                }
            }
        }
        let mut cursor = 0;
        assert_eq!(
            format!("{:?}", lookup_next(&[], 0, &mut cursor)),
            format!("{:?}", lookup(&[], 0))
        );
    }

    #[test]
    fn sequential_cursor_differential_random_walk() {
        let instructions: Vec<_> = (0..1024)
            .map(|i| {
                (
                    i * 3,
                    Insn {
                        opcode: 0,
                        fmt: rd_dex::opcode::Format::F10x,
                        size: 3,
                        kind: Kind::Plain,
                    },
                )
            })
            .collect();
        let mut state = 0x1234_5678u64;
        let mut cursor = 0;
        for _ in 0..32768 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let pc = if state % 4 != 0 && cursor < instructions.len() {
                instructions[cursor].0
            } else {
                (state as usize) % (instructions.len() * 3 + 16)
            };
            let expected = lookup(&instructions, pc);
            let previous = cursor;
            let actual = lookup_next(&instructions, pc, &mut cursor);
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            match actual {
                Ok(i) => assert_eq!(cursor, i + 1),
                Err(_) => assert_eq!(cursor, previous, "failed fetch must not advance"),
            }
        }
    }
}
