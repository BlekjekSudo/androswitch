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
/// M8: nono gap do launch real (`AtomicInteger.<init>`). Mesmo padrão do
/// AtomicReference (campo de heap `value`), aritmética i32 com semântica
/// Java (overflow silencioso — wrapping). VM single-threaded ⇒ CAS exato.
const ATOMIC_INT: &str = "Ljava/util/concurrent/atomic/AtomicInteger;";
/// M8: décimo gap do launch real (`Executors.newFixedThreadPool`). A VM é
/// single-threaded: substituto honesto é um executor que roda Runnables/
/// Callables INLINE (síncrono/determinístico), ignorando tamanho de pool e
/// ThreadFactory; `submit` devolve Future já completado.
/// M8: views/iteradores (13º gap: `LinkedHashMap.entrySet`). Views são
/// SNAPSHOTS independentes da coleção de origem (em Java são vivas — desvio
/// documentado); ordem = ordem de inserção (estado é Vec).
const VIEW_SET: &str = "Ljava/util/HashSet;";
const MAP_ENTRY: &str = "Ljava/util/Map$Entry;";
const ITERATOR: &str = "Ljava/util/Iterator;";
const EXECUTORS: &str = "Ljava/util/concurrent/Executors;";
const EXECUTOR: &str = "Ljava/util/concurrent/Executor;";
const EXECUTOR_SVC: &str = "Ljava/util/concurrent/ExecutorService;";
const FUTURE: &str = "Ljava/util/concurrent/Future;";
/// M8: 24º gap do launch real (`ServiceLoader.load`). Sem leitura de
/// `META-INF/services` no APK, o loader é deliberadamente vazio; isso mantém
/// o caminho de inicialização vivo sem inventar providers que não existem.
const SERVICE_LOADER: &str = "Ljava/util/ServiceLoader;";
/// M8: 26º gap do launch real (`ClassLoader.getResources`). Enumeration é o
/// cursor legado da JDK 1.0 — semanticamente o mesmo cursor do Iterator host,
/// então reutiliza o estado `HostObj::Iter`. O runtime ainda não enumera
/// entries do APK como recursos de classpath: a Enumeration host é vazia.
const ENUMERATION: &str = "Ljava/util/Enumeration;";
/// M8: ClassLoader canônico da VM (identidade por VM — ver
/// `framework::class_loader_obj`). `getResources(String)` é o 26º gap real.
const CLASS_LOADER: &str = "Ljava/lang/ClassLoader;";
use crate::framework::{CLASS, THREAD};
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
/// M8: quinto gap do launch real — `newUpdater(Class,Class,String)` caía no
/// NOT_IMPLEMENTED tipado do catch-all `Ljava/`. Updater host: captura o NOME
/// do campo e opera via get_field/put_field (sem volatile/checks — VM
/// single-threaded, documentado).
const ARFU: &str = "Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;";
const WEAK_REF: &str = "Ljava/lang/ref/WeakReference;";
const SOFT_REF: &str = "Ljava/lang/ref/SoftReference;";
const REF_QUEUE: &str = "Ljava/lang/ref/ReferenceQueue;";
const HASH_MAP: &str = "Ljava/util/HashMap;";
/// M8 (40º gap real): classe-marcadora host da view de
/// `Collections.unmodifiableMap` — mutações → UOE, leituras delegam.
const UNMODIFIABLE_MAP: &str = "Ljava/util/Collections$UnmodifiableMap;";
/// M8 (41º gap real): idem para `Collections.singletonList`.
const SINGLETON_LIST: &str = "Ljava/util/Collections$SingletonList;";
/// M8 (43º gap real): idem para `Collections.unmodifiableSet`.
const UNMODIFIABLE_SET: &str = "Ljava/util/Collections$UnmodifiableSet;";
const WEAK_MAP: &str = "Ljava/util/WeakHashMap;";
/// M8: 25º gap do launch real (`LinkedHashSet.<init>`). Reusa o estado List
/// dos demais Sets: deduplicação por igualdade Java e ordem de inserção.
const LINKED_SET: &str = "Ljava/util/LinkedHashSet;";
/// M8: sétimo gap do launch real (`LinkedHashMap.<init>`). Estado Map é
/// `Vec<(chave, valor)>` ⇒ ordem de INSERÇÃO (como LinkedHashMap real); para
/// os métodos essenciais o comportamento é o do HashMap.
const LINKED_MAP: &str = "Ljava/util/LinkedHashMap;";
/// M8: 17º gap do launch real (`ConcurrentHashMap.<init>`). VM
/// single-threaded ⇒ sem segmentação/concorrência: mesmo estado e métodos do
/// HashMap host. `putIfAbsent`/`computeIfAbsent` ficam para sondagem futura.
const CONCURRENT_MAP: &str = "Ljava/util/concurrent/ConcurrentHashMap;";
/// M8: 18º gap do launch real (`LinkedBlockingQueue.<init>`). FIFO sobre o
/// estado List. BLOQUEANTES (take/put com condição) são fail-fast em VM
/// single-threaded: bloquear sem outra thread = deadlock ⇒ `take`/`poll(J,…)`
/// com fila vazia materializam `IllegalStateException` tipado (honesto:
/// prefere falha visível a pendurar). `offer`/`add`/`poll()`/`peek` normais.
const BLOCKING_Q: &str = "Ljava/util/concurrent/LinkedBlockingQueue;";
/// M8: 19º gap do launch real (`ThreadLocal.<init>`). Single-threaded ⇒ um
/// valor por ThreadLocal (campo de heap "tl_value" + flag "tl_set"): `get`
/// chama `initialValue()` (DEX) na primeira leitura, como na JDK;
/// `withInitial(Supplier)` fica para sondagem futura (exige Callable host).
const THREAD_LOCAL: &str = "Ljava/lang/ThreadLocal;";
/// M8: subclasse direta (mesma família — sem ela o ThreadLocal host não
/// cobria `new InheritableThreadLocal()`, 20º gap sondado). Single-threaded
/// ⇒ "herdável" não tem efeito: comportamento idêntico ao ThreadLocal.
const INHERITABLE_TL: &str = "Ljava/lang/InheritableThreadLocal;";
/// Coleções abstratas da plataforma modeladas no classpath cujo `<init>()V`
/// é no-op host (M8, 37º gap) — ver `call_instance_intrinsic_mref`.
const ABSTRACT_COLLECTION_CTORS: [&str; 6] = [
    "Ljava/util/AbstractCollection;",
    "Ljava/util/AbstractList;",
    "Ljava/util/AbstractSequentialList;",
    "Ljava/util/AbstractSet;",
    "Ljava/util/AbstractMap;",
    "Ljava/util/AbstractQueue;",
];

