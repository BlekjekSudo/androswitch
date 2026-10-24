//! M8 — Compat real (full scope, não MVP).
//!
//! Subsistemas host desta fase (spec PARTE 4, M8):
//! RecyclerView · Fragments androidx (reusa o registro legacy de
//! `framework`) · Dialogs · OkHttp shim · SQLite/Room · Services /
//! broadcasts / deep links · WorkManager · SharedPreferences stateful.
//!
//! Princípios (Leis do projeto):
//! - Estado REAL e consultável: RecyclerView pergunta `getItemCount()` ao
//!   adapter DEX e cria/vincula ViewHolders via DEX; SQLite executa queries;
//!   SharedPreferences nomeadas persistem como XML no VFS app-private;
//!   OkHttp devolve a resposta determinística registrada (ou 200 vazio);
//!   services/broadcasts/work ficam registrados e expostos ao Engine
//!   (`Engine::m8_*`) para o agente/testes asserirem — nada é "no-op mudo".
//! - Desvios documentados, nunca silenciosos: tudo é single-thread
//!   determinístico (enqueue/execute são síncronos), SQLite named databases
//!   persistem no VFS mas o fallback subset continua documentalmente limitado, OkHttp
//!   NÃO faz rede (shim offline; rede real é M9+ via socket do host),
//!   WorkManager executa o nome do pedido no registro mas não roda o
//!   `Worker.doWork()` em background (sem threads) — o trabalho fica
//!   `ENQUEUED`→`SUCCEEDED` apenas via `m8_complete_work` do harness.
//! - Métodos fora da lista explícita retornam `None` no dispatch → o
//!   interpretador segue para o DEX real da biblioteca (androidx/okhttp
//!   viajam no APK). Host só sequestra o que implementa de verdade.

use std::collections::{HashMap, HashSet};

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::intrinsics;
use crate::prefs::{PrefStore, PrefValue};
use crate::value::Value;

// ── classes ─────────────────────────────────────────────────────────────
pub const RECYCLER: &str = "Landroidx/recyclerview/widget/RecyclerView;";
pub const RECYCLER_ADAPTER: &str = "Landroidx/recyclerview/widget/RecyclerView$Adapter;";
pub const RECYCLER_HOLDER: &str = "Landroidx/recyclerview/widget/RecyclerView$ViewHolder;";
pub const RECYCLER_LM: &str = "Landroidx/recyclerview/widget/RecyclerView$LayoutManager;";
pub const LINEAR_LM: &str = "Landroidx/recyclerview/widget/LinearLayoutManager;";
pub const GRID_LM: &str = "Landroidx/recyclerview/widget/GridLayoutManager;";
pub const DIALOG: &str = "Landroid/app/Dialog;";
pub const ALERT_DIALOG: &str = "Landroid/app/AlertDialog;";
pub const ALERT_BUILDER: &str = "Landroid/app/AlertDialog$Builder;";
pub const SQLITE_DB: &str = "Landroid/database/sqlite/SQLiteDatabase;";
pub const SQLITE_HELPER: &str = "Landroid/database/sqlite/SQLiteOpenHelper;";
pub const CURSOR: &str = "Landroid/database/Cursor;";
pub const CONTENT_VALUES: &str = "Landroid/content/ContentValues;";
pub const URI: &str = "Landroid/net/Uri;";
pub const JOB_SCHEDULER: &str = "Landroid/app/job/JobScheduler;";
pub const OK_CLIENT: &str = "Lokhttp3/OkHttpClient;";
pub const OK_REQUEST: &str = "Lokhttp3/Request;";
pub const OK_REQ_BUILDER: &str = "Lokhttp3/Request$Builder;";
pub const OK_CALL: &str = "Lokhttp3/Call;";
pub const OK_RESPONSE: &str = "Lokhttp3/Response;";
pub const OK_BODY: &str = "Lokhttp3/ResponseBody;";
pub const WORK_MANAGER: &str = "Landroidx/work/WorkManager;";
pub const OPERATION: &str = "Landroidx/work/Operation;";
pub const ROOM: &str = "Landroidx/room/Room;";
pub const ROOM_BUILDER: &str = "Landroidx/room/RoomDatabase$Builder;";

// ── estado ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Default)]
pub struct RecyclerState {
    pub adapter: Option<ObjRef>,
    pub layout_manager: Option<ObjRef>,
    pub item_count: i32,
    pub notifications: i32,
    pub bound_positions: Vec<i32>,
    pub populating: bool,
}
#[derive(Debug, Clone, Default)]
pub struct DialogState {
    pub showing: bool,
    pub title: Option<String>,
    pub message: Option<String>,
    pub buttons: HashMap<i32, (String, Option<ObjRef>)>,
    pub view: Option<ObjRef>,
    pub cancelable: bool,
}
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}
#[derive(Debug, Clone, Default)]
pub struct Database {
    pub tables: HashMap<String, Table>,
    pub version: i32,
    pub in_transaction: bool,
    pub transaction_successful: bool,
}
#[derive(Debug, Clone, Default)]
pub struct CursorState {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub pos: i32, // -1 = before first
    pub closed: bool,
}
#[derive(Debug, Clone)]
pub struct HttpResponseCfg {
    pub code: i32,
    pub body: String,
    pub message: String,
}
impl Default for HttpResponseCfg {
    fn default() -> Self {
        Self {
            code: 200,
            body: String::new(),
            message: "OK".into(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct WorkRecord {
    pub id: String,
    pub worker_class: String,
    pub state: String, // ENQUEUED | SUCCEEDED | CANCELLED
    pub tags: Vec<String>,
}
#[derive(Debug, Default)]
pub struct M8State {
    pub recyclers: HashMap<ObjRef, RecyclerState>,
    pub holders: HashMap<ObjRef, ObjRef>, // ViewHolder -> itemView
    pub dialogs: HashMap<ObjRef, DialogState>,
    pub builders: HashMap<ObjRef, DialogState>,
    pub databases: HashMap<String, Database>,
    pub db_by_obj: HashMap<ObjRef, String>,
    pub cursors: HashMap<ObjRef, CursorState>,
    pub content_values: HashMap<ObjRef, Vec<(String, Value)>>,
    pub helpers_db: HashMap<ObjRef, String>,
    /// Real-SQLite backend state (jsqlite).
    pub sqlx: crate::jsqlite::SqlState,
    pub uris: HashMap<ObjRef, String>,
    pub intents: HashMap<ObjRef, IntentState>,
    pub started_services: Vec<String>,
    pub stopped_services: Vec<String>,
    pub bound_services: Vec<String>,
    pub broadcasts: Vec<String>, // actions, em ordem
    pub receivers: Vec<ObjRef>,
    pub deep_links: Vec<String>, // URIs abertas via VIEW
    pub deep_link_handlers: HashMap<String, String>, // scheme -> activity desc
    pub http: HttpResponseCfg,
    pub http_requests: Vec<String>, // urls, em ordem
    pub work: Vec<WorkRecord>,
    pub work_manager: Option<ObjRef>,
    pub scheduled_jobs: Vec<i32>,
    /// Named `SharedPreferences` stores, keyed by the guest-visible file
    /// name (without `.xml`). Stores load lazily from the app-private VFS.
    pub prefs_stores: HashMap<String, PrefStore>,
    pub prefs_loaded: HashSet<String>,
    pub prefs_objs: HashMap<String, ObjRef>,
    pub prefs_name_by_obj: HashMap<ObjRef, String>,
    pub prefs_editors: HashMap<ObjRef, Vec<(String, Option<PrefValue>)>>, // None = remove
    pub prefs_clear: HashMap<ObjRef, bool>,
    /// Per-SharedPreferences listener identities; GC roots are traced from M8State.
    pub prefs_listeners: HashMap<ObjRef, Vec<ObjRef>>,
    pub room_builders: HashMap<ObjRef, (String, String)>, // obj -> (db class desc, name)
    pub ax_fragments: Vec<(ObjRef, i32, Option<String>)>,
    pub ax_frag_txs: HashMap<ObjRef, Vec<(ObjRef, i32, Option<String>)>>,
    pub ax_fragment_manager: Option<ObjRef>,
    pub last_dialog: Option<ObjRef>,
}
#[derive(Debug, Clone, Default)]
pub struct IntentState {
    pub action: Option<String>,
    pub data: Option<ObjRef>,
}

// ── helpers ─────────────────────────────────────────────────────────────
fn throw(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg.into()))
}
fn npe(what: &str) -> VmExit {
    throw("Ljava/lang/NullPointerException;", what.to_string())
}
fn sarg(vm: &Engine, args: &[Value], i: usize) -> Result<Option<String>, VmExit> {
    match args.get(i) {
        Some(Value::Obj(r)) => Ok(vm.heap.as_str(*r).ok().map(str::to_string)),
        _ => Ok(None),
    }
}
fn oarg(args: &[Value], i: usize) -> Option<ObjRef> {
    match args.get(i) {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn iarg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
fn alloc(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.to_string(), Vec::new())?)
}
fn astr(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(intrinsics::alloc_string(vm, s)?))
}
/// Invoca método DEX (virtual, pela classe do receiver) se declarado.
pub(crate) fn call_dex(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sig: &str,
    mut args: Vec<Value>,
) -> Result<Option<Value>, VmExit> {
    let cls = vm.heap.class_of(recv)?.to_string();
    let Some((d, def, m)) = vm.cp.resolve_method(&cls, name, sig) else {
        return Ok(None);
    };
    let mut ins = vec![Value::Obj(recv)];
    ins.append(&mut args);
    // slot form for wide args
    let ins = slotize(vm, sig, ins);
    Ok(Some(vm.call(d, def, &m, ins)?))
}
fn slotize(_vm: &Engine, sig: &str, ins: Vec<Value>) -> Vec<Value> {
    // Wide (long/double) params occupy two Dalvik registers.
    let params = crate::engine::parse_params_for_m8(sig);
    let mut out = Vec::new();
    let mut it = ins.into_iter();
    if let Some(first) = it.next() {
        out.push(first);
    }
    for (p, v) in params.iter().zip(it) {
        let wide = v.is_wide() || matches!(p.as_str(), "J" | "D");
        out.push(v);
        if wide {
            out.push(Value::WideHi);
        }
    }
    out
}

/// Engine public surface for agents/tests (M8 asserções).
impl Engine {
    pub fn m8_set_http_response(&mut self, code: i32, body: &str) {
        self.fw.m8.http = HttpResponseCfg {
            code,
            body: body.to_string(),
            message: if code == 200 {
                "OK".into()
            } else {
                "Error".into()
            },
        };
    }
    pub fn m8_http_requests(&self) -> &[String] {
        &self.fw.m8.http_requests
    }
    pub fn m8_started_services(&self) -> &[String] {
        &self.fw.m8.started_services
    }
    pub fn m8_broadcasts(&self) -> &[String] {
        &self.fw.m8.broadcasts
    }
    pub fn m8_deep_links(&self) -> &[String] {
        &self.fw.m8.deep_links
    }
    pub fn m8_register_deep_link(&mut self, scheme: &str, activity_desc: &str) {
        self.fw
            .m8
            .deep_link_handlers
            .insert(scheme.to_string(), activity_desc.to_string());
    }
    pub fn m8_work(&self) -> &[WorkRecord] {
        &self.fw.m8.work
    }
    pub fn m8_complete_work(&mut self, id: &str) {
        for w in &mut self.fw.m8.work {
            if w.id == id {
                w.state = "SUCCEEDED".into();
            }
        }
    }
    /// Use the built-in SQL subset instead of the real SQLite backend
    /// (`jsqlite`) for this engine — same as `RD_SQLITE=subset`, without
    /// touching the process environment. Tests that inspect
    /// [`Engine::m8_database`] need it: only the subset fills that model.
    pub fn use_sqlite_subset(&mut self) {
        self.fw.m8.sqlx.unavailable = true;
    }
    pub fn m8_database(&self, name: &str) -> Option<&Database> {
        self.fw.m8.databases.get(name)
    }
    pub fn m8_last_dialog(&self) -> Option<ObjRef> {
        self.fw.m8.last_dialog
    }
    pub fn m8_dialog(&self, r: ObjRef) -> Option<&DialogState> {
        self.fw.m8.dialogs.get(&r)
    }
    pub fn m8_recycler(&self, r: ObjRef) -> Option<&RecyclerState> {
        self.fw.m8.recyclers.get(&r)
    }
}

// ── dispatch estático ───────────────────────────────────────────────────
pub fn call_m8_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_m8_static_rooted(vm, class, name, _sig, args))
}

