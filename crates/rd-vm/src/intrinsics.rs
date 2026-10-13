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
/// M8 (compat real): primeiro gap de launch do app real org.primftpd 8.2.
const ATOMIC_REF: &str = "Ljava/util/concurrent/atomic/AtomicReference;";
const MATH: &str = "Ljava/lang/Math;";
/// M8 (compat real): coleções java.util host — segundo gap de launch do
/// org.primftpd (`WeakHashMap.<init>`). WeakHashMap ≡ HashMap (sem GC na
/// arena, entradas nunca são limpas — limitação documentada); LinkedList
/// ≡ ArrayList (mesmo contrato de List para os métodos suportados).
/// M8: base de enums — terceiro gap do launch real (EIIE em Enum.<init>).
const ENUM: &str = "Ljava/lang/Enum;";
/// M8: família java.lang.ref — quarto gap do launch real (WeakReference.
/// <init>). Sem GC na arena ⇒ referências fracas/soft NUNCA são limpas
/// espontaneamente; o referent vive até `clear()` — limitação documentada
/// (mesma política do WeakHashMap host).
const WEAK_REF: &str = "Ljava/lang/ref/WeakReference;";
const SOFT_REF: &str = "Ljava/lang/ref/SoftReference;";
const REF_QUEUE: &str = "Ljava/lang/ref/ReferenceQueue;";
const HASH_MAP: &str = "Ljava/util/HashMap;";
const WEAK_MAP: &str = "Ljava/util/WeakHashMap;";
const ARRAY_LIST: &str = "Ljava/util/ArrayList;";
const LINKED_LIST: &str = "Ljava/util/LinkedList;";
const BOOLEAN: &str = "Ljava/lang/Boolean;";
const DOUBLE: &str = "Ljava/lang/Double;";
const FLOAT: &str = "Ljava/lang/Float;";
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
/// `<init>` de WeakReference/SoftReference (M8): referent no campo de heap
/// "referent". A variante com ReferenceQueue IGNORA a fila — sem GC na arena
/// nada é enfileirado (documentado).
fn ref_init(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let referent = if sig == "()V" {
        Value::Null
    } else {
        args[0].clone()
    };
    crate::err::put_field_result(vm.heap.put_field(recv, "referent", referent))?;
    Ok(Some(Value::Null))
}

/// get/clear de WeakReference/SoftReference (M8).
fn ref_get_clear(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match name {
        "get" => Ok(Some(
            vm.heap.get_field(recv, "referent").unwrap_or(Value::Null),
        )),
        "clear" => {
            crate::err::put_field_result(vm.heap.put_field(recv, "referent", Value::Null))?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// Enum host (M8): estado nos campos de heap `name`/`ordinal` (padrão
/// StringBuilder/Throwable). `values()`/`valueOf()` são estáticos sintéticos
/// da classe do app (DEX) — executam normal no interpretador.
fn enum_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            // args: (String name, int ordinal) — name chega como Obj do DEX
            crate::err::put_field_result(vm.heap.put_field(recv, "name", args[0].clone()))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "ordinal", args[1].clone()))?;
            Ok(Some(Value::Null))
        }
        "name" | "toString" => Ok(Some(vm.heap.get_field(recv, "name").unwrap_or(Value::Null))),
        "ordinal" => {
            let v = vm.heap.get_field(recv, "ordinal").unwrap_or(Value::Int(0));
            Ok(Some(Value::Int(v.as_int().unwrap_or(0))))
        }
        _ => Ok(None),
    }
}

/// (name, sig) de Map suportados como host (M8).
fn map_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(I)V")
            | ("<init>", "(IF)V")
            | ("<init>", "(Ljava/util/Map;)V")
            | (
                "put",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
            )
            | ("get", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | ("containsKey", "(Ljava/lang/Object;)Z")
            | ("containsValue", "(Ljava/lang/Object;)Z")
            | ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
    )
}