const ARRAY_LIST: &str = "Ljava/util/ArrayList;";
const LINKED_LIST: &str = "Ljava/util/LinkedList;";
/// M8: coleções concorrentes — sexto gap do launch real
/// (`CopyOnWriteArraySet.<init>`). VM single-threaded ⇒ cópia-em-escrita é
/// irrelevante: ArraySet vira Set host (add deduplica) e ArrayList segue o
/// estado List comum.
/// M8: 15º gap do launch real (`ArrayDeque.<init>`). Deque host sobre o
/// estado `List` (frente = índice 0). Como na JDK: NÃO aceita null (NPE) e
/// removeFirst/pop em vazio → NoSuchElementException; poll/peek → null.
const ARRAY_DEQUE: &str = "Ljava/util/ArrayDeque;";
const COW_SET: &str = "Ljava/util/concurrent/CopyOnWriteArraySet;";
const COW_LIST: &str = "Ljava/util/concurrent/CopyOnWriteArrayList;";
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
        // M8: AtomicReferenceFieldUpdater.newUpdater — as classes (tclass/
        // vclass) são IGNORADAS (sem access check; volatile não se aplica a
        // VM single-threaded); só o nome do campo é capturado no updater.
        (
            ARFU,
            "newUpdater",
            "(Ljava/lang/Class;Ljava/lang/Class;Ljava/lang/String;)Ljava/util/concurrent/atomic/AtomicReferenceFieldUpdater;",
        ) => {
            let Some(_) = args[2].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "newUpdater com fieldName null",
                )));
            };
            let r = vm.heap.alloc_instance(ARFU.to_string(), vec![])?;
            crate::err::put_field_result(
                vm.heap.put_field(r, "arfu_name", args[2].clone()),
            )?;
            Ok(Some(Value::Obj(r)))
        }
        // M8 (23º gap real): Android NÃO tem SecurityManager (deprecated na
        // JDK; APIs de segurança são no-op) ⇒ sempre null.
        (
            "Ljava/lang/System;",
            "getSecurityManager",
            "()Ljava/lang/SecurityManager;",
        ) => Ok(Some(Value::Null)),
        // M8 (21º gap real): System.getProperty — tabela da plataforma com
        // valores coerentes com o ambiente Android/VM; desconhecida → null
        // (semântica JDK). getProperty(k,def) devolve def; getProperties fora.
        ("Ljava/lang/System;", "getProperty", "(Ljava/lang/String;)Ljava/lang/String;") => Ok(
            Some(system_get_property(vm, args)?.unwrap_or(Value::Null)),
        ),
        (
            "Ljava/lang/System;",
            "getProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ) => Ok(Some(
            system_get_property(vm, args)?.unwrap_or_else(|| args[1].clone()),
        )),
        // M8: Thread statics — VM single-threaded: UMA thread sintética
        // "main"; sleep/yield operam sobre o relógio virtual (≡ SystemClock).
        ("Ljava/lang/Thread;", "currentThread", "()Ljava/lang/Thread;") => {
            Ok(Some(Value::Obj(vm.main_thread_obj()?)))
        }
        ("Ljava/lang/Thread;", "sleep", "(J)V") | ("Ljava/lang/Thread;", "sleep", "(JI)V") => {
            let ms = match args.first() {
                Some(Value::Long(v)) => (*v).max(0) as u64,
                _ => 0,
            };
            vm.advance_clock(ms)?;
            Ok(Some(Value::Null))
        }
        ("Ljava/lang/Thread;", "yield", "()V") => Ok(Some(Value::Null)),
        // M8 (14º gap real): Collections.synchronized* — VM single-threaded,
        // sincronização é no-op ⇒ o wrapper devolve a PRÓPRIA coleção
        // (identidade; desvio documentado em compat_gaps). null → NPE.
        ("Ljava/util/Collections;", n, s) if collections_sync_wrapper(n, s) => {
            let Some(r) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Collections.synchronized* com coleção null",
                )));
            };
            Ok(Some(Value::Obj(r)))
        }
        // M8 (40º gap real): Collections.unmodifiableMap — pedido pelo
        // launch real (PftpdApp.onCreate → LogController.<clinit>). View de
        // leitura em classe-marcadora host: leituras delegam ao estado
        // (SNAPSHOT das entradas na criação — desvio de view documentado
        // como o de synchronized*); mutações pela view → UOE (JDK). null →
        // NPE; coleção sem estado host → VM_TYPE_ERROR tipado.
        (
            "Ljava/util/Collections;",
            "unmodifiableMap",
            "(Ljava/util/Map;)Ljava/util/Map;",
        ) => {
            let Some(Value::Obj(src)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Collections.unmodifiableMap com coleção null",
                )));
            };
            let entries = match vm.fw.objects.get(src) {
                Some(crate::framework::HostObj::Map { entries }) => entries.clone(),
                _ => {
                    return Err(crate::err::vm_error(
                        "VM_TYPE_ERROR",
                        "unmodifiableMap sobre coleção sem estado host (Map)",
                    )
                    .into())
                }
            };
            let r = vm
                .heap
                .alloc_instance(UNMODIFIABLE_MAP.to_string(), vec![])?;
            vm.fw
                .objects
                .insert(r, crate::framework::HostObj::Map { entries });
            Ok(Some(Value::Obj(r)))
        }
        // M8 (41º gap real): Collections.singletonList — pedido pelo launch
        // real (LogController.readPrefs): lista host imutável de 1 elemento
        // em classe-marcadora; mutações pela view → UOE (JDK). Elemento
        // null é aceito (JDK não valida) — a lista contém literalmente null.
        (
            "Ljava/util/Collections;",
            "singletonList",
            "(Ljava/lang/Object;)Ljava/util/List;",
        ) => {
            let v = args
                .first()
                .cloned()
                .unwrap_or(Value::Null);
            let r = vm
                .heap
                .alloc_instance(SINGLETON_LIST.to_string(), vec![])?;
            vm.fw
                .objects
                .insert(r, crate::framework::HostObj::List { elems: vec![v] });
            Ok(Some(Value::Obj(r)))
        }
        // M8 (43º gap real): Collections.unmodifiableSet — pedido pelo
        // launch real (LogController.readPrefs): view dedup de leitura em
        // classe-marcadora (snapshot; desvio de view documentado —
        // mesmo regime de unmodifiableMap); mutações → UOE; null → NPE;
        // sem estado host → VM_TYPE_ERROR tipado.
        (
            "Ljava/util/Collections;",
            "unmodifiableSet",
            "(Ljava/util/Set;)Ljava/util/Set;",
        ) => {
            let Some(Value::Obj(src)) = args.first() else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Collections.unmodifiableSet com coleção null",
                )));
            };
            let elems = match vm.fw.objects.get(src) {
                Some(crate::framework::HostObj::List { elems }) => elems.clone(),
                _ => {
                    return Err(crate::err::vm_error(
                        "VM_TYPE_ERROR",
                        "unmodifiableSet sobre coleção sem estado host (Set)",
                    )
                    .into())
                }
            };
            let r = vm
                .heap
                .alloc_instance(UNMODIFIABLE_SET.to_string(), vec![])?;
            vm.fw
                .objects
                .insert(r, crate::framework::HostObj::List { elems });
            Ok(Some(Value::Obj(r)))
        }
        // M8 (24º gap real, elevado a descoberta real no 45º gap): providers
        // vêm de `META-INF/services/<nome binário da interface>` quando o APK
        // está anexado à Engine (`Engine::set_resources` — o mesmo objeto já
        // usado para dex/AXML). Formato JDK: linhas, comentários `#`,
        // nomes binários com pontos. Sem APK anexado ou sem arquivo →
        // loader vazio (explícito e seguro como antes). O ClassLoader pode
        // ser null (bootstrap) na API Java; só service é obrigatório e
        // precisa preservar a NPE de contrato.
        (
            SERVICE_LOADER,
            "load",
            "(Ljava/lang/Class;)Ljava/util/ServiceLoader;",
        )
        | (
            SERVICE_LOADER,
            "load",
            "(Ljava/lang/Class;Ljava/lang/ClassLoader;)Ljava/util/ServiceLoader;",
        ) => {
            let Some(cr) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "ServiceLoader.load com service null",
                )));
            };
            // descritor da interface do service a partir do objeto Class.
            let service_desc = match vm.heap.get_field(cr, "class_name") {
                Ok(Value::Obj(sr)) => vm.heap.as_str(sr)?.to_string(),
                _ => String::new(),
            };
            let providers = service_providers_for(vm, &service_desc)?;
            let r = vm.heap.alloc_instance(SERVICE_LOADER.to_string(), vec![])?;
            vm.fw.objects.insert(
                r,
                crate::framework::HostObj::ServiceLoader {
                    service_desc,
                    providers,
                },
            );
            Ok(Some(Value::Obj(r)))
        }
        // M8: fábricas de thread pool → executor host (execução inline).
        // Tamanho do pool e ThreadFactory são ignorados (VM single-threaded).
        (c, n, s) if c == EXECUTORS && executors_factory(n, s) => {
            let r = vm.heap.alloc_instance(EXECUTOR_SVC.to_string(), vec![])?;
            Ok(Some(Value::Obj(r)))
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
/// Valor atual de um AtomicInteger host (campo "value"; tolerante a Long).
fn atomic_int_cur(vm: &Engine, recv: crate::heap::ObjRef) -> i32 {
    match vm.heap.get_field(recv, "value") {
        Ok(Value::Int(v)) => v,
        Ok(Value::Long(v)) => v as i32,
        _ => 0,
    }
}

/// i32 de um argumento (Int direto; Long estreita; resto → 0).
fn arg_i32(v: &Value) -> i32 {
    match v {
        Value::Int(i) => *i,
        Value::Long(l) => *l as i32,
        _ => 0,
    }
}

/// (name, sig) mutadores de Set bloqueados pela view de
/// `Collections.unmodifiableSet` (JDK: UnsupportedOperationException).
fn set_mutation_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("add", "(Ljava/lang/Object;)Z") | ("remove", "(Ljava/lang/Object;)Z") | ("clear", "()V")
    )
}

/// (name, sig) mutadores de List bloqueados pela view de
/// `Collections.singletonList` (JDK: UnsupportedOperationException).
fn list_mutation_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("add", "(Ljava/lang/Object;)Z")
            | ("add", "(ILjava/lang/Object;)V")
            | ("set", "(ILjava/lang/Object;)Ljava/lang/Object;")
            | ("remove", "(I)Ljava/lang/Object;")
            | ("remove", "(Ljava/lang/Object;)Z")
            | ("clear", "()V")
    )
}

/// (name, sig) mutadores de Map bloqueados pela view de
/// `Collections.unmodifiableMap` (JDK: UnsupportedOperationException).
fn map_mutation_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        (
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
        ) | ("remove", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | ("clear", "()V")
    )
}

/// (name, sig) de wrappers sincronizados de `Collections` (M8): identidade —
/// a VM é single-threaded, nada a sincronizar.
fn collections_sync_wrapper(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("synchronizedMap", "(Ljava/util/Map;)Ljava/util/Map;")
            | (
                "synchronizedSortedMap",
                "(Ljava/util/SortedMap;)Ljava/util/SortedMap;"
            )
            | ("synchronizedList", "(Ljava/util/List;)Ljava/util/List;")
            | ("synchronizedSet", "(Ljava/util/Set;)Ljava/util/Set;")
            | (
                "synchronizedSortedSet",
                "(Ljava/util/SortedSet;)Ljava/util/SortedSet;"
            )
            | (
                "synchronizedCollection",
                "(Ljava/util/Collection;)Ljava/util/Collection;"
            )
    )
}

/// (name, sig) de `Class` host suportados (M8).
fn class_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("getName", "()Ljava/lang/String;")
            | ("getSimpleName", "()Ljava/lang/String;")
            | ("toString", "()Ljava/lang/String;")
            | ("getClassLoader", "()Ljava/lang/ClassLoader;")
    )
}

