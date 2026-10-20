//! Framework Android host (M3 — CORE-05/CORE-06 subset headless).
//!
//! Classes de plataforma (Activity/View/TextView/Button/LinearLayout/
//! FrameLayout/Handler/Looper/Context/Intent/Bundle) não vivem no DEX do app:
//! são classes HOST cujo estado fica fora do heap Dalvik, indexado por
//! `ObjRef` (o heap guarda o "shell" da instância; o estado Java real vive
//! aqui). O dispatch segue o padrão dos intrinsics: método com classe
//! declarante `Landroid/…` cai aqui ANTES da resolução DEX.
//!
//! Semântica M3 (documentada):
//! - Single-thread determinístico: o Looper principal é uma fila com clock
//!   virtual (`HostState.clock_ms`); `advance_clock` dispara postDelayed.
//! - Layout mínimo: LinearLayout (vertical/horizontal) empilha; FrameLayout
//!   sobrepõe em (0,0). Alturas de leaf: TextView/Button 48px; View 0.
//!   Render pixel e LayoutInflater de XML de layout (Resources/ARSC) entram
//!   na sequência do M3 — aqui a árvore é programática (setContentView(View))
//!   ou via layout resources quando resolúvel.
//! - Touch: hit-test de baixo para cima na z-order (último filho primeiro);
//!   o listener (objeto do app que implementa View.OnClickListener) é
//!   invocado via resolução DEX.

use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::intrinsics;
use crate::value::Value;

pub const ACTIVITY: &str = "Landroid/app/Activity;";
pub const APPLICATION: &str = "Landroid/app/Application;";
/// M8: FragmentManager legacy (`android.app`, deprecated na API 28 mas ainda
/// usado por apps reais como o org.primftpd — 29º gap do launch real).
pub const FRAGMENT_MANAGER: &str = "Landroid/app/FragmentManager;";
/// M8 (44º gap real): PreferenceManager/SharedPreferences host.
pub const PREFERENCE_MANAGER: &str = "Landroid/preference/PreferenceManager;";
/// Interface de plataforma (mref das chamadas das activities).
pub const SHARED_PREFS: &str = "Landroid/content/SharedPreferences;";
/// Classe impl canônica do host (SharedPreferencesImpl, como no AOSP).
pub const SHARED_PREFS_IMPL: &str = "Landroid/content/SharedPreferencesImpl;";
/// M8 (31º gap): FragmentTransaction legacy — ops pendentes até `commit`.
pub const FRAGMENT_TX: &str = "Landroid/app/FragmentTransaction;";
/// M8 (32º gap): Fragment legacy — a classe da fragment do app estende esta;
/// o ctor do super precisa existir como no-op (como Activity/APPLICATION).
pub const FRAGMENT_CLS: &str = "Landroid/app/Fragment;";
pub const CONTEXT: &str = "Landroid/content/Context;";
pub const INTENT: &str = "Landroid/content/Intent;";
pub const BUNDLE: &str = "Landroid/os/Bundle;";
pub const VIEW: &str = "Landroid/view/View;";
pub const VIEWGROUP: &str = "Landroid/view/ViewGroup;";
pub const TEXTVIEW: &str = "Landroid/widget/TextView;";
pub const BUTTON: &str = "Landroid/widget/Button;";
pub const LINEARLAYOUT: &str = "Landroid/widget/LinearLayout;";
pub const FRAMELAYOUT: &str = "Landroid/widget/FrameLayout;";
pub const HANDLER: &str = "Landroid/os/Handler;";
pub const LOOPER: &str = "Landroid/os/Looper;";
/// M8: relógio host (relógio virtual determinístico — ver `call_host_static`).
pub const SYSTEM_CLOCK: &str = "Landroid/os/SystemClock;";
/// M8: Thread host — a VM é single-threaded: existe UMA thread sintética
/// "main" (id 1, não-daemon), devolvida por `Looper.getThread`/
/// `Thread.currentThread`.
pub const THREAD: &str = "Ljava/lang/Thread;";
pub const CLASS: &str = "Ljava/lang/Class;";

/// Dimensões da window virtual (piso moto-e5 em px — 720x1440 HD+).
pub const WINDOW_W: i32 = 720;
pub const WINDOW_H: i32 = 1440;
/// Altura default de widget single-line (touch target de 1 linha).
pub const ROW_H: i32 = 48;
/// Densidade virtual dp→px do M3.2: 720px / 2.0 = viewport de 360dp (padrão
/// Android). sp usa a mesma escala (sem font scale no modelo headless).
pub const DENSITY: f32 = 2.0;

/// issue #40: cap de profundidade para caminhos host na árvore de views
/// (hit-test/layout/dump) — árvore programática profunda (addView) não pode
/// estourar a stack Rust e abortar o processo. Mesmo teto do inflate.
const MAX_VIEW_DEPTH: usize = 512;

/// Virtual vsync period.
const VSYNC_MS: u64 = 16;
/// Cap on vsync-stepped ticks per `advance_clock` (about 64 s of animation).
const MAX_VSYNC_TICKS: usize = 4000;

/// Estado host de um objeto. O heap guarda apenas a classe; o estado Java
/// real fica aqui. Um ObjRef = um HostObj.
/// issue #46: visibilidade com 3 estados reais do Android — INVISIBLE ocupa
/// espaço (não desenha, não clica); GONE é removido do layout (irmãos sobem).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    Visible,
    Invisible,
    Gone,
}

