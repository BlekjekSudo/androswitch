//! Intrínsecos de plataforma — métodos de java.lang que o M2 executa
//! nativamente (as classes não estão no DEX). Qualquer método de plataforma
//! não listado responde `NOT_IMPLEMENTED` com o alvo nomeado (Lei 1).

use crate::engine::Engine;
use crate::err::{not_implemented, Throwable, VmExit};
use crate::heap::HeapObj;
use crate::repr;
use crate::value::Value;

const STRING: &str = "Ljava/lang/String;";
const SB: &str = "Ljava/lang/StringBuilder;";
const OBJECT: &str = "Ljava/lang/Object;";
const MATH: &str = "Ljava/lang/Math;";
const INTEGER: &str = "Ljava/lang/Integer;";
const LONG: &str = "Ljava/lang/Long;";
const PRINTSTREAM: &str = "Ljava/io/PrintStream;";
const NFE: &str = "Ljava/lang/NumberFormatException;";
const ARITH: &str = "Ljava/lang/ArithmeticException;";

/// Semântica FLOOR do Java (issue #48): `div_euclid`/`rem_euclid` do Rust são
/// SEMÂNTICA EUCLIDIANA — `floorDiv(4,−3)` Java = −2, `div_euclid` dá −1.
/// floorDiv = truncado com ajuste quando o resto é não-zero e tem sinal
/// oposto ao divisor; floorMod = a − floorDiv·b (com wrapping nos cantos
/// MIN/−1, que a JVM também sofre por contrato).
fn java_floor_div_i32(a: i32, b: i32) -> i32 {
    let q = a.wrapping_div(b);
    let r = a.wrapping_rem(b);
    if r != 0 && ((r < 0) != (b < 0)) {
        q.wrapping_sub(1)
    } else {
        q
    }
}
fn java_floor_mod_i32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(java_floor_div_i32(a, b).wrapping_mul(b))
}
fn java_floor_div_i64(a: i64, b: i64) -> i64 {
    let q = a.wrapping_div(b);
    let r = a.wrapping_rem(b);
    if r != 0 && ((r < 0) != (b < 0)) {
        q.wrapping_sub(1)
    } else {
        q
    }
}
fn java_floor_mod_i64(a: i64, b: i64) -> i64 {
    a.wrapping_sub(java_floor_div_i64(a, b).wrapping_mul(b))
}

/// toString de um objeto para print/println/append(Object)/String.valueOf
/// (issues #43/#48): Str → conteúdo; boxes de plataforma → valor; classe de
/// usuário que DECLARA toString → executa no DEX (pode lançar exceção Java);
/// caso contrário → `Classe@id` (contrato Object).
pub(crate) fn object_to_string(vm: &mut Engine, r: crate::heap::ObjRef) -> Result<String, VmExit> {
    if let Ok(HeapObj::Str(s)) = vm.heap.get(r) {
        return Ok(s.clone());
    }
    let cls = vm.heap.class_of(r)?.to_string();
    match call_instance_intrinsic(vm, &cls, "toString", "()Ljava/lang/String;", r, &[])? {
        Some(Value::Obj(sref)) => Ok(vm.heap.as_str(sref)?.to_string()),
        Some(Value::Null) => Ok("null".to_string()),
        Some(_) => Err(crate::err::vm_error(
            "INVALID_FORMAT",
            format!("{cls}->toString() retornou não-string"),
        )
        .into()),
        None => {
            // classe declara toString no DEX: executa de verdade
            if let Some((dex_idx, def, m)) =
                vm.cp
                    .resolve_method(&cls, "toString", "()Ljava/lang/String;")
            {
                match vm.call(dex_idx, def, &m, vec![Value::Obj(r)])? {
                    Value::Obj(sref) => Ok(vm.heap.as_str(sref)?.to_string()),
                    Value::Null => Ok("null".to_string()),
                    _ => Err(crate::err::vm_error(
                        "INVALID_FORMAT",
                        format!("{cls}->toString() retornou não-string"),
                    )
                    .into()),
                }
            } else {
                Ok(default_to_string(&cls, r))
            }
        }
    }
}