/// Descritor → nome binário Java ("Lcom/x/Y;" → "com.x.Y"; arrays/primitivos
/// mantêm o descritor, como "[I").
fn descriptor_to_class_name(desc: &str) -> String {
    match desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')) {
        Some(inner) => inner.replace('/', "."),
        None => desc.to_string(),
    }
}

/// Class host (M8): nome vem do campo de heap "class_name" (descritor).
fn class_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    // getClassLoader não precisa do descritor: um ClassLoader canônico por VM
    // (single-classloader; a distinção bootstrap=null não se aplica ao host).
    if name == "getClassLoader" {
        let l = vm.class_loader_obj()?;
        return Ok(Some(Value::Obj(l)));
    }
    let desc = match vm.heap.get_field(recv, "class_name") {
        Ok(Value::Obj(r)) => vm.heap.as_str(r)?.to_string(),
        _ => String::new(),
    };
    let bin = descriptor_to_class_name(&desc);
    let out = match name {
        "getName" => bin,
        "getSimpleName" => {
            // Java: sem pacote e sem classes externas ("La/b/C$D;" → "D")
            let tail = desc.trim_start_matches('L').trim_end_matches(';');
            let after_slash = tail.rsplit('/').next().unwrap_or(tail);
            after_slash
                .rsplit('$')
                .next()
                .unwrap_or(after_slash)
                .to_string()
        }
        "toString" => format!("class {bin}"),
        _ => return Ok(None),
    };
    Ok(Some(Value::Obj(alloc_string(vm, out)?)))
}

/// (name, sig) de fábricas `Executors` suportadas (M8).
fn executors_factory(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        (
            "newFixedThreadPool",
            "(I)Ljava/util/concurrent/ExecutorService;"
        ) | (
            "newFixedThreadPool",
            "(ILjava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;"
        ) | (
            "newSingleThreadExecutor",
            "()Ljava/util/concurrent/ExecutorService;"
        ) | (
            "newSingleThreadExecutor",
            "(Ljava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;"
        ) | (
            "newCachedThreadPool",
            "()Ljava/util/concurrent/ExecutorService;"
        ) | (
            "newCachedThreadPool",
            "(Ljava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;"
        )
    )
}

/// (name, sig) de ExecutorService host suportados (M8).
fn exec_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("execute", "(Ljava/lang/Runnable;)V")
            | (
                "submit",
                "(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;"
            )
            | (
                "submit",
                "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;"
            )
            | (
                "submit",
                "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;"
            )
            | ("shutdown", "()V")
            | ("shutdownNow", "()Ljava/util/List;")
            | ("isShutdown", "()Z")
            | ("isTerminated", "()Z")
    )
}

/// (name, sig) de Future host suportados (M8) — sempre já completado.
fn future_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("get", "()Ljava/lang/Object;")
            | (
                "get",
                "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;"
            )
            | ("isDone", "()Z")
            | ("isCancelled", "()Z")
            | ("cancel", "(Z)Z")
    )
}

/// Roda `Runnable.run()` inline (mesmo mecanismo do drain do Looper).
fn exec_run_inline(vm: &mut Engine, r: crate::heap::ObjRef) -> Result<(), VmExit> {
    let rclass = vm.heap.class_of(r)?.to_string();
    let Some((d, def, m)) = vm.cp.resolve_method(&rclass, "run", "()V") else {
        return Err(crate::classpath::unresolved_method(&rclass, "run", "()V").into());
    };
    vm.call(d, def, &m, vec![Value::Obj(r)])?;
    Ok(())
}

/// Roda `Callable.call()` inline e devolve o valor produzido.
fn exec_call_inline(vm: &mut Engine, r: crate::heap::ObjRef) -> Result<Value, VmExit> {
    let rclass = vm.heap.class_of(r)?.to_string();
    let Some((d, def, m)) = vm
        .cp
        .resolve_method(&rclass, "call", "()Ljava/lang/Object;")
    else {
        return Err(
            crate::classpath::unresolved_method(&rclass, "call", "()Ljava/lang/Object;").into(),
        );
    };
    vm.call(d, def, &m, vec![Value::Obj(r)])
}

/// Future já completado com `result` (campo de heap "future_result").
fn exec_completed_future(vm: &mut Engine, result: Value) -> Result<Option<Value>, VmExit> {
    let f = vm.heap.alloc_instance(FUTURE.to_string(), vec![])?;
    crate::err::put_field_result(vm.heap.put_field(f, "future_result", result))?;
    Ok(Some(Value::Obj(f)))
}

fn exec_npe(msg: &str) -> VmExit {
    VmExit::Exception(Throwable::new("Ljava/lang/NullPointerException;", msg))
}

/// ExecutorService host (M8): execução inline determinística.
fn exec_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (name, sig) {
        ("execute", _) => {
            let Some(r) = args[0].as_ref()? else {
                return Err(exec_npe("execute sem runnable"));
            };
            exec_run_inline(vm, r)?;
            Ok(Some(Value::Null))
        }
        ("submit", "(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;") => {
            let Some(r) = args[0].as_ref()? else {
                return Err(exec_npe("submit sem runnable"));
            };
            exec_run_inline(vm, r)?;
            exec_completed_future(vm, Value::Null)
        }
        ("submit", "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;") => {
            let Some(r) = args[0].as_ref()? else {
                return Err(exec_npe("submit sem runnable"));
            };
            exec_run_inline(vm, r)?;
            exec_completed_future(vm, args[1].clone())
        }
        ("submit", "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;") => {
            let Some(r) = args[0].as_ref()? else {
                return Err(exec_npe("submit sem callable"));
            };
            let v = exec_call_inline(vm, r)?;
            exec_completed_future(vm, v)
        }
        ("shutdown", _) => {
            crate::err::put_field_result(vm.heap.put_field(recv, "exec_shutdown", Value::Int(1)))?;
            Ok(Some(Value::Null))
        }
        ("shutdownNow", _) => {
            use crate::framework::HostObj;
            crate::err::put_field_result(vm.heap.put_field(recv, "exec_shutdown", Value::Int(1)))?;
            // execução é inline ⇒ nunca há fila pendente → lista vazia
            let l = vm.heap.alloc_instance(ARRAY_LIST.to_string(), vec![])?;
            vm.fw.objects.insert(l, HostObj::List { elems: Vec::new() });
            Ok(Some(Value::Obj(l)))
        }
        ("isShutdown", _) | ("isTerminated", _) => {
            let down =
                matches!(vm.heap.get_field(recv, "exec_shutdown"), Ok(Value::Int(v)) if v != 0);
            Ok(Some(Value::Int(down as i32)))
        }
        _ => Ok(None),
    }
}

/// Future host (M8): sempre já completado (execução inline).
fn future_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match name {
        "get" => Ok(Some(
            vm.heap
                .get_field(recv, "future_result")
                .unwrap_or(Value::Null),
        )),
        "isDone" => Ok(Some(Value::Int(1))),
        "isCancelled" => Ok(Some(Value::Int(0))),
        "cancel" => Ok(Some(Value::Int(0))), // já completado → não cancelável
        _ => Ok(None),
    }
}

/// (name, sig) de `ServiceLoader` suportados (M8). O loader host é sempre
/// vazio enquanto a leitura de `META-INF/services` não fizer parte do runtime.
fn service_loader_supported(name: &str, sig: &str) -> bool {
    matches!((name, sig), ("iterator", "()Ljava/util/Iterator;"))
}

/// M8 (45º gap): parser do arquivo de configuração
/// `META-INF/services/<nome binário>` do APK anexado (formato JDK: nomes
/// binários, `#` comenta o restante da linha). Sem APK ou sem arquivo →
/// lista vazia (contrato: "no providers found" — java.util.ServiceLoader).
/// Erros de leitura do zip viram VM_ERROR tipado (configuração corrompida),
/// não silêncio.
fn service_providers_for(vm: &mut Engine, service_desc: &str) -> Result<Vec<String>, VmExit> {
    let bin = service_desc
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".");
    if bin.is_empty() {
        return Ok(Vec::new());
    }
    let Some(apk) = &vm.fw.resources else {
        return Ok(Vec::new());
    };
    let path = format!("META-INF/services/{bin}");
    let Some(entry) = apk.zip.find(&path) else {
        return Ok(Vec::new());
    };
    let bytes = apk.zip.read_entry(entry).map_err(|e| {
        crate::err::vm_error(
            "VM_ERROR",
            format!("lendo {path} do APK para ServiceLoader: {}", e.cause),
        )
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        let clean = line.split('#').next().unwrap_or("").trim();
        if !clean.is_empty() {
            out.push(format!("L{};", clean.replace('.', "/")));
        }
    }
    Ok(out)
}