impl Vis {
    pub fn from_java(v: i32) -> Self {
        match v {
            1 | 4 => Vis::Invisible, // legacy XML ordinal / Android INVISIBLE
            2 | 8 => Vis::Gone,      // legacy XML ordinal / Android GONE
            _ => Vis::Visible,       // 0 e valores desconhecidos → visible
        }
    }
    pub fn to_java(self) -> i32 {
        match self {
            Vis::Visible => 0,
            Vis::Invisible => 1,
            Vis::Gone => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HostObj {
    /// Activity — window com a árvore de views; finished encerra o M3.
    Activity {
        window: Option<ObjRef>,
        finished: bool,
        intent: Option<ObjRef>,
    },
    Window {
        content: Option<ObjRef>,
    },
    /// Todo o leque de views (o "tipo" vem da classe DEX do objeto):
    /// TextView/Button usam `text`; LinearLayout/FrameLayout usam
    /// `children`+`orientation`.
    View {
        id: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        vis: Vis,
        enabled: bool,
        click_listener: Option<ObjRef>,
        /// android:onClick="metodo" (XML) — resolve no contexto da ACTIVITY
        click_method: Option<String>,
        parent: Option<ObjRef>,
        text: String,
        children: Vec<ObjRef>,
        orientation: i32, // 0 = horizontal, 1 = vertical (default Android)
        /// issue #59: layout_width com dimensão explícita (px) — vence o slot
        /// distribuído pelo pai (match_parent implícito = None). Sempre ≤
        /// WINDOW_W (validado no inflate; clamp aqui por defesa em profundidade).
        w_explicit: Option<i32>,
    },
    Handler {
        looper: ObjRef,
    },
    Looper {
        queue: Vec<QueuedMsg>,
    },
    Intent {
        component: Option<String>,
        extras: Vec<(String, Value)>,
    },
    Bundle {
        map: Vec<(String, Value)>,
    },
    /// M8: coleções java.util host — Map (HashMap/WeakHashMap) e List
    /// (ArrayList/LinkedList). Estado linear (volumes pequenos); igualdade
    /// de chave/elemento via `intrinsics::values_equal` (String/boxes por
    /// valor, senão identidade). WeakHashMap keys are weak during GC;
    /// values remain strong and null-key entries are never GC-removed.
    Map {
        entries: Vec<(Value, Value)>,
    },
    List {
        elems: Vec<Value>,
    },
    /// M8: iterador host — snapshot de elementos + cursor (determinístico;
    /// removal writes through when rd$owner is set; immutable snapshots
    /// still reject remove).
    Iter {
        elems: Vec<Value>,
        pos: usize,
    },
    /// M8 (45º gap real — slf4j 2.x do primftpd): ServiceLoader host com os
    /// providers descobertos em `META-INF/services/<nome binário da
    /// interface>` (arquivo indexado de qualquer classes.dex do APK).
    /// `service_desc` guarda o descritor da interface; `providers` são
    /// descritores de classes impl (comentários `#` e linhas vazias
    /// descartados como no formato java.util.ServiceLoader).
    ServiceLoader {
        service_desc: String,
        providers: Vec<String>,
    },
    ReflectField {
        declaring_class: String,
        name: String,
        field_type: String,
        modifiers: u32,
        is_static: bool,
        accessible: bool,
    },
}

#[derive(Debug, Clone)]
pub struct QueuedMsg {
    pub when_ms: u64,
    /// Runnable do app — o drain invoca run() via resolução DEX. For a
    /// Message entry (handler_msg.rs) this is the Message object.
    pub runnable: ObjRef,
    /// target Handler (removeCallbacks/removeMessages scope)
    pub handler: Option<ObjRef>,
    /// true → `runnable` is an android.os.Message to dispatchMessage
    pub is_message: bool,
    /// `postDelayed(r, token, ms)` / `removeCallbacksAndMessages(token)`
    pub token: Option<ObjRef>,
}

/// Estado de framework da Engine (host objects + clock virtual + launch).
/// Focus belongs to the paused window, not the currently visible Activity.
#[derive(Debug, Clone, Copy)]
pub struct SuspendedActivity {
    pub activity: ObjRef,
    pub focused_view: Option<ObjRef>,
    pub ime_connection: Option<(ObjRef, ObjRef)>,
    #[cfg(feature = "advanced-components")]
    pub editor_focus: Option<ObjRef>,
    #[cfg(feature = "advanced-components")]
    pub keyboard_visible: bool,
}
#[derive(Debug, Default)]
pub struct HostState {
    pub objects: HashMap<ObjRef, HostObj>,
    /// `List.subList` views: view → (parent, offset, len). See
    /// `jcollections::sublist_*` (write-through semantics).
    pub(crate) sublists: HashMap<ObjRef, (ObjRef, usize, usize)>,
    pub(crate) sublist_order: std::collections::VecDeque<ObjRef>,
    /// clock virtual (ms) — postDelayed/dispatch por ordem de tempo
    pub clock_ms: u64,
    /// Last platform `InputConnection` + its `EditorInfo` opened by
    /// `type_text` on a non-EditText editor (Compose): ENTER performs the
    /// editor's IME action through it.
    pub(crate) ime_connection: Option<(ObjRef, ObjRef)>,
    /// The window's focused view (AOSP ViewRootImpl.mFocusedView); see
    /// `view_layout::move_focus`.
    pub(crate) focused_view: Option<ObjRef>,
    /// A view called invalidate/requestLayout: ViewRootImpl schedules a
    /// traversal for the next vsync (`advance_clock` runs `layout_pass`).
    pub(crate) traversal_pending: bool,
    /// re-entrancy guard for the traversal run from `advance_clock`
    pub(crate) in_traversal: bool,
    main_looper: Option<ObjRef>,
    /// M8: thread "main" sintética (VM single-threaded — ver `THREAD`)
    pub(crate) main_thread: Option<ObjRef>,
    /// M8: objetos `Class` canônicos por descritor (getClass devolve sempre a
    /// mesma referência para a mesma classe — identidade como na JVM).
    class_objs: HashMap<String, ObjRef>,
    /// M8: ClassLoader canônico (um por VM — single-classloader; a distinção
    /// bootstrap=null do Java não se aplica ao runtime host).
    class_loader: Option<ObjRef>,
    /// M8: FragmentManager canônico do host (um por VM — identidade estável
    /// como a do ClassLoader; 29º gap do launch real).
    fragment_manager: Option<ObjRef>,
    /// M8 (31º gap): registro de fragments comittadas (estado mínimo —
    /// sem lifecycle DEX, sem back stack/animations). Ordem = commit.
    fragments: Vec<FragRecord>,
    /// M8 (31º gap): transações abertas por objeto FragmentTransaction
    /// (ops só tocam o registro no commit; 2º commit → IllegalStateException).
    frag_txs: HashMap<ObjRef, FragTxState>,
    /// activity em execução (o app trivial M3 tem uma por vez)
    pub current_activity: Option<ObjRef>,
    /// Paused activities in this bounded single-task model; GC roots via HostState.
    pub activity_stack: Vec<SuspendedActivity>,
    /// package do app (preenchido no launch_app; getPackageName)
    pub package_name: String,
    /// M8 (38º gap real): Application canônica do app — Android cria UMA por
    /// processo antes de qualquer activity. Classe vem do android:name do
    /// manifest (injetada por `Engine::set_application_class`) ou
    /// Landroid/app/Application; quando o app não declara subclasse.
    application: Option<ObjRef>,
    /// M8 (38º gap): descritor da classe de Application do manifest.
    pub application_class: Option<String>,
    /// Canonical synthetic Unsafe singleton used by Java/Kotlin reflection.
    pub unsafe_singleton: Option<ObjRef>,
    pub unsafe_offsets: HashMap<i64, (String, String, String)>,
    pub runtime: Option<ObjRef>,
    pub time_units: HashMap<String, ObjRef>,
    /// M8 (44º gap real): objeto canônico das SharedPreferences padrão
    /// (getDefaultSharedPreferences) — SharedPreferencesImpl do host, vazio
    /// (sem persistência em arquivo nesta milestone).
    pub shared_prefs_impl: Option<ObjRef>,
    /// M3.2: APK aberto (Resources) — layouts AXML + resources.arsc para o
    /// LayoutInflater. Injetado via `Engine::set_resources` (CLI `rd app run`).
    pub resources: Option<rd_apk::Apk>,
    /// M8 full scope: estado dos subsistemas de compat real (RecyclerView,
    /// Dialogs, OkHttp, SQLite/Room, Services/Broadcasts/DeepLinks,
    /// WorkManager, SharedPreferences stateful) — ver `crate::m8`.
    pub m8: crate::m8::M8State,
    #[cfg(feature = "advanced-components")]
    pub m9: crate::m9::State,
}

/// M8 (31º gap): fragment registrada por um commit de FragmentTransaction.
#[derive(Debug, Clone)]
pub struct FragRecord {
    pub obj: ObjRef,
    /// containerViewId (0 = sem container — convenção AOSP; findFragmentById
    /// nunca casa com 0).
    pub container_id: i32,
    pub tag: Option<String>,
}

/// M8 (31º gap): operação pendente de uma FragmentTransaction host.
#[derive(Debug, Clone)]
pub enum FragOp {
    Add {
        rec: FragRecord,
    },
    /// remove TODAS as fragments com o container e adiciona esta (AOSP).
    Replace {
        rec: FragRecord,
    },
    Remove {
        obj: ObjRef,
    },
}

/// M8 (31º gap): estado de uma FragmentTransaction host aberta.
#[derive(Debug, Clone, Default)]
pub struct FragTxState {
    pub ops: Vec<FragOp>,
    pub committed: bool,
}

impl HostState {
    pub fn get(&self, r: ObjRef) -> Option<&HostObj> {
        self.objects.get(&r)
    }

    pub fn clock(&self) -> u64 {
        self.clock_ms
    }
}

// ── relatórios de input do agente (M5) ──────────────────────────────────

/// View atravessada por um swipe (identidade pela classe + bounds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossedView {
    pub class: String,
    /// (x, y, w, h) em px do viewport.
    pub bounds: (i32, i32, i32, i32),
}

/// Resultado de um swipe: views únicas atravessadas (em ordem do gesto) e
/// se o gesto terminou como clique (down e up na mesma view — semântica
/// Android de tap vs scroll).
#[derive(Debug, Clone)]
pub struct SwipeReport {
    pub crossed: Vec<CrossedView>,
    pub clicked: bool,
}

/// Resultado de um evento de tecla: se foi entregue e o efeito observável.
#[derive(Debug, Clone)]
pub struct KeyReport {
    pub key: String,
    pub delivered: bool,
    pub effect: String,
}

/// Teclas suportadas pelo modelo v1 (honesto: o resto é NOT_IMPLEMENTED
/// com sugestão apontando esta lista).
pub const SUPPORTED_KEYS: &[&str] = &["BACK", "ENTER", "DEL"];

impl Engine {
    /// M3.2: injeta o APK aberto como fonte de Resources (layouts AXML +
    /// resources.arsc). O CLI `rd app run` chama isto UMA vez por execução.
    pub fn set_resources(&mut self, apk: rd_apk::Apk) {
        #[cfg(feature = "advanced-components")]
        {
            self.fw.m9.target_sdk = apk.manifest.target_sdk;
            self.fw.m9.notifications_allowed = apk.manifest.target_sdk < 33;
        }
        self.fw.resources = Some(apk);
    }

    /// Resources injetados — erro tipado se ausentes (nunca silencioso).
    pub fn resources(&self) -> Result<&rd_apk::Apk, VmExit> {
        self.fw.resources.as_ref().ok_or_else(|| {
            crate::err::with_suggestion(
                crate::err::vm_error(
                    "RESOURCES_MISSING",
                    "operação de Resources sem APK injetado (layouts/arsc)",
                ),
                "use `rd app run <apk>` (ou Engine::set_resources) — chamadas via Engine::new direto não têm Resources",
            )
            .into()
        })
    }

    /// String de recurso com fallback para o nome qualificado (mesmo padrão do
    /// manifest.resolve_label: valor do arsc → "@type/key" como último recurso).
    pub fn resolve_string(&self, resid: u32) -> Result<String, VmExit> {
        let apk = self.resources()?;
        let arsc = apk
            .arsc
            .as_ref()
            .ok_or_else(|| crate::err::vm_error("RESOURCES_MISSING", "APK sem resources.arsc"))?;
        Ok(arsc
            .resolve_string(resid)
            .or_else(|| arsc.resolve_name(resid))
            .ok_or_else(|| {
                crate::err::with_suggestion(
                    crate::err::vm_error(
                        "RESOURCE_NOT_FOUND",
                        format!("recurso 0x{resid:08x} não resolvido no resources.arsc"),
                    ),
                    "resid inválido para este APK (R.string/R.id são gerados por APK)",
                )
            })?)
    }
}

#[inline]
fn point_in_view_bounds(x: i32, y: i32, rect: (i32, i32, i32, i32)) -> bool {
    let (left, top, width, height) = rect;
    width > 0
        && height > 0
        && x >= left
        && y >= top
        && i64::from(x) < i64::from(left) + i64::from(width)
        && i64::from(y) < i64::from(top) + i64::from(height)
}

fn new_view() -> HostObj {
    HostObj::View {
        id: 0,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        vis: Vis::Visible,
        enabled: true,
        click_listener: None,
        click_method: None,
        parent: None,
        text: String::new(),
        children: Vec::new(),
        orientation: 0,
        w_explicit: None,
    }
}

impl Engine {
    /// Anexa estado host a um objeto recém-alocado (chamado do new-instance).
    /// A herança é por DESCRIÇÃO de superclass gravada no DEX — caminhar
    /// nomes não exige as classes de plataforma no DEX.
    pub fn attach_host_state(&mut self, class: &str, r: ObjRef) -> Result<(), VmExit> {
        if self.fw.objects.contains_key(&r) || r == 0 {
            return Ok(());
        }
        if self.cp.is_subtype(class, ACTIVITY) || self.cp.is_subtype(class, APPLICATION) {
            self.fw.objects.insert(
                r,
                HostObj::Activity {
                    window: None,
                    finished: false,
                    intent: None,
                },
            );
        } else if self.cp.is_subtype(class, HANDLER) {
            let looper = self.main_looper_obj()?;
            self.fw.objects.insert(r, HostObj::Handler { looper });
        } else if self.cp.is_subtype(class, INTENT) {
            self.fw.objects.insert(
                r,
                HostObj::Intent {
                    component: None,
                    extras: Vec::new(),
                },
            );
        } else if self.cp.is_subtype(class, BUNDLE) {
            self.fw
                .objects
                .insert(r, HostObj::Bundle { map: Vec::new() });
        } else if self.cp.is_subtype(class, VIEW) {
            let mut v = new_view();
            // TextView/Button têm altura de touch target por default
            if self.cp.is_subtype(class, TEXTVIEW) {
                if let HostObj::View { h, .. } = &mut v {
                    *h = ROW_H;
                }
            }
            self.fw.objects.insert(r, v);
        }
        Ok(())
    }

    pub(crate) fn main_looper_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(l) = self.fw.main_looper {
            return Ok(l);
        }
        let r = self
            .heap
            .alloc_instance(LOOPER.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.fw
            .objects
            .insert(r, HostObj::Looper { queue: Vec::new() });
        self.fw.main_looper = Some(r);
        Ok(r)
    }

    /// M8: thread "main" sintética (lazy, como o Looper). Única thread da VM —
    /// `Looper.getThread` e `Thread.currentThread` devolvem sempre esta.
    pub(crate) fn main_thread_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(t) = self.fw.main_thread {
            return Ok(t);
        }
        let r = self
            .heap
            .alloc_instance(THREAD.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.fw.main_thread = Some(r);
        Ok(r)
    }

    /// Allocate an instance of `desc` with its host state attached (the
    /// `new-instance` path, for reflective construction).
    pub(crate) fn new_object(&mut self, desc: &str) -> Result<ObjRef, VmExit> {
        let r = self.heap.alloc_instance(desc.to_string(), Vec::new())?;
        self.attach_host_state(desc, r)?;
        Ok(r)
    }

    /// M8: objeto `Class` canônico do descritor (lazy; identidade estável —
    /// `a.getClass() == a.getClass()`). Campo de heap "class_name" guarda o
    /// DESCRITOR (ex.: "Lcom/x/Y;"); os métodos de Class o convertem.
    pub(crate) fn class_object_for(&mut self, desc: &str) -> Result<ObjRef, VmExit> {
        if let Some(c) = self.fw.class_objs.get(desc) {
            return Ok(*c);
        }
        let r = self
            .heap
            .alloc_instance(CLASS.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        let name = self.heap.alloc_string(desc.to_string()).map_err(|e| {
            VmExit::Exception(crate::err::Throwable::new(
                "Ljava/lang/OutOfMemoryError;",
                format!(
                    "alocação de {} bytes excede o heap de {} bytes",
                    e.requested, e.budget
                ),
            ))
        })?;
        crate::err::put_field_result(self.heap.put_field(r, "class_name", Value::Obj(name)))?;
        // legacy field read by Intent(Context, Class)/setClass
        crate::err::put_field_result(self.heap.put_field(r, "name", Value::Obj(name)))?;
        self.fw.class_objs.insert(desc.to_string(), r);
        Ok(r)
    }

    /// M8: ClassLoader canônico da VM (lazy; identidade estável —
    /// `getClassLoader()` de qualquer classe devolve o mesmo objeto).
    pub(crate) fn class_loader_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(l) = self.fw.class_loader {
            return Ok(l);
        }
        let r = self
            .heap
            .alloc_instance("Ljava/lang/ClassLoader;".to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.fw.class_loader = Some(r);
        Ok(r)
    }

    /// One deterministic Runtime facade per Engine.
    pub(crate) fn runtime_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(runtime) = self.fw.runtime {
            return Ok(runtime);
        }
        let runtime = self
            .heap
            .alloc_instance("Ljava/lang/Runtime;".to_string(), Vec::new())
            .map_err(crate::err::oom_throwable)?;
        self.fw.runtime = Some(runtime);
        Ok(runtime)
    }

    /// Canonical enum singleton for java.util.concurrent.TimeUnit.
    pub(crate) fn time_unit_obj(&mut self, name: &str) -> Result<ObjRef, VmExit> {
        if let Some(obj) = self.fw.time_units.get(name) {
            return Ok(*obj);
        }
        let nanos = match name {
            "NANOSECONDS" => 1i64,
            "MICROSECONDS" => 1_000,
            "MILLISECONDS" => 1_000_000,
            "SECONDS" => 1_000_000_000,
            "MINUTES" => 60_000_000_000,
            "HOURS" => 3_600_000_000_000,
            "DAYS" => 86_400_000_000_000,
            _ => return Err(crate::err::vm_error("NOT_FOUND", format!("TimeUnit.{name}")).into()),
        };
        let enum_name = self
            .heap
            .alloc_string(name.to_string())
            .map_err(crate::err::oom_throwable)?;
        let obj = self
            .heap
            .alloc_instance(
                "Ljava/util/concurrent/TimeUnit;".to_string(),
                vec![
                    ("name".to_string(), Value::Obj(enum_name)),
                    ("nanos_per_unit".to_string(), Value::Long(nanos)),
                ],
            )
            .map_err(crate::err::oom_throwable)?;
        self.fw.time_units.insert(name.to_string(), obj);
        Ok(obj)
    }

    /// M8: FragmentManager canônico do host (lazy; identidade estável —
    /// `getFragmentManager()` devolve sempre o mesmo objeto, como no Android
    /// real por Activity; a VM tem uma única activity por execução).
    pub(crate) fn fragment_manager_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(f) = self.fw.fragment_manager {
            return Ok(f);
        }
        let r = self
            .heap
            .alloc_instance(FRAGMENT_MANAGER.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.fw.fragment_manager = Some(r);
        Ok(r)
    }

    /// M8 (38º gap real): Application canônica (uma por VM, identidade
    /// estável como no Android). Lazy para TODAS as rotas de chamada —
    /// criada também pelo launch (ver `launch_app`): a do manifest vence.
    pub(crate) fn application_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(a) = self.fw.application {
            return Ok(a);
        }
        let r = self
            .heap
            .alloc_instance(APPLICATION.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.attach_host_state(APPLICATION, r)?;
        self.fw.application = Some(r);
        Ok(r)
    }

    /// M8 (45º gap real — providers de ServiceLoader): instancia uma classe
    /// do DEX: aloca + anexa estado host + <clinit> (`ensure_initialized`) +
    /// `<init>()V` quando declarado na própria classe. Classe ausente →
    /// NoClassDefFoundError (espelha o runtime; o caller em ServiceLoader
    /// converte/usa conforme a nota do 45º gap).
    /// M8: wrapper público (crate) — Room.build instancia a DB do app.
    pub fn new_dex_instance_pub(&mut self, desc: &str) -> Result<ObjRef, VmExit> {
        self.new_dex_instance(desc)
    }

    pub(crate) fn new_dex_instance(&mut self, desc: &str) -> Result<ObjRef, VmExit> {
        if self.cp.find_class(desc).is_none() {
            return Err(VmExit::Exception(crate::err::Throwable::new(
                "Ljava/lang/NoClassDefFoundError;",
                format!("{desc} não resolvido nos dex do app"),
            )));
        }
        let r = self
            .heap
            .alloc_instance(desc.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.attach_host_state(desc, r)?;
        self.ensure_initialized(desc)?;
        if let Some((d, def, m)) = self.cp.resolve_method(desc, "<init>", "()V") {
            self.call(d, def, &m, vec![Value::Obj(r)])?;
        }
        Ok(r)
    }

    /// M8 (44º gap real — SharedPreferences padrão para
    /// `PreferenceManager.getDefaultSharedPreferences`): objeto canônico
    /// (classe SharedPreferencesImpl, como no AOSP — a VM não persiste em
    /// arquivo nesta milestone: prefs começam vazias e vivem nesta VM).
    pub(crate) fn shared_prefs_obj(&mut self) -> Result<ObjRef, VmExit> {
        if let Some(p) = self.fw.shared_prefs_impl {
            return Ok(p);
        }
        let r = self
            .heap
            .alloc_instance(SHARED_PREFS_IMPL.to_string(), Vec::new())
            .map_err(|e| {
                VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/OutOfMemoryError;",
                    format!(
                        "alocação de {} bytes excede o heap de {} bytes",
                        e.requested, e.budget
                    ),
                ))
            })?;
        self.fw.shared_prefs_impl = Some(r);
        Ok(r)
    }

    // ── driver do app trivial (DoD M3) ───────────────────────────────────

    /// Cria a activity, roda o lifecycle e devolve a ref. `package_name`
    /// alimenta getPackageName; extras vão no intent de launch.
    fn capture_activity_focus(&self, activity: ObjRef) -> SuspendedActivity {
        SuspendedActivity {
            activity,
            focused_view: self.fw.focused_view,
            ime_connection: self.fw.ime_connection,
            #[cfg(feature = "advanced-components")]
            editor_focus: self.fw.m9.focused,
            #[cfg(feature = "advanced-components")]
            keyboard_visible: self.fw.m9.keyboard_visible,
        }
    }
    fn restore_activity_focus(&mut self, state: SuspendedActivity) {
        self.fw.focused_view = state.focused_view;
        self.fw.ime_connection = state.ime_connection;
        #[cfg(feature = "advanced-components")]
        {
            self.fw.m9.focused = state.editor_focus;
            self.fw.m9.keyboard_visible = state.keyboard_visible;
        }
        self.fw.traversal_pending = true;
    }
    /// Bounded, single-task Activity history. It is strongly rooted for GC.
    pub fn start_activity(
        &mut self,
        desc: &str,
        package: &str,
        extras: Vec<(String, Value)>,
    ) -> Result<ObjRef, VmExit> {
        if self.cp.find_class(desc).is_none() {
            return Err(crate::err::vm_error(
                "MISSING_ENTRY",
                format!("activity {desc} is absent from DEX"),
            )
            .into());
        }
        let depth = self.fw.activity_stack.len();
        if depth >= 64 {
            return Err(crate::err::vm_error("LIMIT_EXCEEDED", "activity stack limit64").into());
        }
        let previous = self
            .fw
            .current_activity
            .filter(|r| {
                matches!(
                    self.fw.get(*r),
                    Some(HostObj::Activity {
                        finished: false,
                        ..
                    })
                )
            })
            .map(|r| self.capture_activity_focus(r));
        if let Some(state) = previous {
            let class = self.heap.class_of(state.activity)?.to_string();
            self.run_lifecycle_step(&class, state.activity, "onPause", "()V", Vec::new())?;
            self.fw.activity_stack.push(state);
        }
        match self.launch_activity_inner(desc, package, extras) {
            Ok(r) => {
                if let Some(state) = previous {
                    if matches!(
                        self.fw.get(state.activity),
                        Some(HostObj::Activity {
                            finished: false,
                            ..
                        })
                    ) {
                        let class = self.heap.class_of(state.activity)?.to_string();
                        self.run_lifecycle_step(
                            &class,
                            state.activity,
                            "onStop",
                            "()V",
                            Vec::new(),
                        )?;
                    }
                }
                Ok(r)
            }
            Err(e) => {
                self.fw.activity_stack.truncate(depth);
                if let Some(state) = previous {
                    if matches!(
                        self.fw.get(state.activity),
                        Some(HostObj::Activity {
                            finished: false,
                            ..
                        })
                    ) {
                        self.fw.current_activity = Some(state.activity);
                        self.restore_activity_focus(state);
                        let class = self.heap.class_of(state.activity)?.to_string();
                        self.run_lifecycle_step(
                            &class,
                            state.activity,
                            "onResume",
                            "()V",
                            Vec::new(),
                        )?;
                    }
                }
                Err(e)
            }
        }
    }
    /// Finish once and restore the prior live Activity (if any).
    pub fn finish_activity(&mut self, activity: ObjRef) -> Result<(), VmExit> {
        match self.fw.objects.get_mut(&activity) {
            Some(HostObj::Activity { finished, .. }) if !*finished => {
                *finished = true;
            }
            _ => return Ok(()),
        }
        let class = self.heap.class_of(activity)?.to_string();
        self.run_lifecycle_step(&class, activity, "onPause", "()V", Vec::new())?;
        self.run_lifecycle_step(&class, activity, "onStop", "()V", Vec::new())?;
        self.run_lifecycle_step(&class, activity, "onDestroy", "()V", Vec::new())?;
        if self.fw.current_activity == Some(activity) {
            while let Some(state) = self.fw.activity_stack.pop() {
                if !matches!(
                    self.fw.get(state.activity),
                    Some(HostObj::Activity {
                        finished: false,
                        ..
                    })
                ) {
                    continue;
                }
                self.fw.current_activity = Some(state.activity);
                self.restore_activity_focus(state);
                let class = self.heap.class_of(state.activity)?.to_string();
                self.run_lifecycle_step(&class, state.activity, "onRestart", "()V", Vec::new())?;
                self.run_lifecycle_step(&class, state.activity, "onStart", "()V", Vec::new())?;
                self.run_lifecycle_step(&class, state.activity, "onResume", "()V", Vec::new())?;
                break;
            }
        }
        Ok(())
    }
    /// An explicit root launch replaces old task history but preserves any
    /// child activities launched by the new root's own onCreate callback.
    pub fn launch_app(
        &mut self,
        desc: &str,
        package: &str,
        extras: Vec<(String, Value)>,
    ) -> Result<ObjRef, VmExit> {
        let old_depth = self.fw.activity_stack.len();
        let result = self.launch_activity_inner(desc, package, extras);
        if result.is_ok() {
            self.fw
                .activity_stack
                .drain(..old_depth.min(self.fw.activity_stack.len()));
        }
        result
    }
    fn launch_activity_inner(
        &mut self,
        activity_desc: &str,
        package_name: &str,
        intent_extras: Vec<(String, Value)>,
    ) -> Result<ObjRef, VmExit> {
        // issue #82: resolver a classe ANTES de alocar. Uma classe ausente do
        // DEX não pode ser lançada (no Android o `am start` falharia com
        // ActivityNotFoundException); antes a VM alocava a instância vazia
        // mesmo assim e devolvia sucesso — o chamador (rd mcp launch) relatava
        // `launched: true` para um estado que nunca existiu.
        if self.cp.find_class(activity_desc).is_none() {
            return Err(crate::err::with_suggestion(
                crate::err::vm_error(
                    "MISSING_ENTRY",
                    format!("classe {activity_desc} não existe no DEX — não há o que lançar"),
                ),
                "confira o nome da activity (package_list/ui_tree) ou lance sem `activity` para usar a LAUNCHER declarada no AndroidManifest.xml",
            )
            .into());
        }
        self.fw.package_name = package_name.to_string();
        // M8 (38º gap real): Android cria a Application do processo ANTES
        // da activity e roda Application.onCreate — o DEX do app depende
        // disso (getApplication() devolve a instância do manifest).
        {
            let app_cls = self
                .fw
                .application_class
                .clone()
                .filter(|c| self.cp.find_class(c).is_some())
                .unwrap_or_else(|| APPLICATION.to_string());
            if self.fw.application.is_none() {
                let ar = self.heap.alloc_instance(app_cls.clone(), Vec::new())?;
                self.attach_host_state(&app_cls, ar)?;
                self.fw.application = Some(ar);
                // <init> da classe, se o DEX declara (super = host no-op).
                if let Some((d, def, m)) = self.cp.resolve_method(&app_cls, "<init>", "()V") {
                    self.call(d, def, &m, vec![Value::Obj(ar)])?;
                }
                // Android: Application.onCreate ANTES do onCreate da
                // activity (só o que o DEX declara; framework já cobre
                // APPLICATION.onCreate como no-op do runtime).
                if let Some((d, def, m)) = self.cp.resolve_method(&app_cls, "onCreate", "()V") {
                    self.call(d, def, &m, vec![Value::Obj(ar)])?;
                }
            }
        }
        self.ensure_initialized(activity_desc)?;
        let r = self
            .heap
            .alloc_instance(activity_desc.to_string(), Vec::new())?;
        self.attach_host_state(activity_desc, r)?;
        // intent de launch
        let ir = self.heap.alloc_instance(INTENT.to_string(), Vec::new())?;
        self.fw.objects.insert(
            ir,
            HostObj::Intent {
                component: Some(activity_desc.to_string()),
                extras: intent_extras,
            },
        );
        if let Some(HostObj::Activity { intent, .. }) = self.fw.objects.get_mut(&r) {
            *intent = Some(ir);
        }
        self.fw.current_activity = Some(r);
        self.fw.focused_view = None;
        self.fw.ime_connection = None;
        #[cfg(feature = "advanced-components")]
        {
            self.fw.m9.focused = None;
            self.fw.m9.keyboard_visible = false;
        }

        // <init>()V (se o DEX declara) — ins=1 (this)
        if let Some((d, def, m)) = self.cp.resolve_method(activity_desc, "<init>", "()V") {
            self.call(d, def, &m, vec![Value::Obj(r)])?;
        }
        // lifecycle (só o que o DEX declara — super default é no-op)
        self.run_lifecycle_step(
            activity_desc,
            r,
            "onCreate",
            "(Landroid/os/Bundle;)V",
            vec![Value::Null],
        )?;
        self.run_lifecycle_step(activity_desc, r, "onStart", "()V", Vec::new())?;
        self.run_lifecycle_step(activity_desc, r, "onResume", "()V", Vec::new())?;
        // handleResumeActivity → makeVisible: ViewRootImpl attaches the
        // window tree (onAttachedToWindow + attach listeners)
        crate::view_attach::attach_activity_window(self, r)?;
        // then the main Looper runs: posted messages, ready threads, frames
        self.drain_looper()?;
        Ok(r)
    }

    fn run_lifecycle_step(
        &mut self,
        activity_desc: &str,
        activity: ObjRef,
        name: &str,
        sig: &str,
        args: Vec<Value>,
    ) -> Result<(), VmExit> {
        use crate::activity_callbacks::{dispatch, event_of, Phase};
        let event = event_of(name);
        let bundle = args.first().cloned();
        if let Some(e) = event {
            dispatch(self, activity, e, Phase::Pre, bundle.clone())?;
        }
        if let Some((d, def, m)) = self.cp.resolve_method(activity_desc, name, sig) {
            let mut slots = vec![Value::Obj(activity)];
            slots.extend(args);
            self.call(d, def, &m, slots)?;
        }
        if let Some(e) = event {
            dispatch(self, activity, e, Phase::Main, bundle.clone())?;
            dispatch(self, activity, e, Phase::Post, bundle)?;
        }
        Ok(())
    }

    /// Toque simulado do agente (DoD M3): hit-test na window da activity
    /// corrente e dispatch do onClick do listener (se houver).
    /// Devolve `true` se algum listener foi acionado.
    pub fn touch_app(&mut self, x: i32, y: i32) -> Result<bool, VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "nenhum app em execução (launch_app primeiro)",
            )
            .into());
        };
        // input lands on a laid-out window (first traversal), as on a device
        let _ = self.ensure_laid_out();
        // the topmost dialog window takes the input (WindowManager z-order)
        if let Some(&(dlg, root)) = crate::m8::showing_dialog_roots(self).last() {
            let inside = matches!(self.fw.get(root),
                Some(HostObj::View { x: rx, y: ry, w, h, .. })
                    if point_in_view_bounds(x, y, (*rx, *ry, *w, *h)));
            if inside {
                if let Some(t) = self.hit_test(root, x, y) {
                    if self.click_listener_of(t).is_some() || self.click_method_of(t).is_some() {
                        return self.dispatch_click(t);
                    }
                }
                return self.touch_sequence(root, &[(x, y)]);
            }
            // outside the dialog window: Dialog.onTouchEvent (DOWN, UP)
            let now = self.fw.clock() as i64;
            let mut consumed = false;
            for action in [
                crate::android_motion::ACTION_DOWN,
                crate::android_motion::ACTION_UP,
            ] {
                let ev = crate::android_motion::new_event(
                    self, action, x as f32, y as f32, 0.0, 0.0, now, now,
                )?;
                let r = self.invoke_instance(
                    dlg,
                    "onTouchEvent",
                    "(Landroid/view/MotionEvent;)Z",
                    &[Value::Obj(ev)],
                )?;
                consumed |= matches!(r, Value::Int(v) if v != 0);
                if !crate::m8::showing_dialog_roots(self)
                    .iter()
                    .any(|(d, _)| *d == dlg)
                {
                    break;
                }
            }
            return Ok(consumed);
        }
        let Some(content) = self.window_content_of(activity) else {
            if std::env::var("RD_FW_DEBUG").is_ok() {
                eprintln!("[fw] touch: sem content (window vazia)");
            }
            return Ok(false); // sem setContentView: nada a tocar
        };
        if std::env::var("RD_FW_DEBUG").is_ok() {
            let mut d = String::from("[fw] tree no touch:\n");
            self.dump_view(content, 0, &mut d);
            eprintln!("{d}");
        }
        let Some(target) = self.hit_test(content, x, y) else {
            if std::env::var("RD_FW_DEBUG").is_ok() {
                eprintln!("[fw] touch: hit-test sem alvo em ({x},{y})");
                let mut d = String::new();
                self.dump_view(content, 0, &mut d);
                eprintln!("{d}");
            }
            return Ok(false);
        };
        if self.click_listener_of(target).is_some() || self.click_method_of(target).is_some() {
            return self.dispatch_click(target);
        }
        // No host-level click handler: deliver a real DOWN/UP MotionEvent
        // pair through the view tree (custom views, Compose, gestures).
        self.touch_sequence(content, &[(x, y)])
    }

    /// Sends DOWN at the first point, MOVE for the middle points and UP at
    /// the last one through `root.dispatchTouchEvent` (16 ms apart, with
    /// the looper drained between events). Returns whether the DOWN was
    /// consumed.
    pub fn touch_sequence(&mut self, root: ObjRef, pts: &[(i32, i32)]) -> Result<bool, VmExit> {
        self.touch_sequence_timed(root, pts, 16)
    }

    /// `touch_sequence` with `step_ms` between consecutive events.
    pub fn touch_sequence_timed(
        &mut self,
        root: ObjRef,
        pts: &[(i32, i32)],
        step_ms: u64,
    ) -> Result<bool, VmExit> {
        let Some(&(x0, y0)) = pts.first() else {
            return Ok(false);
        };
        let (rx, ry) = match self.fw.get(root) {
            Some(HostObj::View { x, y, .. }) => (*x, *y),
            _ => (0, 0),
        };
        let down = self.fw.clock() as i64;
        let n = pts.len().max(2);
        let mut consumed = false;
        for i in 0..n {
            let (px, py) = if i == n - 1 {
                *pts.last().unwrap_or(&(x0, y0))
            } else {
                pts[i.min(pts.len() - 1)]
            };
            let action = if i == 0 {
                crate::android_motion::ACTION_DOWN
            } else if i == n - 1 {
                crate::android_motion::ACTION_UP
            } else {
                crate::android_motion::ACTION_MOVE
            };
            let now = self.fw.clock() as i64;
            let ev = crate::android_motion::new_event(
                self,
                action,
                (px - rx) as f32,
                (py - ry) as f32,
                rx as f32,
                ry as f32,
                down,
                now,
            )?;
            let r = crate::view_layout::dispatch_touch(self, root, ev)?;
            if i == 0 {
                consumed = r;
            }
            if i != n - 1 {
                self.advance_clock(step_ms.max(1))?;
            }
        }
        Ok(consumed)
    }

    /// M3.2: dispatch do clique em `target` — listener do app
    /// (setOnClickListener → onClick(View)) OU android:onClick="método"
    /// (resolve no contexto da ACTIVITY corrente, semântica Android XML).
    /// `false` = view sem nenhum dos dois (nunca silencioso com RD_FW_DEBUG).
    pub(crate) fn dispatch_click(&mut self, target: ObjRef) -> Result<bool, VmExit> {
        if let Some(listener) = self.click_listener_of(target) {
            let lclass = self.heap.class_of(listener)?.to_string();
            // View.OnClickListener.onClick(View) — o DEX do app implementa
            let Some((d, def, m)) =
                self.cp
                    .resolve_method(&lclass, "onClick", "(Landroid/view/View;)V")
            else {
                return Err(crate::classpath::unresolved_method(
                    &lclass,
                    "onClick",
                    "(Landroid/view/View;)V",
                )
                .into());
            };
            self.call(d, def, &m, vec![Value::Obj(listener), Value::Obj(target)])?;
            return Ok(true);
        }
        if let Some(method) = self.click_method_of(target) {
            let Some(activity) = self.fw.current_activity else {
                return Err(crate::err::vm_error(
                    "INVALID_STATE",
                    "android:onClick sem activity corrente",
                )
                .into());
            };
            let aclass = self.heap.class_of(activity)?.to_string();
            let Some((d, def, m)) =
                self.cp
                    .resolve_method(&aclass, &method, "(Landroid/view/View;)V")
            else {
                return Err(crate::classpath::unresolved_method(
                    &aclass,
                    &method,
                    "(Landroid/view/View;)V",
                )
                .into());
            };
            self.call(d, def, &m, vec![Value::Obj(activity), Value::Obj(target)])?;
            return Ok(true);
        }
        if std::env::var("RD_FW_DEBUG").is_ok() {
            let cls = self.heap.class_of(target).unwrap_or("?");
            eprintln!("[fw] touch: alvo #{target} ({cls}) sem listener nem android:onClick");
        }
        Ok(false)
    }

    /// M5 — swipe determinístico do agente: amostra o caminho em passos
    /// fixos (8 inclusive as pontas), hit-testa cada amostra e registra as
    /// views únicas atravessadas em ordem. Android: down e up na MESMA view
    /// viram clique (dispatch do listener); caminhos que cruzam views
    /// distintas são scroll/fling (v1: só relatório honesto, sem efeito).
    /// O clock virtual avança `ms` (duração do gesto) — determinístico.
    pub fn swipe_app(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        ms: u64,
    ) -> Result<SwipeReport, VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "nenhum app em execução (launch_app primeiro)",
            )
            .into());
        };
        let Some(content) = self.window_content_of(activity) else {
            // sem setContentView: nada a atravessar (mesma política do touch)
            return Ok(SwipeReport {
                crossed: Vec::new(),
                clicked: false,
            });
        };
        let _ = self.ensure_laid_out();
        const STEPS: i32 = 8;
        // the topmost window under the start point takes the gesture
        // (WindowManager z-order): a showing dialog, else the activity
        let mut root = content;
        if let Some(&(_, dr)) = crate::m8::showing_dialog_roots(self).last() {
            if matches!(self.fw.get(dr),
                Some(HostObj::View { x, y, w, h, .. })
                    if x1 >= *x && x1 < x + w && y1 >= *y && y1 < y + h)
            {
                root = dr;
            }
        }
        // DEX views (custom views, Compose, RecyclerView, …) handle drags
        // themselves: deliver a real DOWN → MOVE… → UP MotionEvent stream.
        let mut dex = Vec::new();
        self.collect_dex_roots(root, &mut dex, 0);
        // down+up on the same view with a host click handler stays a click
        // (Android tap-vs-scroll semantics of the host widget model)
        let start_t = self.hit_test(root, x1, y1);
        let host_click = start_t.is_some()
            && start_t == self.hit_test(root, x2, y2)
            && start_t.is_some_and(|t| {
                self.click_listener_of(t).is_some() || self.click_method_of(t).is_some()
            });
        if !host_click && (!dex.is_empty() || root != content) {
            let pts: Vec<(i32, i32)> = (0..=STEPS)
                .map(|i| {
                    let t = i as f32 / STEPS as f32;
                    (
                        x1 + ((x2 - x1) as f32 * t).round() as i32,
                        y1 + ((y2 - y1) as f32 * t).round() as i32,
                    )
                })
                .collect();
            let step = (ms / STEPS as u64).max(16);
            let mut crossed: Vec<CrossedView> = Vec::new();
            for &(x, y) in &pts {
                if let Some(t) = self.hit_test(root, x, y) {
                    let entry = CrossedView {
                        class: self.heap.class_of(t)?.to_string(),
                        bounds: self.view_bounds(t),
                    };
                    if crossed.last() != Some(&entry) {
                        crossed.push(entry);
                    }
                }
            }
            self.touch_sequence_timed(root, &pts, step)?;
            return Ok(SwipeReport {
                crossed,
                clicked: false,
            });
        }
        let mut crossed: Vec<CrossedView> = Vec::new();
        let mut first: Option<ObjRef> = None;
        let mut last: Option<ObjRef> = None;
        for i in 0..=STEPS {
            let t = i as f32 / STEPS as f32;
            let x = x1 + ((x2 - x1) as f32 * t).round() as i32;
            let y = y1 + ((y2 - y1) as f32 * t).round() as i32;
            let Some(target) = self.hit_test(content, x, y) else {
                continue;
            };
            if first.is_none() {
                first = Some(target);
            }
            last = Some(target);
            let cls = self.heap.class_of(target)?.to_string();
            let entry = CrossedView {
                class: cls,
                bounds: self.view_bounds(target),
            };
            if crossed.last() != Some(&entry) {
                crossed.push(entry);
            }
        }
        if ms > 0 {
            self.advance_clock(ms)?;
        }
        let mut clicked = false;
        if let (Some(f), Some(l)) = (first, last) {
            if f == l {
                clicked = self.dispatch_click(l)?;
            }
        }
        Ok(SwipeReport { crossed, clicked })
    }

    fn view_bounds(&self, r: ObjRef) -> (i32, i32, i32, i32) {
        match self.fw.get(r) {
            Some(HostObj::View { x, y, w, h, .. }) => (*x, *y, *w, *h),
            _ => (0, 0, 0, 0),
        }
    }

    /// M5 — digitação no "foco": o modelo host ainda não rastreia foco real
    /// (issue #54 — focusable é honesto=false até o modelo rastrear); a
    /// política v1 digita na PRIMEIRA EditText visível+habilitada da árvore
    /// (padrão de telas single-input). `replace=true` substitui; default
    /// (false) anexa — semântica de digitação. Devolve o texto final.
    pub fn type_text(&mut self, text: &str, replace: bool) -> Result<String, VmExit> {
        let Some(activity) = self.fw.current_activity else {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "nenhum app em execução (launch_app primeiro)",
            )
            .into());
        };
        let Some(content) = self.window_content_of(activity) else {
            return Err(crate::err::vm_error(
                "INVALID_STATE",
                "sem window/content (setContentView primeiro)",
            )
            .into());
        };
        let Some(target) = self.find_edit_text(content, 0)? else {
            // Compose / custom editors: the system IME path (focused editor →
            // onCreateInputConnection → commitText), topmost window first
            if let Some(done) = self.type_text_ime(content, text, replace)? {
                return Ok(done);
            }
            return Err(crate::err::with_suggestion(
                crate::err::vm_error(
                    "INVALID_STATE",
                    "nenhuma EditText visível+habilitada na árvore da activity corrente",
                ),
                "o modelo v1 digita em EditText; crie um EditText no layout do app",
            )
            .into());
        };
        #[cfg(feature = "advanced-components")]
        {
            let target = self.fw.m9.focused.unwrap_or(target);
            self.m9_focus(target)?;
            if replace {
                let n = self.view_text(target)?.encode_utf16().count();
                self.m9_set_selection(0, n)?;
            }
            self.m9_commit_text(text, 1, false)
        }
        #[cfg(not(feature = "advanced-components"))]
        let cur = self.view_text(target)?;
        #[cfg(not(feature = "advanced-components"))]
        let final_text = if replace {
            text.to_string()
        } else {
            format!("{cur}{text}")
        };
        #[cfg(not(feature = "advanced-components"))]
        {
            self.set_view_text(target, final_text.clone())?;
            Ok(final_text)
        }
    }

    /// Types through the platform IME contract, the way Android's input
    /// method does for editors that are not host EditTexts (Jetpack Compose
    /// `BasicTextField`, custom editors): the visible view whose class
    /// overrides `onCreateInputConnection` is asked for an `InputConnection`
    /// (it returns one only while one of its editors has focus — tap the
    /// field first), then `commitText` is called on it. `Ok(None)` = no
    /// focused editor anywhere (the caller reports the usual error).
    fn type_text_ime(
        &mut self,
        content: ObjRef,
        text: &str,
        replace: bool,
    ) -> Result<Option<String>, VmExit> {
        const SIG: &str =
            "(Landroid/view/inputmethod/EditorInfo;)Landroid/view/inputmethod/InputConnection;";
        let mut roots: Vec<ObjRef> = crate::m8::showing_dialog_roots(self)
            .into_iter()
            .map(|(_, d)| d)
            .rev()
            .collect();
        roots.push(content);
        let mut editors = Vec::new();
        for r in roots {
            self.collect_ime_views(r, 0, SIG, &mut editors);
        }
        for view in editors {
            let info = self.heap.alloc_instance(
                "Landroid/view/inputmethod/EditorInfo;".into(),
                vec![
                    ("inputType".into(), Value::Int(0)),
                    ("imeOptions".into(), Value::Int(0)),
                    ("initialSelStart".into(), Value::Int(-1)),
                    ("initialSelEnd".into(), Value::Int(-1)),
                    ("initialCapsMode".into(), Value::Int(0)),
                    ("actionId".into(), Value::Int(0)),
                    ("fieldId".into(), Value::Int(0)),
                    ("privateImeOptions".into(), Value::Null),
                    ("hintText".into(), Value::Null),
                    ("label".into(), Value::Null),
                    ("packageName".into(), Value::Null),
                    ("extras".into(), Value::Null),
                    ("contentMimeTypes".into(), Value::Null),
                ],
            )?;
            let ic = match self.invoke_instance(
                view,
                "onCreateInputConnection",
                SIG,
                &[Value::Obj(info)],
            )? {
                Value::Obj(ic) if ic != 0 => ic,
                _ => continue, // this view has no focused editor
            };
            if replace {
                let _ = self.invoke_instance(ic, "beginBatchEdit", "()Z", &[])?;
                let _ = self.invoke_instance(
                    ic,
                    "deleteSurroundingText",
                    "(II)Z",
                    &[Value::Int(i32::MAX / 2), Value::Int(i32::MAX / 2)],
                )?;
                let _ = self.invoke_instance(ic, "endBatchEdit", "()Z", &[])?;
            }
            let s = crate::intrinsics::alloc_string(self, text.to_string())?;
            let _ = self.invoke_instance(
                ic,
                "commitText",
                "(Ljava/lang/CharSequence;I)Z",
                &[Value::Obj(s), Value::Int(1)],
            )?;
            // read back what the editor now holds (before + after cursor)
            let read = |m: &str, e: &mut Engine| -> Result<String, VmExit> {
                match e.invoke_instance(
                    ic,
                    m,
                    "(II)Ljava/lang/CharSequence;",
                    &[Value::Int(10_000), Value::Int(0)],
                )? {
                    Value::Obj(cs) if cs != 0 => crate::intrinsics::object_to_string(e, cs),
                    _ => Ok(String::new()),
                }
            };
            let before = read("getTextBeforeCursor", self)?;
            let after = read("getTextAfterCursor", self)?;
            self.fw.ime_connection = Some((ic, info));
            return Ok(Some(format!("{before}{after}")));
        }
        Ok(None)
    }

    /// Visible views whose runtime class implements `onCreateInputConnection`
    /// in app code (DFS, same order as the dump).
    fn collect_ime_views(&mut self, root: ObjRef, depth: usize, sig: &str, out: &mut Vec<ObjRef>) {
        if depth > MAX_VIEW_DEPTH {
            return;
        }
        let (vis, children) = match self.fw.get(root) {
            Some(HostObj::View { vis, children, .. }) => (*vis, children.clone()),
            _ => return,
        };
        if vis != Vis::Visible {
            return;
        }
        if let Ok(class) = self.heap.class_of(root).map(|c| c.to_string()) {
            if self
                .cp
                .resolve_method(&class, "onCreateInputConnection", sig)
                .is_some()
            {
                out.push(root);
            }
        }
        for c in children {
            self.collect_ime_views(c, depth + 1, sig, out);
        }
    }

    /// Primeira EditText (classe contém "EditText" — subclasses do app
    /// incluídas) visível+habilitada na árvore, DFS (mesma ordem do dump).
    fn find_edit_text(&self, root: ObjRef, depth: usize) -> Result<Option<ObjRef>, VmExit> {
        if depth > MAX_VIEW_DEPTH {
            return Ok(None);
        }
        let (vis, enabled, children) = match self.fw.get(root) {
            Some(HostObj::View {
                vis,
                enabled,
                children,
                ..
            }) => (*vis, *enabled, children.clone()),
            _ => return Ok(None),
        };
        if vis == Vis::Visible && enabled {
            let cls = self.heap.class_of(root)?.to_string();
            if cls.contains("EditText") {
                return Ok(Some(root));
            }
        }
        for c in children {
            if let Some(hit) = self.find_edit_text(c, depth + 1)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    /// M5 — evento de tecla. BACK finaliza a activity corrente (mesma
    /// semântica do host `(ACTIVITY, "finish")`); DEL apaga o último
    /// caractere da EditText (política de foco da type_text); ENTER é
    /// entregue sem efeito (v1 sem IME action). Outras teclas: erro tipado
    /// NOT_IMPLEMENTED listando as suportadas — nunca silêncio.
    pub fn key_event(&mut self, key: &str) -> Result<KeyReport, VmExit> {
        let key = key.to_ascii_uppercase();
        let require_activity = |fw: &HostState| -> Result<ObjRef, VmExit> {
            fw.current_activity.ok_or_else(|| {
                crate::err::vm_error(
                    "INVALID_STATE",
                    "nenhum app em execução (launch_app primeiro)",
                )
                .into()
            })
        };
        match key.as_str() {
            "BACK" => {
                let activity = require_activity(&self.fw)?;
                // the topmost dialog window gets BACK first (Dialog.onBackPressed;
                // ComponentDialog routes it through its OnBackPressedDispatcher)
                if let Some(&(dlg, _)) = crate::m8::showing_dialog_roots(self).last() {
                    self.invoke_instance(dlg, "onBackPressed", "()V", &[])?;
                    self.drain_looper()?;
                    return Ok(KeyReport {
                        key: "BACK".into(),
                        delivered: true,
                        effect: "dialog onBackPressed".into(),
                    });
                }
                // AOSP: Activity.onBackPressed() — virtual, so ComponentActivity's
                // OnBackPressedDispatcher (Navigation back stack, callbacks)
                // runs; the base implementation finishes the activity
                let r = self.invoke_instance(activity, "onBackPressed", "()V", &[]);
                if r.is_err() {
                    if let Some(HostObj::Activity { finished, .. }) =
                        self.fw.objects.get_mut(&activity)
                    {
                        *finished = true;
                    }
                }
                r?;
                self.drain_looper()?;
                let finished = matches!(
                    self.fw.get(activity),
                    Some(HostObj::Activity { finished: true, .. })
                );
                Ok(KeyReport {
                    key: "BACK".into(),
                    delivered: true,
                    effect: if finished {
                        "activity finalizada (finish)".into()
                    } else {
                        "onBackPressed tratado pelo app".into()
                    },
                })
            }
            "DEL" => {
                let activity = require_activity(&self.fw)?;
                let Some(content) = self.window_content_of(activity) else {
                    return Err(crate::err::vm_error(
                        "INVALID_STATE",
                        "sem window/content (setContentView primeiro)",
                    )
                    .into());
                };
                let Some(target) = self.find_edit_text(content, 0)? else {
                    return Err(crate::err::with_suggestion(
                        crate::err::vm_error(
                            "INVALID_STATE",
                            "DEL sem EditText na árvore (política de foco da type_text)",
                        ),
                        "crie um EditText no layout do app",
                    )
                    .into());
                };
                let mut cur = self.view_text(target)?;
                let effect = if cur.pop().is_none() {
                    "texto já vazio".to_string()
                } else {
                    self.set_view_text(target, cur.clone())?;
                    format!("texto agora {cur:?}")
                };
                Ok(KeyReport {
                    key: "DEL".into(),
                    delivered: true,
                    effect,
                })
            }
            "ENTER" => {
                require_activity(&self.fw)?;
                if let Some((ic, info)) = self.fw.ime_connection {
                    // IME action declared by the editor (EditorInfo.imeOptions
                    // & IME_MASK_ACTION); unspecified/none → DONE, like the
                    // stock keyboard's enter key
                    let opts = match self.heap.get_field(info, "imeOptions") {
                        Ok(Value::Int(v)) => v,
                        _ => 0,
                    };
                    let action = match opts & 0xff {
                        0 | 1 => 6,
                        a => a,
                    };
                    let r = self.invoke_instance(
                        ic,
                        "performEditorAction",
                        "(I)Z",
                        &[Value::Int(action)],
                    )?;
                    return Ok(KeyReport {
                        key: "ENTER".into(),
                        delivered: matches!(r, Value::Int(1)),
                        effect: format!(
                            "IME action {action} enviada ao editor focado (performEditorAction)"
                        ),
                    });
                }
                Ok(KeyReport {
                    key: "ENTER".into(),
                    delivered: true,
                    effect: "entregue, sem IME action no modelo v1".into(),
                })
            }
            other => Err(crate::err::with_suggestion(
                crate::err::not_implemented(format!(
                    "tecla {other} fora do modelo v1 (input de teclado)"
                )),
                format!("teclas suportadas: {}", SUPPORTED_KEYS.join(", ")),
            )
            .into()),
        }
    }

    /// M5 — mensagens pendentes na fila do Looper principal (saúde do
    /// runtime na tool `runtime_health`).
    pub fn looper_pending(&self) -> usize {
        let Some(l) = self.fw.main_looper else {
            return 0;
        };
        self.fw
            .objects
            .get(&l)
            .map(|h| match h {
                HostObj::Looper { queue } => queue.len(),
                _ => 0,
            })
            .unwrap_or(0)
    }

    /// Avança o clock virtual e executa as mensagens vencidas (postDelayed).
    /// Devolve quantas runnables rodaram.
    pub fn advance_clock(&mut self, ms: u64) -> Result<usize, VmExit> {
        // While frame callbacks are pending (animations), time advances in
        // 16 ms vsync ticks so each frame sees a later frameTimeNanos — a
        // single jump would hand every frame the same timestamp and freeze
        // animations. Without pending frames it jumps straight to the
        // target, and drain_looper still runs messages in deadline order.
        let target = self.fw.clock_ms.saturating_add(ms);
        let mut ran = 0usize;
        let mut ticks = 0usize;
        loop {
            let now = self.fw.clock_ms;
            let animating = !self.frames.callbacks.is_empty() || self.fw.traversal_pending;
            let step_to = if animating && ticks < MAX_VSYNC_TICKS {
                ticks += 1;
                // next vsync boundary after `now`
                ((now / VSYNC_MS) + 1) * VSYNC_MS
            } else {
                target
            };
            self.fw.clock_ms = step_to.min(target).max(now);
            #[cfg(feature = "advanced-components")]
            self.m9_expire_notifications()?;
            ran += self.drain_looper()?;
            // ViewRootImpl.performTraversals on this vsync: re-measure and
            // re-lay out the window when a view asked for it, so layout-driven
            // work (onGloballyPositioned, anchors of sheets/drawers, lazy
            // lists) progresses without needing a screenshot.
            if self.fw.traversal_pending && !self.fw.in_traversal {
                self.fw.traversal_pending = false;
                if std::env::var("RD_TRACE_DRAW").is_ok() {
                    eprintln!(
                        "D/RustyDroid: traversal at {} ms (frame callbacks {})",
                        self.fw.clock_ms,
                        self.frames.callbacks.len()
                    );
                }
                self.fw.in_traversal = true;
                let r = self.layout_pass();
                self.fw.in_traversal = false;
                if let Err(e) = r {
                    match e {
                        VmExit::Exception(t) => {
                            eprintln!(
                                "W/RustyDroid: traversal failed: {} {:?}",
                                t.class, t.message
                            )
                        }
                        other => return Err(other),
                    }
                }
                ran += self.drain_looper()?;
            }
            if self.fw.clock_ms >= target {
                return Ok(ran);
            }
        }
    }

    /// Executa mensagens com `when_ms <= clock` — SEMPRE a de deadline mais
    /// cedo primeiro (issue #50; tie-break estável: empate → inserção mais
    /// antiga). Antes: primeira vencida por inserção — postDelayed(A,1000)
    /// antes de postDelayed(B,500) rodava A primeiro, divergindo do Android.
    pub fn drain_looper(&mut self) -> Result<usize, VmExit> {
        let looper = self.main_looper_obj()?;
        let now = self.fw.clock_ms;
        let mut ran = 0usize;
        // async results queued by requestPermissions (perms.rs)
        ran += crate::perms::deliver_pending(self)?;
        loop {
            // menor deadline entre os vencidos; idx empata estável
            let due = self.fw.objects.get(&looper).and_then(|h| match h {
                HostObj::Looper { queue } => queue
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.when_ms <= now)
                    .min_by_key(|(i, m)| (m.when_ms, *i))
                    .map(|(i, m)| (i, m.clone())),
                _ => None,
            });
            let Some((idx, msg)) = due else { break };
            if let Some(HostObj::Looper { queue }) = self.fw.objects.get_mut(&looper) {
                queue.remove(idx);
            }
            if std::env::var("RD_FW_DEBUG").is_ok() {
                eprintln!(
                    "[fw] drain: runnable #{} ({}) quando {} <= {}",
                    msg.runnable,
                    self.heap.class_of(msg.runnable).unwrap_or("?"),
                    msg.when_ms,
                    now
                );
            }
            if msg.is_message {
                crate::handler_msg::dispatch_message(self, msg.handler, msg.runnable)?;
                ran += 1;
                continue;
            }
            let rclass = self.heap.class_of(msg.runnable)?.to_string();
            let Some((d, def, m)) = self.cp.resolve_method(&rclass, "run", "()V") else {
                return Err(crate::classpath::unresolved_method(&rclass, "run", "()V").into());
            };
            self.call(d, def, &m, vec![Value::Obj(msg.runnable)])?;
            ran += 1;
        }
        // Main is idle: give ready green threads a turn (threads.rs). What
        // they post to the Looper runs on the next pass; alternate until
        // both are quiescent (fuel bounds pathological ping-pong).
        if crate::threads::run_ready(self, usize::MAX)? > 0 {
            ran += 1 + self.drain_looper()?;
        }
        // Looper idle → vsync: run due Choreographer frames (choreographer.rs),
        // draining what each frame posts. Bounded per drain; nested drains
        // don't pump frames.
        if !self.frame_pump_active {
            self.frame_pump_active = true;
            let mut frames = 0;
            let res = (|| -> Result<(), VmExit> {
                while frames < crate::choreographer::MAX_FRAMES_PER_DRAIN
                    && crate::choreographer::has_due(self)
                {
                    ran += crate::choreographer::run_frame(self)?;
                    ran += self.drain_looper()?;
                    frames += 1;
                }
                Ok(())
            })();
            self.frame_pump_active = false;
            res?;
        }
        Ok(ran)
    }

    /// Dump textual da árvore de views da activity corrente (seed do UI dump
    /// do M4 — formato estável para agentes).
    pub fn dump_ui(&self) -> String {
        let Some(activity) = self.fw.current_activity else {
            return String::new();
        };
        let Some(content) = self.window_content_of(activity) else {
            return String::new();
        };
        let mut out = String::new();
        self.dump_view(content, 0, &mut out);
        out
    }

    pub fn activity_is_finished(&self) -> bool {
        self.fw
            .current_activity
            .and_then(|a| self.fw.get(a))
            .map(|h| matches!(h, HostObj::Activity { finished: true, .. }))
            .unwrap_or(false)
    }

    // ── helpers internos da árvore ────────────────────────────────────────

    pub(crate) fn window_content_of(&self, activity: ObjRef) -> Option<ObjRef> {
        let w = match self.fw.get(activity)? {
            HostObj::Activity {
                window: Some(w), ..
            } => *w,
            _ => return None,
        };
        match self.fw.get(w)? {
            HostObj::Window { content } => *content,
            _ => None,
        }
    }

    fn click_listener_of(&self, v: ObjRef) -> Option<ObjRef> {
        match self.fw.get(v)? {
            HostObj::View { click_listener, .. } => *click_listener,
            _ => None,
        }
    }

    /// android:onClick="metodo" capturado pelo LayoutInflater (M3.2).
    fn click_method_of(&self, v: ObjRef) -> Option<String> {
        match self.fw.get(v)? {
            HostObj::View { click_method, .. } => click_method.clone(),
            _ => None,
        }
    }

    /// Deepest view visível/habilitada contendo o ponto (z-order: último
    /// filho primeiro); devolve o topo com listener? Não — devolve o deepest
    /// hit; o dispatch checa listener nele.
    /// issue #40: cap de profundidade — árvore programática hostil (addView
    /// profundo) não pode estourar a stack no hit-test (abort do processo).
    fn hit_test(&self, root: ObjRef, x: i32, y: i32) -> Option<ObjRef> {
        self.hit_test_depth(root, x, y, 0)
    }

    fn hit_test_depth(&self, root: ObjRef, x: i32, y: i32, depth: usize) -> Option<ObjRef> {
        if depth > MAX_VIEW_DEPTH {
            return None;
        }
        let h = self.fw.get(root)?;
        let HostObj::View {
            x: vx,
            y: vy,
            w,
            h: vh,
            vis,
            enabled,
            children,
            ..
        } = h
        else {
            return None;
        };
        // issue #46: invisible e gone não recebem toque (só visible)
        if *vis != Vis::Visible || !point_in_view_bounds(x, y, (*vx, *vy, *w, *vh)) {
            return None;
        }
        for c in children.iter().rev() {
            if let Some(hit) = self.hit_test_depth(*c, x, y, depth + 1) {
                return Some(hit);
            }
        }
        (*enabled).then_some(root)
    }

    /// Layout recursivo mínimo: largura uniforme (match_parent implícito no
    /// M3); altura = default do leaf (48px p/ TextView/Button) ou soma
    /// (vertical) / máximo (horizontal) dos filhos.
    pub(crate) fn layout(&mut self, v: ObjRef, x: i32, y: i32, w: i32) {
        self.layout_depth(v, x, y, w, 0)
    }

    fn layout_depth(&mut self, v: ObjRef, x: i32, y: i32, slot_w: i32, depth: usize) {
        // issue #40: cap — árvore programática profunda não aborta o processo
        if depth > MAX_VIEW_DEPTH {
            return;
        }
        // Views whose frame the guest set via `View.layout` (custom
        // ViewGroups, Compose's AndroidComposeView and its layers) own their
        // bounds and their subtree: this minimal host pass would otherwise
        // overwrite them with its own wrap height (0 for a childless leaf).
        if self
            .view_props
            .contains_key(&(v, "rd_laid_out".to_string()))
        {
            return;
        }
        let (orientation, children, own_h, own_w_explicit) = match self.fw.get(v) {
            Some(HostObj::View {
                orientation,
                children,
                h,
                w_explicit,
                ..
            }) => (*orientation, children.clone(), *h, *w_explicit),
            _ => return,
        };
        // issue #59: largura EXPLÍCITA vence o slot distribuído pelo pai
        // (match_parent implícito = None → slot). Clamp de defesa: o inflate
        // já valida ≤ WINDOW_W, mas a API host (addView em árvore
        // programática) também pode chegar aqui com valores hostis.
        let w = own_w_explicit.map_or(slot_w, |ew| ew.clamp(0, WINDOW_W));
        let mut acc_y = y;
        let mut acc_x = x;
        let mut max_h = 0i32;
        // issue #60: o divisor do ramo horizontal considera APENAS filhos
        // não-GONE (antes: `w / children.len()` contava GONE e o filho
        // visível recebia metade da largura com slot vazio) e respeita as
        // larguras explícitas — os SEM largura dividem o restante igualmente
        // (simplificação M3: weight/gravity ficaram de FORA do M4 — backlog pós-M5;
        // issue #64: deferral re-apontado, o M4 não os implementou).
        let visible: Vec<ObjRef> = children
            .iter()
            .copied()
            .filter(|c| self.vis_of(*c) != Vis::Gone)
            .collect();
        let default_w = if orientation == 1 {
            w
        } else {
            let explicit: i32 = visible
                .iter()
                .filter_map(|c| self.w_explicit_of(*c))
                .fold(0i32, |acc, ew| acc.saturating_add(ew.clamp(0, WINDOW_W)));
            let free = visible.len()
                - visible
                    .iter()
                    .filter(|c| self.w_explicit_of(**c).is_some())
                    .count();
            if free == 0 {
                0
            } else {
                w.saturating_sub(explicit).max(0) / free as i32
            }
        };
        for c in &visible {
            let child_w = match self.w_explicit_of(*c) {
                Some(ew) => ew.clamp(0, WINDOW_W),
                None => default_w,
            };
            let (cx, cy) = match orientation {
                1 => (x, acc_y),
                _ => (acc_x, y),
            };
            self.layout_depth(*c, cx, cy, child_w, depth + 1);
            let ch = self.h_of(*c);
            match orientation {
                // issue #45: saturating — dimensões hostis não podem wrapar
                // para coordenadas negativas (dump/hit-test corruptos)
                1 => acc_y = acc_y.saturating_add(ch),
                _ => acc_x = acc_x.saturating_add(child_w.max(0)),
            }
            max_h = max_h.max(ch);
        }
        if let Some(HostObj::View {
            x: fx,
            y: fy,
            w: fw,
            h: fh,
            ..
        }) = self.fw.objects.get_mut(&v)
        {
            // largura sempre propagada (leaf/container); altura = default do
            // leaf, soma (vertical) ou max (horizontal, uma linha)
            *fx = x;
            *fy = y;
            *fw = w;
            *fh = if children.is_empty() {
                own_h
            } else if orientation == 1 {
                acc_y.saturating_sub(y)
            } else {
                max_h
            };
        }
    }

    /// issue #59: largura explícita declarada por um view (layout_width px),
    /// se houver.
    fn w_explicit_of(&self, v: ObjRef) -> Option<i32> {
        match self.fw.get(v) {
            Some(HostObj::View { w_explicit, .. }) => *w_explicit,
            _ => None,
        }
    }

    /// issue #46: visibilidade host da view (layout/hit-test/dump).
    fn vis_of(&self, v: ObjRef) -> Vis {
        match self.fw.get(v) {
            Some(HostObj::View { vis, .. }) => *vis,
            _ => Vis::Visible,
        }
    }

    fn h_of(&self, v: ObjRef) -> i32 {
        match self.fw.get(v) {
            Some(HostObj::View { h, .. }) => *h,
            _ => 0,
        }
    }

    fn dump_view(&self, v: ObjRef, depth: usize, out: &mut String) {
        // issue #40: cap — dump de árvore hostil profunda não estoura stack
        if depth > MAX_VIEW_DEPTH {
            out.push_str(&format!(
                "  ...árvore além de {MAX_VIEW_DEPTH} níveis (cortado)\n"
            ));
            return;
        }
        let Some(HostObj::View {
            id,
            x,
            y,
            w,
            h,
            vis,
            enabled,
            text,
            children,
            orientation,
            ..
        }) = self.fw.get(v)
        else {
            return;
        };
        let class = self.heap.class_of(v).unwrap_or("?").to_string();
        let name = class.trim_start_matches('L').trim_end_matches(';');
        let indent = "  ".repeat(depth);
        let vis_tag = match *vis {
            Vis::Visible => "",
            Vis::Invisible => " INVISIBLE", // ocupa espaço, não desenha/clique
            Vis::Gone => " GONE",           // removido do layout (issue #46)
        };
        let en = if *enabled { "" } else { " DISABLED" };
        let txt = if text.is_empty() {
            String::new()
        } else {
            format!(" text={text:?}")
        };
        // M3.2: id nomeado quando o arsc conhece (dump vira mapa do agente)
        let idtxt = if *id == 0 {
            "@id/0".to_string()
        } else {
            self.fw
                .resources
                .as_ref()
                .and_then(|apk| apk.arsc.as_ref())
                .and_then(|a| a.resolve_name(*id as u32))
                .unwrap_or_else(|| format!("@id/{id}"))
        };
        let orient = match (children.is_empty(), *orientation) {
            (true, _) => String::new(),
            (false, 1) => " vertical".to_string(),
            (false, _) => " horizontal".to_string(),
        };
        out.push_str(&format!(
            "{indent}{name} {idtxt} [{x},{y} {w}x{h}]{orient}{txt}{vis_tag}{en}\n"
        ));
        for c in children {
            self.dump_view(*c, depth + 1, out);
        }
    }

    pub(crate) fn find_view_by_id(&self, root: ObjRef, id: i32) -> Option<ObjRef> {
        let (vid, children) = match self.fw.get(root)? {
            HostObj::View {
                id: vid, children, ..
            } => (*vid, children.clone()),
            _ => return None,
        };
        if vid != 0 && vid == id {
            return Some(root);
        }
        for c in children {
            if let Some(hit) = self.find_view_by_id(c, id) {
                return Some(hit);
            }
        }
        None
    }

    // ── writes de estado usados pela dispatch ─────────────────────────────

    pub(crate) fn view_text(&self, r: ObjRef) -> Result<String, VmExit> {
        match self.fw.get(r) {
            Some(HostObj::View { text, .. }) => Ok(text.clone()),
            _ => {
                Err(crate::err::vm_error("INVALID_FORMAT", format!("#{r} não é view host")).into())
            }
        }
    }

    pub(crate) fn set_view_text(&mut self, r: ObjRef, s: String) -> Result<(), VmExit> {
        match self.fw.objects.get_mut(&r) {
            Some(HostObj::View { text, .. }) => {
                *text = s;
                Ok(())
            }
            _ => {
                Err(crate::err::vm_error("INVALID_FORMAT", format!("#{r} não é view host")).into())
            }
        }
    }
}

