//! Motor da VM: estado global (classpath, heap arena, campos estáticos),
//! máquina de chamadas, inicialização de classes (`<clinit>`/static_values)
//! e propagação de exceções entre frames.

use crate::maps::{HashMap, HashSet};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use rd_dex::classes::ClassDef;
use rd_dex::code::CodeItem;
use rd_dex::methods::EncodedMethod;

use crate::classpath::Classpath;
use crate::err::{Throwable, VmExit};
use crate::heap::{Heap, ObjRef};
use crate::interp;
use crate::intrinsics;
use crate::value::Value;

pub const ACC_STATIC: u32 = 0x0008;
pub const ACC_NATIVE: u32 = 0x0100;
pub const ACC_ABSTRACT: u32 = 0x0400;

/// Configuração de execução (orçamentos do piso moto-e5 por default).
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// limite de instruções executadas (contra loop infinito) — VM_FUEL
    pub fuel: u64,
    /// profundidade máxima da pilha de chamadas — VM_STACK_OVERFLOW
    pub max_depth: usize,
    /// heap do app em bytes — 256 MB = piso E5 (PRF-06)
    pub heap_budget: usize,
    /// Observabilidade OPT-IN (default OFF para preservar todas as saídas
    /// existentes, incl. golden vectors byte-idênticos): exceções Java
    /// não tratadas anotam uma linha `\n  at classe->metodo(proto)` por
    /// frame enquanto desempilham — o stack trace da JVM, innermost-first.
    /// Erros estruturados da VM (NOT_FOUND/…) não recebem frames.
    pub exception_trace: bool,
}

/// Reserva mínima preservada acima da guard page nativa (512 KiB) e orçamento
/// conservador de fallback quando `/proc` não está disponível (1,25 MiB).
#[cfg(all(feature = "std", target_os = "linux"))]
const NATIVE_STACK_GUARD_RESERVE: usize = 512 * 1024;
/// Folga mínima abaixo do orçamento para o último salto antes da próxima
/// checagem (#469). Só limita o orçamento quando a VMA é maior que a pilha real.
#[cfg(all(feature = "std", target_os = "linux"))]
const NATIVE_STACK_HEADROOM: usize = 1024 * 1024;
const FALLBACK_NATIVE_STACK_BYTES: usize = 1_280 * 1024;
#[cfg(feature = "std")]
const MAX_MAIN_NATIVE_STACK_BYTES: usize = 6 * 1024 * 1024;

#[inline(never)]
fn current_stack_addr() -> usize {
    let marker = 0u8;
    core::ptr::addr_of!(marker) as usize
}

/// `true` quando a VMA que começa em `lo` tem, imediatamente abaixo, a página
/// guarda `PROT_NONE` que a glibc mapeia para pilhas de thread. É o único caso
/// em que o tamanho da VMA descreve a pilha inteira: sem a página guarda o
/// kernel pode ter fundido a pilha com mapeamentos anônimos vizinhos e
/// `hi - lo` passa a não significar nada (#313, #394).
#[cfg(all(feature = "std", target_os = "linux"))]
fn guard_page_below(lines: &[&str], lo: usize) -> bool {
    lines.iter().any(|prev| {
        let Some((range, rest)) = prev.split_once(' ') else {
            return false;
        };
        let Some((_, hi_hex)) = range.split_once('-') else {
            return false;
        };
        let Ok(prev_hi) = usize::from_str_radix(hi_hex, 16) else {
            return false;
        };
        prev_hi == lo && !rest.starts_with('r')
    })
}

/// Calcula o orçamento seguro de pilha nativa disponível abaixo de `sp_now`
/// na entrada externa da thread atual (executado apenas 1× quando `entry_sp == 0`).
fn detect_native_stack_budget(sp_now: usize) -> usize {
    #[cfg(all(feature = "std", target_os = "linux"))]
    {
        if let Ok(maps) = std::fs::read_to_string("/proc/thread-self/maps") {
            let lines: Vec<&str> = maps.lines().collect();
            for &line in &lines {
                let Some((range, rest)) = line.split_once(' ') else {
                    continue;
                };
                if !rest.starts_with("rw") {
                    continue;
                }
                let Some((lo_hex, hi_hex)) = range.split_once('-') else {
                    continue;
                };
                let (Ok(lo), Ok(hi)) = (
                    usize::from_str_radix(lo_hex, 16),
                    usize::from_str_radix(hi_hex, 16),
                ) else {
                    continue;
                };
                if sp_now >= lo && sp_now <= hi {
                    // A VMA `[stack]` da thread principal cresce sob demanda até `RLIMIT_STACK`.
                    if line.trim_end().ends_with("[stack]") {
                        return main_thread_stack_budget();
                    }
                    // Regressão #313 no CI: com só `livre - 512 KiB` uma
                    // thread de 2 MiB abortava. O guard roda na entrada de
                    // cada frame, então precisa de folga para o frame mais
                    // profundo que ainda será empilhado depois da última
                    // checagem — e esse frame cresce com a versão do rustc em
                    // build debug. Nunca confiamos em mais de METADE da VMA.
                    let half_vma = hi.saturating_sub(lo) / 2;
                    // O tamanho da VMA só é confiável quando ela é *uma pilha
                    // de thread de verdade*: a glibc mapeia uma página guarda
                    // `PROT_NONE` imediatamente abaixo. Sem essa âncora o
                    // kernel pode ter fundido a pilha com mapeamentos anônimos
                    // vizinhos, e então `hi - lo` (e portanto `free`) descreve
                    // uma região muito maior que a pilha real — o orçamento
                    // fica grande demais, o guard nunca dispara e o processo
                    // aborta com SIGABRT no runner do CI (#313/#394).
                    let anchored_by_guard_page = guard_page_below(&lines, lo);
                    let ceiling = if anchored_by_guard_page {
                        half_vma
                    } else {
                        half_vma.min(FALLBACK_NATIVE_STACK_BYTES)
                    };
                    // #469: quando a VMA se funde com um mapeamento rw vizinho
                    // acima da pilha, `half_vma` deixa de limitar e o orçamento
                    // passa a ser `livre - reserva`. Com a reserva antiga
                    // (512 KiB) sobrava pouco para o último salto host→host.
                    // A reserva de 1 MiB garante ≥ 1 MiB de folga abaixo do
                    // orçamento; na VMA exata de 2 MiB o teto `half_vma` (1 MiB)
                    // continua valendo, então a profundidade legítima não muda.
                    let free = sp_now
                        .saturating_sub(lo)
                        .saturating_sub(NATIVE_STACK_HEADROOM);
                    return free.min(ceiling).max(256 * 1024);
                }
            }
        }
    }
    let _ = sp_now;
    #[cfg(feature = "std")]
    if std::thread::current().name() == Some("main") {
        return MAX_MAIN_NATIVE_STACK_BYTES;
    }
    FALLBACK_NATIVE_STACK_BYTES
}

#[cfg(all(feature = "std", target_os = "linux"))]
fn main_thread_stack_budget() -> usize {
    if let Ok(limits) = std::fs::read_to_string("/proc/self/limits") {
        for line in limits.lines() {
            if let Some(rest) = line.strip_prefix("Max stack size") {
                if let Some(tok) = rest.split_whitespace().next() {
                    if let Ok(bytes) = tok.parse::<usize>() {
                        return bytes
                            .saturating_sub(4 * NATIVE_STACK_GUARD_RESERVE)
                            .clamp(FALLBACK_NATIVE_STACK_BYTES, MAX_MAIN_NATIVE_STACK_BYTES);
                    }
                }
            }
        }
    }
    MAX_MAIN_NATIVE_STACK_BYTES
}

impl Default for VmConfig {
    fn default() -> Self {
        VmConfig {
            fuel: 200_000_000,
            max_depth: 128,
            heap_budget: crate::heap::DEFAULT_HEAP_BUDGET,
            exception_trace: false,
        }
    }
}

/// M7 — telemetria de launch (opt-in via `RD_LAUNCH_TRACE`): decompõe o
/// custo do caminho frio (`launch_app_cold` ~330 ms no primftpd para só
/// ~236k instruções guest) em resolução de método/classe, resolução de
/// strings, intrinsics host e `<clinit>`. Contadores em `Cell` para os
/// pontos quentes poderem acumular via `&self` sem tocar as assinaturas
/// de resolução; desligada (`None` no `Engine`), cada ponto custa um
/// branch. O resíduo (total − Σcategorias, contra `fuel_used`) estima o
/// loop de despacho. Campos por inclusão, não partição: um intrinsic
/// que chama guest conta o tempo aninhado no próprio balde.
#[derive(Default)]
#[cfg(feature = "std")]
pub(crate) struct LaunchTrace {
    resolve_ns: core::cell::Cell<u64>,
    resolve_calls: core::cell::Cell<u64>,
    intrinsic_ns: core::cell::Cell<u64>,
    intrinsic_calls: core::cell::Cell<u64>,
    clinit_ns: core::cell::Cell<u64>,
    clinit_calls: core::cell::Cell<u64>,
    string_ns: core::cell::Cell<u64>,
    string_calls: core::cell::Cell<u64>,
    /// v2: tempo de intrinsic por classe declarante (descritor) — o
    /// balde agregado sozinho não diz QUAIS stubs custam ~5,7 µs.
    intrinsic_by_class: core::cell::RefCell<HashMap<String, (u64, u64)>>,
}