/// M8 (24º/45º gaps): `ServiceLoader.iterator()` — instancia EAGER os
/// providers descobertos (desvio de preguiça do JDK: iterador deveria
/// instanciar sob demanda; documentado — volume de providers real = 1-3) e
/// devolve o iterador host do snapshot. Provider ausente dos dex →
/// ServiceConfigurationError (JLS: configuração inválida), não skip.
/// Exceções dos ctores propagam-se (JDK as embrulha em
/// ServiceConfigurationError — desvio documentado).
fn service_loader_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
) -> Result<Option<Value>, VmExit> {
    let descs = match vm.fw.objects.get(&recv) {
        Some(crate::framework::HostObj::ServiceLoader { providers, .. }) => providers.clone(),
        // loader antigo sem estado (garantia para objetos criados antes do
        // 45º gap ter estado — defesa em profundidade): vazio e explícito.
        _ => Vec::new(),
    };
    let mut elems = Vec::with_capacity(descs.len());
    for d in descs {
        if vm.cp.find_class(&d).is_none() {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/util/ServiceConfigurationError;",
                format!("ServiceLoader: provider {d} ausente dos dex do app"),
            )));
        }
        let obj = vm.new_dex_instance(&d)?;
        elems.push(Value::Obj(obj));
    }
    let it = vm.heap.alloc_instance(ITERATOR.to_string(), vec![])?;
    vm.fw
        .objects
        .insert(it, crate::framework::HostObj::Iter { elems, pos: 0 });
    Ok(Some(Value::Obj(it)))
}

/// Aloca uma Enumeration host vazia (M8): cursor sobre zero elementos sobre o
/// mesmo estado `HostObj::Iter` do Iterator host.
fn alloc_empty_enumeration(vm: &mut Engine) -> Result<Value, VmExit> {
    let e = vm.heap.alloc_instance(ENUMERATION.to_string(), vec![])?;
    vm.fw.objects.insert(
        e,
        crate::framework::HostObj::Iter {
            elems: Vec::new(),
            pos: 0,
        },
    );
    Ok(Value::Obj(e))
}

/// (name, sig) de Enumeration host suportados (M8).
fn enumeration_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("hasMoreElements", "()Z") | ("nextElement", "()Ljava/lang/Object;")
    )
}

/// Enumeration host (M8): cursor legado idêntico ao Iterator — só mudam os
/// nomes (`hasMoreElements`/`nextElement`), então delega ao cursor de Iterator
/// (`nextElement` vazio materializa `NoSuchElementException`, semântica Java).
fn enumeration_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    let cursor_name = match name {
        "hasMoreElements" => "hasNext",
        _ => "next",
    };
    iterator_intrinsic(vm, recv, cursor_name)
}

/// (name, sig) de ClassLoader host suportados (M8).
fn class_loader_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        (
            "getResources",
            "(Ljava/lang/String;)Ljava/util/Enumeration;"
        )
    )
}

/// `ClassLoader.getResources(String)` (M8, 26º gap do launch real): a VM não
/// expõe entries do APK/DEX como recursos de classpath, então a Enumeration é
/// deliberadamente vazia — nunca null (contrato JDK), sem inventar recursos
/// inexistentes e sem varrer `META-INF/services` (descoberta SPI real segue
/// fora do escopo). `name` null materializa NPE (`Objects.requireNonNull`).
fn class_loader_get_resources(vm: &mut Engine, args: &[Value]) -> Result<Option<Value>, VmExit> {
    if args[0].as_ref()?.is_none() {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "ClassLoader.getResources com name null",
        )));
    }
    alloc_empty_enumeration(vm).map(Some)
}

/// (name, sig) de Iterator host suportados (M8).
fn iterator_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("hasNext", "()Z") | ("next", "()Ljava/lang/Object;") | ("remove", "()V")
    )
}

/// Iterator host (M8): cursor sobre snapshot. `next` sem elemento →
/// NoSuchElementException materializado (semântica Java).
fn iterator_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    use crate::framework::HostObj;
    match name {
        "hasNext" => {
            let more = match vm.fw.objects.get(&recv) {
                Some(HostObj::Iter { elems, pos }) => *pos < elems.len(),
                _ => false,
            };
            Ok(Some(Value::Int(more as i32)))
        }
        "next" => {
            let Some(HostObj::Iter { elems, pos }) = vm.fw.objects.get_mut(&recv) else {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "Iterator host sem estado (use iterator())",
                )
                .into());
            };
            if *pos >= elems.len() {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/util/NoSuchElementException;",
                    "Iterator.next sem próximo elemento",
                )));
            }
            let v = elems[*pos].clone();
            *pos += 1;
            Ok(Some(v))
        }
        "remove" => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/UnsupportedOperationException;",
            "Iterator host: remove não suportado (snapshot)",
        ))),
        _ => Ok(None),
    }
}

/// (name, sig) de Map.Entry host suportados (M8).
fn entry_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("getKey", "()Ljava/lang/Object;")
            | ("getValue", "()Ljava/lang/Object;")
            | ("setValue", "(Ljava/lang/Object;)Ljava/lang/Object;")
    )
}

/// Map.Entry host (M8): campos de heap do snapshot. setValue atualiza o par
/// do snapshot (não propaga ao Map de origem — view independente, documentado).
fn entry_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "getKey" => Ok(Some(
            vm.heap.get_field(recv, "entry_key").unwrap_or(Value::Null),
        )),
        "getValue" => Ok(Some(
            vm.heap
                .get_field(recv, "entry_value")
                .unwrap_or(Value::Null),
        )),
        "setValue" => {
            let old = vm
                .heap
                .get_field(recv, "entry_value")
                .unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(recv, "entry_value", args[0].clone()))?;
            Ok(Some(old))
        }
        _ => Ok(None),
    }
}

/// (name, sig) de Thread host suportados (M8).
fn thread_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("getName", "()Ljava/lang/String;")
            | ("setName", "(Ljava/lang/String;)V")
            | ("getId", "()J")
            | ("threadId", "()I")
            | ("isDaemon", "()Z")
            | ("isAlive", "()Z")
            | ("toString", "()Ljava/lang/String;")
    )
}

/// Thread host (M8): a VM é single-threaded — uma única thread "main".
fn thread_intrinsic(vm: &mut Engine, name: &str, _sig: &str) -> Result<Option<Value>, VmExit> {
    match name {
        "getName" => Ok(Some(Value::Obj(alloc_string(vm, "main".to_string())?))),
        "setName" => Ok(Some(Value::Null)), // ignorado — thread única
        "getId" => Ok(Some(Value::Long(1))),
        "threadId" => Ok(Some(Value::Int(1))),
        "isDaemon" => Ok(Some(Value::Int(0))),
        "isAlive" => Ok(Some(Value::Int(1))),
        "toString" => Ok(Some(Value::Obj(alloc_string(
            vm,
            "Thread[main,1,main]".to_string(),
        )?))),
        _ => Ok(None),
    }
}

/// (name, sig) de AtomicReferenceFieldUpdater suportados como host (M8).
fn arfu_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("get", "(Ljava/lang/Object;)Ljava/lang/Object;")
            | ("set", "(Ljava/lang/Object;Ljava/lang/Object;)V")
            | (
                "getAndSet",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
            )
            | (
                "compareAndSet",
                "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z"
            )
            | (
                "weakCompareAndSet",
                "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z"
            )
    )
}

/// Métodos de instância do updater (M8): operam no campo capturado por
/// `newUpdater` (campo de heap `arfu_name`) sobre o objeto alvo. Alvo null →
/// NPE materializado (semântica Java). CAS por identidade de referência
/// (mesma política do AtomicReference; VM single-threaded ⇒ exato).
fn arfu_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let name_ref = vm
        .heap
        .get_field(recv, "arfu_name")
        .map_err(|_| {
            crate::err::vm_error(
                "VM_TYPE_ERROR",
                "AtomicReferenceFieldUpdater sem nome de campo (use newUpdater)",
            )
        })?
        .as_ref()?
        .ok_or_else(|| {
            VmExit::Exception(Throwable::new(
                "Ljava/lang/NullPointerException;",
                "updater sem fieldName",
            ))
        })?;
    let field = vm.heap.as_str(name_ref)?.to_string();
    let Some(target) = args[0].as_ref()? else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            format!("{name} em alvo null (campo {field})"),
        )));
    };
    match name {
        "get" => Ok(Some(
            vm.heap.get_field(target, &field).unwrap_or(Value::Null),
        )),
        "set" => {
            crate::err::put_field_result(vm.heap.put_field(target, &field, args[1].clone()))?;
            Ok(Some(Value::Null))
        }
        "getAndSet" => {
            let old = vm.heap.get_field(target, &field).unwrap_or(Value::Null);
            crate::err::put_field_result(vm.heap.put_field(target, &field, args[1].clone()))?;
            Ok(Some(old))
        }
        "compareAndSet" | "weakCompareAndSet" => {
            let cur = vm.heap.get_field(target, &field).unwrap_or(Value::Null);
            if ref_identity_eq(&cur, &args[1]) {
                crate::err::put_field_result(vm.heap.put_field(target, &field, args[2].clone()))?;
                Ok(Some(Value::Int(1)))
            } else {
                Ok(Some(Value::Int(0)))
            }
        }
        _ => Ok(None),
    }
}

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
        "compareTo" => {
            // Java (Enum.compareTo): this.ordinal - other.ordinal — subtração
            // simples, overflow inclusive (comportamento documentado na JDK).
            let a = vm
                .heap
                .get_field(recv, "ordinal")
                .unwrap_or(Value::Int(0))
                .as_int()
                .unwrap_or(0);
            let Some(other) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Enum.compareTo(null)",
                )));
            };
            let b = vm
                .heap
                .get_field(other, "ordinal")
                .unwrap_or(Value::Int(0))
                .as_int()
                .unwrap_or(0);
            Ok(Some(Value::Int(a.wrapping_sub(b))))
        }
        // M8 (34º gap real): Enum.clone() é FINAL na JDK/libcore e SEMPRE
        // lança CloneNotSupportedException — nunca copia. Materializamos a
        // exceção capturável (o app pode tratá-la e seguir, como em helpers
        // reflectivos); silenciar ou copiar seria semântica inventada.
        "clone" => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/CloneNotSupportedException;",
            "Cannot clone an enum (java.lang.Enum.clone é final e sempre lança)",
        ))),
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
            | ("entrySet", "()Ljava/util/Set;")
            | ("keySet", "()Ljava/util/Set;")
            | ("values", "()Ljava/util/Collection;")
    )
}