// ── dispatch de métodos host (chamado do interp.rs) ─────────────────────────

/// invoke-static sobre classes android/* (Looper hoje).
pub fn call_host_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_host_static_rooted(vm, class, name, _sig, args))
}

fn call_host_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (LOOPER, "getMainLooper") | (LOOPER, "myLooper") => {
            Ok(Some(Value::Obj(vm.main_looper_obj()?)))
        }
        (LOOPER, "loop") => {
            vm.drain_looper()?;
            Ok(Some(Value::Null))
        }
        // ── M8: SystemClock (oitavo gap do launch real — org.primftpd) ────
        // O relógio é o VIRTUAL determinístico (`HostState.clock_ms`): mesmo
        // contrato do Looper/postDelayed — nada de tempo de parede (testes
        // reprodutíveis; `sleep` dispara callbacks agendados).
        (SYSTEM_CLOCK, "uptimeMillis")
        | (SYSTEM_CLOCK, "elapsedRealtime")
        | (SYSTEM_CLOCK, "currentThreadTimeMillis") => Ok(Some(Value::Long(vm.fw.clock() as i64))),
        (SYSTEM_CLOCK, "elapsedRealtimeNanos") => {
            Ok(Some(Value::Long(vm.fw.clock() as i64 * 1_000_000)))
        }
        // ── M8 (44º gap real — SharedPreferences padrão) ──
        // getDefaultSharedPreferences: objeto canônico do host (vazio nesta
        // milestone — não persiste em arquivo; o editor vem quando pedido).
        (PREFERENCE_MANAGER, "getDefaultSharedPreferences") => {
            Ok(Some(Value::Obj(vm.shared_prefs_obj()?)))
        }
        (SYSTEM_CLOCK, "sleep") => {
            // sleep(ms): avança o relógio virtual e drena o Looper
            let ms = match args.first() {
                Some(Value::Long(v)) => (*v).max(0) as u64,
                _ => 0,
            };
            vm.advance_clock(ms)?;
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

/// invoke-virtual/direct sobre classes android/*. `recv` é o ObjRef do shell.
/// (name, sig) de `FragmentTransaction` suportados (M8, 31º gap real):
/// mutações fluentes (devolvem a própria transação) + commit.
fn fragment_tx_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        (
            "add",
            "(Landroid/app/Fragment;Ljava/lang/String;)Landroid/app/FragmentTransaction;"
        ) | (
            "add",
            "(ILandroid/app/Fragment;)Landroid/app/FragmentTransaction;"
        ) | (
            "add",
            "(ILandroid/app/Fragment;Ljava/lang/String;)Landroid/app/FragmentTransaction;"
        ) | (
            "replace",
            "(ILandroid/app/Fragment;)Landroid/app/FragmentTransaction;"
        ) | (
            "replace",
            "(ILandroid/app/Fragment;Ljava/lang/String;)Landroid/app/FragmentTransaction;"
        ) | (
            "remove",
            "(Landroid/app/Fragment;)Landroid/app/FragmentTransaction;"
        ) | ("commit", "()I")
            | ("commitAllowingStateLoss", "()I")
    )
}