#[cfg(feature = "std")]
impl LaunchTrace {
    #[inline]
    fn add(ns: &core::cell::Cell<u64>, calls: &core::cell::Cell<u64>, v: u64) {
        ns.set(ns.get().saturating_add(v));
        calls.set(calls.get().saturating_add(1));
    }
}

/// Motor interpretador. O heap é coletado por mark-sweep em safe points
/// (`Engine::gc` / `maybe_gc`, ver `gc.rs`) e descartado quando o Engine cai.
struct NativeStackState {
    entry_sp: usize,
    budget: usize,
    depth: usize,
}

pub struct Engine {
    pub(crate) value_recursion: HashMap<u64, Vec<crate::value_recursion::Key>>,
    pub(crate) file_temp_sequence: u64,
    #[cfg(any(feature = "std", feature = "upstream-runtime"))]
    pub(crate) regex_tables: crate::jregex::Tables,
    #[cfg(any(feature = "std", feature = "upstream-runtime"))]
    pub(crate) io_tables: crate::jio_chars::Tables,
    #[cfg(any(feature = "std", feature = "upstream-runtime"))]
    pub(crate) tokenizers: HashMap<ObjRef, crate::jstring_tokenizer::State>,
    pub(crate) collection_views: crate::jcollections::Views,
    pub(crate) output: alloc::rc::Rc<crate::app_output::Output>,
    #[cfg(all(not(feature = "desktop-runtime"), feature = "upstream-runtime"))]
    pub(crate) parity_vfs: crate::module_tables::Local<core::cell::RefCell<crate::jfile::Vfs>>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) module_tables: crate::module_tables::ModuleTables,
    pub(crate) framework_backend: Option<alloc::rc::Rc<dyn crate::framework_host::FrameworkHost>>,
    #[cfg(feature = "jit")]
    pub(crate) jit: crate::jit::JitState,
    #[cfg(feature = "native")]
    pub(crate) native: Option<crate::native::NativeState>,
    #[cfg(feature = "native")]
    pub(crate) native_active: bool,
    pub cp: Classpath,
    pub heap: Heap,
    pub config: VmConfig,
    /// (classe, campo) → valor
    pub statics: HashMap<(String, String), Value>,
    /// Bounded Java valueOf identities, shared by desktop and device dispatch.
    /// The collectors retain these VM-owned constants until Engine teardown.
    pub(crate) boxed_cache: HashMap<(String, i64), ObjRef>,
    /// Loaded DEX literals share identity for this Engine's classloader.
    /// Runtime-created Strings are ordinary collectible heap objects.
    pub(crate) literal_strings: HashMap<String, ObjRef>,
    pub(crate) class_objects: HashMap<String, ObjRef>,
    clinit_done: HashSet<String>,
    #[cfg(feature = "desktop-runtime")]
    clinit_running: HashSet<String>,
    /// Only the actual initializing owner may recursively observe its class.
    /// Entries are removed on every completed/failed initialization.
    #[cfg(not(feature = "desktop-runtime"))]
    clinit_running: HashMap<String, u64>,
    /// classes cujo `<clinit>` falhou — JLS 12.4.2: acessos seguintes levantam
    /// NoClassDefFoundError (issue #37)
    clinit_failed: HashSet<String>,
    /// M3: estado host do framework (Activity/View/Handler/…)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub fw: crate::framework::HostState,
    #[cfg(feature = "desktop-runtime")]
    pub(crate) depth: usize,
    pub(crate) fuel_used: u64,
    pub(crate) fuel_window_start: u64,
    /// M7: telemetria de launch — `Some` só com `RD_LAUNCH_TRACE` no
    /// ambiente; desligada, custa um branch por resolução.
    #[cfg(feature = "std")]
    pub(crate) launch_trace: Option<LaunchTrace>,
    /// AG-09: cobertura de métodos — chave `(dex_idx << 32) | method_idx`
    /// de cada corpo DEX que chegou a executar neste Engine.
    pub(crate) coverage: HashSet<u64>,
    #[cfg(feature = "desktop-runtime")]
    pub(crate) frame_labels: Vec<String>,
    /// Cooperative green threads (threads.rs)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub threads: crate::threads::Threads,
    /// Session permission grant set (perms.rs); None = all granted.
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub granted_permissions: Option<HashSet<String>>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) pending_permission_results: Vec<crate::perms::PendingPermissionResult>,
    /// window → (decor, content frame) (window.rs)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) window_decor:
        HashMap<crate::heap::ObjRef, (crate::heap::ObjRef, crate::heap::ObjRef)>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) system_ui_visibility: HashMap<crate::heap::ObjRef, i32>,
    /// General View property store (view_props.rs): (view, property) → value
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) view_props: HashMap<(crate::heap::ObjRef, String), Value>,
    /// `java.text.BreakIterator` state (jbreakiter.rs)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) break_iters: HashMap<crate::heap::ObjRef, crate::jbreakiter::BreakState>,
    /// BigDecimal/BigInteger values: (unscaled, scale) (jbigdecimal.rs).
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) bignums: HashMap<crate::heap::ObjRef, (i128, i32)>,
    /// dex pc of the instruction the interpreter executed last (diagnostics:
    /// VM errors name the faulting instruction of the innermost frame)
    #[cfg(feature = "desktop-runtime")]
    pub(crate) last_pc: usize,
    /// view_attach.rs: OnAttachStateChangeListeners per view
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) attach_listeners: HashMap<crate::heap::ObjRef, Vec<crate::heap::ObjRef>>,
    /// view_attach.rs: views currently attached to a window
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) attached_views: HashSet<crate::heap::ObjRef>,
    /// graphics.rs: native state of host graphics objects + window display list
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) gfx: crate::graphics::Gfx,
    /// android_util.rs: sparse-array contents (sorted)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) sparse: HashMap<crate::heap::ObjRef, alloc::collections::BTreeMap<i64, Value>>,
    /// view_attach.rs: windows made visible (makeVisible)
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) window_attached: HashSet<crate::heap::ObjRef>,
    /// vto.rs: ViewTreeObserver state, and the observer of each tree root
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) vto: HashMap<crate::heap::ObjRef, crate::vto::Observer>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) vto_by_root: HashMap<crate::heap::ObjRef, crate::heap::ObjRef>,
    /// view_attach.rs: the window token handed to attached views
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) window_token: Option<crate::heap::ObjRef>,
    /// choreographer.rs: frame callbacks
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) frames: crate::choreographer::FrameState,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) frame_pump_active: bool,
    /// activity_callbacks.rs: ActivityLifecycleCallbacks registry
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) activity_callbacks: crate::activity_callbacks::Registry,
    /// activity_extras.rs: setResult, back dispatchers and their callbacks
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) activity_results: HashMap<crate::heap::ObjRef, (i32, Option<crate::heap::ObjRef>)>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) back_dispatchers: HashMap<crate::heap::ObjRef, crate::heap::ObjRef>,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) back_callbacks: HashMap<crate::heap::ObjRef, Vec<(i32, crate::heap::ObjRef)>>,
    /// bundle.rs: Intent action/data/type/flags/package/categories
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) intent_meta: HashMap<crate::heap::ObjRef, crate::bundle::IntentMeta>,
    /// content.rs: ContentResolver observers + Settings table
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) content: crate::content::ContentState,
    /// Mark-sweep collector statistics (`gc.rs`).
    pub(crate) gc_stats: crate::gc::GcStats,
    pub(crate) gc_slots_after: usize,
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) gc_frames: Vec<Vec<crate::heap::ObjRef>>,
    #[cfg(not(feature = "desktop-runtime"))]
    pub(crate) execution_frames: crate::portable::execution::Frames,
    #[cfg(not(feature = "desktop-runtime"))]
    pub(crate) portable: crate::portable::State,
    stack_states: HashMap<u64, NativeStackState>,
    /// Active host->host reentrant call depth (`object_to_string`, `obj_equals`, `obj_hash`, etc.).
    #[cfg(feature = "desktop-runtime")]
    pub(crate) host_reentrant_depth: usize,
}

/// Cobertura por classe (AG-09, nível de método).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassCoverage {
    /// Descritor da classe (ex.: `Lcom/x/Main;`).
    pub class: String,
    /// Métodos com corpo (`code_off != 0`) na classe.
    pub methods_total: u32,
    /// Quantos executaram neste Engine.
    pub methods_executed: u32,
}

/// Snapshot de cobertura do Engine (AG-09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageSnapshot {
    /// Classes com pelo menos um método executável.
    pub classes_total: usize,
    pub methods_total: u32,
    pub methods_executed: u32,
    pub classes: Vec<ClassCoverage>,
}