/// Escreve no stdout/stderr do host. PrintStream Java NUNCA lança IOException
/// (spec de java.io.PrintStream) — engolir falha de E/S é a semântica correta.
fn host_print(fd: i32, s: &str) {
    use std::io::Write;
    if fd == 2 {
        let _ = std::io::stderr().write_all(s.as_bytes());
        let _ = std::io::stderr().flush();
        return;
    }
    // issue #55: fd=1 do app segue o modo configurado — o CLI roteia para o
    // stderr do host (stdout reservado ao artefato do comando: dump XML/JSON);
    // default da LIB continua stdout (comportamento Java real).
    let mode = app_stdout_mode();
    match mode {
        HostOut::Std => {
            let _ = std::io::stdout().write_all(s.as_bytes());
            let _ = std::io::stdout().flush();
        }
        HostOut::Stderr => {
            let _ = std::io::stderr().write_all(s.as_bytes());
            let _ = std::io::stderr().flush();
        }
        HostOut::Buffer(buf) => {
            buf.lock()
                .expect("host out buffer")
                .extend_from_slice(s.as_bytes());
        }
    }
}

// ── issue #55: roteamento da saída do app ───────────────────────────────────

/// Destino do fd=1 do app (System.out). fd=2 (System.err) é SEMPRE o stderr
/// do host — Android real manda app stdout/Log para o logcat, nunca para o
/// stream do dump.
#[derive(Clone, Default, Debug)]
pub enum HostOut {
    /// stdout do host (default da biblioteca — semântica Java).
    #[default]
    Std,
    /// stderr do host (modo CLI — stdout reservado ao artefato do comando).
    Stderr,
    /// Buffer em memória (testes — assert do conteúdo impresso).
    Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>),
}

static APP_STDOUT: std::sync::Mutex<HostOut> = std::sync::Mutex::new(HostOut::Std);

fn app_stdout_mode() -> HostOut {
    APP_STDOUT.lock().expect("app stdout mode").clone()
}

/// Configura o destino do System.out do app (issue #55).
pub fn set_app_stdout(mode: HostOut) {
    *APP_STDOUT.lock().expect("app stdout mode") = mode;
}

/// Roteia System.out do app para o STDERR do host (contrato de máquina:
/// stdout reservado ao artefato do comando — issue #55). Idempotente.
pub fn route_app_stdout_to_stderr() {
    set_app_stdout(HostOut::Stderr);
}