/// Estágio validado da transação: existe (veio de beginTransaction) e ainda
/// não foi comittada — caso contrário IllegalStateException (contrato AOSP).
fn frag_tx_open<'s>(
    vm: &'s mut Engine,
    tx: ObjRef,
    action: &str,
) -> Result<&'s mut FragTxState, VmExit> {
    match vm.fw.frag_txs.get_mut(&tx) {
        Some(s) if !s.committed => Ok(s),
        Some(_) => Err(VmExit::Exception(crate::err::Throwable::new(
            "Ljava/lang/IllegalStateException;",
            format!("{action} após commit — transação já finalizada"),
        ))),
        None => Err(VmExit::Exception(crate::err::Throwable::new(
            "Ljava/lang/IllegalStateException;",
            format!("{action}: FragmentTransaction não criada por beginTransaction"),
        ))),
    }
}

/// M8 (31º gap real): FragmentTransaction host — ops pendentes até `commit`,
/// que aplica no registro de fragments da VM (validação ANTES de mutar:
/// transação nunca fica parcialmente aplicada). Fragment null → NPE (AOSP
/// dereferecia o campo mFragmentManager); commit duplo → ISE. Ops sem commit
/// não têm efeito observável no FragmentManager (descarte implícito).
fn fragment_tx_intrinsic(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "add" | "replace" => {
            // formatos: (Fragment,String) | (int,Fragment) | (int,Fragment,String)
            let (container_id, frag_idx, tag_idx) = match args.len() {
                2 if matches!(args[0], Value::Obj(_) | Value::Null) => (0, 0, Some(1)),
                2 => (args[0].as_int()?, 1, None),
                _ => (args[0].as_int()?, 1, Some(2)),
            };
            let Value::Obj(frag) = args[frag_idx] else {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    format!("{name} com fragment null (AOSP dereferencia a fragment)"),
                )));
            };
            let tag = match tag_idx {
                Some(i) => match &args[i] {
                    Value::Obj(r) => Some(vm.heap.as_str(*r)?.to_string()),
                    _ => None,
                },
                None => None,
            };
            let rec = FragRecord {
                obj: frag,
                container_id,
                tag,
            };
            let op = if name == "add" {
                FragOp::Add { rec }
            } else {
                FragOp::Replace { rec }
            };
            frag_tx_open(vm, recv, name)?.ops.push(op);
            Ok(Some(Value::Obj(recv)))
        }
        "remove" => {
            let Value::Obj(frag) = args[0] else {
                return Err(VmExit::Exception(crate::err::Throwable::new(
                    "Ljava/lang/NullPointerException;",
                    "remove com fragment null",
                )));
            };
            frag_tx_open(vm, recv, name)?
                .ops
                .push(FragOp::Remove { obj: frag });
            Ok(Some(Value::Obj(recv)))
        }
        "commit" | "commitAllowingStateLoss" => {
            // state loss não se aplica: o host não tem ciclo de estado de
            // Activity/Fragment — as duas variantes aplicam da mesma forma.
            let ops = frag_tx_open(vm, recv, name)?.ops.clone();
            // valida TUDO antes de tocar o registro (sem aplicação parcial):
            // Add de fragment já registrada → ISE ("Fragment already added").
            for op in &ops {
                if let FragOp::Add { rec } = op {
                    if vm.fw.fragments.iter().any(|f| f.obj == rec.obj) {
                        return Err(VmExit::Exception(crate::err::Throwable::new(
                            "Ljava/lang/IllegalStateException;",
                            "Fragment already added ao registro do host",
                        )));
                    }
                }
            }
            {
                let st = frag_tx_open(vm, recv, name)?;
                st.committed = true;
                st.ops.clear();
            }
            for op in ops {
                match op {
                    FragOp::Add { rec } => vm.fw.fragments.push(rec),
                    FragOp::Replace { rec } => {
                        vm.fw
                            .fragments
                            .retain(|f| f.container_id != rec.container_id);
                        vm.fw.fragments.push(rec);
                    }
                    FragOp::Remove { obj } => vm.fw.fragments.retain(|f| f.obj != obj),
                }
            }
            // commit devolve um id opaco de back-stack entry; sem back stack
            // no host, 0 é o marcador documentado (nenhuma semantic real).
            Ok(Some(Value::Int(0)))
        }
        _ => Ok(None),
    }
}