/// (name, sig) de List suportados como host (M8).
fn list_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(I)V")
            | ("<init>", "(Ljava/util/Collection;)V")
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

/// System.getProperty host (M8): propriedades da plataforma com valores
/// coerentes com a VM (Android/Linux, single-threaded, determinística).
/// Chave desconhecida → `Ok(None)` (null, como na JDK).
fn system_get_property(vm: &mut Engine, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let Some(key_ref) = args[0].as_ref()? else {
        return Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "System.getProperty(null)",
        )));
    };
    let key = vm.heap.as_str(key_ref)?.to_string();
    let val: &str = match key.as_str() {
        "os.name" => "Linux",
        "os.arch" => "arm64-v8a",
        "os.version" => "4.14.0-rustydroid",
        "line.separator" => "\n",
        "file.separator" => "/",
        "path.separator" => ":",
        "java.io.tmpdir" => "/tmp",
        "user.dir" => "/",
        "user.language" => "en",
        "user.region" => "US",
        "user.timezone" => "UTC",
        "java.vendor" => "RustyDroid",
        "java.version" => "0",
        "java.specification.version" => "1.8",
        "java.vm.name" => "RustyDroid VM",
        "java.vm.vendor" => "RustyDroid",
        "java.vm.version" => "0.8",
        "java.class.path" => "",
        "java.boot.class.path" => "",
        "java.library.path" => "/system/lib64",
        "java.home" => "/system",
        "java.runtime.name" => "RustyDroid Runtime",
        "java.runtime.version" => "0.8",
        "java.specification.name" => "Java Platform API Specification",
        "java.specification.vendor" => "RustyDroid",
        "file.encoding" => "UTF-8",
        "http.agent" => "RustyDroid/0.8",
        _ => return Ok(None),
    };
    Ok(Some(Value::Obj(alloc_string(vm, val.to_string())?)))
}

/// (name, sig) de ThreadLocal host suportados (M8).
fn thread_local_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("get", "()Ljava/lang/Object;")
            | ("set", "(Ljava/lang/Object;)V")
            | ("remove", "()V")
    )
}

/// ThreadLocal host (M8): single-threaded ⇒ um valor por instância.
/// `get` sem set prévio chama `initialValue()` (resolução DEX; default de
/// Object/ThreadLocal → null), como na JDK.
fn thread_local_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    recv_class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_value", Value::Null))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_set", Value::Int(0)))?;
            Ok(Some(Value::Null))
        }
        "set" => {
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_value", args[0].clone()))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_set", Value::Int(1)))?;
            Ok(Some(Value::Null))
        }
        "remove" => {
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_value", Value::Null))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_set", Value::Int(0)))?;
            Ok(Some(Value::Null))
        }
        "get" => {
            let set = matches!(vm.heap.get_field(recv, "tl_set"), Ok(Value::Int(v)) if v != 0);
            if set {
                return Ok(Some(
                    vm.heap.get_field(recv, "tl_value").unwrap_or(Value::Null),
                ));
            }
            // primeira leitura: initialValue() via DEX (override de subclasse)
            let initial = if let Some((d, def, m)) =
                vm.cp
                    .resolve_method(recv_class, "initialValue", "()Ljava/lang/Object;")
            {
                vm.call(d, def, &m, vec![Value::Obj(recv)])?
            } else {
                Value::Null
            };
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_value", initial.clone()))?;
            crate::err::put_field_result(vm.heap.put_field(recv, "tl_set", Value::Int(1)))?;
            Ok(Some(initial))
        }
        _ => Ok(None),
    }
}

/// (name, sig) de LinkedBlockingQueue host suportados (M8).
fn queue_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(I)V")
            | ("offer", "(Ljava/lang/Object;)Z")
            | (
                "offer",
                "(Ljava/lang/Object;JLjava/util/concurrent/TimeUnit;)Z"
            )
            | ("add", "(Ljava/lang/Object;)Z")
            | ("put", "(Ljava/lang/Object;)V")
            | ("poll", "()Ljava/lang/Object;")
            | (
                "poll",
                "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;"
            )
            | ("take", "()Ljava/lang/Object;")
            | ("peek", "()Ljava/lang/Object;")
            | ("element", "()Ljava/lang/Object;")
            | ("remove", "(Ljava/lang/Object;)Z")
            | ("contains", "(Ljava/lang/Object;)Z")
            | ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
            | ("remainingCapacity", "()I")
            | ("drainTo", "(Ljava/util/Collection;)I")
            | ("drainTo", "(Ljava/util/Collection;I)I")
    )
}

fn queue_blocked() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/IllegalStateException;",
        "fila vazia: take/poll bloqueante é deadlock em VM single-threaded (substituto honesto: falha visível)",
    ))
}

/// LinkedBlockingQueue host (M8): FIFO sobre `HostObj::List` (frente = 0).
/// Capacidade é ignorada (unbounded — como o default da JDK); null rejeitado.
fn queue_intrinsic(
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
        ("offer", _) | ("add", _) | ("put", _) => {
            if matches!(args[0], Value::Null) {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "LinkedBlockingQueue não aceita null (como na JDK)",
                )));
            }
            // capacidade ignorada ⇒ offer/put nunca bloqueiam (unbounded)
            list_elems(vm, recv)?.push(args[0].clone());
            Ok(Some(if name == "put" {
                Value::Null
            } else {
                Value::Int(1)
            }))
        }
        ("poll", "()Ljava/lang/Object;") | ("peek", _) => {
            let v = {
                let elems = list_elems(vm, recv)?;
                elems.first().cloned()
            };
            if name == "poll" {
                if v.is_none() {
                    return Ok(Some(Value::Null));
                }
                list_elems(vm, recv)?.remove(0);
            }
            Ok(Some(v.unwrap_or(Value::Null)))
        }
        ("poll", _) | ("take", _) | ("element", _) => {
            // bloqueantes/element em fila vazia → falha visível (sem deadlock)
            let elems = list_elems(vm, recv)?;
            if elems.is_empty() {
                if name == "element" {
                    return Err(deque_empty_nse());
                }
                return Err(queue_blocked());
            }
            Ok(Some(elems.remove(0)))
        }
        ("remove", _) | ("contains", _) => {
            let idx = find_elem(vm, recv, &args[0])?;
            if name == "remove" {
                if let Some(i) = idx {
                    list_elems(vm, recv)?.remove(i);
                }
            }
            Ok(Some(Value::Int(idx.is_some() as i32)))
        }
        ("size", _) => {
            let n = list_elems(vm, recv)?.len();
            Ok(Some(Value::Int(n as i32)))
        }
        ("isEmpty", _) => {
            let empty = list_elems(vm, recv)?.is_empty();
            Ok(Some(Value::Int(empty as i32)))
        }
        ("drainTo", _) => {
            // M8 (28º gap real): drena até maxElements (sem a sobrecarga: tudo)
            // na ORDEM da fila para a Collection destino, respeitando o add do
            // destino (Set deduplica — igual à JDK, que chama c.add(e)). O
            // retorno conta os elementos RETIRADOS da fila (um duplicado
            // rejeitado por Set.add já saiu daqui — semântica JDK). Destino
            // null → NPE; drainTo(this) → IllegalArgumentException;
            // maxElements <= 0 → 0 (JDK); destino fora da superfície de
            // Collection host (coleção do app) → VM_TYPE_ERROR tipado, pois
            // não dá para honrar c.add sem chamar DEX.
            let Some(dst) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "drainTo com Collection destino null",
                )));
            };
            if dst == recv {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/IllegalArgumentException;",
                    "drainTo para a própria fila",
                )));
            }
            let max = if sig == "(Ljava/util/Collection;I)I" {
                args[1].as_int()?.max(0) as usize
            } else {
                usize::MAX
            };
            let is_set = matches!(
                vm.heap.class_of(dst),
                Ok(dc) if vm.cp.is_subtype(dc, "Ljava/util/Set;")
            );
            let is_collection = matches!(
                vm.heap.class_of(dst),
                Ok(dc) if vm.cp.is_subtype(dc, "Ljava/util/Collection;")
            );
            if !is_collection {
                return Err(
                    "drainTo: destino fora da superfície de Collection host (coleção do app)"
                        .to_string()
                        .into(),
                );
            }
            // lazy-init do destino (mesmo estilo dos demais intrínsecos de coleção)
            if !matches!(vm.fw.objects.get(&dst), Some(HostObj::List { .. })) {
                vm.fw
                    .objects
                    .insert(dst, HostObj::List { elems: Vec::new() });
            }
            let k = list_elems(vm, recv)?.len().min(max);
            let drained: Vec<Value> = list_elems(vm, recv)?.drain(..k).collect();
            let mut taken = 0usize;
            for v in drained {
                if !(is_set && find_elem(vm, dst, &v)?.is_some()) {
                    list_elems(vm, dst)?.push(v);
                }
                taken += 1; // JDK: retirado da fila conta como transferido
            }
            Ok(Some(Value::Int(taken as i32)))
        }
        ("clear", _) => {
            list_elems(vm, recv)?.clear();
            Ok(Some(Value::Null))
        }
        ("remainingCapacity", _) => Ok(Some(Value::Int(i32::MAX))), // unbounded
        _ => Ok(None),
    }
}