/// Intrínseco STATIC: chamado de invoke-static (e do entrypoint público).
/// `Ok(None)` = nenhum intrínseco casa → segue resolução normal no DEX.
pub fn call_static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name, sig) {
        (MATH, "abs", "(I)I") => Ok(Some(Value::Int(args[0].as_int()?.wrapping_abs()))),
        (MATH, "abs", "(J)J") => Ok(Some(Value::Long(args[0].as_long()?.wrapping_abs()))),
        (MATH, "abs", "(F)F") => Ok(Some(Value::Float(args[0].as_float()?.abs()))),
        (MATH, "abs", "(D)D") => Ok(Some(Value::Double(args[0].as_double()?.abs()))),
        (MATH, "min", "(II)I") => Ok(Some(Value::Int(args[0].as_int()?.min(args[1].as_int()?)))),
        (MATH, "max", "(II)I") => Ok(Some(Value::Int(args[0].as_int()?.max(args[1].as_int()?)))),
        (MATH, "min", "(JJ)J") => Ok(Some(Value::Long(
            args[0].as_long()?.min(args[1].as_long()?),
        ))),
        (MATH, "max", "(JJ)J") => Ok(Some(Value::Long(
            args[0].as_long()?.max(args[1].as_long()?),
        ))),
        (MATH, "min", "(FF)F") => Ok(Some(Value::Float(java_min_f32(
            args[0].as_float()?,
            args[1].as_float()?,
        )))),
        (MATH, "max", "(FF)F") => Ok(Some(Value::Float(java_max_f32(
            args[0].as_float()?,
            args[1].as_float()?,
        )))),
        (MATH, "min", "(DD)D") => Ok(Some(Value::Double(java_min_f64(
            args[0].as_double()?,
            args[1].as_double()?,
        )))),
        (MATH, "max", "(DD)D") => Ok(Some(Value::Double(java_max_f64(
            args[0].as_double()?,
            args[1].as_double()?,
        )))),
        (MATH, "sqrt", "(D)D") => Ok(Some(Value::Double(args[0].as_double()?.sqrt()))),
        (MATH, "pow", "(DD)D") => Ok(Some(Value::Double(java_pow(
            args[0].as_double()?,
            args[1].as_double()?,
        )))),
        (MATH, "floor", "(D)D") => Ok(Some(Value::Double(args[0].as_double()?.floor()))),
        (MATH, "ceil", "(D)D") => Ok(Some(Value::Double(args[0].as_double()?.ceil()))),
        // issue #27: rotina emitida por d8 em código real
        (MATH, "round", "(F)I") => {
            let v = args[0].as_float()?;
            Ok(Some(Value::Int(java_round_f32(v))))
        }
        (MATH, "round", "(D)J") => {
            let v = args[0].as_double()?;
            Ok(Some(Value::Long(java_round_f64(v))))
        }
        (MATH, "floorDiv", "(II)I") => {
            let a = args[0].as_int()?;
            let b = args[1].as_int()?;
            if b == 0 {
                return Err(VmExit::Exception(Throwable::new(ARITH, "divide by zero")));
            }
            // issue #48: floor real do Java (não euclidiano)
            Ok(Some(Value::Int(java_floor_div_i32(a, b))))
        }
        (MATH, "floorMod", "(II)I") => {
            let a = args[0].as_int()?;
            let b = args[1].as_int()?;
            if b == 0 {
                return Err(VmExit::Exception(Throwable::new(ARITH, "divide by zero")));
            }
            Ok(Some(Value::Int(java_floor_mod_i32(a, b))))
        }
        (MATH, "floorDiv", "(JJ)J") => {
            let a = args[0].as_long()?;
            let b = args[1].as_long()?;
            if b == 0 {
                return Err(VmExit::Exception(Throwable::new(ARITH, "divide by zero")));
            }
            Ok(Some(Value::Long(java_floor_div_i64(a, b))))
        }
        (MATH, "floorMod", "(JJ)J") => {
            let a = args[0].as_long()?;
            let b = args[1].as_long()?;
            if b == 0 {
                return Err(VmExit::Exception(Throwable::new(ARITH, "divide by zero")));
            }
            Ok(Some(Value::Long(java_floor_mod_i64(a, b))))
        }
        (MATH, "signum", "(D)D") => {
            let v = args[0].as_double()?;
            Ok(Some(Value::Double(if v.is_nan() {
                f64::NAN
            } else if v == 0.0 {
                v
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            })))
        }
        (INTEGER, "parseInt", "(Ljava/lang/String;)I") => {
            // issue #37: parseInt(null) → NumberFormatException("null") na JVM
            // (não NPE — o javadoc de Integer.parseInt especifica NFE)
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(_) => string_arg(vm, &args[0])?,
            };
            parse_int(&s)
                .map(|v| Ok(Some(Value::Int(v))))
                .map_err(|e| {
                    VmExit::Exception(Throwable::new("Ljava/lang/NumberFormatException;", e))
                })?
        }
        (LONG, "parseLong", "(Ljava/lang/String;)J") => {
            // issue #48: parseLong(null) → NumberFormatException("null") como
            // parseInt (o javadoc de Long.parseLong especifica NFE, não NPE)
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(_) => string_arg(vm, &args[0])?,
            };
            parse_long(&s)
                .map(|v| Ok(Some(Value::Long(v))))
                .map_err(|e| VmExit::Exception(Throwable::new(NFE, e)))?
        }
        (STRING, "valueOf", "(I)Ljava/lang/String;") => {
            let v = args[0].as_int()?;
            Ok(Some(Value::Obj(alloc_string(vm, v.to_string())?)))
        }
        // issue #27: autoboxing — d8 emite Integer.valueOf para TODO List<Integer>/
        // Collections; sem isto, código Java real falha logo no primeiro uso
        (INTEGER, "valueOf", "(I)Ljava/lang/Integer;") => {
            let v = args[0].as_int()?;
            // issue #49: OOM → OutOfMemoryError capturável
            let r = vm
                .heap
                .alloc_instance(
                    INTEGER.to_string(),
                    vec![("value".to_string(), Value::Int(v))],
                )
                .map_err(crate::err::oom_throwable)?;
            Ok(Some(Value::Obj(r)))
        }
        (INTEGER, "toString", "(I)Ljava/lang/String;") => {
            let v = args[0].as_int()?;
            Ok(Some(Value::Obj(alloc_string(vm, v.to_string())?)))
        }
        (INTEGER, "compare", "(II)I") => {
            let a = args[0].as_int()?;
            let b = args[1].as_int()?;
            Ok(Some(Value::Int(match a.cmp(&b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })))
        }
        (LONG, "valueOf", "(J)Ljava/lang/Long;") => {
            let v = args[0].as_long()?;
            // issue #49: OOM → OutOfMemoryError capturável
            let r = vm
                .heap
                .alloc_instance(
                    LONG.to_string(),
                    vec![("value".to_string(), Value::Long(v))],
                )
                .map_err(crate::err::oom_throwable)?;
            Ok(Some(Value::Obj(r)))
        }
        (STRING, "valueOf", "(J)Ljava/lang/String;") => {
            let v = args[0].as_long()?;
            Ok(Some(Value::Obj(alloc_string(vm, v.to_string())?)))
        }
        (STRING, "valueOf", "(Z)Ljava/lang/String;") => {
            let v = args[0].as_int()? != 0;
            Ok(Some(Value::Obj(alloc_string(vm, v.to_string())?)))
        }
        (STRING, "valueOf", "(C)Ljava/lang/String;") => {
            let v = char::from_u32(args[0].as_int()? as u32).unwrap_or('\u{FFFD}');
            Ok(Some(Value::Obj(alloc_string(vm, v.to_string())?)))
        }
        (STRING, "valueOf", "(F)Ljava/lang/String;") => {
            let v = args[0].as_float()?;
            Ok(Some(Value::Obj(alloc_string(vm, repr::java_float(v))?)))
        }
        (STRING, "valueOf", "(D)Ljava/lang/String;") => {
            let v = args[0].as_double()?;
            Ok(Some(Value::Obj(alloc_string(vm, repr::java_double(v))?)))
        }
        (STRING, "valueOf", "(Ljava/lang/Object;)Ljava/lang/String;") => {
            // issues #43/#48: null → "null"; String → conteúdo; boxes → valor;
            // classe de usuário com toString declarado → executa; resto → Cls@id
            match args[0].as_ref()? {
                None => Ok(Some(Value::Obj(alloc_string(vm, "null".into())?))),
                Some(r) => {
                    let s = object_to_string(vm, r)?;
                    Ok(Some(Value::Obj(alloc_string(vm, s)?)))
                }
            }
        }
        _ => {
            if class.starts_with("Ljava/") || class.starts_with("Ljavax/") {
                return Err(not_implemented(format!("intrínseco {class}->{name}{sig}")).into());
            }
            Ok(None)
        }
    }
}