pub fn call_host_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_host_instance_rooted(vm, mref_class, name, sig, recv, args))
}

fn call_host_instance_rooted(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if std::env::var("RD_FW_DEBUG").is_ok() {
        eprintln!("[fw] host dispatch: {mref_class}->{name}{sig} recv=#{recv}");
    }
    // classes do receiver (concreto) e da method ref (declarante)
    let recv_class = vm.heap.class_of(recv)?.to_string();
    let is_activity = |vm: &Engine| {
        vm.cp.is_subtype(&recv_class, ACTIVITY) || vm.cp.is_subtype(&recv_class, APPLICATION)
    };
    let is_view = |vm: &Engine| {
        let r = vm.cp.is_subtype(&recv_class, VIEW);
        if std::env::var("RD_FW_DEBUG").is_ok() && !r {
            eprintln!(
                "[fw] is_subtype({recv_class}, VIEW) = false; sup={:?}",
                vm.cp.superclass_of(&recv_class)
            );
        }
        r
    };

    match (mref_class, name) {
        // ── M8: Looper.getThread — thread "main" sintética (VM single-threaded).
        // Métodos de Ljava/lang/Thread; despacham via intrinsics (não Landroid/).
        (LOOPER, "getThread") => Ok(Some(Value::Obj(vm.main_thread_obj()?))),
        // ── Activity/Context ────────────────────────────────────────────────
        (ACTIVITY, "<init>") | (APPLICATION, "<init>") | (CONTEXT, "<init>") => {
            Ok(Some(Value::Null))
        }
        // ── M8 (32º gap real): Fragment.<init>()V — a fragment do app chama o
        // ctor do super via invokespecial; o host não tem estado de ctor, que
        // vira no-op (mesmo padrão de Activity/Application/Context).
        (FRAGMENT_CLS, "<init>") if sig == "()V" => Ok(Some(Value::Null)),
        (ACTIVITY, "getIntent") | (CONTEXT, "getIntent") => {
            let intent = match vm.fw.get(recv) {
                Some(HostObj::Activity { intent, .. }) => *intent,
                _ => None,
            };
            match intent {
                Some(i) => Ok(Some(Value::Obj(i))),
                None => Err(not_impl("getIntent antes do launch")),
            }
        }
        // ── M8 (29º gap real): Activity.getFragmentManager — FragmentManager
        // canônico do host (identidade estável). O estado de fragments ainda
        // não existe: back stack/transações seguem tipadas quando sondadas.
        // ── M8 (29º gap real): Activity.getFragmentManager — FragmentManager
        // canônico do host (identidade estável). O estado de fragments ainda
        // não existe: back stack/transações seguem tipadas quando sondadas.
        (ACTIVITY, "getFragmentManager")
            if sig == "()Landroid/app/FragmentManager;" && is_activity(vm) =>
        {
            Ok(Some(Value::Obj(vm.fragment_manager_obj()?)))
        }
        // ── SharedPreferences (M8, 44º gap real: LogController.readPrefs) ──
        // Prefs do host COMEÇAM VAZIAS (sem persistência em arquivo nesta
        // milestone): getXxx devolve o default da chamada — comportamento
        // correto de prefs recém-criadas (AOSP devolve o default), não um
        // stub inseguro. Chave null → NPE (AOSP levanta NPE em getStringWithDefault).
        // ── M8 full: SharedPreferences$Editor stateful ──────────────────
        _ if mref_class == "Landroid/content/SharedPreferences$Editor;" => match name {
            "putString" | "putInt" | "putLong" | "putFloat" | "putBoolean" => {
                let key = match args.first() {
                    Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
                    _ => {
                        return Err(VmExit::Exception(crate::err::Throwable::new(
                            "Ljava/lang/NullPointerException;",
                            "Editor.put com key null",
                        )))
                    }
                };
                vm.fw
                    .m8
                    .prefs_editors
                    .entry(recv)
                    .or_default()
                    .push((key, args.get(1).cloned()));
                Ok(Some(Value::Obj(recv)))
            }
            "remove" => {
                let key = match args.first() {
                    Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
                    _ => return Ok(Some(Value::Obj(recv))),
                };
                vm.fw
                    .m8
                    .prefs_editors
                    .entry(recv)
                    .or_default()
                    .push((key, None));
                Ok(Some(Value::Obj(recv)))
            }
            "clear" => {
                vm.fw.m8.prefs_clear.insert(recv, true);
                Ok(Some(Value::Obj(recv)))
            }
            "commit" => {
                apply_prefs_editor(vm, recv);
                Ok(Some(Value::Int(1)))
            }
            "apply" => {
                apply_prefs_editor(vm, recv);
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        _ if recv_class == SHARED_PREFS_IMPL && mref_class == SHARED_PREFS => match (name, sig) {
            ("edit", "()Landroid/content/SharedPreferences$Editor;") => {
                let e = vm.heap.alloc_instance(
                    "Landroid/content/SharedPreferences$Editor;".to_string(),
                    Vec::new(),
                )?;
                Ok(Some(Value::Obj(e)))
            }
            ("getAll", "()Ljava/util/Map;") => Ok(Some(Value::Obj(m8_prefs_get_all(vm)?))),
            ("getString", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;")
            | ("getStringSet", "(Ljava/lang/String;Ljava/util/Set;)Ljava/util/Set;")
            | ("getBoolean", "(Ljava/lang/String;Z)Z")
            | ("getInt", "(Ljava/lang/String;I)I")
            | ("getLong", "(Ljava/lang/String;J)J")
            | ("getFloat", "(Ljava/lang/String;F)F") => {
                if let Some(Value::Obj(kr)) = args.first() {
                    let key = vm.heap.as_str(*kr)?.to_string();
                    Ok(Some(match vm.fw.m8.prefs.get(&key) {
                        Some(v) => v.clone(),
                        None => args.get(1).cloned().unwrap_or(Value::Null),
                    }))
                } else {
                    Err(VmExit::Exception(crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        format!("SharedPreferences.{name} com key null"),
                    )))
                }
            }
            ("contains", "(Ljava/lang/String;)Z") => {
                if let Some(Value::Obj(kr)) = args.first() {
                    let key = vm.heap.as_str(*kr)?.to_string();
                    Ok(Some(Value::Int(vm.fw.m8.prefs.contains_key(&key) as i32)))
                } else {
                    Err(VmExit::Exception(crate::err::Throwable::new(
                        "Ljava/lang/NullPointerException;",
                        "SharedPreferences.contains com key null",
                    )))
                }
            }
            ("edit", _) => Ok(None),
            _ => Ok(None),
        },
        // ── M8 (38º gap real): Activity.getApplication — final na
        // plataforma; devolve a Application canônica da VM (criada no
        // launch com a classe do manifest, ou Landroid/app/Application;
        // por default). Para chamadas cujo mref_class é a própria classe
        // do app (ex.: MainTabsActivity), o braço correspondente vive em
        // intrinsics::call_instance_intrinsic_mref (o gate Landroid/ do
        // interp não captura descritor de classe do app).
        (ACTIVITY, "getApplication") if sig == "()Landroid/app/Application;" && is_activity(vm) => {
            Ok(Some(Value::Obj(vm.application_obj()?)))
        }
        // ── M8 (39º gap real): Context.getApplicationContext — para
        // activities é a mesma Application canônica de getApplication.
        (ACTIVITY, "getApplicationContext") | (CONTEXT, "getApplicationContext")
            if sig == "()Landroid/content/Context;" && is_activity(vm) =>
        {
            Ok(Some(Value::Obj(vm.application_obj()?)))
        }
        // ── M8 (30º/31º gaps reais): consultas ao registro de fragments do
        // host. Sem commit prévio ⇒ null (contrato Android para fragment não
        // encontrada, nunca NOT_IMPLEMENTED); tag null ⇒ null (a
        // FragmentManagerImpl ignora tag null). Se houver mais de uma com o
        // mesmo tag/id, a ÚLTIMA registrada vence (AOSP percorre do fim).
        (FRAGMENT_MANAGER, "findFragmentByTag")
            if sig == "(Ljava/lang/String;)Landroid/app/Fragment;" =>
        {
            let want = match args.first() {
                Some(Value::Obj(r)) => vm.heap.as_str(*r)?.to_string(),
                _ => return Ok(Some(Value::Null)),
            };
            let found = vm
                .fw
                .fragments
                .iter()
                .rev()
                .find(|f| f.tag.as_deref() == Some(want.as_str()));
            Ok(Some(match found {
                Some(f) => Value::Obj(f.obj),
                None => Value::Null,
            }))
        }
        (FRAGMENT_MANAGER, "findFragmentById") if sig == "(I)Landroid/app/Fragment;" => {
            let id = args[0].as_int()?;
            // containerViewId 0 = sem container (AOSP); nunca casa.
            let found = vm
                .fw
                .fragments
                .iter()
                .rev()
                .find(|f| id != 0 && f.container_id == id);
            Ok(Some(match found {
                Some(f) => Value::Obj(f.obj),
                None => Value::Null,
            }))
        }
        // ── M8 (31º gap real): beginTransaction → FragmentTransaction host
        // nova por chamada; as ops ficam pendentes até o commit aplicar no
        // registro. Sem back stack/animations/lifecycle DEX (documentado).
        (FRAGMENT_MANAGER, "beginTransaction") if sig == "()Landroid/app/FragmentTransaction;" => {
            let tx = vm
                .heap
                .alloc_instance(FRAGMENT_TX.to_string(), Vec::new())?;
            vm.fw.frag_txs.insert(tx, FragTxState::default());
            Ok(Some(Value::Obj(tx)))
        }
        (FRAGMENT_TX, _) if fragment_tx_supported(name, sig) => {
            fragment_tx_intrinsic(vm, recv, name, args)
        }
        // ── M8 (33º gap real): executePendingTransactions — no host o commit
        // aplica SINCRONAMENTE, nunca há fila pendente; resposta honesta é
        // false (AOSP: "nothing was executed"). Nunca NOT_IMPLEMENTED.
        (FRAGMENT_MANAGER, "executePendingTransactions") if sig == "()Z" => Ok(Some(Value::Int(0))),
        (CONTEXT, "getSharedPreferences")
            if sig == "(Ljava/lang/String;I)Landroid/content/SharedPreferences;"
                && is_activity(vm) =>
        {
            Ok(Some(Value::Obj(vm.shared_prefs_obj()?)))
        }
        (ACTIVITY, "getPackageName") | (CONTEXT, "getPackageName") if is_activity(vm) => {
            let s = vm.fw.package_name.clone();
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, s)?)))
        }
        (ACTIVITY, "setContentView") | (ACTIVITY, "addContentView")
            if sig == "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V" =>
        {
            // window.rs: params are recorded on the view; the headless window
            // lays the content root out at full size either way
            let Some(Value::Obj(content)) = args.first() else {
                return Err(
                    crate::err::vm_error("INVALID_FORMAT", "setContentView sem view").into(),
                );
            };
            set_content_view(vm, recv, *content)?;
            Ok(Some(Value::Null))
        }
        (ACTIVITY, "setContentView") if sig == "(Landroid/view/View;)V" => {
            let Some(Value::Obj(content)) = args.first() else {
                return Err(
                    crate::err::vm_error("INVALID_FORMAT", "setContentView sem view").into(),
                );
            };
            set_content_view(vm, recv, *content)?;
            Ok(Some(Value::Null))
        }
        (ACTIVITY, "setContentView") if sig == "(I)V" => {
            // M3.2: LayoutInflater de layout XML real — resid → @layout/key →
            // AXML do APK → árvore de views host (erro tipado, nunca silencioso)
            let resid = args[0].as_int()? as u32;
            let root = crate::inflate::inflate_resource(vm, resid)?;
            set_content_view(vm, recv, root)?;
            Ok(Some(Value::Null))
        }
        (ACTIVITY, "getString") | (CONTEXT, "getString") if sig == "(I)Ljava/lang/String;" => {
            let resid = args[0].as_int()? as u32;
            let s = vm.resolve_string(resid)?;
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, s)?)))
        }
        (ACTIVITY, "findViewById") | (CONTEXT, "findViewById") | (VIEW, "findViewById") => {
            // Assinatura canônica (I)Landroid/view/View; — args[0] é o RESID
            // (o receiver já saiu em `recv`). O guard antigo exigia Obj em
            // args[0] e fazia o findViewById devolver Null SEMPRE (bug
            // latente, sem cobertura até o M3.2 exercitar a rota).
            let want = match args.first() {
                Some(Value::Int(v)) => *v,
                Some(Value::Null) | None => return Ok(Some(Value::Null)),
                _ => {
                    return Err(crate::err::vm_error(
                        "INVALID_FORMAT",
                        "findViewById: resid não é int",
                    )
                    .into())
                }
            };
            // A View searches ITS OWN subtree (View.findViewById contract).
            // Before, the receiver was treated as an activity, so
            // view.findViewById always returned null.
            if matches!(vm.fw.get(recv), Some(HostObj::View { .. })) {
                return Ok(Some(
                    vm.find_view_by_id(recv, want)
                        .map_or(Value::Null, Value::Obj),
                ));
            }
            // Activity: the window decor when it exists (android.R.id.content
            // resolves, window.rs), else the content root.
            let decor = match vm.fw.get(recv) {
                Some(HostObj::Activity {
                    window: Some(w), ..
                }) => vm.window_decor.get(w).map(|d| d.0),
                _ => None,
            };
            if let Some(d) = decor {
                return Ok(Some(
                    vm.find_view_by_id(d, want).map_or(Value::Null, Value::Obj),
                ));
            }
            let Some(content) = vm.window_content_of(recv) else {
                return Ok(Some(Value::Null));
            };
            Ok(Some(
                vm.find_view_by_id(content, want)
                    .map_or(Value::Null, Value::Obj),
            ))
        }
        (ACTIVITY, "finish") => {
            vm.finish_activity(recv)?;
            Ok(Some(Value::Null))
        }
        (ACTIVITY, "getWindow") => {
            // Window host criada sob demanda (setContentView(View) também a cria)
            let w = ensure_window(vm, recv)?;
            Ok(Some(Value::Obj(w)))
        }
        (ACTIVITY, "startActivity") | (CONTEXT, "startActivity") => {
            let Some(Value::Obj(ir)) = args.first() else {
                return Err(
                    crate::err::vm_error("INVALID_FORMAT", "startActivity sem intent").into(),
                );
            };
            let component = match vm.fw.get(*ir) {
                Some(HostObj::Intent { component, .. }) => component.clone(),
                _ => None,
            };
            let Some(target) = component else {
                return Err(crate::err::not_implemented(
                    "startActivity com intent implícito/action — M3 só navega por componente",
                )
                .into());
            };
            let extras = match vm.fw.get(*ir) {
                Some(HostObj::Intent { extras, .. }) => extras.clone(),
                _ => Vec::new(),
            };
            let pkg = vm.fw.package_name.clone();
            let _new = vm.start_activity(&target, &pkg, extras)?;
            Ok(Some(Value::Null))
        }
        // lifecycle via super (user chama super.onCreate etc.) → no-op host
        (ACTIVITY, "onCreate")
        | (ACTIVITY, "onStart")
        | (ACTIVITY, "onResume")
        | (ACTIVITY, "onPause")
        | (ACTIVITY, "onStop")
        | (ACTIVITY, "onDestroy")
        | (ACTIVITY, "onRestart")
        | (APPLICATION, "onCreate") => Ok(Some(Value::Null)),

        // ── View ────────────────────────────────────────────────────────────
        (VIEW, "<init>")
        | (VIEWGROUP, "<init>")
        | (LINEARLAYOUT, "<init>")
        | (FRAMELAYOUT, "<init>")
            if is_view(vm) =>
        {
            Ok(Some(Value::Null))
        }
        (VIEW, "setId") if is_view(vm) => {
            // issue #54: setId aceita qualquer i32 (semântica Java real —
            // findViewById precisa achar o que o app setou). O que NÃO acontece
            // mais é o dump fabricar resource-id para id sem recurso: o
            // build_ui_node só nomeia ids que RESOLVEM no arsc (namespace
            // honesto — id cru tipo 123 sai com resource-id vazio).
            let v = args[0].as_int()?;
            if let Some(HostObj::View { id, .. }) = vm.fw.objects.get_mut(&recv) {
                *id = v;
            }
            Ok(Some(Value::Null))
        }
        (VIEW, "getId") if is_view(vm) => {
            let id = match vm.fw.get(recv) {
                Some(HostObj::View { id, .. }) => *id,
                _ => 0,
            };
            Ok(Some(Value::Int(id)))
        }
        (VIEW, "setEnabled") if is_view(vm) => {
            let v = args[0].as_int()? != 0;
            if let Some(HostObj::View { enabled, .. }) = vm.fw.objects.get_mut(&recv) {
                *enabled = v;
            }
            Ok(Some(Value::Null))
        }
        (VIEW, "isEnabled") if is_view(vm) => {
            let en = match vm.fw.get(recv) {
                Some(HostObj::View { enabled, .. }) => *enabled,
                _ => false,
            };
            Ok(Some(Value::Int(en as i32)))
        }
        (VIEW, "setVisibility") if is_view(vm) => {
            let v = args[0].as_int()?;
            if let Some(HostObj::View { vis, .. }) = vm.fw.objects.get_mut(&recv) {
                *vis = Vis::from_java(v); // accepts Android 0/4/8 and legacy 0/1/2
            }
            Ok(Some(Value::Null))
        }
        (VIEW, "getVisibility") if is_view(vm) => {
            let vis = match vm.fw.get(recv) {
                Some(HostObj::View { vis, .. }) => *vis,
                _ => Vis::Visible,
            };
            Ok(Some(Value::Int(vis.to_java())))
        }
        (VIEW, "setOnClickListener") if is_view(vm) => {
            let listener = match args.first() {
                Some(Value::Obj(l)) => Some(*l),
                Some(Value::Null) | None => None,
                _ => {
                    return Err(crate::err::vm_error(
                        "INVALID_FORMAT",
                        "setOnClickListener: argumento não é objeto",
                    )
                    .into())
                }
            };
            if let Some(HostObj::View { click_listener, .. }) = vm.fw.objects.get_mut(&recv) {
                *click_listener = listener;
            }
            Ok(Some(Value::Null))
        }
        (VIEW, "performClick") if is_view(vm) => {
            let hit = vm.dispatch_click(recv)?;
            Ok(Some(Value::Int(hit as i32)))
        }
        (VIEW, "getParent") if is_view(vm) => {
            let p = match vm.fw.get(recv) {
                Some(HostObj::View { parent, .. }) => *parent,
                _ => None,
            };
            Ok(p.map(Value::Obj))
        }
        (VIEW, "getWidth") if is_view(vm) => {
            let w = match vm.fw.get(recv) {
                Some(HostObj::View { w, .. }) => *w,
                _ => 0,
            };
            Ok(Some(Value::Int(w)))
        }
        (VIEW, "getHeight") if is_view(vm) => {
            let h = match vm.fw.get(recv) {
                Some(HostObj::View { h, .. }) => *h,
                _ => 0,
            };
            Ok(Some(Value::Int(h)))
        }

        // ── TextView/Button ─────────────────────────────────────────────────
        (TEXTVIEW, "<init>") | (BUTTON, "<init>") if is_view(vm) => Ok(Some(Value::Null)),
        (TEXTVIEW, "setText") | (BUTTON, "setText") if is_view(vm) => {
            let s = match args.first() {
                Some(Value::Obj(sr)) => vm.heap.as_str(*sr)?.to_string(),
                // M3.2: setText(int resId) resolve no resources.arsc
                Some(Value::Int(resid)) => vm.resolve_string(*resid as u32)?,
                _ => {
                    return Err(crate::err::vm_error(
                        "INVALID_FORMAT",
                        "setText: argumento não é String nem resid",
                    )
                    .into())
                }
            };
            vm.set_view_text(recv, s)?;
            Ok(Some(Value::Null))
        }
        (TEXTVIEW, "getText") | (BUTTON, "getText") if is_view(vm) => {
            let s = vm.view_text(recv)?;
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, s)?)))
        }

        // ── ViewGroup/LinearLayout/FrameLayout ─────────────────────────────
        (VIEWGROUP, "addView") | (LINEARLAYOUT, "addView") | (FRAMELAYOUT, "addView") => {
            let Some(Value::Obj(child)) = args.first() else {
                return Err(crate::err::vm_error("INVALID_FORMAT", "addView sem view").into());
            };
            add_child(vm, recv, *child)?;
            Ok(Some(Value::Null))
        }
        (VIEWGROUP, "getChildCount")
        | (LINEARLAYOUT, "getChildCount")
        | (FRAMELAYOUT, "getChildCount") => {
            let n = match vm.fw.get(recv) {
                Some(HostObj::View { children, .. }) => children.len() as i32,
                _ => 0,
            };
            Ok(Some(Value::Int(n)))
        }
        (VIEWGROUP, "getChildAt") | (LINEARLAYOUT, "getChildAt") | (FRAMELAYOUT, "getChildAt") => {
            // Android: out-of-range index → null. `Ok(None)` would mean
            // "not handled" and surfaced as NOT_FOUND.
            let i = args[0].as_int()?;
            let child = match vm.fw.get(recv) {
                Some(HostObj::View { children, .. }) if i >= 0 => children.get(i as usize).copied(),
                _ => None,
            };
            Ok(Some(child.map_or(Value::Null, Value::Obj)))
        }
        (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "indexOfChild") => {
            let c = args.first().and_then(|v| v.as_ref().ok().flatten());
            let idx = match (vm.fw.get(recv), c) {
                (Some(HostObj::View { children, .. }), Some(c)) => children
                    .iter()
                    .position(|x| *x == c)
                    .map_or(-1, |p| p as i32),
                _ => -1,
            };
            Ok(Some(Value::Int(idx)))
        }
        (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeView")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeViewInLayout")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeViews")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeViewsInLayout")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeViewAt")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeAllViews")
        | (VIEWGROUP | LINEARLAYOUT | FRAMELAYOUT, "removeAllViewsInLayout") => {
            let removed: Vec<ObjRef> = match vm.fw.objects.get_mut(&recv) {
                Some(HostObj::View { children, .. }) => match name {
                    "removeViews" | "removeViewsInLayout" => {
                        let st = args
                            .first()
                            .and_then(|v| v.as_int().ok())
                            .unwrap_or(0)
                            .max(0) as usize;
                        let n = args
                            .get(1)
                            .and_then(|v| v.as_int().ok())
                            .unwrap_or(0)
                            .max(0) as usize;
                        let st = st.min(children.len());
                        let en = (st + n).min(children.len());
                        children.drain(st..en).collect()
                    }
                    "removeView" | "removeViewInLayout" => {
                        let c = args.first().and_then(|v| v.as_ref().ok().flatten());
                        let before = children.clone();
                        children.retain(|x| Some(*x) != c);
                        before.into_iter().filter(|x| Some(*x) == c).collect()
                    }
                    "removeViewAt" => {
                        let i = args.first().and_then(|v| v.as_int().ok()).unwrap_or(-1);
                        if i >= 0 && (i as usize) < children.len() {
                            vec![children.remove(i as usize)]
                        } else {
                            Vec::new()
                        }
                    }
                    _ => std::mem::take(children),
                },
                _ => Vec::new(),
            };
            for c in removed {
                if let Some(HostObj::View { parent, .. }) = vm.fw.objects.get_mut(&c) {
                    *parent = None;
                }
                crate::view_attach::dispatch_detach(vm, c)?;
            }
            Ok(Some(Value::Null))
        }
        (LINEARLAYOUT, "setOrientation") => {
            let v = args[0].as_int()?;
            if let Some(HostObj::View { orientation, .. }) = vm.fw.objects.get_mut(&recv) {
                *orientation = v;
            }
            Ok(Some(Value::Null))
        }

        // ── Handler ────────────────────────────────────────────────────────
        (HANDLER, "<init>") => {
            let looper = match args.first() {
                Some(Value::Obj(l)) => *l,
                _ => {
                    // Handler() usa o looper da thread corrente = main no M3
                    vm.main_looper_obj()?
                }
            };
            if let Some(HostObj::Handler { looper: l }) = vm.fw.objects.get_mut(&recv) {
                *l = looper;
            }
            Ok(Some(Value::Null))
        }
        (HANDLER, "post") => {
            let Some(Value::Obj(r)) = args.first() else {
                return Err(crate::err::vm_error("INVALID_FORMAT", "post sem runnable").into());
            };
            enqueue(vm, recv, *r, vm.fw.clock_ms)?;
            Ok(Some(Value::Int(1)))
        }
        (HANDLER, "postDelayed") => {
            let Some(Value::Obj(r)) = args.first() else {
                return Err(
                    crate::err::vm_error("INVALID_FORMAT", "postDelayed sem runnable").into(),
                );
            };
            let delay = args.get(1).map(|v| v.as_long()).transpose()?.unwrap_or(0);
            // issue #50: clock + i64::MAX estoura u64 (panic em debug; em
            // release envolve → mensagem "vencia" imediatamente). Clamp em
            // u64::MAX/2: nunca vence dentro de qualquer horizonte do runtime.
            let when = vm
                .fw
                .clock_ms
                .checked_add(delay.max(0) as u64)
                .unwrap_or(u64::MAX / 2);
            enqueue(vm, recv, *r, when)?;
            Ok(Some(Value::Int(1)))
        }

        // ── Intent ─────────────────────────────────────────────────────────
        (INTENT, "<init>") => {
            // <init>(Context, Class) → component do field "name" do Class host
            if let Some(Value::Obj(cls)) = args.get(1) {
                let desc = class_name_of(vm, *cls)?;
                if let Some(HostObj::Intent { component, .. }) = vm.fw.objects.get_mut(&recv) {
                    *component = Some(desc);
                }
            }
            Ok(Some(Value::Null))
        }
        (INTENT, "setClass") => {
            let Some(Value::Obj(cls)) = args.get(1) else {
                return Err(crate::err::vm_error("INVALID_FORMAT", "setClass sem Class").into());
            };
            let desc = class_name_of(vm, *cls)?;
            if let Some(HostObj::Intent { component, .. }) = vm.fw.objects.get_mut(&recv) {
                *component = Some(desc);
            }
            Ok(Some(Value::Null))
        }
        (INTENT, "putExtra") if sig.ends_with("Ljava/lang/String;Ljava/lang/String;)V") => {
            put_extra(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (INTENT, "putExtra") if sig.ends_with("Ljava/lang/String;I)V") => {
            put_extra(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (INTENT, "putExtra") if sig.ends_with("Ljava/lang/String;J)V") => {
            put_extra(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (INTENT, "putExtra") if sig.ends_with("Ljava/lang/String;Z)V") => {
            put_extra(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (INTENT, "getStringExtra") => get_extra_string(vm, recv, args),
        (INTENT, "getIntExtra") => {
            let d = args.get(1).map(|v| v.as_int()).transpose()?.unwrap_or(0);
            Ok(Some(match get_extra(vm, recv, args)? {
                Some(Value::Int(v)) => Value::Int(v),
                _ => Value::Int(d),
            }))
        }
        (INTENT, "getLongExtra") => {
            let d = args.get(1).map(|v| v.as_long()).transpose()?.unwrap_or(0);
            Ok(Some(match get_extra(vm, recv, args)? {
                Some(Value::Long(v)) => Value::Long(v),
                _ => Value::Long(d),
            }))
        }
        (INTENT, "getBooleanExtra") => {
            let d = args.get(1).map(|v| v.as_int()).transpose()?.unwrap_or(0);
            Ok(Some(match get_extra(vm, recv, args)? {
                Some(Value::Int(v)) => Value::Int(v),
                _ => Value::Int(d),
            }))
        }

        // ── Bundle ─────────────────────────────────────────────────────────
        (BUNDLE, "<init>") => Ok(Some(Value::Null)),
        (BUNDLE, "putString") => {
            bundle_put(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (BUNDLE, "putInt") => {
            bundle_put(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (BUNDLE, "putLong") => {
            bundle_put(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (BUNDLE, "putBoolean") => {
            bundle_put(vm, recv, args)?;
            Ok(Some(Value::Null))
        }
        (BUNDLE, "getString") => Ok(match bundle_get(vm, recv, args)? {
            Some(v @ Value::Obj(_)) => Some(v),
            _ => None,
        }),
        (BUNDLE, "getInt") => {
            let d = args.get(1).map(|v| v.as_int()).transpose()?.unwrap_or(0);
            Ok(Some(match bundle_get(vm, recv, args)? {
                Some(Value::Int(v)) => Value::Int(v),
                _ => Value::Int(d),
            }))
        }
        (BUNDLE, "getLong") => {
            let d = args.get(1).map(|v| v.as_long()).transpose()?.unwrap_or(0);
            Ok(Some(match bundle_get(vm, recv, args)? {
                Some(Value::Long(v)) => Value::Long(v),
                _ => Value::Long(d),
            }))
        }
        (BUNDLE, "getBoolean") => {
            let d = args.get(1).map(|v| v.as_int()).transpose()?.unwrap_or(0);
            Ok(Some(match bundle_get(vm, recv, args)? {
                Some(Value::Int(v)) => Value::Int(v),
                _ => Value::Int(d),
            }))
        }

        // ── Class (const-class host) ───────────────────────────────────────
        (CLASS, "getName") => {
            let s = class_name_of(vm, recv)?;
            // Class.getName usa pontos: Lcom/A; → com.a
            let dotted = s
                .trim_start_matches('L')
                .trim_end_matches(';')
                .replace('/', ".");
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, dotted)?)))
        }

        _ => Ok(None),
    }
}

// ── helpers da dispatch ──────────────────────────────────────────────────────

/// M8 full: aplica um Editor de SharedPreferences (commit/apply síncronos).
fn apply_prefs_editor(vm: &mut Engine, editor: ObjRef) {
    if vm.fw.m8.prefs_clear.remove(&editor).unwrap_or(false) {
        vm.fw.m8.prefs.clear();
    }
    if let Some(edits) = vm.fw.m8.prefs_editors.remove(&editor) {
        for (k, v) in edits {
            match v {
                Some(val) => {
                    vm.fw.m8.prefs.insert(k, val);
                }
                None => {
                    vm.fw.m8.prefs.remove(&k);
                }
            }
        }
    }
}

/// M8 full: `getAll()` — HashMap host com chaves String reais alocadas.
fn m8_prefs_get_all(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Ljava/util/HashMap;".to_string(), Vec::new())?;
    let mut entries = Vec::new();
    let keys: Vec<(String, Value)> = vm
        .fw
        .m8
        .prefs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in keys {
        let ks = intrinsics::alloc_string(vm, k)?;
        entries.push((Value::Obj(ks), v));
    }
    vm.fw.objects.insert(r, HostObj::Map { entries });
    Ok(r)
}

fn not_impl(what: &str) -> VmExit {
    crate::err::not_implemented(what).into()
}

fn class_name_of(vm: &Engine, cls: ObjRef) -> Result<String, VmExit> {
    let v = vm
        .heap
        .get_field(cls, "name")
        .map_err(|e| VmExit::Error(crate::err::vm_error("INVALID_FORMAT", e)))?;
    match v {
        Value::Obj(r) => Ok(vm.heap.as_str(r)?.to_string()),
        _ => Err(crate::err::vm_error("INVALID_FORMAT", "Class host sem name").into()),
    }
}

pub(crate) fn ensure_window(vm: &mut Engine, activity: ObjRef) -> Result<ObjRef, VmExit> {
    if let Some(HostObj::Activity {
        window: Some(w), ..
    }) = vm.fw.get(activity)
    {
        return Ok(*w);
    }
    let w = vm
        .heap
        .alloc_instance("Landroid/view/Window;".to_string(), Vec::new())?;
    vm.fw.objects.insert(w, HostObj::Window { content: None });
    if let Some(HostObj::Activity { window, .. }) = vm.fw.objects.get_mut(&activity) {
        *window = Some(w);
    }
    Ok(w)
}

/// M3.2 (pub(crate)): também usado pelo LayoutInflater (setContentView(I)).
pub(crate) fn set_content_view(
    vm: &mut Engine,
    activity: ObjRef,
    content: ObjRef,
) -> Result<(), VmExit> {
    let w = ensure_window(vm, activity)?;
    if let Some(HostObj::Window { content: c }) = vm.fw.objects.get_mut(&w) {
        *c = Some(content);
    }
    crate::window::on_content_set(vm, w, content)?;
    vm.fw.traversal_pending = true;
    // layout do root com as dimensões da window
    vm.layout(content, 0, 0, WINDOW_W);
    let _ = WINDOW_H;
    Ok(())
}

/// M3.2 (pub(crate)): também usado pelo LayoutInflater ao montar a árvore.
/// anexa child ao parent SEM re-layout — usado pela INFLAÇÃO (issue #45: o
/// setContentView leita a raiz UMA vez no fim; re-layout por filho era
/// O(N²) — Σ layout(prefixo) ≈ N²/2 walks no setContentView de app grande).
/// M8: wrapper público (crate) de `attach_child` — RecyclerView anexa os
/// itemViews dos ViewHolders como filhos host.
pub fn attach_child_public(vm: &mut Engine, parent: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    attach_child(vm, parent, child)
}

pub(crate) fn attach_child(vm: &mut Engine, parent: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    if let Some(HostObj::View { parent: p, .. }) = vm.fw.objects.get_mut(&child) {
        *p = Some(parent);
    }
    match vm.fw.objects.get_mut(&parent) {
        Some(HostObj::View { children, .. }) => children.push(child),
        _ => return Err(crate::err::vm_error("INVALID_FORMAT", "addView em não-ViewGroup").into()),
    }
    // inserted under an attached parent → attached now (ViewGroup.addViewInner)
    crate::view_attach::on_child_added(vm, parent, child)
}

pub(crate) fn add_child(vm: &mut Engine, parent: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    attach_child(vm, parent, child)?;
    // re-layout do container na sua posição corrente — addView PROGRAMÁTICO
    // (pós-setContentView): O(N) por chamada, não cumulativo (issue #45)
    let (px, py, pw) = match vm.fw.get(parent) {
        Some(HostObj::View { x, y, w, .. }) => (*x, *y, if *w == 0 { WINDOW_W } else { *w }),
        _ => (0, 0, WINDOW_W),
    };
    vm.layout(parent, px, py, pw);
    Ok(())
}

fn enqueue(vm: &mut Engine, handler: ObjRef, runnable: ObjRef, when_ms: u64) -> Result<(), VmExit> {
    let looper = match vm.fw.get(handler) {
        Some(HostObj::Handler { looper }) => *looper,
        _ => vm.main_looper_obj()?,
    };
    match vm.fw.objects.get_mut(&looper) {
        Some(HostObj::Looper { queue }) => {
            queue.push(QueuedMsg {
                when_ms,
                runnable,
                handler: Some(handler),
                is_message: false,
                token: None,
            });
            Ok(())
        }
        _ => Err(crate::err::vm_error("INVALID_FORMAT", "looper host ausente").into()),
    }
}

fn put_extra(vm: &mut Engine, recv: ObjRef, args: &[Value]) -> Result<(), VmExit> {
    let key = string_arg_of(vm, args.first())?;
    let val = args
        .get(1)
        .cloned()
        .ok_or_else(|| crate::err::vm_error("INVALID_FORMAT", "putExtra sem valor"))?;
    if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&recv) {
        extras.retain(|(k, _)| *k != key);
        extras.push((key, val));
    }
    Ok(())
}

fn get_extra(vm: &Engine, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let key = string_arg_of(vm, args.first())?;
    Ok(match vm.fw.get(recv) {
        Some(HostObj::Intent { extras, .. }) => extras
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone()),
        _ => None,
    })
}

fn get_extra_string(
    vm: &mut Engine,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match get_extra(vm, recv, args)? {
        Some(Value::Obj(r)) => {
            let s = vm.heap.as_str(r)?.to_string();
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, s)?)))
        }
        _ => Ok(Some(Value::Null)),
    }
}

fn bundle_put(vm: &mut Engine, recv: ObjRef, args: &[Value]) -> Result<(), VmExit> {
    let key = string_arg_of(vm, args.first())?;
    let val = args
        .get(1)
        .cloned()
        .ok_or_else(|| crate::err::vm_error("INVALID_FORMAT", "put sem valor"))?;
    if let Some(HostObj::Bundle { map }) = vm.fw.objects.get_mut(&recv) {
        map.retain(|(k, _)| *k != key);
        map.push((key, val));
    }
    Ok(())
}

fn bundle_get(vm: &Engine, recv: ObjRef, args: &[Value]) -> Result<Option<Value>, VmExit> {
    let key = string_arg_of(vm, args.first())?;
    Ok(match vm.fw.get(recv) {
        Some(HostObj::Bundle { map }) => {
            map.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
        }
        _ => None,
    })
}

fn string_arg_of(vm: &Engine, v: Option<&Value>) -> Result<String, VmExit> {
    match v {
        Some(Value::Obj(r)) => Ok(vm.heap.as_str(*r)?.to_string()),
        _ => Err(VmExit::Exception(Throwable::new(
            "Ljava/lang/NullPointerException;",
            "chave String é null",
        ))),
    }
}

/// M4: nó da árvore de UI ESTRUTURADA — fonte de verdade para o uiautomator
/// dump e o screenshot do rd-render (campo a campo, não string soup).
#[derive(Debug, Clone)]
pub struct UiNode {
    /// Classe Java ("android.widget.TextView").
    pub class: String,
    /// resource-id no formato uiautomator ("com.pkg:id/tv") — issue #54:
    /// SÓ ids que resolvem no arsc (nunca "com.pkg:id/123" fabricado);
    /// package id 0x01 sai como "android:id/…" (nunca sequestrado pelo
    /// package do app).
    pub resource_id: Option<String>,
    pub text: String,
    /// (x, y, w, h) em px do viewport.
    pub bounds: (i32, i32, i32, i32),
    pub visibility: Vis,
    pub enabled: bool,
    pub clickable: bool,
    /// issue #54: propriedade PRÓPRIA (antes era fudge de `clickable` — no
    /// Android real são estados distintos: TextView clicável não é focável,
    /// EditText é focável sem ser clicável). O modelo host ainda não rastreia
    /// foco — honesto = false (documentado no README do formato).
    pub focusable: bool,
    /// issue #62: subárvore cortada pelo cap de profundidade — o dump emite
    /// marcador para o agente NÃO ler a árvore como completa.
    pub truncated: bool,
    pub children: Vec<UiNode>,
}

impl Engine {
    /// Read-only hit metadata (no child subtree) from the same foreground window as touch_app.
    /// A point outside a modal dialog has no target: probing never dismisses it.
    pub fn view_at_point(&self, x: i32, y: i32) -> Option<UiNode> {
        let activity = self.fw.current_activity?;
        let root = if let Some((_, root)) = crate::m8::showing_dialog_roots(self).last() {
            *root
        } else {
            self.window_content_of(activity)?
        };
        self.hit_test(root, x, y)
            .map(|r| self.build_ui_node_inner(r, 0, false))
    }

    /// M4: legacy structured activity tree, with dialog roots appended.
    /// `None` means there is no current activity/content window.
    pub fn view_tree(&self) -> Option<UiNode> {
        let activity = self.fw.current_activity?;
        let content = self.window_content_of(activity)?;
        let mut root = self.build_ui_node(content, 0);
        // showing dialog windows are part of the visible hierarchy (topmost
        // last, as uiautomator lists windows)
        for (_, d) in crate::m8::showing_dialog_roots(self) {
            root.children.push(self.build_ui_node(d, 1));
        }
        Some(root)
    }

    /// A bounded legacy-shaped snapshot. Dialogs are appended to the content
    /// root for compatibility; use view_roots_bounded for independent windows.
    pub fn view_tree_bounded(&self, max_nodes: usize) -> Option<UiNode> {
        let mut roots = self.view_roots_bounded(max_nodes)?;
        let mut root = roots.remove(0);
        root.children.extend(roots);
        Some(root)
    }

    /// Snapshot independent activity/dialog windows with a shared node budget.
    /// This bounds construction, not just result traversal. Window visibility
    /// must not inherit from the unrelated activity content root.
    pub fn view_roots_bounded(&self, max_nodes: usize) -> Option<Vec<UiNode>> {
        let activity = self.fw.current_activity?;
        let mut remaining = max_nodes.max(1);
        let mut roots = Vec::new();
        if let Some(content) = self.window_content_of(activity) {
            roots.push(self.build_ui_node_bounded(content, 0, &mut remaining));
        }
        for (_, dialog) in crate::m8::showing_dialog_roots(self) {
            if remaining == 0 {
                roots[0].truncated = true;
                break;
            }
            roots.push(self.build_ui_node_bounded(dialog, 1, &mut remaining));
        }
        if roots.is_empty() {
            None
        } else {
            Some(roots)
        }
    }

    fn build_ui_node_bounded(&self, v: ObjRef, depth: usize, remaining: &mut usize) -> UiNode {
        let mut node = self.build_ui_node_inner(v, depth, false);
        *remaining -= 1;
        if node.truncated {
            return node;
        }
        if let Some(HostObj::View { children, .. }) = self.fw.get(v) {
            for child in children {
                if *remaining == 0 {
                    node.truncated = true;
                    break;
                }
                node.children
                    .push(self.build_ui_node_bounded(*child, depth + 1, remaining));
            }
        }
        node
    }

    /// issue #54: resource-id HONESTO para o dump uiautomator:
    /// - id sem nome no arsc → `None` (o uiautomator real também devolve
    ///   vazio: viewIdResourceName só existe para ids de RECURSO — nunca
    ///   fabricar "com.pkg:id/123" a partir de View.setId cru);
    /// - package id 0x01 → prefixo "android" (ids de framework NUNCA ganham
    ///   o package do app — um arsc hostil com package id 0x01 não sequestra
    ///   android:id/…);
    /// - demais: nome do package do arsc que DECLARA o id (fallback: package
    ///   do app).
    fn resource_ref_name(&self, resid: u32) -> Option<String> {
        let pid = (resid >> 24) & 0xFF;
        let apk = self.fw.resources.as_ref()?;
        let arsc = apk.arsc.as_ref()?;
        let name = arsc.resolve_name(resid)?; // "@type/key"
        let name = name.trim_start_matches('@');
        let prefix = if pid == 0x01 {
            "android".to_string()
        } else {
            arsc.packages
                .iter()
                .find(|p| (p.id as u32) == pid)
                .map(|p| p.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| self.fw.package_name.clone())
        };
        Some(format!("{prefix}:{name}"))
    }

    pub(crate) fn build_ui_node(&self, v: ObjRef, depth: usize) -> UiNode {
        self.build_ui_node_inner(v, depth, true)
    }

    fn build_ui_node_inner(&self, v: ObjRef, depth: usize, include_children: bool) -> UiNode {
        // issue #40/#62: mesmo cap do dump textual (árvore programática
        // hostil) — e o corte é SINALIZADO (truncated) para o dump XML marcar
        // (antes: o agente via a subárvore cortada como se fosse completa).
        let truncated = depth > MAX_VIEW_DEPTH;
        let children_view: Vec<ObjRef> = if truncated || !include_children {
            Vec::new()
        } else {
            match self.fw.get(v) {
                Some(HostObj::View { children, .. }) => children.clone(),
                _ => Vec::new(),
            }
        };
        let (class, resource_id, text, bounds, visibility, enabled, clickable) =
            match self.fw.get(v) {
                Some(HostObj::View {
                    id,
                    x,
                    y,
                    w,
                    h,
                    vis,
                    enabled,
                    text,
                    click_listener,
                    click_method,
                    ..
                }) => {
                    let class = self
                        .heap
                        .class_of(v)
                        .unwrap_or("Landroid/view/View;")
                        .trim_start_matches('L')
                        .trim_end_matches(';')
                        .replace('/', ".");
                    // issue #54: só nomeia ids que resolvem no arsc (namespace
                    // honesto — sem "id/123" fabricado, sem sequestro de android:id)
                    let resource_id = if *id == 0 {
                        None
                    } else {
                        self.resource_ref_name(*id as u32)
                    };
                    (
                        class,
                        resource_id,
                        text.clone(),
                        (*x, *y, *w, *h),
                        *vis,
                        *enabled,
                        click_listener.is_some() || click_method.is_some(),
                    )
                }
                _ => (
                    "android.view.View".to_string(),
                    None,
                    String::new(),
                    (0, 0, 0, 0),
                    Vis::Visible,
                    true,
                    false,
                ),
            };
        let children = children_view
            .iter()
            .map(|c| self.build_ui_node(*c, depth + 1))
            .collect();
        let focusable = cfg!(feature = "advanced-components") && class.ends_with("EditText");
        UiNode {
            class,
            resource_id,
            text,
            bounds,
            visibility,
            enabled,
            clickable,
            focusable,
            truncated,
            children,
        }
    }
}

#[cfg(test)]
mod point_hit_tests {
    use super::*;
    #[test]
    fn bounded_windows_include_dialogs_without_activity_content() {
        let mut vm = Engine::new(vec![], crate::VmConfig::default());
        let activity = vm
            .heap
            .alloc_instance("Landroid/app/Activity;".into(), vec![])
            .unwrap();
        let dialog = vm
            .heap
            .alloc_instance("Landroid/app/Dialog;".into(), vec![])
            .unwrap();
        let root = vm
            .heap
            .alloc_instance("Landroid/view/View;".into(), vec![])
            .unwrap();
        vm.fw.current_activity = Some(activity);
        vm.fw.objects.insert(
            activity,
            HostObj::Activity {
                window: None,
                finished: false,
                intent: None,
            },
        );
        let mut view = new_view();
        if let HostObj::View { w, h, .. } = &mut view {
            *w = 100;
            *h = 100;
        }
        vm.fw.objects.insert(root, view);
        vm.fw.m8.dialogs.insert(
            dialog,
            crate::m8::DialogState {
                showing: true,
                view: Some(root),
                ..Default::default()
            },
        );
        assert!(
            vm.view_tree().is_none(),
            "legacy API is deliberately unchanged"
        );
        let roots = vm.view_roots_bounded(1).unwrap();
        assert_eq!(roots.len(), 1);
        assert!(!roots[0].truncated);
        assert_eq!(roots[0].bounds, (0, 0, 100, 100));
        assert!(vm.view_at_point(50, 50).is_some());
        vm.fw.m8.dialogs.get_mut(&dialog).unwrap().showing = false;
        assert!(vm.view_roots_bounded(1).is_none());
    }

    #[test]
    fn android_visibility_constants_extend_without_changing_legacy_ordinals() {
        assert_eq!(Vis::from_java(4), Vis::Invisible);
        assert_eq!(Vis::from_java(8), Vis::Gone);
        for ordinal in 0..=2 {
            assert_eq!(Vis::from_java(ordinal).to_java(), ordinal);
        }
        assert_eq!(Vis::from_java(3), Vis::Visible);
    }

    #[test]
    fn bounded_snapshots_preserve_small_trees_and_mark_wide_cuts() {
        let mut vm = Engine::new(vec![], crate::VmConfig::default());
        let mut allocate = |class: &str| vm.heap.alloc_instance(class.into(), vec![]).unwrap();
        let activity = allocate("Landroid/app/Activity;");
        let window = allocate("Landroid/view/Window;");
        let root = allocate("Landroid/view/View;");
        let child = allocate("Landroid/view/View;");
        vm.fw.objects.insert(
            activity,
            HostObj::Activity {
                window: Some(window),
                finished: false,
                intent: None,
            },
        );
        vm.fw.objects.insert(
            window,
            HostObj::Window {
                content: Some(root),
            },
        );
        let mut state = new_view();
        if let HostObj::View { w, h, children, .. } = &mut state {
            *w = 100;
            *h = 100;
            *children = vec![child; 20];
        }
        vm.fw.objects.insert(root, state);
        let mut state = new_view();
        if let HostObj::View { w, h, .. } = &mut state {
            *w = 50;
            *h = 50;
        }
        vm.fw.objects.insert(child, state);
        vm.fw.current_activity = Some(activity);
        let full = vm.view_tree().unwrap();
        let same = vm.view_tree_bounded(21).unwrap();
        assert_eq!(format!("{same:?}"), format!("{full:?}"));
        let cut = vm.view_tree_bounded(3).unwrap();
        assert_eq!(cut.children.len(), 2);
        assert!(cut.truncated);
        let cut = vm.view_tree_bounded(0).unwrap();
        assert!(cut.children.is_empty());
        assert!(cut.truncated);
        let metadata = vm.view_at_point(75, 75).unwrap();
        assert!(metadata.children.is_empty());
        assert!(!metadata.truncated);
        vm.fw.current_activity = None;
        assert!(vm.view_at_point(75, 75).is_none());
    }

    #[test]
    fn view_bounds_match_wide_integer_oracle_at_all_i32_edges() {
        let values = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];
        for x in values {
            for y in values {
                for left in values {
                    for top in values {
                        for width in values {
                            for height in values {
                                let expected = width > 0
                                    && height > 0
                                    && i128::from(x) >= i128::from(left)
                                    && i128::from(y) >= i128::from(top)
                                    && i128::from(x) < i128::from(left) + i128::from(width)
                                    && i128::from(y) < i128::from(top) + i128::from(height);
                                assert_eq!(
                                    point_in_view_bounds(x, y, (left, top, width, height)),
                                    expected,
                                    "point=({x},{y}) rect=({left},{top},{width},{height})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn actual_hit_test_uses_safe_bounds_and_respects_z_order() {
        let mut vm = Engine::new(vec![], crate::VmConfig::default());
        let root = vm
            .heap
            .alloc_instance("Landroid/view/View;".into(), vec![])
            .unwrap();
        let front = vm
            .heap
            .alloc_instance("Landroid/view/View;".into(), vec![])
            .unwrap();
        let mut back_state = new_view();
        if let HostObj::View {
            x,
            y,
            w,
            h,
            children,
            ..
        } = &mut back_state
        {
            *x = i32::MIN;
            *y = i32::MIN;
            *w = i32::MAX;
            *h = i32::MAX;
            children.push(front);
        }
        vm.fw.objects.insert(root, back_state);
        let mut front_state = new_view();
        if let HostObj::View { x, y, w, h, .. } = &mut front_state {
            *x = i32::MIN;
            *y = i32::MIN;
            *w = i32::MAX;
            *h = i32::MAX;
        }
        vm.fw.objects.insert(front, front_state);
        assert_eq!(vm.hit_test(root, -1, -1), None); // right/bottom edges are exclusive
        assert_eq!(vm.hit_test(root, -2, -2), Some(front));
        if let Some(HostObj::View { vis, .. }) = vm.fw.objects.get_mut(&front) {
            *vis = Vis::Gone;
        }
        assert_eq!(vm.hit_test(root, -2, -2), Some(root));
        if let Some(HostObj::View { x, y, w, h, .. }) = vm.fw.objects.get_mut(&root) {
            *x = i32::MAX - 10;
            *y = i32::MAX - 10;
            *w = 100;
            *h = 100;
        }
        assert_eq!(vm.hit_test(root, i32::MAX, i32::MAX), Some(root));
    }
}