impl Engine {
    pub fn new(dexes: Vec<rd_dex::Dex>, config: VmConfig) -> Self {
        // Issue #230: nasce um Heap novo (ObjRefs reiniciam) — o estado
        // host thread_local indexado por refs do heap anterior morre aqui.
        #[cfg(feature = "desktop-runtime")]
        crate::reset_host_thread_state();
        let budget = config.heap_budget;
        Engine {
            value_recursion: HashMap::new(),
            file_temp_sequence: 0,
            #[cfg(any(feature = "std", feature = "upstream-runtime"))]
            regex_tables: Default::default(),
            #[cfg(any(feature = "std", feature = "upstream-runtime"))]
            io_tables: Default::default(),
            #[cfg(any(feature = "std", feature = "upstream-runtime"))]
            tokenizers: Default::default(),
            output: Default::default(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            module_tables: Default::default(),
            #[cfg(all(not(feature = "desktop-runtime"), feature = "upstream-runtime"))]
            parity_vfs: crate::module_tables::Local::new(core::cell::RefCell::new(
                Default::default(),
            )),
            #[cfg(feature = "native")]
            native: None,
            #[cfg(feature = "native")]
            native_active: false,
            framework_backend: None,
            cp: Classpath::new(dexes),
            heap: Heap::new(budget),
            config,
            statics: HashMap::new(),
            boxed_cache: HashMap::new(),
            literal_strings: HashMap::new(),
            class_objects: HashMap::new(),
            collection_views: Default::default(),
            clinit_done: HashSet::new(),
            clinit_running: Default::default(),
            clinit_failed: HashSet::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            fw: Default::default(),
            #[cfg(feature = "desktop-runtime")]
            depth: 0,
            fuel_used: 0,
            fuel_window_start: 0,
            #[cfg(feature = "std")]
            launch_trace: std::env::var("RD_LAUNCH_TRACE")
                .ok()
                .is_some()
                .then(LaunchTrace::default),
            coverage: HashSet::new(),
            #[cfg(feature = "desktop-runtime")]
            frame_labels: Vec::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            threads: Default::default(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            granted_permissions: None,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            pending_permission_results: Vec::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            window_decor: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            system_ui_visibility: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            view_props: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            break_iters: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            bignums: HashMap::new(),
            #[cfg(feature = "desktop-runtime")]
            last_pc: 0,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            attach_listeners: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            attached_views: HashSet::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            gfx: Default::default(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            sparse: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            window_attached: HashSet::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            vto: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            vto_by_root: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            window_token: None,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            frames: Default::default(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            frame_pump_active: false,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            activity_callbacks: Default::default(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            activity_results: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            back_dispatchers: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            back_callbacks: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            intent_meta: HashMap::new(),
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            content: Default::default(),
            gc_stats: Default::default(),
            gc_slots_after: 0,
            #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
            gc_frames: Vec::new(),
            #[cfg(not(feature = "desktop-runtime"))]
            execution_frames: Default::default(),
            #[cfg(not(feature = "desktop-runtime"))]
            portable: Default::default(),
            stack_states: Default::default(),
            #[cfg(feature = "desktop-runtime")]
            host_reentrant_depth: 0,
            #[cfg(feature = "jit")]
            jit: {
                let mut j = crate::jit::JitState::default();
                if std::env::var("RD_JIT")
                    .map(|v| v == "on" || v == "1")
                    .unwrap_or(false)
                {
                    if let Err(e) = j.enable() {
                        j.stats.last_error = Some(e);
                    }
                }
                j
            },
        }
    }

    /// Opt in to the bounded integer leaf JIT (default remains interpreter).
    #[cfg(feature = "jit")]
    pub fn enable_jit(&mut self) -> Result<(), String> {
        self.jit.enable()
    }

    #[cfg(feature = "jit")]
    pub fn jit_stats(&self) -> &crate::jit::JitStats {
        &self.jit.stats
    }

    pub fn heap_used(&self) -> usize {
        self.heap.used()
    }

    /// Pico do heap do app (M7 — orçamento `ram_app_heap_mb`).
    pub fn heap_peak(&self) -> usize {
        self.heap.peak()
    }

    /// M8 (38º gap real): descritor da classe de `Application` declarada no
    /// AndroidManifest.xml (`android:name`). Quando presente, `launch_app`
    /// instancia ESSA classe (e não `Landroid/app/Application;`) e roda o
    /// onCreate DEX dela antes do onCreate da activity — como o Android faz.
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub fn set_application_class(&mut self, application_desc: String) {
        self.fw.application_class = Some(application_desc);
    }

    /// Start a new bounded host/tool operation without losing lifetime counters.
    pub fn reset_fuel_window(&mut self) {
        self.fuel_window_start = self.fuel_used;
    }

    /// M7 (`RD_LAUNCH_TRACE`): marco inicial de uma categoria cronometrada.
    #[inline]
    #[cfg(feature = "std")]
    pub(crate) fn trace_start(&self) -> Option<std::time::Instant> {
        self.launch_trace
            .as_ref()
            .map(|_| std::time::Instant::now())
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) fn trace_resolve(&self, t0: Option<std::time::Instant>) {
        if let (Some(tr), Some(t0)) = (&self.launch_trace, t0) {
            LaunchTrace::add(
                &tr.resolve_ns,
                &tr.resolve_calls,
                t0.elapsed().as_nanos() as u64,
            );
        }
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) fn trace_intrinsic(&self, t0: Option<std::time::Instant>, class: &str) {
        if let (Some(tr), Some(t0)) = (&self.launch_trace, t0) {
            let ns = t0.elapsed().as_nanos() as u64;
            LaunchTrace::add(&tr.intrinsic_ns, &tr.intrinsic_calls, ns);
            let mut by = tr.intrinsic_by_class.borrow_mut();
            match by.get_mut(class) {
                Some(e) => {
                    e.0 = e.0.saturating_add(ns);
                    e.1 = e.1.saturating_add(1);
                }
                None => {
                    by.insert(class.to_string(), (ns, 1));
                }
            }
        }
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) fn trace_string(&self, t0: Option<std::time::Instant>) {
        if let (Some(tr), Some(t0)) = (&self.launch_trace, t0) {
            LaunchTrace::add(
                &tr.string_ns,
                &tr.string_calls,
                t0.elapsed().as_nanos() as u64,
            );
        }
    }

    #[inline]
    #[cfg(feature = "std")]
    fn trace_clinit(&self, t0: Option<std::time::Instant>) {
        if let (Some(tr), Some(t0)) = (&self.launch_trace, t0) {
            LaunchTrace::add(
                &tr.clinit_ns,
                &tr.clinit_calls,
                t0.elapsed().as_nanos() as u64,
            );
        }
    }

    /// M7: linha única de decomposição do launch (`None` sem a flag).
    /// Formato estável o bastante para grep em scripts de medição.
    #[cfg(feature = "std")]
    pub fn launch_trace_report(&self) -> Option<String> {
        let tr = self.launch_trace.as_ref()?;
        let ms = |ns: u64| ns as f64 / 1.0e6;
        let mut top: Vec<(String, (u64, u64))> = tr
            .intrinsic_by_class
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        top.sort_by_key(|t| core::cmp::Reverse(t.1 .0));
        let top: Vec<String> = top
            .iter()
            .take(10)
            .map(|(k, (ns, n))| format!("{k}={:.1}ms/{n}", ms(*ns)))
            .collect();
        Some(format!(
            "[launch-trace] fuel={} resolve={:.1}ms/{} intrinsic={:.1}ms/{} clinit={:.1}ms/{} string={:.1}ms/{} top=[{}]",
            self.fuel_used,
            ms(tr.resolve_ns.get()),
            tr.resolve_calls.get(),
            ms(tr.intrinsic_ns.get()),
            tr.intrinsic_calls.get(),
            ms(tr.clinit_ns.get()),
            tr.clinit_calls.get(),
            ms(tr.string_ns.get()),
            tr.string_calls.get(),
            top.join(" "),
        ))
    }

    #[cfg(not(feature = "std"))]
    pub(crate) fn trace_start(&self) -> Option<()> {
        None
    }
    #[cfg(not(feature = "std"))]
    pub(crate) fn trace_resolve(&self, _: Option<()>) {}
    #[cfg(not(feature = "std"))]
    pub(crate) fn trace_intrinsic(&self, _: Option<()>, _: &str) {}
    #[cfg(not(feature = "std"))]
    pub(crate) fn trace_string(&self, _: Option<()>) {}
    #[cfg(not(feature = "std"))]
    fn trace_clinit(&self, _: Option<()>) {}
    #[cfg(not(feature = "std"))]
    pub fn launch_trace_report(&self) -> Option<String> {
        None
    }

    pub fn fuel_remaining(&self) -> u64 {
        self.config
            .fuel
            .saturating_sub(self.fuel_used.saturating_sub(self.fuel_window_start))
    }

    pub fn fuel_used(&self) -> u64 {
        self.fuel_used
    }

    pub(crate) fn frame_depth(&self) -> usize {
        #[cfg(feature = "desktop-runtime")]
        {
            self.depth
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            self.execution_frames.depth(self.heap.execution_owner())
        }
    }
    pub(crate) fn frame_labels(
        &self,
    ) -> impl DoubleEndedIterator<Item = &String> + ExactSizeIterator {
        #[cfg(feature = "desktop-runtime")]
        {
            self.frame_labels.iter()
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            self.execution_frames.labels(self.heap.execution_owner())
        }
    }

    /// Current execution owner's real DEX frames, innermost first. This is a
    /// bounded runtime diagnostic, not Java StackTraceElement reflection.
    pub fn diagnostic_stack(&self) -> Vec<String> {
        self.frame_labels().rev().take(16).cloned().collect()
    }

    /// Actual declaring DEX class on this execution owner's stack. No parsed
    /// diagnostic labels or another suspended thread's frame are consulted.
    /// No frame means a direct host call has no Java access-checking caller.
    pub fn active_declaring_class(&self) -> Result<Option<String>, VmExit> {
        #[cfg(not(feature = "desktop-runtime"))]
        {
            let owner = match self.native_host() {
                None => 0,
                Some(provider) => provider
                    .current_thread_id()
                    .filter(|id| *id != 0)
                    .ok_or_else(|| {
                        crate::err::vm_error("API_UNAVAILABLE", "reflection caller thread identity")
                    })?,
            };
            Ok(self
                .execution_frames
                .declaring_class(owner)
                .map(str::to_string))
        }
        #[cfg(feature = "desktop-runtime")]
        Err(crate::err::vm_error(
            "API_UNAVAILABLE",
            "portable DEX caller metadata is unavailable on the legacy engine",
        )
        .into())
    }

    /// Ponto de entrada público: invoca um método static e devolve o valor.
    /// Exceções Java não capturadas sobem como `VmExit::Exception`.
    pub fn invoke_static(
        &mut self,
        class: &str,
        method: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Value, VmExit> {
        #[cfg(feature = "jit")]
        if self.jit.stats.enabled && trace_method_pattern().is_none() {
            if let Some(res) = crate::jit::try_invoke_static_fast(self, class, method, sig, args) {
                return res;
            }
        }
        let param_types = parse_param_types(sig).ok_or_else(|| {
            crate::err::vm_error("INVALID_ARG", format!("assinatura malformada: {sig}"))
        })?;
        if param_types.len() != args.len() {
            return Err(crate::err::vm_error(
                "INVALID_ARG",
                format!(
                    "{sig} pede {} argumentos, recebi {}",
                    param_types.len(),
                    args.len()
                ),
            )
            .into());
        }
        // intrínseco de plataforma (Math/Integer/…) não precisa de DEX
        // (strings placeholder → heap primeiro)
        let args: Vec<Value> = args
            .iter()
            .map(|v| match v {
                Value::StrPlaceholder(s) => {
                    intrinsics::alloc_string(self, s.clone()).map(Value::Obj)
                }
                other => Ok(other.clone()),
            })
            .collect::<Result<Vec<_>, VmExit>>()?;
        if let Some(v) = crate::intrinsics::call_native_loader(self, class, method, sig, &args)? {
            return Ok(v);
        }
        #[cfg(feature = "advanced-components")]
        if let Some(v) = crate::m9::call_static(self, class, method, sig, &args)? {
            return Ok(v);
        }
        if let Some(v) = intrinsics::call_static_intrinsic(self, class, method, sig, &args)? {
            return Ok(v);
        }
        #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
        if let Some(v) = crate::m8::call_m8_static(self, class, method, sig, &args)? {
            return Ok(v);
        }
        #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
        if class.starts_with("Landroid/") {
            if let Some(v) = crate::framework::call_host_static(self, class, method, sig, &args)? {
                return Ok(v);
            }
        }
        let Some((dex_idx, def, m)) = self.cp.resolve_method(class, method, sig) else {
            return Err(crate::classpath::unresolved_method(class, method, sig).into());
        };
        if m.access_flags & ACC_STATIC == 0 {
            return Err(crate::err::vm_error(
                "INVALID_ARG",
                format!("{class}->{method}{sig} não é static"),
            )
            .into());
        }
        self.ensure_initialized(class)?;
        let slots = slot_form(&args, &param_types);
        let res = self.call(dex_idx, def, &m, slots);
        #[cfg(feature = "jit")]
        if res.is_ok() && self.jit.stats.enabled {
            let key = ((dex_idx as u64) << 32) | u64::from(m.method_idx);
            if self.jit.is_compiled(key) {
                self.jit.bind_static_ic(class, method, sig, key);
            }
        }
        res
    }

    /// Public entry point: `new C()` — runs `<clinit>` (if pending), allocates
    /// the instance, attaches host state and runs the no-arg constructor.
    pub fn new_instance(&mut self, class: &str) -> Result<ObjRef, VmExit> {
        self.ensure_initialized(class)?;
        let r = self.heap.alloc_instance(class.to_string(), Vec::new())?;
        self.attach_host_state(class, r)?;
        self.invoke_instance(r, "<init>", "()V", &[])?;
        Ok(r)
    }

    /// Public entry point: invoke an instance method on `recv`, resolved from
    /// its runtime class (virtual dispatch). Native methods go through the JNI
    /// bridge like any DEX call.
    pub fn invoke_instance(
        &mut self,
        recv: ObjRef,
        method: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Value, VmExit> {
        let param_types = parse_param_types(sig).ok_or_else(|| {
            crate::err::vm_error("INVALID_ARG", format!("assinatura malformada: {sig}"))
        })?;
        if param_types.len() != args.len() {
            return Err(crate::err::vm_error(
                "INVALID_ARG",
                format!(
                    "{sig} pede {} argumentos, recebi {}",
                    param_types.len(),
                    args.len()
                ),
            )
            .into());
        }
        let args: Vec<Value> = args
            .iter()
            .map(|v| match v {
                Value::StrPlaceholder(s) => {
                    intrinsics::alloc_string(self, s.clone()).map(Value::Obj)
                }
                other => Ok(other.clone()),
            })
            .collect::<Result<Vec<_>, VmExit>>()?;
        let class = self.heap.class_of(recv)?.to_string();
        let resolved = self.cp.resolve_method(&class, method, sig);
        #[cfg(not(feature = "desktop-runtime"))]
        let try_host = !self.cp.has_class_method(&class, method, sig);
        #[cfg(feature = "desktop-runtime")]
        let try_host = resolved.is_none();
        if try_host {
            // Host receiver (String, collections, java.io, android.*): the
            // same host chains the interpreter's invoke uses.
            let tok = self.enter_stack_frame(false)?;
            let host_res = (|| {
                if let Some(v) = intrinsics::call_instance_intrinsic_mref(
                    self, &class, &class, method, sig, recv, &args,
                )? {
                    return Ok(v);
                }
                #[cfg(not(feature = "desktop-runtime"))]
                if let Some(host) = self.framework_host() {
                    if let Some(value) = host.call(self, &class, method, sig, recv, &args)? {
                        return Ok(value);
                    }
                }
                #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
                if let Some(v) =
                    crate::m8::call_m8_instance(self, &class, method, sig, recv, &args)?
                {
                    return Ok(v);
                }
                #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
                if let Some(v) =
                    crate::interp::inherited_android_host(self, &class, method, sig, recv, &args)?
                {
                    return Ok(v);
                }
                #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
                if let Some(v) =
                    crate::view_props::fallback(self, &class, method, sig, recv, &args)?
                {
                    return Ok(v);
                }
                Err(crate::classpath::unresolved_method(&class, method, sig).into())
            })();
            self.leave_stack_frame(tok);
            return host_res;
        }
        let (dex_idx, def, m) =
            resolved.ok_or_else(|| crate::classpath::unresolved_method(&class, method, sig))?;
        if m.access_flags & ACC_STATIC != 0 {
            return Err(crate::err::vm_error(
                "INVALID_ARG",
                format!("{class}->{method}{sig} é static"),
            )
            .into());
        }
        let mut slots = vec![Value::Obj(recv)];
        slots.extend(slot_form(&args, &param_types));
        self.call(dex_idx, def, &m, slots)
    }

    /// Monta a forma de registradores (wide → par com WideHi).
    pub fn slot_form_public(&self, args: &[Value], param_types: &[String]) -> Vec<Value> {
        slot_form(args, param_types)
    }

    fn stack_owner(&self) -> Result<u64, VmExit> {
        #[cfg(feature = "desktop-runtime")]
        {
            Ok(0)
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            self.heap.checked_execution_owner()
        }
    }

    fn enter_stack_frame(&mut self, count_vm: bool) -> Result<(u64, bool), VmExit> {
        let owner = self.stack_owner()?;
        if self.config.max_depth == 0 {
            return Err(
                Throwable::new("Ljava/lang/StackOverflowError;", "VM call depth is zero").into(),
            );
        }
        let sp = current_stack_addr();
        let state = self
            .stack_states
            .entry(owner)
            .or_insert_with(|| NativeStackState {
                entry_sp: sp,
                budget: detect_native_stack_budget(sp),
                depth: 0,
            });
        if state.depth >= self.config.max_depth || state.entry_sp.abs_diff(sp) >= state.budget {
            return Err(Throwable::new(
                "Ljava/lang/StackOverflowError;",
                "native/VM call stack budget exceeded",
            )
            .into());
        }
        state.depth += 1;
        #[cfg(feature = "desktop-runtime")]
        if count_vm {
            self.depth += 1;
        }
        Ok((owner, count_vm))
    }

    fn leave_stack_frame(&mut self, token: (u64, bool)) {
        if let Some(state) = self.stack_states.get_mut(&token.0) {
            state.depth = state.depth.saturating_sub(1);
            if state.depth == 0 {
                self.stack_states.remove(&token.0);
            }
        }
        #[cfg(feature = "desktop-runtime")]
        if token.1 {
            self.depth = self.depth.saturating_sub(1);
        }
    }

    /// Same upstream byte-distance guard, scoped to the real execution owner.
    /// A suspended native callback cannot supply another thread's stack anchor.
    pub(crate) fn check_native_stack(&self) -> Result<(), VmExit> {
        if let Some(state) = self.stack_states.get(&self.stack_owner()?) {
            if state.entry_sp.abs_diff(current_stack_addr()) >= state.budget {
                return Err(Throwable::new(
                    "Ljava/lang/StackOverflowError;",
                    "native call stack budget exceeded",
                )
                .into());
            }
        }
        Ok(())
    }

    /// Executa um frame completo (loop de interpretador em `interp.rs`).
    pub(super) fn call(
        &mut self,
        dex_idx: usize,
        def: ClassDef,
        m: &EncodedMethod,
        ins: Vec<Value>,
    ) -> Result<Value, VmExit> {
        let token = self.enter_stack_frame(true)?;
        let result = self.call_with_monitor(dex_idx, def, m, ins);
        self.leave_stack_frame(token);
        result
    }

    fn call_with_monitor(
        &mut self,
        dex_idx: usize,
        def: ClassDef,
        m: &EncodedMethod,
        ins: Vec<Value>,
    ) -> Result<Value, VmExit> {
        // ACC_DECLARED_SYNCHRONIZED (0x20000) is only a source declaration
        // marker in DEX; its non-native body contains explicit monitor ops.
        // ACC_SYNCHRONIZED (0x20), including native methods, needs VM locking.
        #[cfg(not(feature = "desktop-runtime"))]
        let monitor = if m.access_flags & 0x20 != 0 {
            let object = if m.access_flags & ACC_STATIC != 0 {
                let owner = self.cp.dexes[dex_idx].type_str(def.class_idx).to_string();
                self.class_object_for(&owner)?
            } else {
                ins.first()
                    .ok_or_else(|| {
                        crate::err::vm_error("VM_TYPE_ERROR", "synchronized receiver is missing")
                    })?
                    .as_ref()?
                    .ok_or_else(|| {
                        Throwable::new("Ljava/lang/NullPointerException;", "synchronized receiver")
                    })?
            };
            self.monitor_enter(object)?;
            Some(object)
        } else {
            None
        };
        let result = self.call_body(dex_idx, def, m, ins);
        #[cfg(not(feature = "desktop-runtime"))]
        if let Some(object) = monitor {
            self.monitor_exit(object)?;
        }
        result
    }

    fn call_body(
        &mut self,
        dex_idx: usize,
        def: ClassDef,
        m: &EncodedMethod,
        ins: Vec<Value>,
    ) -> Result<Value, VmExit> {
        let combined = self.frame_depth();
        if combined > self.config.max_depth {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/StackOverflowError;",
                format!(
                    "profundidade de chamadas ≥ {} (limite do motor: {})",
                    combined, self.config.max_depth
                ),
            )));
        }
        #[cfg(feature = "desktop-runtime")]
        if let Some(pat) = trace_method_pattern() {
            let label = self.method_label(dex_idx, m);
            if label.contains(pat) {
                eprintln!("T/RustyDroid: call {label} depth={}", self.depth);
            }
        }
        let Some(code) = self.cp.code(dex_idx, m.code_off)? else {
            if m.access_flags & ACC_NATIVE != 0 {
                #[cfg(feature = "native")]
                {
                    let r = self.cp.method_ref(dex_idx, m.method_idx)?;
                    let nat_res = crate::native::invoke(
                        self,
                        &r.class,
                        &r.name,
                        &r.proto,
                        m.access_flags & ACC_STATIC != 0,
                        &ins,
                    );
                    return nat_res;
                }
                #[cfg(not(feature = "desktop-runtime"))]
                if let Some(backend) = self.native_host() {
                    let owner = self.heap.checked_execution_owner()?;
                    let r = self.cp.method_ref(dex_idx, m.method_idx)?;
                    let declaring_class = self.cp.type_str(dex_idx, def.class_idx);
                    let roots = ins
                        .iter()
                        .filter_map(|value| match value {
                            Value::Obj(r) => Some(*r),
                            _ => None,
                        })
                        .collect();
                    let frame = self.execution_frames.enter(
                        owner,
                        self.method_label(dex_idx, m),
                        declaring_class,
                        roots,
                    )?;
                    let scope = self.heap.gc_scope_depth();
                    // A native diagnostic frame makes callbacks non-outermost.
                    // Pin this invocation's fresh host allocations explicitly;
                    // do not keep unrelated existing heap objects alive.
                    self.heap.gc_host_enter(false);
                    let result = backend.invoke(
                        self,
                        &r.class,
                        &r.name,
                        &r.proto,
                        m.access_flags & ACC_STATIC != 0,
                        &ins,
                    );
                    self.execution_frames.leave(frame);
                    self.heap
                        .gc_scope_restore_return(scope, returned_object(&result));
                    return result;
                }
                #[cfg(not(feature = "native"))]
                return Err(crate::err::not_implemented(format!(
                    "método nativo {}",
                    self.method_label(dex_idx, m)
                ))
                .into());
            }
            return Err(crate::err::vm_error(
                "NOT_FOUND",
                format!(
                    "método {} sem corpo (abstrato?)",
                    self.method_label(dex_idx, m)
                ),
            )
            .into());
        };
        // AG-09: cobertura de métodos — o corpo existe e vai executar.
        self.coverage
            .insert(((dex_idx as u64) << 32) | u64::from(m.method_idx));
        #[cfg(feature = "jit")]
        if self.jit.stats.enabled {
            let key = ((dex_idx as u64) << 32) | u64::from(m.method_idx);
            if let Some(value) = crate::jit::try_exec_method(self, dex_idx, m, key, &code, &ins)? {
                return Ok(value);
            }
        }
        // Ponto de partida DEX-corrto do invoke-super: a SUPER da classe que
        // DECLARA o frame atual (não `super(mref.class)` — Dalvik Reference).
        // Resolvido aqui onde temos o ClassDef do frame em mãos.
        let frame_super = {
            let dex = &self.cp.dexes[dex_idx];
            let s = dex.type_str(def.superclass_idx).to_string();
            if s.is_empty() || s == "?" {
                None
            } else {
                Some(s)
            }
        };
        let frame_label = self.method_label(dex_idx, m);
        #[cfg(feature = "desktop-runtime")]
        self.frame_labels.push(frame_label);
        let scope_depth = self.heap.gc_scope_depth();
        #[cfg(feature = "desktop-runtime")]
        let outermost = self.gc_frames.is_empty();
        #[cfg(not(feature = "desktop-runtime"))]
        let outermost = self.frame_depth() == 0;
        let roots = ins
            .iter()
            .filter_map(|value| match value {
                Value::Obj(object) => Some(*object),
                _ => None,
            })
            .collect();
        #[cfg(feature = "desktop-runtime")]
        self.gc_frames.push(roots);
        #[cfg(not(feature = "desktop-runtime"))]
        let frame = self.execution_frames.enter(
            self.heap.execution_owner(),
            frame_label,
            self.cp.type_str(dex_idx, def.class_idx),
            roots,
        )?;
        if outermost {
            self.heap.gc_host_enter(true);
        }
        self.heap.gc_guest_enter();
        // This PC lives on the invocation's Rust stack across native lease
        // releases, so another real thread cannot replace its error location.
        let mut frame_pc = 0;
        let mut ret = interp::exec_frame(
            self,
            dex_idx,
            frame_super.as_deref(),
            code.clone(),
            ins,
            &mut frame_pc,
        );
        #[cfg(feature = "desktop-runtime")]
        self.gc_frames.pop();
        #[cfg(not(feature = "desktop-runtime"))]
        self.execution_frames.leave(frame);
        #[cfg(not(feature = "desktop-runtime"))]
        self.heap
            .gc_scope_restore_return(scope_depth, returned_object(&ret));
        #[cfg(feature = "desktop-runtime")]
        self.heap.gc_scope_restore(scope_depth);
        // A guest allocation returned into a Rust host temporary must survive
        // the host's next guest callback, even before it is stored in a table.
        #[cfg(feature = "desktop-runtime")]
        match &ret {
            Ok(Value::Obj(r)) => self.heap.gc_pin_return(*r),
            Err(VmExit::Exception(t)) => {
                if let Some(r) = t.obj {
                    self.heap.gc_pin_return(r);
                }
            }
            _ => {}
        }
        #[cfg(feature = "desktop-runtime")]
        self.frame_labels.pop();
        // Observabilidade opt-in (default off — nenhuma saída existente muda):
        // exceção Java que este frame NÃO tratou recebe a linha "at" do frame
        // ao desempilhar (estilo JVM, innermost primeiro). Erros estruturados
        // da VM não são Java e não ganham frames.
        if self.config.exception_trace {
            match ret {
                Err(VmExit::Exception(ref mut t)) => {
                    let line = format!("\n  at {}", self.method_label(dex_idx, m));
                    t.message.get_or_insert_with(String::new).push_str(&line);
                    // A VM-built exception already has its heap object, which
                    // is what guest catch blocks see. Mirror the frame into
                    // its `rd_trace` so caught-and-rethrown exceptions keep
                    // the guest stack.
                    if let Some(obj) = t.obj {
                        let prev = match self.heap.get_field(obj, "rd_trace") {
                            Ok(Value::Obj(r)) => {
                                self.heap.as_str(r).map(str::to_string).unwrap_or_default()
                            }
                            _ => String::new(),
                        };
                        if prev.matches("\n  at ").count() < 24 {
                            if let Ok(r) = self.heap.alloc_string(prev + &line) {
                                let _ = self.heap.put_field(obj, "rd_trace", Value::Obj(r));
                            }
                        }
                    }
                }
                // Structured VM errors (NOT_FOUND/NOT_IMPLEMENTED/...) get
                // the guest stack too, so an agent can see which app code
                // hit the gap. The green-thread park signal is control flow,
                // not an error.
                Err(VmExit::Error(ref mut e)) if e.code != "RD_THREAD_PARK" => {
                    if !e.cause.contains("\n  at ") {
                        // innermost frame: name the faulting instruction
                        #[cfg(feature = "desktop-runtime")]
                        let pc = self.last_pc;
                        #[cfg(not(feature = "desktop-runtime"))]
                        let pc = frame_pc;
                        if let Some((_, insn)) = code.instructions.iter().find(|(a, _)| *a == pc) {
                            let mut d = format!("{insn:?}");
                            d.truncate(160);
                            e.cause.push_str(&format!(" [pc 0x{pc:x}: {d}]"));
                        }
                    }
                    let line = format!("\n  at {}", self.method_label(dex_idx, m));
                    e.cause.push_str(&line);
                }
                _ => {}
            }
        }
        ret
    }

    /// Snapshot de cobertura de métodos (AG-09): denominador = métodos com
    /// corpo (`code_off != 0`) nos DEX do classpath; numerador = corpos que
    /// executaram neste Engine. Classes host (framework/intrinsics) não
    /// entram — não são código DEX do app.
    pub fn coverage_snapshot(&mut self) -> CoverageSnapshot {
        let mut classes = Vec::new();
        let mut methods_total = 0u32;
        let mut methods_executed = 0u32;
        for dex_idx in 0..self.cp.dexes.len() {
            // clone barato: ClassDef é pequeno e evita briga de borrow com
            // class_data(&mut self)
            let defs = self.cp.dexes[dex_idx].class_defs.clone();
            for def in &defs {
                let Ok(Some(data)) = self.cp.class_data(dex_idx, def) else {
                    continue;
                };
                let mut total = 0u32;
                let mut executed = 0u32;
                for m in data
                    .direct_methods
                    .iter()
                    .chain(data.virtual_methods.iter())
                {
                    if m.code_off == 0 {
                        continue; // abstract/native — não executável aqui
                    }
                    total += 1;
                    if self
                        .coverage
                        .contains(&(((dex_idx as u64) << 32) | u64::from(m.method_idx)))
                    {
                        executed += 1;
                    }
                }
                if total == 0 {
                    continue;
                }
                methods_total += total;
                methods_executed += executed;
                classes.push(ClassCoverage {
                    class: self.cp.type_str(dex_idx, def.class_idx),
                    methods_total: total,
                    methods_executed: executed,
                });
            }
        }
        CoverageSnapshot {
            classes_total: classes.len(),
            methods_total,
            methods_executed,
            classes,
        }
    }

    /// Rótulos (`Lcls;->nome(proto)`) dos corpos DEX que executaram neste
    /// Engine, ordenados alfabeticamente (o conjunto interno não tem ordem —
    /// a saída ordenada é determinística para diffs/relatórios). Cobertura de
    /// **entrada de método** (AG-09), gravada sempre pelo interpretador —
    /// zero custo de config; métodos host (framework/intrinsics) não entram.
    /// Uso: postmortem de diagnóstico (ex.: `rd app run --trace` lista o que
    /// executou antes de uma exceção não capturada).
    pub fn executed_labels(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.coverage.len());
        for key in &self.coverage {
            let dex_idx = (key >> 32) as usize;
            let method_idx = (*key & 0xffff_ffff) as u32;
            if let Ok(r) = self.cp.method_ref(dex_idx, method_idx) {
                out.push(format!("{}->{}{}", r.class, r.name, r.proto));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(super) fn method_label(&self, dex_idx: usize, m: &EncodedMethod) -> String {
        match self.cp.method_ref(dex_idx, m.method_idx) {
            Ok(r) => format!("{}->{}{}", r.class, r.name, r.proto),
            Err(_) => format!("method@{}", m.method_idx),
        }
    }

    // ── inicialização de classe + campos estáticos ──────────────────────────

    /// Read-only state for portable adapters which must reject initialization
    /// dependencies they cannot yet schedule. Being loaded is not initialized.
    pub fn device_class_is_initialized(&self, class: &str) -> bool {
        self.clinit_done.contains(class)
    }

    /// Garante que a classe passou por static_values + `<clinit>`.
    /// issue #37 (semântica JLS 12.4.2): falha de `<clinit>` converte para
    /// `ExceptionInInitializerError` no primeiro acesso e vira
    /// `NoClassDefFoundError` permanente nos acessos seguintes.
    pub fn ensure_initialized(&mut self, class: &str) -> Result<(), VmExit> {
        if self.clinit_done.contains(class) {
            return Ok(());
        }
        #[cfg(feature = "desktop-runtime")]
        if self.clinit_running.contains(class) {
            return Ok(());
        }
        if self.clinit_failed.contains(class) {
            return Err(VmExit::Exception(Throwable::new(
                "Ljava/lang/NoClassDefFoundError;",
                format!("{class} falhou na inicialização anterior (JLS 12.4.2)"),
            )));
        }
        #[cfg(not(feature = "desktop-runtime"))]
        {
            let owner = self.heap.checked_execution_owner()?;
            if let Some(initializing_owner) = self.clinit_running.get(class) {
                return if *initializing_owner == owner {
                    Ok(())
                } else {
                    Err(crate::err::vm_error(
                        "API_UNAVAILABLE",
                        format!("class initialization of {class} is in progress on another execution owner; waiting is not implemented"),
                    ).into())
                };
            }
            self.clinit_running.insert(class.to_string(), owner);
        }
        #[cfg(feature = "desktop-runtime")]
        self.clinit_running.insert(class.to_string());
        let t0 = self.trace_start();
        let result = self.initialize_class(class);
        self.trace_clinit(t0);
        self.clinit_running.remove(class);
        match result {
            Ok(()) => {
                self.clinit_done.insert(class.to_string());
                Ok(())
            }
            Err(e) => {
                self.clinit_failed.insert(class.to_string());
                // Infrastructure failures are not Java throwables. Preserve
                // their precise code/cause while preventing a partial <clinit>
                // from being retried. A contended class rejected above was
                // never entered and is not poisoned by another owner's call.
                #[cfg(not(feature = "desktop-runtime"))]
                if matches!(&e, VmExit::Error(_)) {
                    return Err(e);
                }
                // issue #41 (JLS 12.4.2): se a falha VEIO da superclasse (o erro
                // já está embrulhado em EIIE/NCDFE), propaga o erro ORIGINAL —
                // duplo-wrap divergia da JVM e mentia na message
                if let VmExit::Exception(t) = &e {
                    #[cfg(not(feature = "desktop-runtime"))]
                    if self.cp.is_subtype(&t.class, "Ljava/lang/Error;") {
                        // JLS12.4.2 step11: an Error is propagated unchanged;
                        // only non-Error Java exceptions are wrapped in EIIE.
                        return Err(e);
                    }
                    if t.class == "Ljava/lang/ExceptionInInitializerError;"
                        || t.class == "Ljava/lang/NoClassDefFoundError;"
                    {
                        return Err(e);
                    }
                }
                let cause = match &e {
                    VmExit::Exception(t) => {
                        format!("{}: {}", t.class, t.message.clone().unwrap_or_default())
                    }
                    VmExit::Error(rd) => rd.cause.clone(),
                };
                Err(VmExit::Exception(Throwable::new(
                    "Ljava/lang/ExceptionInInitializerError;",
                    cause,
                )))
            }
        }
    }

    /// Resolução CANÔNICA de constantes estáticas de host não embutidas em
    /// DEX (System.out/.err, TimeUnit, wrappers JDK, java.util.Locale,
    /// constantes do android.jar, …).
    ///
    /// Issue #330: fonte ÚNICA compartilhada pelo bytecode Dalvik `sget*`
    /// (interp.rs, opcodes 0x60..=0x66) e pelo acesso reflexivo
    /// (`Class.getField` / `Field.get(null)` / enum-constant em
    /// intrinsics.rs). Antes da unificação os dois caminhos tinham conjuntos
    /// de providers divergentes (split-brain): o `sget` devolvia 0/Null para
    /// campos que a reflexão enxergava (android_sysservices /
    /// android_stub_surfaces) e a reflexão não via os 8 providers que o
    /// bytecode enxergava (TimeUnit, jtext_format, android_colorspace,
    /// graphics, parcel, platform_base, collections, jtime) — e nenhum via
    /// as constantes `Locale.*`. A ordem dos providers é documentada e
    /// determinística (owners são disjuntos entre providers, então a ordem
    /// define determinismo, não semântica). Hits são cacheados em
    /// `self.statics`: semântica JDK, duas leituras da mesma constante
    /// devolvem o MESMO objeto (identidade), enquanto `Locale.US` permanece
    /// distinto de qualquer `new Locale("en","US")` construído pelo guest.
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub(crate) fn resolve_host_static(
        &mut self,
        owner: &str,
        field: &str,
    ) -> Result<Option<Value>, VmExit> {
        let key = (owner.to_string(), field.to_string());
        if let Some(v) = self.statics.get(&key).cloned() {
            return Ok(Some(v));
        }
        // issues #43/#48: System.out/.err materializam um PrintStream embutido
        // na primeira leitura (println/print são intrínsecos; a saída vai para
        // stdout/stderr do host; spec PrintStream: nunca lança)
        if owner == "Ljava/lang/System;" && (field == "out" || field == "err") {
            let fd = if field == "err" { 2 } else { 1 };
            let r = self
                .heap
                .alloc_instance(
                    "Ljava/io/PrintStream;".to_string(),
                    vec![("fd".to_string(), Value::Int(fd))],
                )
                .map_err(crate::err::oom_throwable)?;
            let v = Value::Obj(r);
            self.statics.insert(key, v.clone());
            return Ok(Some(v));
        }
        // TimeUnit: instâncias dos 7 enums do JDK (sget já as servia; a
        // reflexão não via — unificado aqui)
        if owner == "Ljava/util/concurrent/TimeUnit;"
            && matches!(
                field,
                "NANOSECONDS"
                    | "MICROSECONDS"
                    | "MILLISECONDS"
                    | "SECONDS"
                    | "MINUTES"
                    | "HOURS"
                    | "DAYS"
            )
        {
            let unit = self.time_unit_obj(field)?;
            let v = Value::Obj(unit);
            self.statics.insert(key, v.clone());
            return Ok(Some(v));
        }
        let v = self.host_static_providers(owner, field)?;
        if let Some(v) = &v {
            self.statics.insert(key, v.clone());
        }
        Ok(v)
    }

    /// Cadeia de providers host estáticos, na ordem canônica do issue #330
    /// (a ordem histórica do `sget`, com os providers que faltavam em cada
    /// lado adicionados no fim de suas metades). Owners são disjuntos entre
    /// providers; cada provider devolve `Ok(None)` para classes que não são
    /// suas.
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    fn host_static_providers(&mut self, owner: &str, field: &str) -> Result<Option<Value>, VmExit> {
        if let Some(v) = crate::boxes::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        // issue #330: constantes predefinidas de java.util.Locale (antes
        // `sget-object Locale.US` devolvia Null e quebrava <clinit> de APKs
        // reais — org.primftpd_71.apk via DateTimeFormatter.ofPattern)
        if let Some(v) = crate::android_res::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::jnormalizer::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::jbigdecimal::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::jtext_format::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_colorspace::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_env::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_build::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::graphics::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::parcel::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_util::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::platform_base::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::collections::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::jtime::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        // m10: constantes do android.jar (Context.*_SERVICE, Sensor.TYPE_*,
        // …) existem independem de hardware virtual ativo — mesmo gate do
        // `sget` histórico (feature-only). A reflexão gateava em
        // virtual_hardware_enabled (divergência do split-brain); unificado.
        #[cfg(feature = "virtual-devices")]
        if let Some(v) = crate::m10::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_sysservices::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        if let Some(v) = crate::android_stub_surfaces::host_static(self, owner, field)? {
            return Ok(Some(v));
        }
        Ok(None)
    }

    /// issue #57 (JVMS 5.4.3.2): classe DECLARANTE do slot static de um campo
    /// referenciado por `ref_class` — sobe superclasses/interfaces. Classe
    /// fora do DEX (plataforma embutida, ex. System) declara nela mesma
    /// (System.out/err é materializado no sget). Campo inexistente em toda a
    /// cadeia → `NoSuchFieldError` CAPTURÁVEL (antes: default silencioso e,
    /// no sput, slot divergente por referência).
    pub(crate) fn resolve_static_owner(
        &mut self,
        ref_class: &str,
        name: &str,
    ) -> Result<String, VmExit> {
        if self.cp.find_class(ref_class).is_none() {
            return Ok(ref_class.to_string()); // builtin/plataforma: própria
        }
        if let Some(owner) = self.cp.resolve_static_field(ref_class, name) {
            return Ok(owner);
        }
        Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NoSuchFieldError;",
            format!(
                "campo estático '{name}' não existe em {ref_class} (nem na cadeia de superclasses/interfaces) — JVMS 5.4.3.2"
            ),
        )))
    }

    /// issue #61: valor TIPADO de um recurso (melhor config do arsc) — o que
    /// o LayoutInflater usa para resolver `@dimen`/`@bool` por referência.
    /// Erro tipado se o resid não existe ou o tipo não é representável
    /// (nunca default silencioso — resíduo da deferral da #46).
    #[cfg(any(feature = "desktop-runtime", feature = "upstream-runtime"))]
    pub fn resolve_resource_value(
        &self,
        resid: u32,
    ) -> Result<rd_apk::arsc::ResolvedValue, VmExit> {
        let apk = self.resources()?;
        let arsc = apk
            .arsc
            .as_ref()
            .ok_or_else(|| crate::err::vm_error("RESOURCES_MISSING", "APK sem resources.arsc"))?;
        arsc.resolve_value(resid).ok_or_else(|| {
            crate::err::with_suggestion(
                crate::err::vm_error(
                    "RESOURCE_NOT_FOUND",
                    format!("recurso 0x{resid:08x} não resolvido no resources.arsc (inexistente ou tipo bag/complex)"),
                ),
                "resid inválido para este APK; entradas complexas (bags) não são valores tipados",
            )
            .into()
        })
    }

    fn initialize_class(&mut self, class: &str) -> Result<(), VmExit> {
        // issue #25: JLS 12.4.2 — a superclasse inicializa ANTES da subclasse
        // (o guard de re-entrância em ensure_initialized protege ciclos)
        if let Some(super_desc) = self.cp.superclass_of(class) {
            if super_desc != class {
                self.ensure_initialized(&super_desc)?;
            }
        }
        let Some((dex_idx, def)) = self.cp.find_class(class) else {
            return Ok(()); // classe de plataforma embutida: nada a inicializar
        };
        let (dex_idx, def) = (dex_idx, *def);
        // defaults por tipo + static_values por posição
        let static_values = self.cp.dexes[dex_idx].static_values(&def)?;
        let data = self.cp.class_data(dex_idx, &def)?;
        if let Some(cd) = &data {
            for (i, f) in cd.static_fields.iter().enumerate() {
                let (_fclass, fname, ftype) = self.cp.field_ref(dex_idx, f.field_idx)?;
                let v = match static_values.get(i) {
                    // issue #25: EncodedValue::String alocado no heap — R8/otimizadores
                    // elidem o sput de strings estáticas (viram static_values) e o
                    // campo NÃO pode virar null silenciosamente
                    Some(rd_dex::annotations::EncodedValue::String(idx)) => {
                        let s = self.cp.dexes[dex_idx].string(*idx).to_string();
                        Value::Obj(crate::java_values::literal_string(self, s)?)
                    }
                    Some(ev) => crate::value_from_encoded(ev, &self.cp.dexes[dex_idx])
                        .unwrap_or_else(|| default_for(&ftype)),
                    None => default_for(&ftype),
                };
                self.statics.insert((class.to_string(), fname), v);
            }
        }
        // <clinit> (se existir) — issue #39: procurar APENAS na própria classe.
        // resolve_method sobe a cadeia de supers e re-executaria o <clinit> do
        // ancestral (que já rodou no ensure_initialized(super) acima).
        if let Some((dex_idx, def, m)) = self.cp.find_own_method(class, "<clinit>", "()V") {
            self.call(dex_idx, def, &m, Vec::new())?;
        }
        Ok(())
    }

    // ── exceções ────────────────────────────────────────────────────────────

    /// Procura handler para a exceção lançada em `at_pc`. Materializa o
    /// Throwable como objeto quando o handler vai usá-lo.
    ///
    /// issue #23: o DEX ordena os tries por start_addr — um try EXTERNO que
    /// contém o pc vem antes de um interno. ART/JVM selecionam o MAIS INTERNO
    /// com handler compatível: coletamos todos os tries que cobrem `at_pc` e
    /// vence o de maior start_addr com typed-handler ou catch-all casando.
    pub(super) fn enter_handler(
        &mut self,
        dex_idx: usize,
        code: &Arc<CodeItem>,
        at_pc: usize,
        t: Throwable,
    ) -> Result<usize, VmExit> {
        let mut best: Option<(u32, usize)> = None; // (start_addr, handler_addr)
        for (i, try_item) in code.tries.iter().enumerate() {
            let start = try_item.start_addr as usize;
            if at_pc < start || at_pc >= start + try_item.insn_count as usize {
                continue;
            }
            let handler = &code.handlers[i];
            let mut matched: Option<usize> = None;
            for (type_idx, addr) in &handler.typed {
                let catch_desc = self.cp.type_str(dex_idx, *type_idx);
                if catch_desc == t.class || self.cp.is_subtype(&t.class, &catch_desc) {
                    matched = Some(*addr as usize);
                    break;
                }
            }
            let matched = matched.or(handler.catch_all.map(|a| a as usize));
            if let Some(addr) = matched {
                // mais interno = maior start_addr que ainda cobre o pc
                if best.is_none_or(|(bs, _)| start as u32 >= bs) {
                    best = Some((start as u32, addr));
                }
            }
        }
        best.map(|(_, addr)| Ok(addr))
            .unwrap_or_else(|| Err(VmExit::Exception(t)))
    }

    /// Materialize a VM-built Throwable for exception delivery or a host callback.
    /// Keeps an existing exception object and its subclass state unchanged;
    /// otherwise uses the same path as the interpreter's `move-exception`.
    /// The caller must root the returned object across subsequent VM calls.
    pub fn materialize_throwable(&mut self, t: &Throwable) -> Result<ObjRef, VmExit> {
        if let Some(obj) = t.obj {
            return Ok(obj);
        }
        crate::throwable_message::materialize(self, &t.class, t.message.as_deref())
    }

    /// Cria um Throwable VM-built (ArithmeticException, NPE, …) com objeto.
    pub(super) fn vm_exception(&mut self, class: &str, message: &str) -> Result<Throwable, VmExit> {
        let obj = crate::throwable_message::materialize(self, class, Some(message))?;
        Ok(Throwable {
            class: class.to_string(),
            message: Some(message.to_string()),
            obj: Some(obj),
        })
    }
}

/// Valor default por descritor de campo.
pub fn default_for(desc: &str) -> Value {
    match desc {
        "J" => Value::Long(0),
        "F" => Value::Float(0.0),
        "D" => Value::Double(0.0),
        "L" | "Z" | "B" | "C" | "S" | "I" => Value::Int(0),
        d if d.starts_with('L') || d.starts_with('[') => Value::Null,
        _ => Value::Int(0),
    }
}

/// Converte args tipados para a forma de registradores (wide = 2 slots).
pub fn slot_form(args: &[Value], param_types: &[String]) -> Vec<Value> {
    let mut out = Vec::with_capacity(args.len() + 2);
    for (v, t) in args.iter().zip(param_types) {
        let is_wide = t == "J" || t == "D";
        match (is_wide, v) {
            (true, Value::Long(l)) => {
                out.push(Value::Long(*l));
                out.push(Value::WideHi);
            }
            (true, Value::Double(d)) => {
                out.push(Value::Double(*d));
                out.push(Value::WideHi);
            }
            (_, Value::Long(l)) => {
                // arg passado como long mas param não-wide: trunca (IJ misto é
                // erro do caller; tolerar com truncamento explícito)
                out.push(Value::Int(*l as i32));
            }
            (_, other) => out.push(other.clone()),
        }
    }
    out
}

/// Quebra `(params)ret` em descritores de parâmetro.
/// issue #37: assinatura malformada (ex. `(Lfoo)` sem `;`, `[` solto, letra
/// inválida) devolve None — erro tipado no chamador, nunca pânico/abort
/// (o loop antigo `while b[i] != b';'` estourava o buffer).
/// M8: parâmetros de uma assinatura Dalvik (para slotize de chamadas DEX
/// feitas pelo host, ex.: callbacks de adapter/dialog/broadcast).
pub fn parse_params_for_m8(sig: &str) -> Vec<String> {
    parse_param_types(sig).unwrap_or_default()
}

pub fn parse_param_types(sig: &str) -> Option<Vec<String>> {
    let rest = sig.strip_prefix('(')?;
    let close = rest.rfind(')')?;
    let params = &rest[..close];
    let mut out = Vec::new();
    let b = params.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None; // `[` sem componente
        }
        match b[i] {
            b'L' => {
                while i < b.len() && b[i] != b';' {
                    i += 1;
                }
                if i >= b.len() {
                    return None; // classe sem terminador `;`
                }
            }
            b'J' | b'F' | b'D' | b'B' | b'Z' | b'C' | b'S' | b'I' => {}
            _ => return None, // desc de parâmetro inválido
        }
        i += 1;
        out.push(params[start..i].to_string());
    }
    Some(out)
}

#[cfg(not(feature = "desktop-runtime"))]
fn returned_object(result: &Result<Value, VmExit>) -> Option<ObjRef> {
    match result {
        Ok(Value::Obj(object)) => Some(*object),
        Err(VmExit::Exception(throwable)) => throwable.obj,
        _ => None,
    }
}

/// `RD_TRACE_METHOD=<substring>`: log every guest method entry whose label
/// (`Lpkg/Class;->name(sig)`) contains the substring (debugging aid).
#[cfg(feature = "desktop-runtime")]
fn trace_method_pattern() -> Option<&'static str> {
    static PAT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PAT.get_or_init(|| {
        std::env::var("RD_TRACE_METHOD")
            .ok()
            .filter(|s| !s.is_empty())
    })
    .as_deref()
}