/// Intrínseco DE INSTÂNCIA (invoke-virtual/direct sobre objetos da VM).
/// `receiver` já é Some(ref) — NPE foi tratada pelo chamador.
/// `Ok(None)` = sem intrínseco → dispatch normal no DEX (classe do usuário).
pub fn call_instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: crate::heap::ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (recv_class, name, sig) {
        (OBJECT, "<init>", "()V") => Ok(Some(Value::Null)), // ctor de Object = no-op

        (SB, "<init>", "()V") => {
            let buf = vm.heap.alloc_string(String::new())?;
            crate::err::put_field_result(vm.heap.put_field(recv, "buf", Value::Obj(buf)))?;
            Ok(Some(Value::Null))
        }
        (SB, "<init>", "(Ljava/lang/String;)V") => {
            let s = string_arg(vm, &args[0])?;
            let buf = vm.heap.alloc_string(s)?;
            crate::err::put_field_result(vm.heap.put_field(recv, "buf", Value::Obj(buf)))?;
            Ok(Some(Value::Null))
        }
        (SB, "append", "(I)Ljava/lang/StringBuilder;") => {
            sb_append(vm, recv, &args[0].as_int()?.to_string())?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(J)Ljava/lang/StringBuilder;") => {
            sb_append(vm, recv, &args[0].as_long()?.to_string())?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(Z)Ljava/lang/StringBuilder;") => {
            sb_append(vm, recv, &(args[0].as_int()? != 0).to_string())?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(C)Ljava/lang/StringBuilder;") => {
            let c = char::from_u32(args[0].as_int()? as u32).unwrap_or('\u{FFFD}');
            sb_append(vm, recv, &c.to_string())?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(F)Ljava/lang/StringBuilder;") => {
            sb_append(vm, recv, &repr::java_float(args[0].as_float()?))?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(D)Ljava/lang/StringBuilder;") => {
            sb_append(vm, recv, &repr::java_double(args[0].as_double()?))?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(Ljava/lang/String;)Ljava/lang/StringBuilder;") => {
            // issue #27: Java apendeja o literal "null" (não NPE) — StringBuilder.append((String)null)
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(_) => string_arg(vm, &args[0])?,
            };
            sb_append(vm, recv, &s)?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "append", "(Ljava/lang/Object;)Ljava/lang/StringBuilder;") => {
            // issues #43/#48: append(Object) — null → "null"; String → conteúdo;
            // boxes → valor; toString do usuário EXECUTA (antes caía no default)
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(r) => object_to_string(vm, r)?,
            };
            sb_append(vm, recv, &s)?;
            Ok(Some(Value::Obj(recv)))
        }
        (SB, "toString", "()Ljava/lang/String;") => {
            let s = sb_buf(vm, recv)?;
            let out = vm.heap.alloc_string(s)?;
            Ok(Some(Value::Obj(out)))
        }
        (SB, "length", "()I") => {
            let s = sb_buf(vm, recv)?;
            Ok(Some(Value::Int(s.encode_utf16().count() as i32)))
        }

        (STRING, "length", "()I") => {
            let s = vm.heap.as_str(recv)?.to_string();
            Ok(Some(Value::Int(s.encode_utf16().count() as i32)))
        }
        (STRING, "isEmpty", "()Z") => {
            let s = vm.heap.as_str(recv)?;
            Ok(Some(Value::Int(s.is_empty() as i32)))
        }
        (STRING, "charAt", "(I)C") => {
            let s = vm.heap.as_str(recv)?.to_string();
            let idx = args[0].as_int()? as usize;
            let units: Vec<u16> = s.encode_utf16().collect();
            if idx >= units.len() {
                return Err(exception(
                    vm,
                    "Ljava/lang/StringIndexOutOfBoundsException;",
                    format!("index {idx}, length {}", units.len()),
                )
                .into());
            }
            Ok(Some(Value::Int(units[idx] as i32)))
        }
        (STRING, "hashCode", "()I") => {
            // algoritmo especificado em java.lang.String.hashCode (unidades UTF-16)
            let s = vm.heap.as_str(recv)?.to_string();
            let mut h: i32 = 0;
            for u in s.encode_utf16() {
                h = h.wrapping_mul(31).wrapping_add(u as i32);
            }
            Ok(Some(Value::Int(h)))
        }
        (STRING, "equals", "(Ljava/lang/Object;)Z") => {
            let other = args[0].as_ref()?;
            let v = match other {
                None => false,
                Some(r) => match vm.heap.get(r) {
                    Ok(HeapObj::Str(s)) => vm.heap.as_str(recv)? == s,
                    _ => false,
                },
            };
            Ok(Some(Value::Int(v as i32)))
        }
        (STRING, "concat", "(Ljava/lang/String;)Ljava/lang/String;") => {
            let a = vm.heap.as_str(recv)?.to_string();
            let b = string_arg(vm, &args[0])?;
            let out = vm.heap.alloc_string(format!("{a}{b}"))?;
            Ok(Some(Value::Obj(out)))
        }
        (STRING, "substring", "(I)Ljava/lang/String;") => {
            substring(vm, recv, args[0].as_int()?, None)
        }
        (STRING, "substring", "(II)Ljava/lang/String;") => {
            substring(vm, recv, args[0].as_int()?, Some(args[1].as_int()?))
        }
        // issue #27: cobre usados rotineiramente por d8/código real
        (STRING, "indexOf", "(Ljava/lang/String;)I") => {
            let hay = vm.heap.as_str(recv)?.to_string();
            let needle = string_arg(vm, &args[0])?;
            // índice em unidades UTF-16 (semântica Java)
            Ok(Some(Value::Int(match hay.find(&needle) {
                Some(byte_pos) => hay[..byte_pos].encode_utf16().count() as i32,
                None => -1,
            })))
        }
        (STRING, "startsWith", "(Ljava/lang/String;)Z") => {
            let hay = vm.heap.as_str(recv)?.to_string();
            let needle = string_arg(vm, &args[0])?;
            Ok(Some(Value::Int(hay.starts_with(&needle) as i32)))
        }
        (STRING, "endsWith", "(Ljava/lang/String;)Z") => {
            let hay = vm.heap.as_str(recv)?.to_string();
            let needle = string_arg(vm, &args[0])?;
            Ok(Some(Value::Int(hay.ends_with(&needle) as i32)))
        }
        (STRING, "replace", "(CC)Ljava/lang/String;") => {
            let hay = vm.heap.as_str(recv)?.to_string();
            let from = char::from_u32(args[0].as_int()? as u32).unwrap_or('\u{FFFD}');
            let to = char::from_u32(args[1].as_int()? as u32).unwrap_or('\u{FFFD}');
            let out = hay.replace(from, &to.to_string());
            Ok(Some(Value::Obj(alloc_string(vm, out)?)))
        }

        // exceções construídas por `new` no bytecode do usuário
        ("Ljava/lang/Throwable;", "<init>", "()V") | (_, "<init>", "()V")
            if is_throwable_class(vm, recv_class) =>
        {
            Ok(Some(Value::Null))
        }
        ("Ljava/lang/Throwable;", "<init>", "(Ljava/lang/String;)V")
        | (_, "<init>", "(Ljava/lang/String;)V")
            if is_throwable_class(vm, recv_class) =>
        {
            let msg = match args[0].as_ref()? {
                None => Value::Null,
                Some(_) => Value::Obj(vm.heap.alloc_string(string_arg(vm, &args[0])?)?),
            };
            crate::err::put_field_result(vm.heap.put_field(recv, "message", msg))?;
            Ok(Some(Value::Null))
        }
        (_, "getMessage", "()Ljava/lang/String;") if is_throwable_class(vm, recv_class) => Ok(
            Some(vm.heap.get_field(recv, "message").unwrap_or(Value::Null)),
        ),
        (_, "getLocalizedMessage", "()Ljava/lang/String;")
            if is_throwable_class(vm, recv_class) =>
        {
            Ok(Some(
                vm.heap.get_field(recv, "message").unwrap_or(Value::Null),
            ))
        }

        // issue #27: unboxing (contraparte do valueOf que d8 emite)
        (INTEGER, "intValue", "()I") => {
            let v = vm.heap.get_field(recv, "value")?;
            Ok(Some(Value::Int(v.as_int()?)))
        }
        (LONG, "longValue", "()J") => {
            let v = vm.heap.get_field(recv, "value")?;
            Ok(Some(Value::Long(v.as_long()?)))
        }
        // issue #43: equals/hashCode POR VALOR para as boxes — sem isto,
        // Integer(1000).equals(Integer(1000)) caía no fallback de identidade
        // de Object e dava false (List.contains/Map/Objects.equals errados).
        // Cross-type: Integer(1).equals(Long(1)) = false (spec de Integer.equals).
        (INTEGER, "equals", "(Ljava/lang/Object;)Z") => {
            let mine = vm.heap.get_field(recv, "value")?.as_int()?;
            let v = match args[0].as_ref()? {
                Some(r) => {
                    vm.heap.class_of(r)? == INTEGER
                        && vm.heap.get_field(r, "value")?.as_int()? == mine
                }
                None => false,
            };
            Ok(Some(Value::Int(v as i32)))
        }
        (LONG, "equals", "(Ljava/lang/Object;)Z") => {
            let mine = vm.heap.get_field(recv, "value")?.as_long()?;
            let v = match args[0].as_ref()? {
                Some(r) => {
                    vm.heap.class_of(r)? == LONG
                        && vm.heap.get_field(r, "value")?.as_long()? == mine
                }
                None => false,
            };
            Ok(Some(Value::Int(v as i32)))
        }
        (INTEGER, "hashCode", "()I") => Ok(Some(Value::Int(
            vm.heap.get_field(recv, "value")?.as_int()?,
        ))),
        (LONG, "hashCode", "()I") => {
            // spec Long.hashCode: (int)(value ^ (value >>> 32))
            let v = vm.heap.get_field(recv, "value")?.as_long()? as u64;
            Ok(Some(Value::Int((v ^ (v >> 32)) as i32)))
        }
        // issues #43/#48: System.out/.err — println/print sobre o PrintStream
        // embutido (objeto alocado pelo sget de System.out; interp.rs). PrintStream
        // nunca lança — falha de E/S é engolida (host_print).
        (PRINTSTREAM, "println", "()V") => {
            host_print(ps_fd(vm, recv)?, "\n");
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(I)V") => {
            let s = format!("{}\n", args[0].as_int()?);
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(J)V") => {
            let s = format!("{}\n", args[0].as_long()?);
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(Z)V") => {
            let s = format!("{}\n", args[0].as_int()? != 0);
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(C)V") => {
            let c = char::from_u32(args[0].as_int()? as u32).unwrap_or('\u{FFFD}');
            let s = format!("{c}\n");
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(Ljava/lang/String;)V") => {
            let mut s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(_) => string_arg(vm, &args[0])?,
            };
            s.push('\n');
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "println", "(Ljava/lang/Object;)V") => {
            let mut s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(r) => object_to_string(vm, r)?,
            };
            s.push('\n');
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(Ljava/lang/String;)V") => {
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(_) => string_arg(vm, &args[0])?,
            };
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(Ljava/lang/Object;)V") => {
            let s = match args[0].as_ref()? {
                None => "null".to_string(),
                Some(r) => object_to_string(vm, r)?,
            };
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(I)V") => {
            let s = args[0].as_int()?.to_string();
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(J)V") => {
            let s = args[0].as_long()?.to_string();
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(Z)V") => {
            let s = (args[0].as_int()? != 0).to_string();
            host_print(ps_fd(vm, recv)?, &s);
            Ok(Some(Value::Null))
        }
        (PRINTSTREAM, "print", "(C)V") => {
            let c = char::from_u32(args[0].as_int()? as u32).unwrap_or('\u{FFFD}');
            host_print(ps_fd(vm, recv)?, &c.to_string());
            Ok(Some(Value::Null))
        }
        (INTEGER, "toString", "()Ljava/lang/String;") => {
            let v = vm.heap.get_field(recv, "value")?;
            Ok(Some(Value::Obj(alloc_string(vm, v.as_int()?.to_string())?)))
        }
        (LONG, "toString", "()Ljava/lang/String;") => {
            let v = vm.heap.get_field(recv, "value")?;
            Ok(Some(Value::Obj(alloc_string(
                vm,
                v.as_long()?.to_string(),
            )?)))
        }
        (STRING, "toString", "()Ljava/lang/String;") => {
            // toString de String retorna a própria string (antes do fallback
            // de identidade de Object, que seria errado aqui)
            let s = vm.heap.as_str(recv)?.to_string();
            Ok(Some(Value::Obj(alloc_string(vm, s)?)))
        }

        // issue #27: métodos de Object para classes de usuário — o dispatch
        // antigo morria com NOT_FOUND em java/lang/Object. Verifica primeiro
        // se a classe do usuário NÃO declara o método (aí usa a identidade;
        // se declara, cai no dispatch DEX normal)
        (_, "equals", "(Ljava/lang/Object;)Z")
        | (_, "hashCode", "()I")
        | (_, "toString", "()Ljava/lang/String;")
            if vm.cp.resolve_method(recv_class, name, sig).is_none() =>
        {
            match (name, sig) {
                ("equals", "(Ljava/lang/Object;)Z") => {
                    let other = args[0].as_ref()?;
                    Ok(Some(Value::Int((other == Some(recv)) as i32)))
                }
                ("hashCode", "()I") => Ok(Some(Value::Int(recv as i32))),
                ("toString", "()Ljava/lang/String;") => Ok(Some(Value::Obj(alloc_string(
                    vm,
                    default_to_string(recv_class, recv),
                )?))),
                _ => Ok(None),
            }
        }

        _ => Ok(None),
    }
}