/// (name, sig) de List suportados como host (M8).
fn list_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(I)V")
            | ("add", "(Ljava/lang/Object;)Z")
            | ("add", "(ILjava/lang/Object;)V")
            | ("get", "(I)Ljava/lang/Object;")
            | ("set", "(ILjava/lang/Object;)Ljava/lang/Object;")
            | ("remove", "(I)Ljava/lang/Object;")
            | ("remove", "(Ljava/lang/Object;)Z")
            | ("contains", "(Ljava/lang/Object;)Z")
            | ("indexOf", "(Ljava/lang/Object;)I")
            | ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
    )
}

/// Igualdade JAVA para chaves/elementos de coleções (M8): Null==Null;
/// String por conteúdo; boxes (Integer/Long/Boolean/Double/Float) por valor;
/// senão identidade de referência (default de Object.equals). Classe de
/// usuário que sobrescreve equals NÃO é consultada — limitação documentada
/// em compat_gaps (`java-util-collections`).
pub fn values_equal(heap: &crate::heap::Heap, a: &Value, b: &Value) -> Result<bool, VmExit> {
    match (a, b) {
        (Value::Null, Value::Null) => Ok(true),
        (Value::Null, _) | (_, Value::Null) => Ok(false),
        (Value::Int(x), Value::Int(y)) => Ok(x == y),
        (Value::Long(x), Value::Long(y)) => Ok(x == y),
        (Value::Obj(x), Value::Obj(y)) => {
            if x == y {
                return Ok(true);
            }
            let cx = heap.class_of(*x).unwrap_or("");
            let cy = heap.class_of(*y).unwrap_or("");
            if cx != cy {
                return Ok(false);
            }
            match cx {
                STRING => Ok(heap.as_str(*x)? == heap.as_str(*y)?),
                INTEGER | LONG | BOOLEAN => Ok(heap.get_field(*x, "value")?.as_int()?
                    == heap.get_field(*y, "value")?.as_int()?),
                DOUBLE => Ok(heap.get_field(*x, "value")?.as_double()?
                    == heap.get_field(*y, "value")?.as_double()?),
                FLOAT => Ok(heap.get_field(*x, "value")?.as_float()?
                    == heap.get_field(*y, "value")?.as_float()?),
                _ => Ok(false), // identidade (já comparada acima)
            }
        }
        _ => Ok(false),
    }
}

fn map_entries(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
) -> Result<&mut Vec<(Value, Value)>, VmExit> {
    match vm.fw.objects.get_mut(&recv) {
        Some(crate::framework::HostObj::Map { entries }) => Ok(entries),
        _ => Err(
            crate::err::vm_error("VM_TYPE_ERROR", "coleção java.util sem estado host (Map)").into(),
        ),
    }
}

fn list_elems(vm: &mut Engine, recv: crate::heap::ObjRef) -> Result<&mut Vec<Value>, VmExit> {
    match vm.fw.objects.get_mut(&recv) {
        Some(crate::framework::HostObj::List { elems }) => Ok(elems),
        _ => Err(
            crate::err::vm_error("VM_TYPE_ERROR", "coleção java.util sem estado host (List)")
                .into(),
        ),
    }
}