#[cfg(all(test, target_os = "linux"))]
mod stack_budget_tests {
    use super::guard_page_below;

    /// Linha real de `/proc/thread-self/maps` para uma pilha de thread da glibc:
    /// a página guarda `PROT_NONE` vem imediatamente abaixo.
    #[test]
    fn thread_stack_with_guard_page_is_trusted() {
        let lo = 0x7f30_5233_e000usize;
        let lines = [
            "7f305233d000-7f305233e000 ---p 00000000 00:00 0",
            "7f305233e000-7f3052540000 rw-p 00000000 00:00 0",
        ];
        assert!(guard_page_below(&lines, lo));
    }

    /// O caso que quebrava o CI (#313/#394): o kernel funde a pilha com
    /// mapeamentos anônimos vizinhos, então a VMA que contém o SP é muito maior
    /// que a pilha real e não há página guarda para ancorar `hi - lo`.
    #[test]
    fn merged_anonymous_vma_is_not_trusted() {
        let lo = 0x7f30_5233_e000usize;
        let lines = [
            "7f3052000000-7f305233e000 rw-p 00000000 00:00 0",
            "7f305233e000-7f3052540000 rw-p 00000000 00:00 0",
        ];
        assert!(!guard_page_below(&lines, lo));
    }

    /// Região isolada (sem vizinho adjacente): também não é confiável.
    #[test]
    fn isolated_vma_without_adjacent_guard_is_not_trusted() {
        let lo = 0x7f30_5233_e000usize;
        let lines = [
            "7f3051b3c000-7f3051b3d000 r--p 00000000 08:01 1234 /usr/lib/libc.so.6",
            "7f305233e000-7f3052540000 rw-p 00000000 00:00 0",
        ];
        assert!(!guard_page_below(&lines, lo));
    }