/// fd embutido no objeto PrintStream (1 = stdout, 2 = stderr).
fn ps_fd(vm: &Engine, recv: crate::heap::ObjRef) -> Result<i32, VmExit> {
    Ok(vm.heap.get_field(recv, "fd")?.as_int()?)
}

/// toString default da JVM: `Classe@hash` (hex da identidade).
fn default_to_string(class_desc: &str, r: crate::heap::ObjRef) -> String {
    let name = class_desc
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".");
    format!("{name}@{r:x}")
}

fn is_throwable_class(vm: &Engine, class: &str) -> bool {
    vm.cp.is_subtype(class, "Ljava/lang/Throwable;")
}

fn substring(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    start: i32,
    end: Option<i32>,
) -> Result<Option<Value>, VmExit> {
    let s = vm.heap.as_str(recv)?.to_string();
    let units: Vec<u16> = s.encode_utf16().collect();
    let e = end.unwrap_or(units.len() as i32);
    if start < 0 || e > units.len() as i32 || start > e {
        return Err(exception(
            vm,
            "Ljava/lang/StringIndexOutOfBoundsException;",
            format!("begin {start}, end {e}, length {}", units.len()),
        )
        .into());
    }
    let sub: String = String::from_utf16_lossy(&units[start as usize..e as usize]);
    Ok(Some(Value::Obj(vm.heap.alloc_string(sub)?)))
}

