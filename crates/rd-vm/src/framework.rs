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
            1 => Vis::Invisible,
            2 => Vis::Gone,
            _ => Vis::Visible, // 0 e valores desconhecidos → visible
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
    /// valor, senão identidade). VM de arena não tem GC ⇒ WeakHashMap se
    /// comporta como HashMap (entradas nunca são limpas) — limitação
    /// documentada em compat_gaps.
    Map {
        entries: Vec<(Value, Value)>,
    },
    List {
        elems: Vec<Value>,
    },
    /// M8: iterador host — snapshot de elementos + cursor (determinístico;
    /// `remove` não suportado, como em views imutáveis).
    Iter {
        elems: Vec<Value>,
        pos: usize,
    },
}

#[derive(Debug, Clone)]
pub struct QueuedMsg {
    pub when_ms: u64,
    /// Runnable do app — o drain invoca run() via resolução DEX
    pub runnable: ObjRef,
}

/// Estado de framework da Engine (host objects + clock virtual + launch).
#[derive(Debug, Default)]
pub struct HostState {
    pub objects: HashMap<ObjRef, HostObj>,
    /// clock virtual (ms) — postDelayed/dispatch por ordem de tempo
    pub clock_ms: u64,
    main_looper: Option<ObjRef>,
    /// M8: thread "main" sintética (VM single-threaded — ver `THREAD`)
    main_thread: Option<ObjRef>,
    /// M8: objetos `Class` canônicos por descritor (getClass devolve sempre a
    /// mesma referência para a mesma classe — identidade como na JVM).
    class_objs: HashMap<String, ObjRef>,
    /// M8: ClassLoader canônico (um por VM — single-classloader; a distinção
    /// bootstrap=null do Java não se aplica ao runtime host).
    class_loader: Option<ObjRef>,
    /// M8: FragmentManager canônico do host (um por VM — identidade estável
    /// como a do ClassLoader; 29º gap do launch real).
    fragment_manager: Option<ObjRef>,
    /// activity em execução (o app trivial M3 tem uma por vez)
    pub current_activity: Option<ObjRef>,
    /// package do app (preenchido no launch_app; getPackageName)
    pub package_name: String,
    /// M3.2: APK aberto (Resources) — layouts AXML + resources.arsc para o
    /// LayoutInflater. Injetado via `Engine::set_resources` (CLI `rd app run`).
    pub resources: Option<rd_apk::Apk>,
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