    /// A guarda só vale se for *imediatamente* anterior.
    #[test]
    fn non_adjacent_guard_page_is_not_trusted() {
        let lo = 0x7f30_5233_e000usize;
        let lines = [
            "7f305233c000-7f305233d000 ---p 00000000 00:00 0",
            "7f305233e000-7f3052540000 rw-p 00000000 00:00 0",
        ];
        assert!(!guard_page_below(&lines, lo));
    }
}

#[cfg(all(test, not(feature = "desktop-runtime")))]
mod stack_owner_tests {
    use super::*;
    use alloc::rc::Rc;
    use core::cell::Cell;
    struct Owner(Cell<u64>);
    impl crate::native_host::NativeHost for Owner {
        fn current_thread_id(&self) -> Option<u64> {
            Some(self.0.get())
        }
        fn load(&self, _: &mut Engine, _: &str, _: bool) -> Result<(), VmExit> {
            unreachable!()
        }
        fn invoke(
            &self,
            _: &mut Engine,
            _: &str,
            _: &str,
            _: &str,
            _: bool,
            _: &[Value],
        ) -> Result<Value, VmExit> {
            unreachable!()
        }
        fn roots(&self) -> Vec<ObjRef> {
            vec![]
        }
    }
    #[test]
    fn stack_guards_follow_execution_owners_and_release_after_overflow() {
        let mut vm = Engine::new(
            vec![],
            VmConfig {
                max_depth: 1,
                ..Default::default()
            },
        );
        let owner = Rc::new(Owner(Cell::new(10)));
        vm.bind_native(owner.clone());
        let a = vm.enter_stack_frame(true).unwrap();
        assert!(
            matches!(vm.enter_stack_frame(false), Err(VmExit::Exception(e)) if e.class == "Ljava/lang/StackOverflowError;")
        );
        owner.0.set(20);
        let b = vm.enter_stack_frame(false).unwrap();
        // A can resume and unwind while B still owns a suspended callback.
        owner.0.set(10);
        vm.leave_stack_frame(a);
        assert!(!vm.stack_states.contains_key(&10));
        assert_eq!(vm.stack_states[&20].depth, 1);
        let resumed = vm.enter_stack_frame(true).unwrap();
        vm.leave_stack_frame(resumed);
        owner.0.set(20);
        vm.leave_stack_frame(b);
        assert!(vm.stack_states.is_empty());
        owner.0.set(0);
        assert!(vm.enter_stack_frame(true).is_err());
        assert!(vm.stack_states.is_empty());
    }
    #[test]
    fn zero_depth_rejection_does_not_publish_a_stale_stack_anchor() {
        let mut vm = Engine::new(
            vec![],
            VmConfig {
                max_depth: 0,
                ..Default::default()
            },
        );
        assert!(vm.enter_stack_frame(true).is_err());
        assert!(vm.stack_states.is_empty());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_stack_budget_tests {
    use super::*;

    /// #469: em thread de 2 MiB, o orçamento deixa pelo menos 1 MiB de folga
    /// entre o orçamento e o início (base) da pilha — margem para o último
    /// salto host→host antes da próxima checagem.
    #[test]
    fn two_mib_thread_budget_keeps_one_mib_headroom_469() {
        let handle = std::thread::Builder::new()
            .name("budget-469".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let sp = current_stack_addr();
                let maps = std::fs::read_to_string("/proc/thread-self/maps").unwrap_or_default();
                let lo = maps
                    .lines()
                    .filter_map(|l| {
                        let (range, rest) = l.split_once(' ')?;
                        let (lo_hex, hi_hex) = range.split_once('-')?;
                        let (lo, hi) = (
                            usize::from_str_radix(lo_hex, 16).ok()?,
                            usize::from_str_radix(hi_hex, 16).ok()?,
                        );
                        (rest.starts_with("rw") && sp >= lo && sp <= hi).then_some(lo)
                    })
                    .next();
                (detect_native_stack_budget(sp), lo, sp)
            })
            .expect("spawn 2 MiB thread");
        let (budget, lo, sp) = handle.join().expect("budget thread");
        if let Some(lo) = lo {
            assert!(
                sp - lo - budget >= NATIVE_STACK_HEADROOM,
                "orçamento {budget} B deixa menos de 1 MiB de folga (pilha {} B)",
                sp - lo
            );
        }
        assert!(budget >= 256 * 1024, "orçamento {budget} B abaixo do piso");
    }
}