fn sb_buf(vm: &Engine, recv: crate::heap::ObjRef) -> Result<String, VmExit> {
    match vm.heap.get_field(recv, "buf")? {
        Value::Obj(r) => Ok(vm.heap.as_str(r)?.to_string()),
        _ => Err(crate::err::vm_error("INVALID_FORMAT", "StringBuilder sem buffer").into()),
    }
}

fn sb_append(vm: &mut Engine, recv: crate::heap::ObjRef, s: &str) -> Result<(), VmExit> {
    let mut buf = sb_buf(vm, recv)?;
    buf.push_str(s);
    let r = vm.heap.alloc_string(buf)?;
    crate::err::put_field_result(vm.heap.put_field(recv, "buf", Value::Obj(r)))
}

pub fn alloc_string(vm: &mut Engine, s: String) -> Result<crate::heap::ObjRef, VmExit> {
    vm.heap.alloc_string(s).map_err(|oom| {
        // issue #37: OOM de string também é OutOfMemoryError capturável
        VmExit::Exception(Throwable::new(
            "Ljava/lang/OutOfMemoryError;",
            format!(
                "alocação de {} bytes excede o heap de {} bytes",
                oom.requested, oom.budget
            ),
        ))
    })
}

pub fn string_arg(vm: &Engine, v: &Value) -> Result<String, VmExit> {
    match v.as_ref()? {
        None => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "argumento String é null",
        ))),
        Some(r) => Ok(vm.heap.as_str(r)?.to_string()),
    }
}