/// (name, sig) de ArrayDeque host suportados (M8).
fn deque_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(I)V")
            | ("addFirst", "(Ljava/lang/Object;)V")
            | ("addLast", "(Ljava/lang/Object;)V")
            | ("offerFirst", "(Ljava/lang/Object;)Z")
            | ("offerLast", "(Ljava/lang/Object;)Z")
            | ("removeFirst", "()Ljava/lang/Object;")
            | ("removeLast", "()Ljava/lang/Object;")
            | ("pollFirst", "()Ljava/lang/Object;")
            | ("pollLast", "()Ljava/lang/Object;")
            | ("getFirst", "()Ljava/lang/Object;")
            | ("getLast", "()Ljava/lang/Object;")
            | ("peekFirst", "()Ljava/lang/Object;")
            | ("peekLast", "()Ljava/lang/Object;")
            | ("push", "(Ljava/lang/Object;)V")
            | ("pop", "()Ljava/lang/Object;")
            | ("add", "(Ljava/lang/Object;)Z")
            | ("offer", "(Ljava/lang/Object;)Z")
            | ("remove", "()Ljava/lang/Object;")
            | ("remove", "(Ljava/lang/Object;)Z")
            | ("removeFirstOccurrence", "(Ljava/lang/Object;)Z")
            | ("removeLastOccurrence", "(Ljava/lang/Object;)Z")
            | ("contains", "(Ljava/lang/Object;)Z")
            | ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
    )
}

/// Última ocorrência de `target` (varredura reversa; igualdade Java).
fn find_elem_last(
    vm: &Engine,
    recv: crate::heap::ObjRef,
    target: &Value,
) -> Result<Option<usize>, VmExit> {
    let Some(crate::framework::HostObj::List { elems }) = vm.fw.objects.get(&recv) else {
        return Ok(None);
    };
    for i in (0..elems.len()).rev() {
        if values_equal(&vm.heap, &elems[i], target)? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

fn deque_null_rejected() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/lang/NullPointerException;",
        "ArrayDeque não aceita elemento null (como na JDK)",
    ))
}

fn deque_empty_nse() -> VmExit {
    VmExit::Exception(Throwable::new(
        "Ljava/util/NoSuchElementException;",
        "ArrayDeque vazio (removeFirst/pop/getFirst/getLast)",
    ))
}

/// ArrayDeque host (M8): Deque sobre `HostObj::List` — frente = índice 0.
fn deque_intrinsic(
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
        ("addFirst", _) | ("push", _) | ("offerFirst", _) => {
            if matches!(args[0], Value::Null) {
                return Err(deque_null_rejected());
            }
            list_elems(vm, recv)?.insert(0, args[0].clone());
            Ok(Some(if name == "offerFirst" {
                Value::Int(1)
            } else {
                Value::Null
            }))
        }
        ("addLast", _) | ("add", _) | ("offerLast", _) | ("offer", _) => {
            if matches!(args[0], Value::Null) {
                return Err(deque_null_rejected());
            }
            list_elems(vm, recv)?.push(args[0].clone());
            Ok(Some(if name == "addLast" {
                Value::Null
            } else {
                Value::Int(1)
            }))
        }
        ("removeFirst", _) | ("pop", _) | ("remove", "()Ljava/lang/Object;") => {
            let elems = list_elems(vm, recv)?;
            if elems.is_empty() {
                return Err(deque_empty_nse());
            }
            Ok(Some(elems.remove(0)))
        }
        ("removeLast", _) => {
            let elems = list_elems(vm, recv)?;
            if elems.is_empty() {
                return Err(deque_empty_nse());
            }
            Ok(Some(elems.pop().expect("não-vazio verificado")))
        }
        ("pollFirst", _) => {
            let elems = list_elems(vm, recv)?;
            Ok(if elems.is_empty() {
                Some(Value::Null)
            } else {
                Some(elems.remove(0))
            })
        }
        ("pollLast", _) => {
            let elems = list_elems(vm, recv)?;
            Ok(Some(elems.pop().unwrap_or(Value::Null)))
        }
        ("getFirst", _) => {
            let elems = list_elems(vm, recv)?;
            match elems.first() {
                Some(v) => Ok(Some(v.clone())),
                None => Err(deque_empty_nse()),
            }
        }
        ("getLast", _) => {
            let elems = list_elems(vm, recv)?;
            match elems.last() {
                Some(v) => Ok(Some(v.clone())),
                None => Err(deque_empty_nse()),
            }
        }
        ("peekFirst", _) => {
            let v = list_elems(vm, recv)?.first().cloned();
            Ok(Some(v.unwrap_or(Value::Null)))
        }
        ("peekLast", _) => {
            let v = list_elems(vm, recv)?.last().cloned();
            Ok(Some(v.unwrap_or(Value::Null)))
        }
        ("remove", _) | ("removeFirstOccurrence", _) => {
            let idx = find_elem(vm, recv, &args[0])?;
            if let Some(i) = idx {
                list_elems(vm, recv)?.remove(i);
            }
            Ok(Some(Value::Int(idx.is_some() as i32)))
        }
        ("removeLastOccurrence", _) => {
            let idx = find_elem_last(vm, recv, &args[0])?;
            if let Some(i) = idx {
                list_elems(vm, recv)?.remove(i);
            }
            Ok(Some(Value::Int(idx.is_some() as i32)))
        }
        ("contains", _) => {
            let found = find_elem(vm, recv, &args[0])?.is_some();
            Ok(Some(Value::Int(found as i32)))
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

/// (name, sig) de Set host suportados (M8, CopyOnWriteArraySet/LinkedHashSet).
fn set_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("<init>", "()V")
            | ("<init>", "(Ljava/util/Collection;)V")
            | ("add", "(Ljava/lang/Object;)Z")
            | ("remove", "(Ljava/lang/Object;)Z")
            | ("contains", "(Ljava/lang/Object;)Z")
            | ("size", "()I")
            | ("isEmpty", "()Z")
            | ("clear", "()V")
    )
}