    fn main_looper_obj(&mut self) -> Result<ObjRef, VmExit> {
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

    // ── driver do app trivial (DoD M3) ───────────────────────────────────

    /// Cria a activity, roda o lifecycle e devolve a ref. `package_name`
    /// alimenta getPackageName; extras vão no intent de launch.
    pub fn launch_app(
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
        if let Some((d, def, m)) = self.cp.resolve_method(activity_desc, name, sig) {
            let mut slots = vec![Value::Obj(activity)];
            slots.extend(args);
            self.call(d, def, &m, slots)?;
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
        self.dispatch_click(target)
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
        const STEPS: i32 = 8;
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
            return Err(crate::err::with_suggestion(
                crate::err::vm_error(
                    "INVALID_STATE",
                    "nenhuma EditText visível+habilitada na árvore da activity corrente",
                ),
                "o modelo v1 digita em EditText; crie um EditText no layout do app",
            )
            .into());
        };
        let cur = self.view_text(target)?;
        let final_text = if replace {
            text.to_string()
        } else {
            format!("{cur}{text}")
        };
        self.set_view_text(target, final_text.clone())?;
        Ok(final_text)
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
                if let Some(HostObj::Activity { finished, .. }) = self.fw.objects.get_mut(&activity)
                {
                    *finished = true;
                }
                Ok(KeyReport {
                    key: "BACK".into(),
                    delivered: true,
                    effect: "activity finalizada (finish)".into(),
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
        self.fw.clock_ms += ms;
        self.drain_looper()
    }

    /// Executa mensagens com `when_ms <= clock` — SEMPRE a de deadline mais
    /// cedo primeiro (issue #50; tie-break estável: empate → inserção mais
    /// antiga). Antes: primeira vencida por inserção — postDelayed(A,1000)
    /// antes de postDelayed(B,500) rodava A primeiro, divergindo do Android.
    pub fn drain_looper(&mut self) -> Result<usize, VmExit> {
        let looper = self.main_looper_obj()?;
        let now = self.fw.clock_ms;
        let mut ran = 0usize;
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
            let rclass = self.heap.class_of(msg.runnable)?.to_string();
            let Some((d, def, m)) = self.cp.resolve_method(&rclass, "run", "()V") else {
                return Err(crate::classpath::unresolved_method(&rclass, "run", "()V").into());
            };
            self.call(d, def, &m, vec![Value::Obj(msg.runnable)])?;
            ran += 1;
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

    fn window_content_of(&self, activity: ObjRef) -> Option<ObjRef> {
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
        if *vis != Vis::Visible || x < *vx || y < *vy || x >= vx + w || y >= vy + vh {
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
    fn layout(&mut self, v: ObjRef, x: i32, y: i32, w: i32) {
        self.layout_depth(v, x, y, w, 0)
    }

    fn layout_depth(&mut self, v: ObjRef, x: i32, y: i32, slot_w: i32, depth: usize) {
        // issue #40: cap — árvore programática profunda não aborta o processo
        if depth > MAX_VIEW_DEPTH {
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

    fn find_view_by_id(&self, root: ObjRef, id: i32) -> Option<ObjRef> {
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

    fn view_text(&self, r: ObjRef) -> Result<String, VmExit> {
        match self.fw.get(r) {
            Some(HostObj::View { text, .. }) => Ok(text.clone()),
            _ => {
                Err(crate::err::vm_error("INVALID_FORMAT", format!("#{r} não é view host")).into())
            }
        }
    }

    fn set_view_text(&mut self, r: ObjRef, s: String) -> Result<(), VmExit> {
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
pub fn call_host_instance(
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
        // ── M8 (30º gap real): FragmentManager ainda VAZIO (nenhuma transação
        // host existe) ⇒ findFragmentByTag/findFragmentById respondem null —
        // contrato Android para fragment não encontrado, nunca NOT_IMPLEMENTED.
        // Tag null também devolve null (FragmentManagerImpl ignora tag null).
        (FRAGMENT_MANAGER, "findFragmentByTag")
            if sig == "(Ljava/lang/String;)Landroid/app/Fragment;" =>
        {
            Ok(Some(Value::Null))
        }
        (FRAGMENT_MANAGER, "findFragmentById") if sig == "(I)Landroid/app/Fragment;" => {
            Ok(Some(Value::Null))
        }
        (ACTIVITY, "getPackageName") | (CONTEXT, "getPackageName") if is_activity(vm) => {
            let s = vm.fw.package_name.clone();
            Ok(Some(Value::Obj(intrinsics::alloc_string(vm, s)?)))
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
            let Some(content) = vm.window_content_of(recv) else {
                return Ok(Some(Value::Null));
            };
            Ok(vm.find_view_by_id(content, want).map(Value::Obj))
        }
        (ACTIVITY, "finish") => {
            if let Some(HostObj::Activity { finished, .. }) = vm.fw.objects.get_mut(&recv) {
                *finished = true;
            }
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
            // nova activity: pausa a corrente, cria a nova com o intent
            if let Some(cur) = vm.fw.current_activity {
                let cur_desc = vm.heap.class_of(cur)?.to_string(); // Stringown p/ borrow
                vm.run_lifecycle_step(&cur_desc, cur, "onPause", "()V", Vec::new())?;
            }
            let extras = match vm.fw.get(*ir) {
                Some(HostObj::Intent { extras, .. }) => extras.clone(),
                _ => Vec::new(),
            };
            let pkg = vm.fw.package_name.clone();
            let _new = vm.launch_app(&target, &pkg, extras)?;
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
                *vis = Vis::from_java(v); // 0/1/2 → visible/invisible/gone
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
        (VIEWGROUP, "getChildCount") | (LINEARLAYOUT, "getChildCount") => {
            let n = match vm.fw.get(recv) {
                Some(HostObj::View { children, .. }) => children.len() as i32,
                _ => 0,
            };
            Ok(Some(Value::Int(n)))
        }
        (VIEWGROUP, "getChildAt") | (LINEARLAYOUT, "getChildAt") => {
            let i = args[0].as_int()? as usize;
            let child = match vm.fw.get(recv) {
                Some(HostObj::View { children, .. }) => children.get(i).copied(),
                _ => None,
            };
            Ok(child.map(Value::Obj))
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
    // layout do root com as dimensões da window
    vm.layout(content, 0, 0, WINDOW_W);
    let _ = WINDOW_H;
    Ok(())
}

/// M3.2 (pub(crate)): também usado pelo LayoutInflater ao montar a árvore.
/// anexa child ao parent SEM re-layout — usado pela INFLAÇÃO (issue #45: o
/// setContentView leita a raiz UMA vez no fim; re-layout por filho era
/// O(N²) — Σ layout(prefixo) ≈ N²/2 walks no setContentView de app grande).
pub(crate) fn attach_child(vm: &mut Engine, parent: ObjRef, child: ObjRef) -> Result<(), VmExit> {
    if let Some(HostObj::View { parent: p, .. }) = vm.fw.objects.get_mut(&child) {
        *p = Some(parent);
    }
    match vm.fw.objects.get_mut(&parent) {
        Some(HostObj::View { children, .. }) => children.push(child),
        _ => return Err(crate::err::vm_error("INVALID_FORMAT", "addView em não-ViewGroup").into()),
    }
    Ok(())
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
            queue.push(QueuedMsg { when_ms, runnable });
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
    /// M4 (DoD "get_ui_tree com ids certos"): árvore de UI da activity
    /// corrente, estruturada — o rd-render formata (uiautomator XML) e pinta
    /// (screenshot) a partir daqui. `None` = sem activity/window.
    pub fn view_tree(&self) -> Option<UiNode> {
        let activity = self.fw.current_activity?;
        let content = self.window_content_of(activity)?;
        Some(self.build_ui_node(content, 0))
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

    fn build_ui_node(&self, v: ObjRef, depth: usize) -> UiNode {
        // issue #40/#62: mesmo cap do dump textual (árvore programática
        // hostil) — e o corte é SINALIZADO (truncated) para o dump XML marcar
        // (antes: o agente via a subárvore cortada como se fosse completa).
        let truncated = depth > MAX_VIEW_DEPTH;
        let children_view: Vec<ObjRef> = if truncated {
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
        UiNode {
            class,
            resource_id,
            text,
            bounds,
            visibility,
            enabled,
            clickable,
            focusable: false, // issue #54: honesto até o modelo rastrear foco
            truncated,
            children,
        }
    }
}