pub fn exception(vm: &mut Engine, class: &str, message: String) -> Throwable {
    // melhor esforço para materializar; se o heap estourar, devolve sem objeto
    match vm.vm_exception(class, &message) {
        Ok(t) => t,
        Err(_) => Throwable::new(class, message),
    }
}

fn parse_int(s: &str) -> Result<i32, String> {
    parse_long(s)?
        .try_into()
        .map_err(|_| format!("For input string: \"{s}\""))
}

fn parse_long(s: &str) -> Result<i64, String> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("For input string: \"{s}\""));
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        acc = acc
            .checked_mul(10)
            .and_then(|v| {
                if neg {
                    v.checked_sub((b - b'0') as i64)
                } else {
                    v.checked_add((b - b'0') as i64)
                }
            })
            .ok_or_else(|| format!("For input string: \"{s}\""))?;
    }
    Ok(acc)
}

/// min/max/pow do Java: NaN em qualquer arg → NaN (diferente do Rust, que
/// ignora NaN em min/max); ±0.0 decide por sinal (issue #37 — Rust deixa
/// não-especificado, Java retorna -0.0 no min e +0.0 no max).
fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() {
            a
        } else {
            b
        } // -0.0 vence no min
    } else {
        a.min(b)
    }
}
fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() {
            a
        } else {
            b
        } // +0.0 vence no max
    } else {
        a.max(b)
    }
}
fn java_min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else {
        a.min(b)
    }
}
fn java_max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() {
            a
        } else {
            b
        }
    } else {
        a.max(b)
    }
}

/// Math.round: floor(x + 0.5) com saturação nos limites do tipo (semântica
/// Java — o `as` do Rust já satura, mas NaN → 0 e meio-ponto para cima).
fn java_round_f32(v: f32) -> i32 {
    if v.is_nan() {
        0
    } else {
        (v + 0.5).floor() as i32
    }
}
fn java_round_f64(v: f64) -> i64 {
    if v.is_nan() {
        0
    } else {
        (v + 0.5).floor() as i64
    }
}
fn java_pow(a: f64, b: f64) -> f64 {
    // Java Math.pow casos especiais mínimos cobertos por pow do Rust
    a.powf(b)
}