fn find_key(vm: &Engine, recv: crate::heap::ObjRef, key: &Value) -> Result<Option<usize>, VmExit> {
    let Some(crate::framework::HostObj::Map { entries }) = vm.fw.objects.get(&recv) else {
        return Ok(None);
    };
    for (i, (k, _)) in entries.iter().enumerate() {
        if values_equal(&vm.heap, k, key)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

fn find_elem(
    vm: &Engine,
    recv: crate::heap::ObjRef,
    target: &Value,
) -> Result<Option<usize>, VmExit> {
    let Some(crate::framework::HostObj::List { elems }) = vm.fw.objects.get(&recv) else {
        return Ok(None);
    };
    for (i, e) in elems.iter().enumerate() {
        if values_equal(&vm.heap, e, target)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

fn iobe(msg: String) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/IndexOutOfBoundsException;", msg))
}

/// HashMap/WeakHashMap host (M8). Estado em `HostObj::Map`; sem GC na arena
/// ⇒ WeakHashMap nunca limpa entradas (comportamento documentado).
fn map_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    use crate::framework::HostObj;
    if name == "<init>" {
        let entries = if sig == "(Ljava/util/Map;)V" {
            match args.first() {
                Some(Value::Obj(other)) => match vm.fw.objects.get(other) {
                    Some(HostObj::Map { entries }) => entries.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            }
        } else {
            Vec::new() // capacidade inicial/fator de carga: só performance
        };
        vm.fw.objects.insert(recv, HostObj::Map { entries });
        return Ok(Some(Value::Null));
    }
    if !matches!(vm.fw.objects.get(&recv), Some(HostObj::Map { .. })) {
        // defensivo: instância alocada sem <init> não existe em DEX válido
        vm.fw.objects.insert(
            recv,
            HostObj::Map {
                entries: Vec::new(),
            },
        );
    }
    match (name, sig) {
        ("put", _) => {
            let k = args[0].clone();
            let v = args[1].clone();
            let idx = find_key(vm, recv, &k)?;
            let entries = map_entries(vm, recv)?;
            if let Some(i) = idx {
                let old = std::mem::replace(&mut entries[i].1, v);
                Ok(Some(old))
            } else {
                entries.push((k, v));
                Ok(Some(Value::Null))
            }
        }
        ("get", _) => {
            let idx = find_key(vm, recv, &args[0])?;
            let entries = map_entries(vm, recv)?;
            Ok(Some(idx.map_or(Value::Null, |i| entries[i].1.clone())))
        }
        ("remove", _) => {
            let idx = find_key(vm, recv, &args[0])?;
            let entries = map_entries(vm, recv)?;
            Ok(Some(idx.map_or(Value::Null, |i| entries.remove(i).1)))
        }
        ("containsKey", _) => {
            let found = find_key(vm, recv, &args[0])?.is_some();
            Ok(Some(Value::Int(found as i32)))
        }
        ("containsValue", _) => {
            // clone pequeno: evita briga de borrow entre fw e heap
            let snapshot = map_entries(vm, recv)?.clone();
            let mut found = false;
            for (_, v) in &snapshot {
                if values_equal(&vm.heap, v, &args[0])? {
                    found = true;
                    break;
                }
            }
            Ok(Some(Value::Int(found as i32)))
        }
        ("size", _) => {
            let n = map_entries(vm, recv)?.len();
            Ok(Some(Value::Int(n as i32)))
        }
        ("isEmpty", _) => {
            let empty = map_entries(vm, recv)?.is_empty();
            Ok(Some(Value::Int(empty as i32)))
        }
        ("clear", _) => {
            map_entries(vm, recv)?.clear();
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// ArrayList/LinkedList host (M8). Estado em `HostObj::List`; OOB levanta
/// IndexOutOfBoundsException materializada (semântica Java).
fn list_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    use crate::framework::HostObj;
    if name == "<init>" {
        vm.fw
            .objects
            .insert(recv, HostObj::List { elems: Vec::new() });
        return Ok(Some(Value::Null));
    }
    if !matches!(vm.fw.objects.get(&recv), Some(HostObj::List { .. })) {
        vm.fw
            .objects
            .insert(recv, HostObj::List { elems: Vec::new() });
    }
    match (name, sig) {
        ("add", "(Ljava/lang/Object;)Z") => {
            list_elems(vm, recv)?.push(args[0].clone());
            Ok(Some(Value::Int(1))) // ArrayList.add sempre true
        }
        ("add", "(ILjava/lang/Object;)V") => {
            let i = args[0].as_int()?;
            let elems = list_elems(vm, recv)?;
            if i < 0 || i as usize > elems.len() {
                return Err(iobe(format!("add índice {i}, tamanho {}", elems.len())));
            }
            elems.insert(i as usize, args[1].clone());
            Ok(Some(Value::Null))
        }
        ("get", _) => {
            let i = args[0].as_int()?;
            let elems = list_elems(vm, recv)?;
            if i < 0 || i as usize >= elems.len() {
                return Err(iobe(format!("get índice {i}, tamanho {}", elems.len())));
            }
            Ok(Some(elems[i as usize].clone()))
        }
        ("set", _) => {
            let i = args[0].as_int()?;
            let elems = list_elems(vm, recv)?;
            if i < 0 || i as usize >= elems.len() {
                return Err(iobe(format!("set índice {i}, tamanho {}", elems.len())));
            }
            let old = std::mem::replace(&mut elems[i as usize], args[1].clone());
            Ok(Some(old))
        }
        ("remove", "(I)Ljava/lang/Object;") => {
            let i = args[0].as_int()?;
            let elems = list_elems(vm, recv)?;
            if i < 0 || i as usize >= elems.len() {
                return Err(iobe(format!("remove índice {i}, tamanho {}", elems.len())));
            }
            Ok(Some(elems.remove(i as usize)))
        }
        ("remove", "(Ljava/lang/Object;)Z") => {
            let idx = find_elem(vm, recv, &args[0])?;
            if let Some(i) = idx {
                list_elems(vm, recv)?.remove(i);
            }
            Ok(Some(Value::Int(idx.is_some() as i32)))
        }
        ("contains", _) => {
            let found = find_elem(vm, recv, &args[0])?.is_some();
            Ok(Some(Value::Int(found as i32)))
        }
        ("indexOf", _) => {
            let idx = find_elem(vm, recv, &args[0])?;
            Ok(Some(Value::Int(idx.map_or(-1, |i| i as i32))))
        }
        ("size", _) => {
            let n = list_elems(vm, recv)?.len();
            Ok(Some(Value::Int(n as i32)))
        }
        ("isEmpty", _) => {
            let empty = list_elems(vm, recv)?.is_empty();
            Ok(Some(Value::Int(empty as i32)))
        }
        ("clear", _) => {
            list_elems(vm, recv)?.clear();
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// Igualdade de identidade de referências (o `==` do Java sobre refs) — usada
/// pelo CAS do AtomicReference: Null == Null; Obj(i) == Obj(j) sse i == j.
/// Valores não-ref (Int/Long/…) nunca são payload de AtomicReference via DEX.
fn ref_identity_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Obj(x), Value::Obj(y)) => x == y,
        _ => false,
    }
}

/// Versão sem classe declarante: a mref class é a própria classe do
/// receiver (chamadas de teste e caminhos que não têm method ref).
pub fn call_instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    sig: &str,
    recv: crate::heap::ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    call_instance_intrinsic_mref(vm, recv_class, recv_class, name, sig, recv, args)
}

/// Como `call_instance_intrinsic`, mas distingue a classe DECLARANTE do
/// method ref (`mref_class`) da classe concreta do receiver. Necessário
/// para Enum: a chamada super `Enum.<init>(String,int)` tem mref_class =
/// `Ljava/lang/Enum;` mas receiver de classe do app — e o construtor da
/// classe do app (DEX) NÃO pode ser sequestrado pelo intrínseco.
pub fn call_instance_intrinsic_mref(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: crate::heap::ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (recv_class, name, sig) {
        (OBJECT, "<init>", "()V") => Ok(Some(Value::Null)), // ctor de Object = no-op

        // ── java.lang.Enum (M8) ─────────────────────────────────────────────
        // Só quando o method ref DECLARA Enum: <init>/name/ordinal são finais
        // (ou direct), então não há override de usuário a respeitar. toString
        // é virtual: só cai aqui se ninguém na cadeia DEX o declara.
        (_, name, sig)
            if mref_class == ENUM
                && matches!(
                    (name, sig),
                    ("<init>", "(Ljava/lang/String;I)V")
                        | ("name", "()Ljava/lang/String;")
                        | ("ordinal", "()I")
                ) =>
        {
            enum_intrinsic(vm, recv, name, args)
        }
        (_, "toString", "()Ljava/lang/String;")
            if mref_class == ENUM
                && vm
                    .cp
                    .resolve_method(recv_class, "toString", "()Ljava/lang/String;")
                    .is_none() =>
        {
            enum_intrinsic(vm, recv, "toString", args)
        }

        // ── java.lang.ref (M8) ──────────────────────────────────────────────
        // WeakReference/SoftReference host. <init> é direct: só casa quando o
        // method ref DECLARA a classe (super-chamada de subclasse inclusa) —
        // construtor de subclasse do app segue para o DEX. get/clear só são
        // capturados sem declaração na cadeia DEX (override de usuário vence).
        (_, name, sig)
            if (mref_class == WEAK_REF || mref_class == SOFT_REF)
                && matches!(
                    (name, sig),
                    ("<init>", "()V")
                        | ("<init>", "(Ljava/lang/Object;)V")
                        | (
                            "<init>",
                            "(Ljava/lang/Object;Ljava/lang/ref/ReferenceQueue;)V"
                        )
                ) =>
        {
            ref_init(vm, recv, sig, args)
        }
        (c, name, sig)
            if (mref_class == WEAK_REF
                || mref_class == SOFT_REF
                || c == WEAK_REF
                || c == SOFT_REF)
                && matches!(
                    (name, sig),
                    ("get", "()Ljava/lang/Object;") | ("clear", "()V")
                )
                && vm.cp.resolve_method(recv_class, name, sig).is_none() =>
        {
            ref_get_clear(vm, recv, name)
        }
        // ReferenceQueue: sem GC nada é enfileirado — poll/remove devolvem
        // null para sempre (na JVM real remove() BLOQUEIA; aqui a fila está
        // permanentemente vazia, então null é a resposta honesta).
        (c, name, sig)
            if (mref_class == REF_QUEUE || c == REF_QUEUE)
                && matches!(
                    (name, sig),
                    ("<init>", "()V")
                        | ("poll", "()Ljava/lang/ref/Reference;")
                        | ("remove", "()Ljava/lang/ref/Reference;")
                        | ("remove", "(J)Ljava/lang/ref/Reference;")
                ) =>
        {
            Ok(Some(Value::Null))
        }

        // ── AtomicReference (M8) ────────────────────────────────────────────
        // Estado no campo "value" da instância (mesmo padrão do StringBuilder).
        // VM single-threaded: CAS é EXATO (não há concorrência) e a igualdade
        // é identidade de referência (Null == Null; Obj(i) == Obj(i)).
        (ATOMIC_REF, "<init>", "()V") => {
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Null))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_REF, "<init>", "(Ljava/lang/Object;)V") => {
            crate::err::put_field_result(vm.heap.put_field(recv, "value", args[0].clone()))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_REF, "get", "()Ljava/lang/Object;") => Ok(Some(
            vm.heap.get_field(recv, "value").unwrap_or(Value::Null),
        )),
        (ATOMIC_REF, "set", "(Ljava/lang/Object;)V") => {
            crate::err::put_field_result(vm.heap.put_field(recv, "value", args[0].clone()))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_REF, "getAndSet", "(Ljava/lang/Object;)Ljava/lang/Object;") => {
            let old = vm.heap.get_field(recv, "value").unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", args[0].clone()))?;
            Ok(Some(old))
        }
        (ATOMIC_REF, "compareAndSet", "(Ljava/lang/Object;Ljava/lang/Object;)Z")
        | (ATOMIC_REF, "weakCompareAndSet", "(Ljava/lang/Object;Ljava/lang/Object;)Z") => {
            let cur = vm.heap.get_field(recv, "value").unwrap_or(Value::Null);
            if ref_identity_eq(&cur, &args[0]) {
                crate::err::put_field_result(vm.heap.put_field(recv, "value", args[1].clone()))?;
                Ok(Some(Value::Int(1)))
            } else {
                Ok(Some(Value::Int(0)))
            }
        }
        (
            ATOMIC_REF,
            "compareAndExchange",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ) => {
            let cur = vm.heap.get_field(recv, "value").unwrap_or(Value::Null);
            if ref_identity_eq(&cur, &args[0]) {
                crate::err::put_field_result(vm.heap.put_field(recv, "value", args[1].clone()))?;
            }
            Ok(Some(cur))
        }

        // ── coleções java.util (M8) ─────────────────────────────────────────
        // Só os (name, sig) suportados casam aqui; o resto segue o fluxo
        // normal (Object fallback / DEX / NOT_FOUND tipado).
        (c, name, sig) if (c == HASH_MAP || c == WEAK_MAP) && map_supported(name, sig) => {
            map_intrinsic(vm, recv, name, sig, args)
        }
        (c, name, sig) if (c == ARRAY_LIST || c == LINKED_LIST) && list_supported(name, sig) => {
            list_intrinsic(vm, recv, name, sig, args)
        }

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