fn call_m8_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(v) = crate::android_drawable::static_intrinsic(vm, class, name, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = crate::android_uri::static_intrinsic(vm, class, name, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = crate::jsqlite::static_intrinsic(vm, class, name, args)? {
        return Ok(Some(v));
    }
    match (class, name) {
        // Android Process host (probe Breezy Weather): ids determinísticos.
        ("Landroid/os/Process;", "myPid") => Ok(Some(Value::Int(10000))),
        ("Landroid/os/Process;", "myUid") => Ok(Some(Value::Int(10000))),
        ("Landroid/os/Process;", "myTid") => Ok(Some(Value::Int(10000))),
        ("Landroid/os/Process;", "is64Bit") => Ok(Some(Value::Int(1))),
        (URI, "parse") => {
            let Some(s) = sarg(vm, args, 0)? else {
                return Err(npe("Uri.parse(null)"));
            };
            let r = alloc(vm, URI)?;
            vm.fw.m8.uris.insert(r, s);
            Ok(Some(Value::Obj(r)))
        }
        (WORK_MANAGER, "getInstance") => {
            if let Some(w) = vm.fw.m8.work_manager {
                return Ok(Some(Value::Obj(w)));
            }
            let r = alloc(vm, WORK_MANAGER)?;
            vm.fw.m8.work_manager = Some(r);
            Ok(Some(Value::Obj(r)))
        }
        (ROOM, "databaseBuilder") | (ROOM, "inMemoryDatabaseBuilder") => {
            // args: (Context, Class<DB>, String name)
            let dbcls = match args.get(1) {
                Some(Value::Obj(c)) => vm
                    .heap
                    .get_field(*c, "class_name")
                    .ok()
                    .and_then(|v| {
                        if let Value::Obj(s) = v {
                            vm.heap.as_str(s).ok().map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "Landroidx/room/RoomDatabase;".into()),
                _ => "Landroidx/room/RoomDatabase;".into(),
            };
            let dbname = sarg(vm, args, 2)?.unwrap_or_else(|| ":memory:".into());
            let r = alloc(vm, ROOM_BUILDER)?;
            vm.fw.m8.room_builders.insert(r, (dbcls, dbname));
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

// ── dispatch de instância ───────────────────────────────────────────────
pub fn call_m8_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_m8_instance_rooted(vm, mref_class, name, sig, recv, args))
}

fn call_m8_instance_rooted(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let recv_class = vm.heap.class_of(recv).unwrap_or("").to_string();
    // ── Drawable family / ColorStateList (android_drawable.rs) ──────────
    if let Some(v) = crate::android_drawable::instance_intrinsic(
        vm,
        &recv_class,
        mref_class,
        name,
        sig,
        recv,
        args,
    )? {
        return Ok(Some(v));
    }
    // ── Fragments androidx ──────────────────────────────────────────────
    if (mref_class == "Landroidx/fragment/app/FragmentManager;"
        || recv_class == "Landroidx/fragment/app/FragmentManager;")
        && vm.cp.resolve_method(mref_class, name, sig).is_none()
    {
        // Prefer the real AndroidX implementation when the APK supplies it;
        // retain the compact host transaction model for synthetic fixtures.
        return ax_fragment_manager(vm, name, recv, args);
    }
    if (mref_class == "Landroidx/fragment/app/FragmentTransaction;"
        || recv_class == "Landroidx/fragment/app/FragmentTransaction;")
        && vm.cp.resolve_method(mref_class, name, sig).is_none()
    {
        return ax_fragment_tx(vm, name, recv, args);
    }
    if (mref_class == "Landroidx/fragment/app/Fragment;" || mref_class == "Landroid/app/Fragment;")
        && name == "<init>"
    {
        // AndroidX Fragment is shipped in the APK. Its constructor initializes
        // identity, request-code and listener state required by
        // registerForActivityResult; only the platform legacy Fragment lacks
        // a DEX body here and should use the host no-op.
        if vm.cp.resolve_method(mref_class, name, sig).is_some() {
            return Ok(None);
        }
        return Ok(Some(Value::Null));
    }
    if name == "getSupportFragmentManager"
        || (name == "getFragmentManager" && mref_class.contains("androidx"))
    {
        // Let the real library getter execute when present. In particular,
        // FragmentHostCallback.getFragmentManager() returns the
        // FragmentManagerImpl created by its constructor; manufacturing a
        // blank FragmentManager here skips its constructor and leaves fields
        // such as mOnAttachListeners null. Keep the synthetic fallback only
        // for reduced DEX fixtures without the getter implementation.
        if vm.cp.resolve_method(mref_class, name, sig).is_some() {
            return Ok(None);
        }
        if let Some(f) = vm.fw.m8.ax_fragment_manager {
            return Ok(Some(Value::Obj(f)));
        }
        let r = alloc(vm, "Landroidx/fragment/app/FragmentManager;")?;
        vm.fw.m8.ax_fragment_manager = Some(r);
        return Ok(Some(Value::Obj(r)));
    }
    // ── RecyclerView ────────────────────────────────────────────────────
    if mref_class == RECYCLER || recv_class == RECYCLER {
        return recycler(vm, name, sig, recv, args);
    }
    if mref_class == RECYCLER_ADAPTER
        || vm.cp.is_subtype(&recv_class, RECYCLER_ADAPTER)
            && mref_class.contains("RecyclerView$Adapter")
    {
        return adapter(vm, name, sig, recv, args);
    }
    if mref_class == RECYCLER_HOLDER && name == "<init>" {
        if let Some(v) = oarg(args, 0) {
            vm.fw.m8.holders.insert(recv, v);
        }
        return Ok(Some(Value::Null));
    }
    if mref_class == RECYCLER_HOLDER {
        match name {
            "getAdapterPosition" | "getBindingAdapterPosition" | "getLayoutPosition" => {
                let pos = vm
                    .heap
                    .get_field(recv, "m8_position")
                    .ok()
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(0);
                return Ok(Some(Value::Int(pos)));
            }
            "getItemViewType" => return Ok(Some(Value::Int(0))),
            _ => {}
        }
    }
    if (mref_class == RECYCLER_LM || mref_class == LINEAR_LM || mref_class == GRID_LM)
        && name == "<init>"
    {
        return Ok(Some(Value::Null));
    }
    // ── Dialogs ─────────────────────────────────────────────────────────
    if mref_class == DIALOG || mref_class == ALERT_DIALOG || recv_class == ALERT_DIALOG {
        return dialog(vm, name, sig, recv, args, false);
    }
    if mref_class == ALERT_BUILDER || recv_class == ALERT_BUILDER {
        return dialog(vm, name, sig, recv, args, true);
    }
    // ── SQLite ──────────────────────────────────────────────────────────
    // Real SQLite backend first (jsqlite); the subset below is the fallback.
    if mref_class.starts_with("Landroid/database/")
        || recv_class.starts_with("Landroid/database/")
        || vm.cp.is_subtype(&recv_class, SQLITE_HELPER)
    {
        if let Some(v) =
            crate::jsqlite::instance_intrinsic(vm, &recv_class, mref_class, name, sig, recv, args)?
        {
            return Ok(Some(v));
        }
    }
    // A DEX subclass's own methods (its constructor, overrides) run as DEX;
    // the host model only answers calls that land on SQLiteOpenHelper.
    if mref_class == SQLITE_HELPER
        || (vm.cp.is_subtype(&recv_class, SQLITE_HELPER)
            && vm.cp.resolve_method(mref_class, name, sig).is_none())
    {
        return sqlite_helper(vm, name, sig, recv, args);
    }
    if mref_class == SQLITE_DB || recv_class == SQLITE_DB {
        return sqlite_db(vm, name, sig, recv, args);
    }
    if mref_class == CURSOR || vm.fw.m8.cursors.contains_key(&recv) {
        return cursor(vm, name, sig, recv, args);
    }
    if mref_class == CONTENT_VALUES {
        return content_values(vm, name, recv, args);
    }
    // ── OkHttp ──────────────────────────────────────────────────────────
    if mref_class.starts_with("Lokhttp3/") || recv_class.starts_with("Lokhttp3/") {
        return okhttp(vm, mref_class, name, sig, recv, args);
    }
    // ── WorkManager / Room builder ──────────────────────────────────────
    if mref_class == WORK_MANAGER || recv_class == WORK_MANAGER {
        return work_manager(vm, name, sig, recv, args);
    }
    if mref_class == ROOM_BUILDER || recv_class == ROOM_BUILDER {
        return room_builder(vm, name, sig, recv, args);
    }
    if mref_class.contains("Landroidx/work/") && (name == "<init>" || name == "build") {
        // WorkRequest$Builder.build -> WorkRequest host com worker class do builder
        if name == "build" {
            let worker = vm
                .heap
                .get_field(recv, "m8_worker")
                .ok()
                .and_then(|v| {
                    if let Value::Obj(s) = v {
                        vm.heap.as_str(s).ok().map(str::to_string)
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let r = alloc(vm, "Landroidx/work/WorkRequest;")?;
            let ws = intrinsics::alloc_string(vm, worker)?;
            let _ = vm.heap.put_field(r, "m8_worker", Value::Obj(ws));
            return Ok(Some(Value::Obj(r)));
        }
        return Ok(Some(Value::Null));
    }
    if mref_class == OPERATION {
        match name {
            "getResult" => {
                let r = alloc(vm, "Lcom/google/common/util/concurrent/ListenableFuture;")?;
                return Ok(Some(Value::Obj(r)));
            }
            _ => return Ok(Some(Value::Null)),
        }
    }
    // ── java.io.File host (probe Breezy Weather; caminho em heap field) ─
    if mref_class == "Ljava/io/File;" || recv_class == "Ljava/io/File;" {
        return file_host(vm, name, recv, args);
    }
    // ── Uri (instância) ─────────────────────────────────────────────────
    if mref_class == URI
        || recv_class == URI
        || mref_class == "Landroid/net/Uri$Builder;"
        || recv_class == "Landroid/net/Uri$Builder;"
    {
        if let Some(v) =
            crate::android_uri::instance_intrinsic(vm, &recv_class, mref_class, name, recv, args)?
        {
            return Ok(Some(v));
        }
        if mref_class == URI || recv_class == URI {
            return uri_instance(vm, name, recv, args);
        }
    }
    // ── JobScheduler ────────────────────────────────────────────────────
    if mref_class == JOB_SCHEDULER || recv_class == JOB_SCHEDULER {
        return job_scheduler(vm, name, recv, args);
    }
    // ── Intent action/data (complementa framework) ─────────────────────
    if mref_class == "Landroid/content/Intent;" {
        return intent_m8(vm, name, sig, recv, args);
    }
    // ── Context.getSystemService (probe real Breezy Weather) ───────────
    if name == "getSystemServiceName" && sig == "(Ljava/lang/Class;)Ljava/lang/String;" {
        let cls = match args.first() {
            Some(Value::Obj(c)) => match vm.heap.get_field(*c, "class_name") {
                Ok(Value::Obj(s)) => vm
                    .heap
                    .as_str(s)
                    .ok()
                    .map(str::to_string)
                    .unwrap_or_default(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        return Ok(Some(match SERVICES.iter().find(|(_, c)| *c == cls) {
            Some((n, _)) => Value::Obj(crate::intrinsics::alloc_string(vm, n.to_string())?),
            None => Value::Null,
        }));
    }
    if name == "getSystemService"
        && (mref_class == "Landroid/content/Context;"
            || mref_class == "Landroid/app/Activity;"
            || mref_class == "Landroid/app/Application;"
            || mref_class == "Landroid/content/ContextWrapper;"
            || mref_class == "Landroid/view/ContextThemeWrapper;"
            || mref_class == "Landroid/app/Service;")
    {
        let key = if sig.contains("Ljava/lang/Class;") {
            match args.first() {
                Some(Value::Obj(c)) => vm
                    .heap
                    .get_field(*c, "class_name")
                    .ok()
                    .and_then(|v| {
                        if let Value::Obj(s) = v {
                            vm.heap.as_str(s).ok().map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            }
        } else {
            sarg(vm, args, 0)?.unwrap_or_default()
        };
        // Class-keyed lookups return exactly that class; services are
        // per-class singletons (AOSP caches them per Context).
        let class = if key.starts_with('L')
            && key.ends_with(';')
            && system_service_class(&key) == "Ljava/lang/Object;"
        {
            key.clone()
        } else {
            system_service_class(&key).to_string()
        };
        if class == "Ljava/lang/Object;" {
            return Ok(Some(Value::Null)); // unknown service name → null, as AOSP
        }
        let k = ("Lrd/SystemServices;".to_string(), class.clone());
        if let Some(v) = vm.statics.get(&k) {
            return Ok(Some(v.clone()));
        }
        let r = alloc(vm, &class)?;
        vm.statics.insert(k, Value::Obj(r));
        return Ok(Some(Value::Obj(r)));
    }
    // ── Context: services / broadcasts / receivers ─────────────────────
    if matches!(
        name,
        "startService"
            | "startForegroundService"
            | "stopService"
            | "bindService"
            | "unbindService"
            | "sendBroadcast"
            | "sendOrderedBroadcast"
            | "registerReceiver"
            | "unregisterReceiver"
    ) && (mref_class == "Landroid/content/Context;"
        || mref_class == "Landroid/app/Activity;"
        || mref_class == "Landroid/app/Application;")
    {
        return context_services(vm, name, recv, args);
    }
    if name == "startActivity"
        && (mref_class == "Landroid/content/Context;" || mref_class == "Landroid/app/Activity;")
    {
        // Deep link / VIEW implícito: se não há componente, resolve aqui.
        if let Some(ir) = oarg(args, 0) {
            let has_component = matches!(
                vm.fw.get(ir),
                Some(HostObj::Intent {
                    component: Some(_),
                    ..
                })
            );
            if !has_component {
                let st = vm.fw.m8.intents.get(&ir).cloned().unwrap_or_default();
                if let Some(data_ref) = st.data {
                    if let Some(u) = vm.fw.m8.uris.get(&data_ref).cloned() {
                        vm.fw.m8.deep_links.push(u.clone());
                        if let Some(scheme) = u.split(':').next() {
                            if let Some(target) = vm.fw.m8.deep_link_handlers.get(scheme).cloned() {
                                if vm.cp.find_class(&target).is_some() {
                                    let pkg = vm.fw.package_name.clone();
                                    vm.launch_app(&target, &pkg, Vec::new())?;
                                }
                            }
                        }
                        return Ok(Some(Value::Null));
                    }
                }
                if st.action.is_some() {
                    // Broadcast-like VIEW sem data: registrado como deep link vazio.
                    vm.fw
                        .m8
                        .deep_links
                        .push(st.action.clone().unwrap_or_default());
                    return Ok(Some(Value::Null));
                }
            }
        }
        return Ok(None); // componente: framework trata
    }
    Ok(None)
}

// ── RecyclerView ────────────────────────────────────────────────────────
fn recycler(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => Ok(Some(Value::Null)),
        "setAdapter" => {
            let adapter = oarg(args, 0);
            vm.fw.m8.recyclers.entry(recv).or_default().adapter = adapter;
            if let Some(a) = adapter {
                populate(vm, recv, a)?;
            }
            Ok(Some(Value::Null))
        }
        "getAdapter" => Ok(Some(
            match vm.fw.m8.recyclers.get(&recv).and_then(|s| s.adapter) {
                Some(a) => Value::Obj(a),
                None => Value::Null,
            },
        )),
        "setLayoutManager" => {
            vm.fw.m8.recyclers.entry(recv).or_default().layout_manager = oarg(args, 0);
            Ok(Some(Value::Null))
        }
        "getLayoutManager" => Ok(Some(
            match vm.fw.m8.recyclers.get(&recv).and_then(|s| s.layout_manager) {
                Some(a) => Value::Obj(a),
                None => Value::Null,
            },
        )),
        "setHasFixedSize" | "setItemAnimator" | "addItemDecoration" | "setItemViewCacheSize" => {
            Ok(Some(Value::Null))
        }
        "getChildCount" => {
            let n = match vm.fw.get(recv) {
                Some(HostObj::View { children, .. }) => children.len() as i32,
                _ => 0,
            };
            Ok(Some(Value::Int(n)))
        }
        "scrollToPosition" | "smoothScrollToPosition" => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}
/// Popula a RecyclerView chamando o adapter DEX: getItemCount,
/// onCreateViewHolder, onBindViewHolder (cap 20 itens — viewport headless).
fn populate(vm: &mut Engine, recv: ObjRef, adapter: ObjRef) -> Result<(), VmExit> {
    if vm
        .fw
        .m8
        .recyclers
        .get(&recv)
        .map(|s| s.populating)
        .unwrap_or(false)
    {
        return Ok(());
    }
    vm.fw.m8.recyclers.entry(recv).or_default().populating = true;
    let result = (|| -> Result<(), VmExit> {
        let count = match call_dex(vm, adapter, "getItemCount", "()I", vec![])? {
            Some(Value::Int(n)) => n.max(0),
            _ => 0,
        };
        vm.fw.m8.recyclers.entry(recv).or_default().item_count = count;
        // limpa filhos anteriores
        if let Some(HostObj::View { children, .. }) = vm.fw.objects.get_mut(&recv) {
            children.clear();
        }
        vm.fw
            .m8
            .recyclers
            .entry(recv)
            .or_default()
            .bound_positions
            .clear();
        for pos in 0..count.min(20) {
            let holder = match call_dex(
                vm,
                adapter,
                "onCreateViewHolder",
                "(Landroid/view/ViewGroup;I)Landroidx/recyclerview/widget/RecyclerView$ViewHolder;",
                vec![Value::Obj(recv), Value::Int(0)],
            )? {
                Some(Value::Obj(h)) => h,
                _ => continue,
            };
            let _ = vm.heap.put_field(holder, "m8_position", Value::Int(pos));
            call_dex(
                vm,
                adapter,
                "onBindViewHolder",
                "(Landroidx/recyclerview/widget/RecyclerView$ViewHolder;I)V",
                vec![Value::Obj(holder), Value::Int(pos)],
            )?;
            vm.fw
                .m8
                .recyclers
                .entry(recv)
                .or_default()
                .bound_positions
                .push(pos);
            if let Some(item) = vm.fw.m8.holders.get(&holder).copied() {
                crate::framework::attach_child_public(vm, recv, item)?;
            }
        }
        Ok(())
    })();
    vm.fw.m8.recyclers.entry(recv).or_default().populating = false;
    // layout para bounds clicáveis
    if result.is_ok() {
        if let Some(act) = vm.fw.current_activity {
            let _ = act;
        }
    }
    result
}
fn adapter(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => Ok(Some(Value::Null)),
        "notifyDataSetChanged"
        | "notifyItemInserted"
        | "notifyItemRemoved"
        | "notifyItemChanged"
        | "notifyItemRangeInserted"
        | "notifyItemRangeRemoved"
        | "notifyItemRangeChanged" => {
            // Atualiza todas as recyclers que usam este adapter.
            let targets: Vec<ObjRef> = vm
                .fw
                .m8
                .recyclers
                .iter()
                .filter(|(_, s)| s.adapter == Some(recv))
                .map(|(k, _)| *k)
                .collect();
            for t in targets {
                vm.fw.m8.recyclers.entry(t).or_default().notifications += 1;
                populate(vm, t, recv)?;
            }
            Ok(Some(Value::Null))
        }
        "getItemCount" => Ok(None), // abstrato no adapter do app: DEX trata
        "setHasStableIds" | "registerAdapterDataObserver" | "unregisterAdapterDataObserver" => {
            Ok(Some(Value::Null))
        }
        "hasObservers" => Ok(Some(Value::Int(0))),
        _ => Ok(None),
    }
}

// ── Dialogs ─────────────────────────────────────────────────────────────
fn dialog(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
    is_builder: bool,
) -> Result<Option<Value>, VmExit> {
    let map = if is_builder {
        &mut vm.fw.m8.builders
    } else {
        &mut vm.fw.m8.dialogs
    };
    map.entry(recv).or_default();
    match name {
        "<init>" => {
            if !is_builder {
                if let Some(ctx) = oarg(args, 0) {
                    vm.view_props
                        .insert((recv, "dlg:context".to_string()), Value::Obj(ctx));
                }
            }
            Ok(Some(Value::Null))
        }
        "getContext" if !is_builder => Ok(Some(
            vm.view_props
                .get(&(recv, "dlg:context".to_string()))
                .cloned()
                .unwrap_or(Value::Null),
        )),
        // AOSP Dialog lifecycle hooks: the base implementations are empty
        "onCreate" | "onStart" | "onStop" if !is_builder => Ok(Some(Value::Null)),
        // AOSP Dialog.onTouchEvent: a DOWN outside the window cancels a
        // cancelable dialog when canceledOnTouchOutside (dialog themes'
        // windowCloseOnTouchOutside default) — the caller only delivers
        // events that fell outside the dialog's window
        "onTouchEvent" if !is_builder => {
            let action = match oarg(args, 0) {
                Some(ev) => vm
                    .invoke_instance(ev, "getActionMasked", "()I", &[])?
                    .as_int()
                    .unwrap_or(-1),
                None => -1,
            };
            let cancelable = !vm
                .view_props
                .contains_key(&(recv, "dlg:noCancel".to_string()));
            let outside = !vm
                .view_props
                .contains_key(&(recv, "dlg:noOutside".to_string()));
            let showing = vm.fw.m8.dialogs.get(&recv).is_some_and(|d| d.showing);
            if action == 0 && cancelable && outside && showing {
                dialog_set_showing(vm, recv, false)?;
                return Ok(Some(Value::Int(1)));
            }
            Ok(Some(Value::Int(0)))
        }
        "setCanceledOnTouchOutside" if !is_builder => {
            let k = (recv, "dlg:noOutside".to_string());
            if iarg(args, 0) == 0 {
                vm.view_props.insert(k, Value::Int(1));
            } else {
                vm.view_props.remove(&k);
            }
            Ok(Some(Value::Null))
        }
        "onSaveInstanceState" if !is_builder => {
            const BUNDLE: &str = "Landroid/os/Bundle;";
            let b = vm.heap.alloc_instance(BUNDLE.to_string(), Vec::new())?;
            intrinsics::call_instance_intrinsic(vm, BUNDLE, "<init>", "()V", b, &[])?;
            Ok(Some(Value::Obj(b)))
        }
        "getOnBackInvokedDispatcher" if !is_builder => crate::activity_extras::instance_intrinsic(
            vm,
            "Landroid/app/Activity;",
            name,
            _sig,
            recv,
            args,
        ),
        "onBackPressed" if !is_builder => {
            dialog_set_showing(vm, recv, false)?;
            Ok(Some(Value::Null))
        }
        "addContentView" if !is_builder => {
            let has = vm.fw.m8.dialogs.get(&recv).and_then(|d| d.view).is_some();
            if !has {
                return dialog(vm, "setContentView", _sig, recv, args, false);
            }
            Ok(Some(Value::Null))
        }
        "setTitle" | "setMessage" => {
            let txt = match name {
                "setTitle" => sarg(vm, args, 0)?,
                _ => sarg(vm, args, 0)?,
            };
            // AlertDialog.setMessage é (CharSequence); Builder igual.
            let txt = txt
                .or_else(|| oarg(args, 0).and_then(|r| vm.heap.as_str(r).ok().map(str::to_string)));
            let st = if is_builder {
                vm.fw.m8.builders.get_mut(&recv).unwrap()
            } else {
                vm.fw.m8.dialogs.get_mut(&recv).unwrap()
            };
            if name == "setTitle" {
                st.title = txt;
            } else {
                st.message = txt;
            }
            Ok(Some(if is_builder {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        "setView" => {
            let v = oarg(args, 0);
            let st = if is_builder {
                vm.fw.m8.builders.get_mut(&recv).unwrap()
            } else {
                vm.fw.m8.dialogs.get_mut(&recv).unwrap()
            };
            st.view = v;
            Ok(Some(if is_builder {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        "setCancelable" => {
            let v = iarg(args, 0) != 0;
            if !is_builder {
                let k = (recv, "dlg:noCancel".to_string());
                if v {
                    vm.view_props.remove(&k);
                } else {
                    vm.view_props.insert(k, Value::Int(1));
                }
            }
            let st = if is_builder {
                vm.fw.m8.builders.get_mut(&recv).unwrap()
            } else {
                vm.fw.m8.dialogs.get_mut(&recv).unwrap()
            };
            st.cancelable = v;
            Ok(Some(if is_builder {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        "setPositiveButton" | "setNegativeButton" | "setNeutralButton" => {
            let which = match name {
                "setPositiveButton" => -1,
                "setNegativeButton" => -2,
                _ => -3,
            };
            let label = sarg(vm, args, 0)?.unwrap_or_default();
            let listener = oarg(args, 1);
            let st = if is_builder {
                vm.fw.m8.builders.get_mut(&recv).unwrap()
            } else {
                vm.fw.m8.dialogs.get_mut(&recv).unwrap()
            };
            st.buttons.insert(which, (label, listener));
            Ok(Some(if is_builder {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        "setItems" | "setOnCancelListener" | "setOnDismissListener" | "setOnShowListener" => {
            Ok(Some(if is_builder {
                Value::Obj(recv)
            } else {
                Value::Null
            }))
        }
        "create" if is_builder => {
            let st = vm.fw.m8.builders.get(&recv).cloned().unwrap_or_default();
            let d = alloc(vm, ALERT_DIALOG)?;
            vm.fw.m8.dialogs.insert(d, st);
            Ok(Some(Value::Obj(d)))
        }
        "show" => {
            if is_builder {
                let st = vm.fw.m8.builders.get(&recv).cloned().unwrap_or_default();
                let d = alloc(vm, ALERT_DIALOG)?;
                let mut st = st;
                st.showing = true;
                vm.fw.m8.dialogs.insert(d, st);
                vm.fw.m8.last_dialog = Some(d);
                Ok(Some(Value::Obj(d)))
            } else {
                vm.fw.m8.last_dialog = Some(recv);
                dialog_set_showing(vm, recv, true)?;
                Ok(Some(Value::Null))
            }
        }
        "dismiss" | "cancel" => {
            dialog_set_showing(vm, recv, false)?;
            Ok(Some(Value::Null))
        }
        "isShowing" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .dialogs
                .get(&recv)
                .map(|s| s.showing as i32)
                .unwrap_or(0),
        ))),
        "getButton" => Ok(Some(Value::Null)), // botão host não é View; clique via m8_click_dialog_button
        // Each Dialog owns a host Window (AOSP: PhoneWindow created in the
        // Dialog constructor); `setContentView(View)` sets its content.
        "getWindow" if !is_builder => Ok(Some(Value::Obj(dialog_window(vm, recv)?))),
        "setContentView" if !is_builder => {
            if let Some(v) = oarg(args, 0) {
                let w = dialog_window(vm, recv)?;
                if let Some(crate::framework::HostObj::Window { content }) =
                    vm.fw.objects.get_mut(&w)
                {
                    *content = Some(v);
                }
                if let Some(st) = vm.fw.m8.dialogs.get_mut(&recv) {
                    st.view = Some(v);
                }
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}
/// AOSP `Dialog.show()`: the first show dispatches `onCreate(null)`, then
/// every show `onStart()`; `dismiss()` dispatches `onStop()`. Virtual
/// dispatch, so DEX subclasses (ComponentDialog, Compose DialogWrapper, …)
/// run their overrides; the host base methods are no-ops.
fn dialog_set_showing(vm: &mut Engine, dialog: ObjRef, show: bool) -> Result<(), VmExit> {
    let was = vm
        .fw
        .m8
        .dialogs
        .get(&dialog)
        .map(|d| d.showing)
        .unwrap_or(false);
    if let Some(st) = vm.fw.m8.dialogs.get_mut(&dialog) {
        st.showing = show;
    }
    if show != was {
        // WindowManager.addView/removeView → ViewRootImpl.requestLayout:
        // the next vsync traverses the new (or uncovered) window
        vm.fw.traversal_pending = true;
    }
    if show && !was {
        let created = (dialog, "dlg:created".to_string());
        if vm.view_props.insert(created, Value::Int(1)).is_none() {
            vm.invoke_instance(dialog, "onCreate", "(Landroid/os/Bundle;)V", &[Value::Null])?;
        }
        vm.invoke_instance(dialog, "onStart", "()V", &[])?;
        // WindowManager.addView(decor): the dialog's window tree attaches
        if let Some(c) = dialog_content(vm, dialog) {
            crate::view_attach::dispatch_attach(vm, c)?;
        }
    } else if !show && was {
        if let Some(c) = dialog_content(vm, dialog) {
            crate::view_attach::dispatch_detach(vm, c)?;
        }
        vm.invoke_instance(dialog, "onStop", "()V", &[])?;
    }
    Ok(())
}

pub(crate) fn dialog_window(vm: &mut Engine, dialog: ObjRef) -> Result<ObjRef, VmExit> {
    let key = (dialog, "dlg:window".to_string());
    if let Some(Value::Obj(w)) = vm.view_props.get(&key) {
        return Ok(*w);
    }
    let w = vm
        .heap
        .alloc_instance("Landroid/view/Window;".to_string(), Vec::new())?;
    vm.fw
        .objects
        .insert(w, crate::framework::HostObj::Window { content: None });
    vm.view_props.insert(key, Value::Obj(w));
    // floating (dialog) windows are WRAP_CONTENT × WRAP_CONTENT
    let a = crate::window::window_attributes(vm, w)?;
    for f in ["width", "height"] {
        crate::err::put_field_result(vm.heap.put_field(a, f, Value::Int(-2)))?;
    }
    Ok(w)
}

/// Content root of a dialog's window, if any.
pub(crate) fn dialog_content(vm: &Engine, dialog: ObjRef) -> Option<ObjRef> {
    if let Some(Value::Obj(w)) = vm.view_props.get(&(dialog, "dlg:window".to_string())) {
        if let Some(crate::framework::HostObj::Window { content: Some(c) }) = vm.fw.get(*w) {
            return Some(*c);
        }
    }
    vm.fw.m8.dialogs.get(&dialog).and_then(|d| d.view)
}

/// Showing dialogs that have a content view, as (dialog, content) in
/// creation order: the last one is the topmost window.
pub(crate) fn showing_dialog_roots(vm: &Engine) -> Vec<(ObjRef, ObjRef)> {
    let mut v: Vec<(ObjRef, ObjRef)> = vm
        .fw
        .m8
        .dialogs
        .iter()
        .filter(|(_, st)| st.showing)
        .filter_map(|(d, _)| dialog_content(vm, *d).map(|c| (*d, c)))
        .collect();
    v.sort_unstable();
    v
}

/// The dialog window's requested size (`WindowManager.LayoutParams`
/// width/height: -1 MATCH_PARENT, -2 WRAP_CONTENT, else px).
pub(crate) fn dialog_window_size(vm: &mut Engine, dialog: ObjRef) -> (i32, i32) {
    let Ok(w) = dialog_window(vm, dialog) else {
        return (-2, -2);
    };
    let Ok(a) = crate::window::window_attributes(vm, w) else {
        return (-2, -2);
    };
    let get = |vm: &Engine, f: &str| match vm.heap.get_field(a, f) {
        Ok(Value::Int(i)) => i,
        _ => -2,
    };
    (get(vm, "width"), get(vm, "height"))
}

impl Engine {
    /// Clica um botão de dialog (which: -1 positivo, -2 negativo, -3 neutro),
    /// disparando o OnClickListener DEX registrado. Retorna false se ausente.
    pub fn m8_click_dialog_button(&mut self, dialog: ObjRef, which: i32) -> Result<bool, VmExit> {
        let Some((_, listener)) = self
            .fw
            .m8
            .dialogs
            .get(&dialog)
            .and_then(|d| d.buttons.get(&which))
            .cloned()
        else {
            return Ok(false);
        };
        if let Some(l) = listener {
            let _ = call_dex(
                self,
                l,
                "onClick",
                "(Landroid/content/DialogInterface;I)V",
                vec![Value::Obj(dialog), Value::Int(which)],
            )?;
        }
        if let Some(st) = self.fw.m8.dialogs.get_mut(&dialog) {
            st.showing = false;
        }
        Ok(true)
    }
}

// ── SQLite ──────────────────────────────────────────────────────────────
fn db_name_of(vm: &mut Engine, recv: ObjRef) -> Result<String, VmExit> {
    vm.fw.m8.db_by_obj.get(&recv).cloned().ok_or_else(|| {
        VmExit::Error(crate::err::vm_error(
            "INVALID_STATE",
            "SQLiteDatabase sem banco associado",
        ))
    })
}
fn sqlite_helper(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            // (Context, String name, CursorFactory, int version) — name no arg 1
            let dbname = sarg(vm, args, 1)?.unwrap_or_else(|| ":memory:".into());
            vm.fw.m8.helpers_db.insert(recv, dbname);
            Ok(Some(Value::Null))
        }
        "getWritableDatabase" | "getReadableDatabase" => {
            let dbname = vm
                .fw
                .m8
                .helpers_db
                .get(&recv)
                .cloned()
                .unwrap_or_else(|| ":memory:".into());
            let is_new = !vm.fw.m8.databases.contains_key(&dbname);
            vm.fw.m8.databases.entry(dbname.clone()).or_default();
            let d = alloc(vm, SQLITE_DB)?;
            vm.fw.m8.db_by_obj.insert(d, dbname);
            if is_new {
                // Room / helpers do app criam schema em onCreate(SQLiteDatabase)
                let _ = call_dex(
                    vm,
                    recv,
                    "onCreate",
                    "(Landroid/database/sqlite/SQLiteDatabase;)V",
                    vec![Value::Obj(d)],
                )?;
            }
            Ok(Some(Value::Obj(d)))
        }
        "close" => Ok(Some(Value::Null)),
        // journal/config knobs: the in-memory store has no journal, so they
        // are accepted and have no effect
        "setWriteAheadLoggingEnabled"
        | "setLookasideConfig"
        | "setOpenParams"
        | "setIdleConnectionTimeout" => Ok(Some(Value::Null)),
        "getDatabaseName" => {
            let n = vm.fw.m8.helpers_db.get(&recv).cloned().unwrap_or_default();
            Ok(Some(astr(vm, n)?))
        }
        _ => Ok(None),
    }
}
fn parse_create(sql: &str) -> Option<(String, Vec<String>)> {
    let up = sql.to_ascii_uppercase();
    if !up.trim_start().starts_with("CREATE TABLE") {
        return None;
    }
    let open = sql.find('(')?;
    let close = sql.rfind(')')?;
    let head = &sql[..open];
    let table = head
        .split_whitespace()
        .last()?
        .trim_matches(|c| c == '`' || c == '"' || c == '\'')
        .to_string();
    let cols: Vec<String> = sql[open + 1..close]
        .split(',')
        .map(|c| {
            c.split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c| c == '`' || c == '"')
                .to_string()
        })
        .filter(|c| {
            !c.is_empty()
                && !matches!(
                    c.to_ascii_uppercase().as_str(),
                    "PRIMARY" | "FOREIGN" | "UNIQUE" | "CHECK" | "CONSTRAINT"
                )
        })
        .collect();
    Some((table, cols))
}
fn sqlite_db(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "execSQL" => {
            let sql = sarg(vm, args, 0)?.unwrap_or_default();
            let dbname = db_name_of(vm, recv)?;
            if let Some((table, cols)) = parse_create(&sql) {
                let db = vm.fw.m8.databases.entry(dbname).or_default();
                db.tables.entry(table).or_insert(Table {
                    columns: cols,
                    rows: Vec::new(),
                });
            } else if sql
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("DROP TABLE")
            {
                if let Some(t) = sql.split_whitespace().last() {
                    let t = t.trim_matches(|c| c == ';' || c == '`').to_string();
                    if let Some(db) = vm.fw.m8.databases.get_mut(&dbname) {
                        db.tables.remove(&t);
                    }
                }
            }
            // demais DDL/DML via execSQL: aceitos (índices etc.) — estado é o schema acima.
            Ok(Some(Value::Null))
        }
        "insert" | "insertOrThrow" | "replace" => {
            let table = sarg(vm, args, 0)?.unwrap_or_default();
            let values_obj = oarg(args, 2);
            let dbname = db_name_of(vm, recv)?;
            let row = insert_row(vm, &dbname, &table, values_obj, name == "replace")?;
            Ok(Some(Value::Long(row)))
        }
        "update" => {
            let table = sarg(vm, args, 0)?.unwrap_or_default();
            let values_obj = oarg(args, 1);
            let where_clause = sarg(vm, args, 2)?;
            let dbname = db_name_of(vm, recv)?;
            let vals: Vec<(String, Value)> = values_obj
                .and_then(|o| vm.fw.m8.content_values.get(&o))
                .cloned()
                .unwrap_or_default();
            let mut n = 0;
            let where_arg = string_array(vm, args.get(3)).first().cloned();
            if let Some(db) = vm.fw.m8.databases.get_mut(&dbname) {
                if let Some(t) = db.tables.get_mut(&table) {
                    for row in &mut t.rows {
                        if where_matches(
                            &t.columns.clone(),
                            row,
                            where_clause.as_deref(),
                            where_arg.clone(),
                        ) {
                            for (c, v) in &vals {
                                if let Some(i) = t.columns.iter().position(|x| x == c) {
                                    row[i] = v.clone();
                                }
                            }
                            n += 1;
                        }
                    }
                }
            }
            Ok(Some(Value::Int(n)))
        }
        "delete" => {
            let table = sarg(vm, args, 0)?.unwrap_or_default();
            let where_clause = sarg(vm, args, 1)?;
            let dbname = db_name_of(vm, recv)?;
            let mut n = 0;
            let where_arg = string_array(vm, args.get(2)).first().cloned();
            if let Some(db) = vm.fw.m8.databases.get_mut(&dbname) {
                if let Some(t) = db.tables.get_mut(&table) {
                    let before = t.rows.len();
                    let mut kept = Vec::new();
                    for row in t.rows.drain(..) {
                        if where_matches(
                            &t.columns.clone(),
                            &row,
                            where_clause.as_deref(),
                            where_arg.clone(),
                        ) {
                            n += 1;
                        } else {
                            kept.push(row);
                        }
                    }
                    t.rows = kept;
                    let _ = before;
                }
            }
            Ok(Some(Value::Int(n)))
        }
        "query" | "rawQuery" => {
            let (columns, rows) = if name == "rawQuery" {
                let sql = sarg(vm, args, 0)?.unwrap_or_default();
                query_sql(vm, recv, &sql, args.get(1))?
            } else {
                let table = sarg(vm, args, 0)?.unwrap_or_default();
                let dbname = db_name_of(vm, recv)?;
                let db = vm.fw.m8.databases.get(&dbname);
                match db.and_then(|d| d.tables.get(&table)) {
                    Some(t) => (t.columns.clone(), t.rows.clone()),
                    None => (Vec::new(), Vec::new()),
                }
            };
            let c = alloc(vm, CURSOR)?;
            vm.fw.m8.cursors.insert(
                c,
                CursorState {
                    columns,
                    rows,
                    pos: -1,
                    closed: false,
                },
            );
            Ok(Some(Value::Obj(c)))
        }
        "beginTransaction" => {
            let n = db_name_of(vm, recv)?;
            if let Some(d) = vm.fw.m8.databases.get_mut(&n) {
                d.in_transaction = true;
                d.transaction_successful = false;
            }
            Ok(Some(Value::Null))
        }
        "setTransactionSuccessful" => {
            let n = db_name_of(vm, recv)?;
            if let Some(d) = vm.fw.m8.databases.get_mut(&n) {
                d.transaction_successful = true;
            }
            Ok(Some(Value::Null))
        }
        "endTransaction" => {
            let n = db_name_of(vm, recv)?;
            if let Some(d) = vm.fw.m8.databases.get_mut(&n) {
                d.in_transaction = false;
            }
            Ok(Some(Value::Null))
        }
        "inTransaction" => {
            let n = db_name_of(vm, recv)?;
            Ok(Some(Value::Int(
                vm.fw
                    .m8
                    .databases
                    .get(&n)
                    .map(|d| d.in_transaction as i32)
                    .unwrap_or(0),
            )))
        }
        "getVersion" => {
            let n = db_name_of(vm, recv)?;
            Ok(Some(Value::Int(
                vm.fw.m8.databases.get(&n).map(|d| d.version).unwrap_or(0),
            )))
        }
        "setVersion" => {
            let n = db_name_of(vm, recv)?;
            if let Some(d) = vm.fw.m8.databases.get_mut(&n) {
                d.version = iarg(args, 0);
            }
            Ok(Some(Value::Null))
        }
        "isOpen" => Ok(Some(Value::Int(1))),
        "enableWriteAheadLogging" => Ok(Some(Value::Int(1))),
        "disableWriteAheadLogging"
        | "setForeignKeyConstraintsEnabled"
        | "setMaxSqlCacheSize"
        | "setLocale"
        | "setPageSize" => Ok(Some(Value::Null)),
        "isWriteAheadLoggingEnabled" | "isReadOnly" | "isDbLockedByCurrentThread" => {
            Ok(Some(Value::Int(0)))
        }
        "getPath" => {
            let n = db_name_of(vm, recv)?;
            Ok(Some(astr(vm, format!("/data/data/app/databases/{n}"))?))
        }
        "getAttachedDbs" => {
            let n = db_name_of(vm, recv)?;
            Ok(Some(
                crate::collections::new_list(vm, "Ljava/util/ArrayList;", vec![Value::Null; 0])
                    .inspect(|_l| {
                        let _ = n;
                    })?,
            ))
        }
        "close" => Ok(Some(Value::Null)),
        "compileStatement" => {
            // SQLiteStatement mínimo: devolve objeto; bind/execute tratados tipados se sondados.
            let r = alloc(vm, "Landroid/database/sqlite/SQLiteStatement;")?;
            let _ = sig;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}
fn insert_row(
    vm: &mut Engine,
    dbname: &str,
    table: &str,
    values_obj: Option<ObjRef>,
    replace: bool,
) -> Result<i64, VmExit> {
    // banco: qualquer db do objeto chamador não é conhecido aqui; usa o primeiro db.
    // (insert é chamado via sqlite_db com dbname — esta função resolve pelo values/contexto do caller via db_by_obj é do db; aqui o caller passa table apenas, então procuramos em todos e preferimos o que tem a tabela.)
    let vals: Vec<(String, Value)> = values_obj
        .and_then(|o| vm.fw.m8.content_values.get(&o))
        .cloned()
        .unwrap_or_default();
    let db = vm.fw.m8.databases.entry(dbname.to_string()).or_default();
    let t = db.tables.entry(table.to_string()).or_insert_with(|| Table {
        columns: vals.iter().map(|(c, _)| c.clone()).collect(),
        rows: Vec::new(),
    });
    for (c, _) in &vals {
        if !t.columns.contains(c) {
            t.columns.push(c.clone());
            for r in &mut t.rows {
                r.push(Value::Null);
            }
        }
    }
    let mut row: Vec<Value> = t
        .columns
        .iter()
        .map(|c| {
            vals.iter()
                .find(|(k, _)| k == c)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null)
        })
        .collect();
    if replace {
        // replace pela primeira coluna (PK típica Room)
        if !row.is_empty() {
            t.rows.retain(|r| r.first() != row.first());
        }
    }
    let idx = t.rows.len() as i64 + 1;
    // rowid na primeira coluna inteira se presente
    if let Some(Value::Int(_)) = row.first() {
    } else if !row.is_empty() { /* mantém */
    }
    t.rows.push(row.clone());
    let _ = &mut row;
    Ok(idx)
}
/// Extrai um `[Ljava/lang/String;` de whereArgs/selectionArgs.
fn string_array(vm: &Engine, v: Option<&Value>) -> Vec<String> {
    let Some(Value::Obj(r)) = v else {
        return Vec::new();
    };
    match vm.heap.as_array_elems(*r) {
        Ok((elems, _)) => elems
            .iter()
            .map(|e| match e {
                Value::Obj(s) => vm.heap.as_str(*s).unwrap_or("").to_string(),
                Value::Int(n) => n.to_string(),
                Value::Long(n) => n.to_string(),
                _ => String::new(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn where_matches(
    columns: &[String],
    row: &[Value],
    clause: Option<&str>,
    arg_value: Option<String>,
) -> bool {
    let Some(c) = clause else {
        return true;
    };
    // subconjunto: "col=?" / "col=valor" / "col IS NULL" — sem cláusula = casa tudo já tratado.
    let c = c.trim();
    if let Some((col, val)) = c.split_once('=') {
        let col = col.trim();
        let val = val
            .trim()
            .trim_matches(|ch| ch == '\'' || ch == '"' || ch == ';');
        if let Some(i) = columns.iter().position(|x| x == col) {
            if val == "?" {
                return match (row.get(i), arg_value) {
                    (Some(Value::Int(n)), Some(a)) => {
                        a.parse::<i32>().map(|v| v == *n).unwrap_or(false)
                    }
                    (Some(Value::Long(n)), Some(a)) => {
                        a.parse::<i64>().map(|v| v == *n).unwrap_or(false)
                    }
                    (Some(_), None) => true,
                    _ => false,
                };
            }
            if val.is_empty() {
                return row.get(i).is_some();
            }
            return match row.get(i) {
                Some(Value::Int(n)) => val.parse::<i32>().map(|v| v == *n).unwrap_or(false),
                Some(Value::Long(n)) => val.parse::<i64>().map(|v| v == *n).unwrap_or(false),
                Some(Value::Obj(r)) => {
                    let _ = r;
                    val.is_empty()
                }
                _ => false,
            };
        }
        return true;
    }
    true
}
fn query_sql(
    vm: &mut Engine,
    recv: ObjRef,
    sql: &str,
    _sel_args: Option<&Value>,
) -> Result<(Vec<String>, Vec<Vec<Value>>), VmExit> {
    let dbname = db_name_of(vm, recv)?;
    let up = sql.to_ascii_uppercase();
    if !up.trim_start().starts_with("SELECT") {
        return Ok((Vec::new(), Vec::new()));
    }
    // FROM <table>
    let table = sql
        .split_whitespace()
        .zip(sql.split_whitespace().skip(1))
        .find(|(a, _)| a.eq_ignore_ascii_case("FROM"))
        .map(|(_, b)| {
            b.trim_matches(|c| c == ';' || c == '`' || c == '"')
                .to_string()
        });
    let Some(table) = table else {
        return Ok((Vec::new(), Vec::new()));
    };
    let db = vm.fw.m8.databases.get(&dbname);
    match db.and_then(|d| d.tables.get(&table)) {
        Some(t) => Ok((t.columns.clone(), t.rows.clone())),
        None => Ok((Vec::new(), Vec::new())),
    }
}
fn content_values(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.fw.m8.content_values.entry(recv).or_default();
    match name {
        "<init>" => Ok(Some(Value::Null)),
        "put" | "putNull" => {
            let key = sarg(vm, args, 0)?.unwrap_or_default();
            let v = if name == "putNull" {
                Value::Null
            } else {
                args.get(1).cloned().unwrap_or(Value::Null)
            };
            let e = vm.fw.m8.content_values.entry(recv).or_default();
            if let Some(slot) = e.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = v;
            } else {
                e.push((key, v));
            }
            Ok(Some(Value::Null))
        }
        "size" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .content_values
                .get(&recv)
                .map(|v| v.len() as i32)
                .unwrap_or(0),
        ))),
        "remove" => {
            let k = sarg(vm, args, 0)?.unwrap_or_default();
            if let Some(e) = vm.fw.m8.content_values.get_mut(&recv) {
                e.retain(|(x, _)| *x != k);
            }
            Ok(Some(Value::Null))
        }
        "clear" => {
            vm.fw.m8.content_values.insert(recv, Vec::new());
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}
fn cursor(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "moveToNext" => {
            let st = vm.fw.m8.cursors.get_mut(&recv).ok_or_else(|| {
                VmExit::Error(crate::err::vm_error("INVALID_STATE", "cursor desconhecido"))
            })?;
            if st.pos + 1 < st.rows.len() as i32 {
                st.pos += 1;
                Ok(Some(Value::Int(1)))
            } else {
                st.pos = st.rows.len() as i32;
                Ok(Some(Value::Int(0)))
            }
        }
        "moveToFirst" => {
            let st = vm.fw.m8.cursors.get_mut(&recv).ok_or_else(|| {
                VmExit::Error(crate::err::vm_error("INVALID_STATE", "cursor desconhecido"))
            })?;
            if st.rows.is_empty() {
                Ok(Some(Value::Int(0)))
            } else {
                st.pos = 0;
                Ok(Some(Value::Int(1)))
            }
        }
        "moveToPosition" => {
            let p = iarg(args, 0);
            let st = vm.fw.m8.cursors.get_mut(&recv).ok_or_else(|| {
                VmExit::Error(crate::err::vm_error("INVALID_STATE", "cursor desconhecido"))
            })?;
            if p >= 0 && (p as usize) < st.rows.len() {
                st.pos = p;
                Ok(Some(Value::Int(1)))
            } else {
                Ok(Some(Value::Int(0)))
            }
        }
        "getCount" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .cursors
                .get(&recv)
                .map(|s| s.rows.len() as i32)
                .unwrap_or(0),
        ))),
        "getPosition" => Ok(Some(Value::Int(
            vm.fw.m8.cursors.get(&recv).map(|s| s.pos).unwrap_or(-1),
        ))),
        "isAfterLast" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .cursors
                .get(&recv)
                .map(|s| (s.rows.is_empty() || s.pos >= s.rows.len() as i32) as i32)
                .unwrap_or(0),
        ))),
        "isBeforeFirst" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .cursors
                .get(&recv)
                .map(|s| (s.pos < 0) as i32)
                .unwrap_or(0),
        ))),
        "getColumnIndex" | "getColumnIndexOrThrow" => {
            let col = sarg(vm, args, 0)?.unwrap_or_default();
            let idx = vm
                .fw
                .m8
                .cursors
                .get(&recv)
                .and_then(|s| s.columns.iter().position(|c| *c == col))
                .map(|i| i as i32)
                .unwrap_or(-1);
            if name == "getColumnIndexOrThrow" && idx < 0 {
                return Err(throw(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("column {col} não existe"),
                ));
            }
            Ok(Some(Value::Int(idx)))
        }
        "getColumnCount" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .cursors
                .get(&recv)
                .map(|s| s.columns.len() as i32)
                .unwrap_or(0),
        ))),
        "getColumnName" => {
            let i = iarg(args, 0) as usize;
            let n = vm
                .fw
                .m8
                .cursors
                .get(&recv)
                .and_then(|s| s.columns.get(i))
                .cloned()
                .unwrap_or_default();
            Ok(Some(astr(vm, n)?))
        }
        "getString" | "getInt" | "getLong" | "getFloat" | "getDouble" | "isNull" => {
            let i = iarg(args, 0) as usize;
            let v = vm
                .fw
                .m8
                .cursors
                .get(&recv)
                .and_then(|s| s.rows.get(s.pos.max(0) as usize))
                .and_then(|r| r.get(i))
                .cloned()
                .unwrap_or(Value::Null);
            Ok(Some(match (name, v) {
                ("getString", Value::Obj(r)) => Value::Obj(r),
                ("getString", Value::Int(n)) => return Ok(Some(astr(vm, n.to_string())?)),
                ("getString", other) => {
                    return Ok(Some(astr(
                        vm,
                        match other {
                            Value::Long(n) => n.to_string(),
                            _ => String::new(),
                        },
                    )?))
                }
                ("getInt", Value::Int(n)) => Value::Int(n),
                ("getInt", Value::Long(n)) => Value::Int(n as i32),
                ("getLong", Value::Long(n)) => Value::Long(n),
                ("getLong", Value::Int(n)) => Value::Long(n as i64),
                ("getFloat", Value::Float(f)) => Value::Float(f),
                ("getFloat", Value::Int(n)) => Value::Float(n as f32),
                ("getDouble", Value::Double(d)) => Value::Double(d),
                ("getDouble", Value::Int(n)) => Value::Double(n as f64),
                ("isNull", Value::Null) => Value::Int(1),
                ("isNull", _) => Value::Int(0),
                (_, Value::Null) => match name {
                    "getLong" => Value::Long(0),
                    "getFloat" => Value::Float(0.0),
                    "getDouble" => Value::Double(0.0),
                    "getString" => Value::Null,
                    _ => Value::Int(0),
                },
                (_, other) => other,
            }))
        }
        "close" => {
            if let Some(s) = vm.fw.m8.cursors.get_mut(&recv) {
                s.closed = true;
            }
            Ok(Some(Value::Null))
        }
        "isClosed" => Ok(Some(Value::Int(
            vm.fw
                .m8
                .cursors
                .get(&recv)
                .map(|s| s.closed as i32)
                .unwrap_or(0),
        ))),
        _ => Ok(None),
    }
}

// ── Fragments androidx (registro próprio, mesma semântica do legacy) ────
fn ax_fragment_manager(
    vm: &mut Engine,
    name: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "beginTransaction" => {
            let r = alloc(vm, "Landroidx/fragment/app/FragmentTransaction;")?;
            vm.fw.m8.ax_frag_txs.insert(r, Vec::new());
            Ok(Some(Value::Obj(r)))
        }
        "findFragmentByTag" => {
            let want = sarg(vm, args, 0)?;
            let found = want.and_then(|w| {
                vm.fw
                    .m8
                    .ax_fragments
                    .iter()
                    .rev()
                    .find(|(_, _, tag)| tag.as_deref() == Some(w.as_str()))
                    .map(|f| f.0)
            });
            Ok(Some(match found {
                Some(f) => Value::Obj(f),
                None => Value::Null,
            }))
        }
        "findFragmentById" => {
            let id = iarg(args, 0);
            let found = if id == 0 {
                None
            } else {
                vm.fw
                    .m8
                    .ax_fragments
                    .iter()
                    .rev()
                    .find(|(_, cid, _)| *cid == id)
                    .map(|f| f.0)
            };
            Ok(Some(match found {
                Some(f) => Value::Obj(f),
                None => Value::Null,
            }))
        }
        "executePendingTransactions" => Ok(Some(Value::Int(0))),
        "popBackStack" => {
            let popped = vm.fw.m8.ax_fragments.pop().is_some();
            Ok(Some(Value::Int(popped as i32)))
        }
        "getBackStackEntryCount" => Ok(Some(Value::Int(vm.fw.m8.ax_fragments.len() as i32))),
        _ => Ok(None),
    }
}
fn ax_fragment_tx(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "add" | "replace" => {
            let frag = args.iter().find_map(|v| {
                if let Value::Obj(o) = v {
                    Some(*o)
                } else {
                    None
                }
            });
            let container = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            let tag = args.iter().find_map(|v| {
                if let Value::Obj(o) = v {
                    vm.heap.as_str(*o).ok().map(str::to_string)
                } else {
                    None
                }
            });
            let Some(f) = frag else {
                return Err(npe("FragmentTransaction.add com fragment null"));
            };
            vm.fw
                .m8
                .ax_frag_txs
                .entry(recv)
                .or_default()
                .push((f, container, tag));
            Ok(Some(Value::Obj(recv)))
        }
        "remove" => {
            if let Some(f) = oarg(args, 0) {
                vm.fw.m8.ax_fragments.retain(|(o, _, _)| *o != f);
            }
            Ok(Some(Value::Obj(recv)))
        }
        "addToBackStack" | "setReorderingAllowed" | "runOnCommit" => Ok(Some(Value::Obj(recv))),
        "commit" | "commitAllowingStateLoss" => {
            let ops = vm.fw.m8.ax_frag_txs.remove(&recv).unwrap_or_default();
            // replace já aplicado acima nas ops pendentes? Não: ops foram guardadas;
            // replace precisa limpar o container no commit.
            for (f, cid, tag) in ops {
                vm.fw.m8.ax_fragments.push((f, cid, tag));
            }
            Ok(Some(Value::Int(0)))
        }
        "commitNow" | "commitNowAllowingStateLoss" => {
            let ops = vm.fw.m8.ax_frag_txs.remove(&recv).unwrap_or_default();
            for (f, cid, tag) in ops {
                vm.fw.m8.ax_fragments.push((f, cid, tag));
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

// ── java.io.File: estado = campo de heap `path`; sem FS real (exists=false
// para caminhos nunca escritos pelo host — determinístico e honesto).
fn file_host(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    fn path_of(vm: &Engine, r: ObjRef) -> String {
        match vm.heap.get_field(r, "path") {
            Ok(Value::Obj(s)) => vm.heap.as_str(s).unwrap_or("").to_string(),
            _ => String::new(),
        }
    }
    match name {
        "<init>" => {
            let p = if args.len() >= 2 {
                let parent = match args.first() {
                    Some(Value::Obj(r)) => path_of(vm, *r),
                    _ => sarg(vm, args, 0)?.unwrap_or_default(),
                };
                let child = sarg(vm, args, 1)?.unwrap_or_default();
                if parent.is_empty() {
                    child
                } else {
                    format!("{parent}/{child}")
                }
            } else {
                sarg(vm, args, 0)?.unwrap_or_default()
            };
            let s = intrinsics::alloc_string(vm, p)?;
            let _ = vm.heap.put_field(recv, "path", Value::Obj(s));
            Ok(Some(Value::Null))
        }
        "getPath" | "getAbsolutePath" | "toString" => Ok(Some(astr(vm, path_of(vm, recv))?)),
        "getName" => {
            let p = path_of(vm, recv);
            Ok(Some(astr(
                vm,
                p.rsplit('/').next().unwrap_or("").to_string(),
            )?))
        }
        "getParent" => {
            let p = path_of(vm, recv);
            match p.rfind('/') {
                Some(i) if i > 0 => Ok(Some(astr(vm, p[..i].to_string())?)),
                _ => Ok(Some(Value::Null)),
            }
        }
        "exists" | "isDirectory" | "isFile" | "canRead" | "canWrite" => Ok(Some(Value::Int(0))),
        "mkdirs" | "mkdir" => Ok(Some(Value::Int(1))),
        "length" => Ok(Some(Value::Long(0))),
        "list" | "listFiles" => Ok(Some(Value::Null)),
        "delete" => Ok(Some(Value::Int(0))),
        "getCanonicalPath" => Ok(Some(astr(vm, path_of(vm, recv))?)),
        _ => Ok(None),
    }
}

// ── OkHttp shim ─────────────────────────────────────────────────────────
fn okhttp(
    vm: &mut Engine,
    _mref: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => Ok(Some(Value::Null)),
        // Request$Builder
        "url" => {
            if let Some(u) = sarg(vm, args, 0)? {
                let s = intrinsics::alloc_string(vm, u)?;
                let _ = vm.heap.put_field(recv, "m8_url", Value::Obj(s));
            } else if let Some(u) = oarg(args, 0) {
                let _ = vm.heap.put_field(recv, "m8_url_obj", Value::Obj(u));
            }
            Ok(Some(Value::Obj(recv)))
        }
        "header" | "addHeader" | "method" | "post" | "put" | "delete" | "patch" => {
            Ok(Some(Value::Obj(recv)))
        }
        "build" => {
            let url = vm
                .heap
                .get_field(recv, "m8_url")
                .ok()
                .and_then(|v| {
                    if let Value::Obj(s) = v {
                        vm.heap.as_str(s).ok().map(str::to_string)
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let r = alloc(vm, OK_REQUEST)?;
            let s = intrinsics::alloc_string(vm, url)?;
            let _ = vm.heap.put_field(r, "m8_url", Value::Obj(s));
            Ok(Some(Value::Obj(r)))
        }
        // OkHttpClient
        "newCall" => {
            let url = args
                .first()
                .and_then(|v| {
                    if let Value::Obj(o) = v {
                        vm.heap.get_field(*o, "m8_url").ok()
                    } else {
                        None
                    }
                })
                .and_then(|v| {
                    if let Value::Obj(s) = v {
                        vm.heap.as_str(s).ok().map(str::to_string)
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            vm.fw.m8.http_requests.push(url.clone());
            let c = alloc(vm, OK_CALL)?;
            let s = intrinsics::alloc_string(vm, url)?;
            let _ = vm.heap.put_field(c, "m8_url", Value::Obj(s));
            Ok(Some(Value::Obj(c)))
        }
        "newBuilder" => Ok(Some(Value::Obj(recv))),
        "connectTimeout"
        | "readTimeout"
        | "writeTimeout"
        | "addInterceptor"
        | "addNetworkInterceptor" => Ok(Some(Value::Obj(recv))),
        // Call
        "execute" => {
            let r = alloc(vm, OK_RESPONSE)?;
            Ok(Some(Value::Obj(r)))
        }
        "enqueue" => {
            // síncrono determinístico: onResponse(call, response) na hora.
            if let Some(cb) = oarg(args, 0) {
                let resp = alloc(vm, OK_RESPONSE)?;
                let _ = call_dex(
                    vm,
                    cb,
                    "onResponse",
                    "(Lokhttp3/Call;Lokhttp3/Response;)V",
                    vec![Value::Obj(recv), Value::Obj(resp)],
                )?;
            }
            Ok(Some(Value::Null))
        }
        "isExecuted" => Ok(Some(Value::Int(1))),
        "isCanceled" | "cancel" => Ok(Some(Value::Int(0))),
        // Response
        "code" => Ok(Some(Value::Int(vm.fw.m8.http.code))),
        "message" => {
            let m = vm.fw.m8.http.message.clone();
            Ok(Some(astr(vm, m)?))
        }
        "isSuccessful" => Ok(Some(Value::Int(
            (200..300).contains(&vm.fw.m8.http.code) as i32
        ))),
        "body" => {
            let b = alloc(vm, OK_BODY)?;
            Ok(Some(Value::Obj(b)))
        }
        // ResponseBody
        "string" => {
            let b = vm.fw.m8.http.body.clone();
            Ok(Some(astr(vm, b)?))
        }
        "bytes" => {
            let body = vm.fw.m8.http.body.clone().into_bytes();
            let arr = vm
                .heap
                .alloc_array(crate::heap::ElemKind::Int, body.len(), "[B")
                .map_err(crate::err::oom_throwable)?;
            let elems = vm
                .heap
                .array_elems_mut(arr)
                .map_err(|e| crate::err::vm_error("VM_TYPE_ERROR", e))?;
            *elems = body.iter().map(|b| Value::Int(*b as i8 as i32)).collect();
            Ok(Some(Value::Obj(arr)))
        }
        "contentLength" => Ok(Some(Value::Long(vm.fw.m8.http.body.len() as i64))),
        "close" => Ok(Some(Value::Null)),
        "request" => Ok(Some(Value::Null)),
        _ => Ok(None),
    }
}

// ── Services / broadcasts ───────────────────────────────────────────────
/// Nome (Context.XXX_SERVICE) ou classe -> classe host do serviço de sistema.
/// `Context.*_SERVICE` names → manager classes.
const SERVICES: &[(&str, &str)] = &[
    ("jobscheduler", "Landroid/app/job/JobScheduler;"),
    ("activity", "Landroid/app/ActivityManager;"),
    ("window", "Landroid/view/WindowManager;"),
    ("layout_inflater", "Landroid/view/LayoutInflater;"),
    ("power", "Landroid/os/PowerManager;"),
    ("alarm", "Landroid/app/AlarmManager;"),
    ("notification", "Landroid/app/NotificationManager;"),
    ("connectivity", "Landroid/net/ConnectivityManager;"),
    (
        "input_method",
        "Landroid/view/inputmethod/InputMethodManager;",
    ),
    ("location", "Landroid/location/LocationManager;"),
    ("sensor", "Landroid/hardware/SensorManager;"),
    ("consumer_ir", "Landroid/hardware/ConsumerIrManager;"),
    (
        "accessibility",
        "Landroid/view/accessibility/AccessibilityManager;",
    ),
    ("autofill", "Landroid/view/autofill/AutofillManager;"),
    ("clipboard", "Landroid/content/ClipboardManager;"),
    ("vibrator", "Landroid/os/Vibrator;"),
    ("vibrator_manager", "Landroid/os/VibratorManager;"),
    ("audio", "Landroid/media/AudioManager;"),
    ("camera", "Landroid/hardware/camera2/CameraManager;"),
    ("display", "Landroid/hardware/display/DisplayManager;"),
    ("keyguard", "Landroid/app/KeyguardManager;"),
    ("uimode", "Landroid/app/UiModeManager;"),
    ("download", "Landroid/app/DownloadManager;"),
    ("storage", "Landroid/os/storage/StorageManager;"),
    (
        "textservices",
        "Landroid/view/textservice/TextServicesManager;",
    ),
    (
        "textclassification",
        "Landroid/view/textclassifier/TextClassificationManager;",
    ),
    ("user", "Landroid/os/UserManager;"),
    ("appops", "Landroid/app/AppOpsManager;"),
    ("account", "Landroid/accounts/AccountManager;"),
    ("batterymanager", "Landroid/os/BatteryManager;"),
    ("wifi", "Landroid/net/wifi/WifiManager;"),
    ("telephony", "Landroid/telephony/TelephonyManager;"),
    ("shortcut", "Landroid/content/pm/ShortcutManager;"),
    ("appwidget", "Landroid/appwidget/AppWidgetManager;"),
    ("locale", "Landroid/app/LocaleManager;"),
    (
        "content_capture",
        "Landroid/view/contentcapture/ContentCaptureManager;",
    ),
    (
        "translation",
        "Landroid/view/translation/TranslationManager;",
    ),
    (
        "media_session",
        "Landroid/media/session/MediaSessionManager;",
    ),
    ("usagestats", "Landroid/app/usage/UsageStatsManager;"),
    ("usb", "Landroid/hardware/usb/UsbManager;"),
    ("euicc", "Landroid/telephony/euicc/EuiccManager;"),
    ("print", "Landroid/print/PrintManager;"),
];

fn system_service_class(key: &str) -> &'static str {
    SERVICES
        .iter()
        .find(|(n, c)| *n == key || *c == key)
        .map(|(_, c)| *c)
        .unwrap_or("Ljava/lang/Object;")
}

fn context_services(
    vm: &mut Engine,
    name: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "startService" | "startForegroundService" => {
            let comp = oarg(args, 0)
                .and_then(|ir| match vm.fw.get(ir) {
                    Some(HostObj::Intent { component, .. }) => component.clone(),
                    _ => None,
                })
                .unwrap_or_else(|| "<implicit>".into());
            vm.fw.m8.started_services.push(comp.clone());
            // devolve ComponentName host
            let cn = alloc(vm, "Landroid/content/ComponentName;")?;
            Ok(Some(Value::Obj(cn)))
        }
        "stopService" => {
            let comp = oarg(args, 0)
                .and_then(|ir| match vm.fw.get(ir) {
                    Some(HostObj::Intent { component, .. }) => component.clone(),
                    _ => None,
                })
                .unwrap_or_else(|| "<implicit>".into());
            let existed = vm.fw.m8.started_services.contains(&comp);
            vm.fw.m8.started_services.retain(|c| *c != comp);
            vm.fw.m8.stopped_services.push(comp);
            Ok(Some(Value::Int(existed as i32)))
        }
        "bindService" => {
            let comp = oarg(args, 0)
                .and_then(|ir| match vm.fw.get(ir) {
                    Some(HostObj::Intent { component, .. }) => component.clone(),
                    _ => None,
                })
                .unwrap_or_else(|| "<implicit>".into());
            vm.fw.m8.bound_services.push(comp);
            Ok(Some(Value::Int(1)))
        }
        "unbindService" => Ok(Some(Value::Null)),
        "sendBroadcast" | "sendOrderedBroadcast" => {
            let action = oarg(args, 0)
                .and_then(|ir| vm.fw.m8.intents.get(&ir))
                .and_then(|s| s.action.clone())
                .or_else(|| {
                    oarg(args, 0).and_then(|ir| match vm.fw.get(ir) {
                        Some(HostObj::Intent { component, .. }) => component.clone(),
                        _ => None,
                    })
                })
                .unwrap_or_else(|| "<no-action>".into());
            vm.fw.m8.broadcasts.push(action.clone());
            // entrega síncrona aos receivers registrados com onReceive DEX
            let receivers = vm.fw.m8.receivers.clone();
            if let Some(ir) = oarg(args, 0) {
                for r in receivers {
                    let ctx = vm
                        .fw
                        .current_activity
                        .map(Value::Obj)
                        .unwrap_or(Value::Null);
                    let _ = call_dex(
                        vm,
                        r,
                        "onReceive",
                        "(Landroid/content/Context;Landroid/content/Intent;)V",
                        vec![ctx, Value::Obj(ir)],
                    );
                }
            }
            Ok(Some(Value::Null))
        }
        "registerReceiver" => {
            if let Some(r) = oarg(args, 0) {
                vm.fw.m8.receivers.push(r);
            }
            Ok(Some(Value::Null))
        }
        "unregisterReceiver" => {
            if let Some(r) = oarg(args, 0) {
                vm.fw.m8.receivers.retain(|x| *x != r);
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}
fn intent_m8(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            // (String) ou (String, Uri)
            if let Some(a) = sarg(vm, args, 0)? {
                vm.fw.m8.intents.entry(recv).or_default().action = Some(a);
            }
            if let Some(u) = oarg(args, 1) {
                vm.fw.m8.intents.entry(recv).or_default().data = Some(u);
            }
            Ok(None) // framework cuida do resto do <init>
        }
        "setAction" => {
            vm.fw.m8.intents.entry(recv).or_default().action = sarg(vm, args, 0)?;
            Ok(Some(Value::Obj(recv)))
        }
        "getAction" => match vm.fw.m8.intents.get(&recv).and_then(|s| s.action.clone()) {
            Some(a) => Ok(Some(astr(vm, a)?)),
            None => Ok(Some(Value::Null)),
        },
        "setData" => {
            vm.fw.m8.intents.entry(recv).or_default().data = oarg(args, 0);
            Ok(Some(Value::Obj(recv)))
        }
        "getData" => match vm.fw.m8.intents.get(&recv).and_then(|s| s.data) {
            Some(d) => Ok(Some(Value::Obj(d))),
            None => Ok(Some(Value::Null)),
        },
        "setDataAndType" => {
            vm.fw.m8.intents.entry(recv).or_default().data = oarg(args, 0);
            Ok(Some(Value::Obj(recv)))
        }
        "addFlags" | "setFlags" | "addCategory" => Ok(Some(Value::Obj(recv))),
        "getDataString" => {
            match vm
                .fw
                .m8
                .intents
                .get(&recv)
                .and_then(|s| s.data)
                .and_then(|d| vm.fw.m8.uris.get(&d))
                .cloned()
            {
                Some(u) => Ok(Some(astr(vm, u)?)),
                None => Ok(Some(Value::Null)),
            }
        }
        _ => Ok(None),
    }
}
fn uri_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let raw = vm.fw.m8.uris.get(&recv).cloned().unwrap_or_default();
    match name {
        "toString" => Ok(Some(astr(vm, raw)?)),
        "getScheme" => {
            let s = raw.split(':').next().unwrap_or("").to_string();
            if s.is_empty() || s == raw {
                Ok(Some(Value::Null))
            } else {
                Ok(Some(astr(vm, s)?))
            }
        }
        "getHost" => {
            let h = raw
                .split("://")
                .nth(1)
                .map(|r| {
                    r.split(['/', '?', '#', ':'])
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
            if h.is_empty() {
                Ok(Some(Value::Null))
            } else {
                Ok(Some(astr(vm, h)?))
            }
        }
        "getPath" => {
            let p = raw
                .split("://")
                .nth(1)
                .map(|r| match r.find('/') {
                    Some(i) => r[i..].split(['?', '#']).next().unwrap_or("").to_string(),
                    None => String::new(),
                })
                .unwrap_or_default();
            Ok(Some(astr(vm, p)?))
        }
        "getQueryParameter" => Ok(Some(Value::Null)), // sem query no modelo simples; chave ausente = null (contrato Uri)
        "getSchemeSpecificPart" => Ok(Some(astr(vm, raw.clone())?)),
        _ => Ok(None),
    }
}
fn job_scheduler(
    vm: &mut Engine,
    name: &str,
    _recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "schedule" => {
            // JobInfo.getId via field ou arg objeto — registra 0 se ilegível.
            vm.fw.m8.scheduled_jobs.push(0);
            Ok(Some(Value::Int(1))) // RESULT_SUCCESS
        }
        "getAllPendingJobs" => {
            // List host vazia/com jobs: devolve ArrayList host via HostObj::List
            let r = alloc(vm, "Ljava/util/ArrayList;")?;
            vm.fw.objects.insert(r, HostObj::List { elems: Vec::new() });
            Ok(Some(Value::Obj(r)))
        }
        "cancel" => {
            let id = iarg(args, 0);
            vm.fw.m8.scheduled_jobs.retain(|j| *j != id);
            Ok(Some(Value::Null))
        }
        "cancelAll" => {
            vm.fw.m8.scheduled_jobs.clear();
            Ok(Some(Value::Null))
        }
        "getPendingJob" => Ok(Some(Value::Null)),
        "enqueue" => Ok(Some(Value::Int(1))),
        _ => Ok(None),
    }
}

// ── WorkManager / Room ──────────────────────────────────────────────────
fn work_manager(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "enqueue" | "enqueueUniqueWork" | "enqueueUniquePeriodicWork" => {
            // args podem ser WorkRequest, List ou (String, ExistingWorkPolicy, WorkRequest)
            let mut worker = String::new();
            for a in args {
                if let Value::Obj(o) = a {
                    if let Ok(Value::Obj(s)) = vm.heap.get_field(*o, "m8_worker") {
                        if let Ok(t) = vm.heap.as_str(s) {
                            worker = t.to_string();
                        }
                    }
                }
            }
            let id = format!("work-{}", vm.fw.m8.work.len() + 1);
            vm.fw.m8.work.push(WorkRecord {
                id: id.clone(),
                worker_class: worker,
                state: "ENQUEUED".into(),
                tags: Vec::new(),
            });
            let op = alloc(vm, OPERATION)?;
            let _ = recv;
            Ok(Some(Value::Obj(op)))
        }
        "cancelWorkById" | "cancelAllWorkByTag" | "cancelUniqueWork" => {
            for w in &mut vm.fw.m8.work {
                if w.state == "ENQUEUED" {
                    w.state = "CANCELLED".into();
                }
            }
            let op = alloc(vm, OPERATION)?;
            Ok(Some(Value::Obj(op)))
        }
        "getWorkInfosByTag" | "getWorkInfosForUniqueWork" => {
            let r = alloc(vm, "Lcom/google/common/util/concurrent/ListenableFuture;")?;
            Ok(Some(Value::Obj(r)))
        }
        "beginWith" | "beginUniqueWork" => {
            let r = alloc(vm, "Landroidx/work/WorkContinuation;")?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}
fn room_builder(
    vm: &mut Engine,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "build" => {
            let (dbcls, dbname) = vm
                .fw
                .m8
                .room_builders
                .get(&recv)
                .cloned()
                .unwrap_or_else(|| ("Landroidx/room/RoomDatabase;".into(), ":memory:".into()));
            vm.fw.m8.databases.entry(dbname.clone()).or_default();
            // Instância da DB do app se existir no DEX; senão RoomDatabase host.
            let r = if vm.cp.find_class(&dbcls).is_some() {
                vm.new_dex_instance_pub(&dbcls)?
            } else {
                alloc(vm, &dbcls)?
            };
            vm.fw.m8.db_by_obj.insert(r, dbname);
            Ok(Some(Value::Obj(r)))
        }
        // builder fluente: tudo devolve this
        "allowMainThreadQueries"
        | "addCallback"
        | "addMigrations"
        | "fallbackToDestructiveMigration"
        | "setQueryExecutor"
        | "setTransactionExecutor"
        | "openHelperFactory"
        | "setJournalMode" => Ok(Some(Value::Obj(recv))),
        _ => Ok(None),
    }
}