/// Set host (M8): reusa `HostObj::List` com add deduplicado. Para
/// LinkedHashSet o Vec também preserva a ordem de inserção; para
/// CopyOnWriteArraySet a VM single-threaded torna a cópia-em-escrita irrelevante.
fn set_intrinsic(
    vm: &mut Engine,
    recv: crate::heap::ObjRef,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    use crate::framework::HostObj;
    if name == "<init>" {
        // M8 (42º gap real): `<init>(Collection)` — snapshot do iterador da
        // fonte host COM dedup de Set (values_equal; ordem preservada —
        // LinkedHashSet idem). Fonte null → NPE (c.toArray() na JDK); fonte
        // sem estado host → VM_TYPE_ERROR tipado (não itera coleção do app).
        let elems = if sig == "(Ljava/util/Collection;)V" {
            let Some(src) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "Set.<init> com Collection null",
                )));
            };
            let Some(HostObj::List { elems: src_elems }) = vm.fw.objects.get(&src) else {
                return Err(crate::err::vm_error(
                    "VM_TYPE_ERROR",
                    "Set.<init>(Collection): fonte sem estado host (coleção do app)",
                )
                .into());
            };
            let src_snapshot = src_elems.clone();
            let mut out: Vec<Value> = Vec::new();
            'outer: for el in src_snapshot {
                for x in &out {
                    if values_equal(&vm.heap, x, &el)? {
                        continue 'outer;
                    }
                }
                out.push(el);
            }
            out
        } else {
            Vec::new()
        };
        vm.fw.objects.insert(recv, HostObj::List { elems });
        return Ok(Some(Value::Null));
    }
    if !matches!(vm.fw.objects.get(&recv), Some(HostObj::List { .. })) {
        vm.fw
            .objects
            .insert(recv, HostObj::List { elems: Vec::new() });
    }
    match name {
        "add" => {
            // Set: já presente → false (não duplica)
            if find_elem(vm, recv, &args[0])?.is_some() {
                return Ok(Some(Value::Int(0)));
            }
            list_elems(vm, recv)?.push(args[0].clone());
            Ok(Some(Value::Int(1)))
        }
        "remove" => {
            let idx = find_elem(vm, recv, &args[0])?;
            if let Some(i) = idx {
                list_elems(vm, recv)?.remove(i);
            }
            Ok(Some(Value::Int(idx.is_some() as i32)))
        }
        "contains" => {
            let found = find_elem(vm, recv, &args[0])?.is_some();
            Ok(Some(Value::Int(found as i32)))
        }
        "size" => {
            let n = list_elems(vm, recv)?.len();
            Ok(Some(Value::Int(n as i32)))
        }
        "isEmpty" => {
            let empty = list_elems(vm, recv)?.is_empty();
            Ok(Some(Value::Int(empty as i32)))
        }
        "clear" => {
            list_elems(vm, recv)?.clear();
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
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
        // views (M8): snapshot em ordem de inserção; Map.Entry host carrega
        // chave/valor em campos de heap ("entry_key"/"entry_value").
        ("entrySet", _) | ("keySet", _) | ("values", _) => {
            let snapshot = map_entries(vm, recv)?.clone();
            let mut elems = Vec::with_capacity(snapshot.len());
            for (k, v) in snapshot {
                let item = match name {
                    "keySet" => k,
                    "values" => v,
                    _ => {
                        let en = vm.heap.alloc_instance(MAP_ENTRY.to_string(), vec![])?;
                        crate::err::put_field_result(vm.heap.put_field(en, "entry_key", k))?;
                        crate::err::put_field_result(vm.heap.put_field(en, "entry_value", v))?;
                        Value::Obj(en)
                    }
                };
                elems.push(item);
            }
            let view = vm.heap.alloc_instance(VIEW_SET.to_string(), vec![])?;
            vm.fw.objects.insert(view, HostObj::List { elems });
            Ok(Some(Value::Obj(view)))
        }
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
        // M8 (27º gap real): `<init>(Collection)` — snapshot na ordem do
        // iterador da fonte host (ArrayList admite duplicatas; dedup fica a
        // cargo da fonte). Fonte null → NPE (c.toArray() na JDK). Coleção do
        // app sem estado host não é iterável sem chamar DEX: erro tipado
        // honesto em vez de lista silenciosamente vazia/errada.
        let elems = if sig == "(Ljava/util/Collection;)V" {
            let Some(src) = args[0].as_ref()? else {
                return Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "ArrayList.<init> com Collection null",
                )));
            };
            match vm.fw.objects.get(&src) {
                Some(HostObj::List { elems }) => elems.clone(),
                _ => {
                    return Err(crate::err::vm_error(
                        "VM_TYPE_ERROR",
                        "ArrayList.<init>(Collection): fonte sem estado host (coleção do app)",
                    )
                    .into())
                }
            }
        } else {
            Vec::new()
        };
        vm.fw.objects.insert(recv, HostObj::List { elems });
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

        // ── <init>()V de coleções abstratas da plataforma (M8, 37º gap) ─────
        // Classe DEX estendendo DIRETAMENTE uma coleção abstrata host (ex.:
        // guava estende AbstractList) chama o super ctor via invoke-direct.
        // O corpo real é trivial (contadores em zero) e o estado vive no
        // nosso modelo — no-op correto, na linha do <init> de Object/
        // Activity/Application/Context/Fragment. Casa por mref.class: o
        // receiver é subclasse DEX, então recv_class nunca bate o nome.
        (_, "<init>", "()V") if ABSTRACT_COLLECTION_CTORS.contains(&mref_class) => {
            Ok(Some(Value::Null))
        }

        // ── Object.getClass (M8, 16º gap real) ──────────────────────────────
        // final: nenhuma classe DEX pode declará-lo ⇒ captura irrestrita é
        // correta. Devolve o objeto Class canônico da classe do receiver
        // (identidade estável por descritor — como na JVM).
        (_, "getClass", "()Ljava/lang/Class;") => {
            let cls = vm.heap.class_of(recv)?.to_string();
            let c = vm.class_object_for(&cls)?;
            Ok(Some(Value::Obj(c)))
        }
        // ── Activity.getApplication (M8, 38º gap real): final na plataforma;
        // o DEX do app referencia o método pela PRÓPRIA classe do receiver
        // (mref_class = ex.: Lorg/primftpd/ui/MainTabsActivity;), que não é
        // Landroid/ e por isso nunca alcançaria o braço do framework. A
        // identidade vive em `Engine::application_obj` (framework.rs) —
        // criada no launch com a classe android:name do manifest. Captura
        // irrestrita correta: getApplication é final — nada em DEX a
        // reimplementa.
        (_, "getApplication", "()Landroid/app/Application;")
            if vm.cp.is_subtype(recv_class, crate::framework::ACTIVITY)
                || vm.cp.is_subtype(recv_class, crate::framework::APPLICATION) =>
        {
            Ok(Some(Value::Obj(vm.application_obj()?)))
        }
        // ── Context.getApplicationContext (M8, 39º gap real): final na
        // plataforma (ContextWrapper delega ao base); para Activities é o
        // MESMO objeto de getApplication (o app registra si mesmo como base).
        (.., "getApplicationContext", "()Landroid/content/Context;")
            if vm.cp.is_subtype(recv_class, crate::framework::ACTIVITY)
                || vm.cp.is_subtype(recv_class, crate::framework::APPLICATION)
                || vm.cp.is_subtype(recv_class, crate::framework::CONTEXT) =>
        {
            Ok(Some(Value::Obj(vm.application_obj()?)))
        }
        // Métodos de Class host: só sem declaração DEX (app não define
        // java.lang.Class; guarda defensiva contra shadowing).
        (c, name, sig)
            if c == CLASS
                && class_supported(name, sig)
                && vm.cp.resolve_method(recv_class, name, sig).is_none() =>
        {
            class_intrinsic(vm, recv, name)
        }

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
                        | ("compareTo", "(Ljava/lang/Enum;)I")
                        | ("clone", "()Ljava/lang/Object;")
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
        // M8 (12º gap real): enums do APP herdam os métodos do Enum host —
        // Enum não tem DEX, então a resolução falha quando o method ref
        // declara a SUBCLASSE (ex.: Lifecycle$State.compareTo). Fallback:
        // recv <: Enum e ninguém na cadeia DEX declara (name,sig) → host.
        // Override de usuário (toString próprio etc.) vence — resolve Some.
        (_, name, sig)
            if mref_class != ENUM
                && matches!(
                    (name, sig),
                    ("name", "()Ljava/lang/String;")
                        | ("ordinal", "()I")
                        | ("toString", "()Ljava/lang/String;")
                        | ("compareTo", "(Ljava/lang/Enum;)I")
                        | ("clone", "()Ljava/lang/Object;")
                )
                && vm.cp.is_subtype(recv_class, ENUM)
                && vm.cp.resolve_method(recv_class, name, sig).is_none() =>
        {
            enum_intrinsic(vm, recv, name, args)
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

        // ── AtomicReferenceFieldUpdater (M8) ───────────────────────────────
        // Instância criada por newUpdater (classe concreta = ARFU); métodos
        // operam no campo capturado do objeto alvo.
        (c, name, sig) if (c == ARFU || mref_class == ARFU) && arfu_supported(name, sig) => {
            arfu_intrinsic(vm, recv, name, args)
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

        // ── AtomicInteger (M8) ──────────────────────────────────────────────
        // Estado no campo "value" (Int). Overflow i32 segue a semântica Java
        // (silencioso/wrapping); longValue/floatValue/doubleValue alargam.
        (ATOMIC_INT, "<init>", "()V") => {
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(0)))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_INT, "<init>", "(I)V") => {
            let v = arg_i32(&args[0]);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_INT, "get", "()I") | (ATOMIC_INT, "intValue", "()I") => {
            Ok(Some(Value::Int(atomic_int_cur(vm, recv))))
        }
        (ATOMIC_INT, "set", "(I)V") => {
            let v = arg_i32(&args[0]);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Null))
        }
        (ATOMIC_INT, "getAndSet", "(I)I") => {
            let cur = atomic_int_cur(vm, recv);
            let v = arg_i32(&args[0]);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Int(cur)))
        }
        (ATOMIC_INT, "compareAndSet", "(II)Z") | (ATOMIC_INT, "weakCompareAndSet", "(II)Z") => {
            let cur = atomic_int_cur(vm, recv);
            let expect = arg_i32(&args[0]);
            if cur == expect {
                let v = arg_i32(&args[1]);
                crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
                Ok(Some(Value::Int(1)))
            } else {
                Ok(Some(Value::Int(0)))
            }
        }
        (ATOMIC_INT, "incrementAndGet", "()I") => {
            let v = atomic_int_cur(vm, recv).wrapping_add(1);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Int(v)))
        }
        (ATOMIC_INT, "decrementAndGet", "()I") => {
            let v = atomic_int_cur(vm, recv).wrapping_sub(1);
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Int(v)))
        }
        (ATOMIC_INT, "getAndIncrement", "()I") => {
            let cur = atomic_int_cur(vm, recv);
            crate::err::put_field_result(vm.heap.put_field(
                recv,
                "value",
                Value::Int(cur.wrapping_add(1)),
            ))?;
            Ok(Some(Value::Int(cur)))
        }
        (ATOMIC_INT, "getAndDecrement", "()I") => {
            let cur = atomic_int_cur(vm, recv);
            crate::err::put_field_result(vm.heap.put_field(
                recv,
                "value",
                Value::Int(cur.wrapping_sub(1)),
            ))?;
            Ok(Some(Value::Int(cur)))
        }
        (ATOMIC_INT, "addAndGet", "(I)I") => {
            let v = atomic_int_cur(vm, recv).wrapping_add(arg_i32(&args[0]));
            crate::err::put_field_result(vm.heap.put_field(recv, "value", Value::Int(v)))?;
            Ok(Some(Value::Int(v)))
        }
        (ATOMIC_INT, "getAndAdd", "(I)I") => {
            let cur = atomic_int_cur(vm, recv);
            crate::err::put_field_result(vm.heap.put_field(
                recv,
                "value",
                Value::Int(cur.wrapping_add(arg_i32(&args[0]))),
            ))?;
            Ok(Some(Value::Int(cur)))
        }
        (ATOMIC_INT, "longValue", "()J") => Ok(Some(Value::Long(atomic_int_cur(vm, recv) as i64))),
        (ATOMIC_INT, "floatValue", "()F") => {
            Ok(Some(Value::Float(atomic_int_cur(vm, recv) as f32)))
        }
        (ATOMIC_INT, "doubleValue", "()D") => {
            Ok(Some(Value::Double(atomic_int_cur(vm, recv) as f64)))
        }
        (ATOMIC_INT, "toString", "()Ljava/lang/String;") => {
            let t = atomic_int_cur(vm, recv).to_string();
            Ok(Some(Value::Obj(alloc_string(vm, t)?)))
        }

        // ── ExecutorService/Future host (M8) ────────────────────────────────
        // Execução INLINE: execute/submit rodam o Runnable/Callable na hora
        // (determinístico; tamanho do pool irrelevante). Exceções propagam
        // imediatamente (em Java seriam capturadas no Future) — desvio
        // documentado em compat_gaps.
        (c, name, sig)
            if (c == EXECUTOR_SVC || mref_class == EXECUTOR_SVC || mref_class == EXECUTOR)
                && exec_supported(name, sig) =>
        {
            exec_intrinsic(vm, recv, name, sig, args)
        }
        (c, name, sig) if (c == FUTURE || mref_class == FUTURE) && future_supported(name, sig) => {
            future_intrinsic(vm, recv, name)
        }

        // ── Thread host (M8) ────────────────────────────────────────────────
        // Single-threaded: a instância é sempre a "main" sintética (nome fixo;
        // setName é ignorado). start()/interrupt() seguem fora (não sondados).
        (c, name, sig) if c == THREAD && thread_supported(name, sig) => {
            thread_intrinsic(vm, name, sig)
        }

        // ── coleções java.util (M8) ─────────────────────────────────────────
        // Só os (name, sig) suportados casam aqui; o resto segue o fluxo
        // normal (Object fallback / DEX / NOT_FOUND tipado).
        // ── Collections.unmodifiableSet (M8, 43º gap): mutações → UOE;
        // leituras com semântica de SET (dedup) — mesma família da view Map.
        (c, name, sig) if c == UNMODIFIABLE_SET && set_mutation_supported(name, sig) => {
            Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/UnsupportedOperationException;",
                "set imutável host (Collections.unmodifiableSet)",
            )))
        }
        (c, name, sig) if c == UNMODIFIABLE_SET && set_supported(name, sig) => {
            set_intrinsic(vm, recv, name, sig, args)
        }
        // ── Collections.unmodifiableMap (M8, 40º gap): mutações pela view →
        // UOE como na JDK (throwable Java materializado); leituras delegam.
        (c, name, sig) if c == UNMODIFIABLE_MAP && map_mutation_supported(name, sig) => {
            Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/UnsupportedOperationException;",
                "mapa imutável host (Collections.unmodifiableMap)",
            )))
        }
        (c, name, sig) if c == UNMODIFIABLE_MAP && map_supported(name, sig) => {
            map_intrinsic(vm, recv, name, sig, args)
        }
        (c, name, sig)
            if (c == HASH_MAP || c == WEAK_MAP || c == LINKED_MAP || c == CONCURRENT_MAP)
                && map_supported(name, sig) =>
        {
            map_intrinsic(vm, recv, name, sig, args)
        }
        // ── Collections.singletonList (M8, 41º gap): mutações pela view →
        // UOE como na JDK; leituras delegam ao estado List normal.
        (c, name, sig) if c == SINGLETON_LIST && list_mutation_supported(name, sig) => {
            Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/UnsupportedOperationException;",
                "lista imutável host (Collections.singletonList)",
            )))
        }
        (c, name, sig) if c == SINGLETON_LIST && list_supported(name, sig) => {
            list_intrinsic(vm, recv, name, sig, args)
        }
        (c, name, sig)
            if (c == ARRAY_LIST || c == LINKED_LIST || c == COW_LIST)
                && list_supported(name, sig) =>
        {
            list_intrinsic(vm, recv, name, sig, args)
        }
        // Sets host (M8): mesmo estado List, mas add DEDUPLICA por igualdade
        // Java (values_equal) e devolve false se já presente. LinkedHashSet
        // preserva ordem de inserção porque o estado é Vec.
        (c, name, sig)
            if (c == COW_SET || c == VIEW_SET || c == LINKED_SET) && set_supported(name, sig) =>
        {
            set_intrinsic(vm, recv, name, sig, args)
        }
        // ArrayDeque (M8): Deque host sobre o estado List (frente = índice 0)
        (c, name, sig) if c == ARRAY_DEQUE && deque_supported(name, sig) => {
            deque_intrinsic(vm, recv, name, sig, args)
        }
        // LinkedBlockingQueue (M8): FIFO; bloqueantes fail-fast (sem deadlock)
        (c, name, sig) if c == BLOCKING_Q && queue_supported(name, sig) => {
            queue_intrinsic(vm, recv, name, sig, args)
        }
        // ThreadLocal (M8): um valor por VM. <init> só quando o mref DECLARA
        // ThreadLocal (super() de subclasse inclusa; ctor da subclasse segue o
        // DEX). get/set/remove por subtipagem e só sem declaração DEX
        // (override de usuário vence).
        (c, name, sig)
            if thread_local_supported(name, sig)
                && ((name == "<init>"
                    && (mref_class == THREAD_LOCAL || mref_class == INHERITABLE_TL))
                    || (name != "<init>"
                        && (c == THREAD_LOCAL || vm.cp.is_subtype(recv_class, THREAD_LOCAL))
                        && vm.cp.resolve_method(recv_class, name, sig).is_none())) =>
        {
            thread_local_intrinsic(vm, recv, recv_class, name, args)
        }
        // M8 (34º gap real): array.clone() — arrays são Cloneable (JLS 10.7)
        // e o clone é uma SHALLOW COPY com o mesmo descritor/ElemKind. É o
        // rastro clássico de `values()`/`$VALUES.clone()` de enums (ex.:
        // androidx Lifecycle$Event): o method ref declara o TIPO DO ARRAY
        // (`[L...;`), não Enum. Enum.clone segue regra própria (CNSE) acima.
        (c, name, sig)
            if c.starts_with('[') && (name, sig) == ("clone", "()Ljava/lang/Object;") =>
        {
            let (elem, elems, elem_class) = match vm.heap.get(recv)? {
                crate::heap::HeapObj::Array {
                    elem,
                    elems,
                    elem_class,
                } => (*elem, elems.clone(), elem_class.clone()),
                // receiver não-array com method ref de array é ilegal — mas
                // não é daqui: deixa o fluxo seguir para o erro de resolução.
                _ => return Ok(None),
            };
            let dst = vm.heap.alloc_array(elem, elems.len(), &elem_class)?;
            vm.heap.array_elems_mut(dst)?.extend(elems);
            Ok(Some(Value::Obj(dst)))
        }
        // ServiceLoader (M8, 45º gap): iterator() instancia os providers
        // descobertos em META-INF/services (ver service_loader_intrinsic).
        (c, name, sig) if c == SERVICE_LOADER && service_loader_supported(name, sig) => {
            service_loader_intrinsic(vm, recv)
        }
        // ClassLoader (M8): getResources(String) → Enumeration host vazia —
        // recursos de classpath do APK ainda não são enumerados (26º gap real).
        (c, name, sig) if c == CLASS_LOADER && class_loader_supported(name, sig) => {
            class_loader_get_resources(vm, args)
        }
        // Enumeration host (M8): cursor legado ≡ Iterator (nomes JDK 1.0).
        (c, name, sig) if c == ENUMERATION && enumeration_supported(name, sig) => {
            enumeration_intrinsic(vm, recv, name)
        }
        // iterator() de List/Set host (incl. views de Map) — M8
        (c, name, sig)
            if (c == ARRAY_LIST
                || c == LINKED_LIST
                || c == COW_LIST
                || c == COW_SET
                || c == VIEW_SET
                || c == LINKED_SET
                || c == ARRAY_DEQUE)
                && name == "iterator"
                && sig == "()Ljava/util/Iterator;" =>
        {
            let elems = list_elems(vm, recv)?.clone();
            let it = vm.heap.alloc_instance(ITERATOR.to_string(), vec![])?;
            vm.fw
                .objects
                .insert(it, crate::framework::HostObj::Iter { elems, pos: 0 });
            Ok(Some(Value::Obj(it)))
        }
        // Iterator host (M8): hasNext/next sobre o snapshot; remove → UOE
        (c, name, sig) if c == ITERATOR && iterator_supported(name, sig) => {
            iterator_intrinsic(vm, recv, name)
        }
        // Map.Entry host (M8): par chave/valor do snapshot de entrySet
        (c, name, sig) if c == MAP_ENTRY && entry_supported(name, sig) => {
            entry_intrinsic(vm, recv, name, args)
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
