//! Apêndice D — AndroidX/Jetpack host shims por módulo (registro:
//! `data/jetpack.toml`, validado por `rd-framework::fd_jetpack`).
//!
//! Princípio (o mesmo do M8): bibliotecas AndroidX **viajam no APK**.
//! O host só sequestra o que modela de verdade — e, para classes com
//! corpo DEX, o padrão é devolver `None` e deixar a biblioteca real
//! executar. O shim host entra quando (a) o objeto já tem estado host,
//! (b) o APK não fornece o corpo DEX (fixtures sintéticas, bibliotecas
//! removidas por shrink), ou (c) a API é uma fachada para um serviço que
//! não existe em sandbox (FileProvider, BiometricPrompt, Custom Tabs,
//! MultiDex.install) — nesses casos a resposta é estruturada,
//! determinística, programável pelo agente e **consultável** via
//! `Engine::androidx_*`. Nada é no-op mudo: lançamentos, URIs, prompts
//! biométricos, navegação e inicializações ficam registrados.
//!
//! Desvios documentados (nunca silenciosos): VM single-thread
//! determinística — `postValue` equivale a `setValue`, listeners de
//! futures rodam inline (executor ignorado), `observe` só reentrega em
//! `setValue`/no registro (ativação por transição de lifecycle não
//! reentrega sozinha), FileProvider não valida o XML de paths,
//! DataStore modela a camada de valores `Preferences` (o `Flow`/`edit`
//! suspenso continua gap de coroutines), e `NavController` é uma pilha
//! de ids sem inflar o grafo. Compose (UI-05) continua
//! `not_implemented` no registro — ver `data/jetpack.toml`.

use std::collections::{BTreeMap, HashMap};

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::intrinsics;
use crate::value::Value;

// ── classes ─────────────────────────────────────────────────────────────
pub const LIFECYCLE: &str = "Landroidx/lifecycle/Lifecycle;";
pub const LIFECYCLE_REGISTRY: &str = "Landroidx/lifecycle/LifecycleRegistry;";
pub const LIFECYCLE_STATE: &str = "Landroidx/lifecycle/Lifecycle$State;";
pub const LIFECYCLE_EVENT: &str = "Landroidx/lifecycle/Lifecycle$Event;";
pub const LIVEDATA: &str = "Landroidx/lifecycle/LiveData;";
pub const MUTABLE_LIVEDATA: &str = "Landroidx/lifecycle/MutableLiveData;";
pub const MEDIATOR_LIVEDATA: &str = "Landroidx/lifecycle/MediatorLiveData;";
pub const VIEW_MODEL_STORE: &str = "Landroidx/lifecycle/ViewModelStore;";
pub const VIEW_MODEL_PROVIDER: &str = "Landroidx/lifecycle/ViewModelProvider;";
pub const SAVED_STATE_HANDLE: &str = "Landroidx/lifecycle/SavedStateHandle;";
pub const ARRAY_MAP: &str = "Landroidx/collection/ArrayMap;";
pub const SIMPLE_ARRAY_MAP: &str = "Landroidx/collection/SimpleArrayMap;";
pub const ARRAY_SET: &str = "Landroidx/collection/ArraySet;";
pub const SPARSE_ARRAY_COMPAT: &str = "Landroidx/collection/SparseArrayCompat;";
pub const LONG_SPARSE_ARRAY: &str = "Landroidx/collection/LongSparseArray;";
pub const LRU_CACHE: &str = "Landroidx/collection/LruCache;";
pub const PREFERENCES: &str = "Landroidx/datastore/preferences/core/Preferences;";
pub const MUTABLE_PREFERENCES: &str = "Landroidx/datastore/preferences/core/MutablePreferences;";
pub const PREFERENCES_KEY: &str = "Landroidx/datastore/preferences/core/Preferences$Key;";
pub const WORK_DATA: &str = "Landroidx/work/Data;";
pub const WORK_DATA_BUILDER: &str = "Landroidx/work/Data$Builder;";
pub const NAV_CONTROLLER: &str = "Landroidx/navigation/NavController;";
pub const NAV_DESTINATION: &str = "Landroidx/navigation/NavDestination;";
pub const APP_INITIALIZER: &str = "Landroidx/startup/AppInitializer;";
pub const FILE_PROVIDER: &str = "Landroidx/core/content/FileProvider;";
pub const CONTEXT_COMPAT: &str = "Landroidx/core/content/ContextCompat;";
pub const ACTIVITY_COMPAT: &str = "Landroidx/core/app/ActivityCompat;";
pub const CANCELLATION_SIGNAL: &str = "Landroidx/core/os/CancellationSignal;";
pub const MULTIDEX: &str = "Landroidx/multidex/MultiDex;";
pub const BIOMETRIC_MANAGER: &str = "Landroidx/biometric/BiometricManager;";
pub const BIOMETRIC_PROMPT: &str = "Landroidx/biometric/BiometricPrompt;";
pub const PROMPT_INFO: &str = "Landroidx/biometric/BiometricPrompt$PromptInfo;";
pub const PROMPT_INFO_BUILDER: &str = "Landroidx/biometric/BiometricPrompt$PromptInfo$Builder;";
pub const AUTH_RESULT: &str = "Landroidx/biometric/BiometricPrompt$AuthenticationResult;";
pub const CUSTOM_TABS_INTENT: &str = "Landroidx/browser/customtabs/CustomTabsIntent;";
pub const CUSTOM_TABS_BUILDER: &str = "Landroidx/browser/customtabs/CustomTabsIntent$Builder;";
pub const LISTENABLE_FUTURE: &str = "Lcom/google/common/util/concurrent/ListenableFuture;";
pub const SETTABLE_FUTURE: &str = "Lcom/google/common/util/concurrent/SettableFuture;";

const LIFECYCLE_STATES: [&str; 5] = ["DESTROYED", "INITIALIZED", "CREATED", "STARTED", "RESUMED"];
const LIFECYCLE_EVENTS: [&str; 7] = [
    "ON_CREATE",
    "ON_START",
    "ON_RESUME",
    "ON_PAUSE",
    "ON_STOP",
    "ON_DESTROY",
    "ON_ANY",
];

/// `BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE` — o default honesto:
/// nenhum hardware biométrico existe até o agente programar um.
pub const BIOMETRIC_ERROR_NO_HARDWARE: i32 = 12;
/// `BiometricManager.BIOMETRIC_SUCCESS`.
pub const BIOMETRIC_SUCCESS: i32 = 0;
/// `BiometricPrompt.ERROR_HW_NOT_PRESENT`.
pub const BIOMETRIC_ERROR_HW_NOT_PRESENT: i32 = 12;

// ── estado ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Default)]
pub struct LifecycleHost {
    pub owner: Option<ObjRef>,
    pub state: String,
    pub observers: Vec<ObjRef>,
}

#[derive(Debug, Clone)]
pub struct LiveObserver {
    pub owner: Option<ObjRef>,
    pub registry: Option<ObjRef>,
    pub observer: ObjRef,
    pub last_version: i32,
}

#[derive(Debug, Clone)]
pub struct LiveDataHost {
    pub value: Value,
    pub version: i32,
    pub observers: Vec<LiveObserver>,
    /// As flags do AOSP (`mDispatchingValue`/`mDispatchInvalidated`):
    /// um `setValue` dentro de `onChanged` marca invalidação e a
    /// entrega recomeça iterativamente — sem elas, o despacho síncrono
    /// recursia até ao SIGABRT do hospedeiro.
    pub dispatching: bool,
    pub dispatch_invalidated: bool,
}

impl Default for LiveDataHost {
    fn default() -> Self {
        Self {
            value: Value::Null,
            version: -1,
            observers: Vec::new(),
            dispatching: false,
            dispatch_invalidated: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LruHost {
    pub max_size: i32,
    /// (key, value, size) — frente = menos recente.
    pub entries: Vec<(Value, Value, i32)>,
    pub size: i32,
    pub hit_count: i32,
    pub miss_count: i32,
    pub put_count: i32,
    pub eviction_count: i32,
}

#[derive(Debug, Clone, Default)]
pub struct NavHost {
    pub stack: Vec<i32>,
    pub graph_id: i32,
}

#[derive(Debug, Clone, Default)]
pub struct CancelHost {
    pub canceled: bool,
    pub listener: Option<ObjRef>,
}

#[derive(Debug, Clone, Default)]
pub struct PromptInfo {
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub negative: String,
}

/// Resultado biométrico programável pelo agente.
#[derive(Debug, Clone)]
pub enum BiometricOutcome {
    Success,
    Failed,
    Error(i32, String),
}

#[derive(Debug, Clone)]
pub struct FutureHost {
    pub done: bool,
    pub cancelled: bool,
    pub result: Value,
    pub error: Option<String>,
    pub listeners: Vec<ObjRef>,
}

impl Default for FutureHost {
    fn default() -> Self {
        Self {
            done: false,
            cancelled: false,
            result: Value::Null,
            error: None,
            listeners: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AndroidxState {
    pub lifecycles: HashMap<ObjRef, LifecycleHost>,
    pub lifecycle_state_objs: HashMap<String, ObjRef>,
    pub lifecycle_event_objs: HashMap<String, ObjRef>,
    pub state_name_by_obj: HashMap<ObjRef, String>,
    pub event_name_by_obj: HashMap<ObjRef, String>,
    pub livedata: HashMap<ObjRef, LiveDataHost>,
    pub vm_stores: HashMap<ObjRef, Vec<(String, ObjRef)>>,
    pub vm_providers: HashMap<ObjRef, (Option<ObjRef>, Option<ObjRef>)>,
    pub saved_state: HashMap<ObjRef, Vec<(String, Value)>>,
    pub array_maps: HashMap<ObjRef, Vec<(Value, Value)>>,
    pub array_sets: HashMap<ObjRef, Vec<Value>>,
    pub sparse: HashMap<ObjRef, BTreeMap<i64, Value>>,
    pub lru: HashMap<ObjRef, LruHost>,
    pub pref_keys: HashMap<ObjRef, String>,
    pub preferences: HashMap<ObjRef, Vec<(String, Value)>>,
    pub work_data: HashMap<ObjRef, Vec<(String, Value)>>,
    pub nav: HashMap<ObjRef, NavHost>,
    pub nav_dest_ids: HashMap<ObjRef, i32>,
    pub last_nav_controller: Option<ObjRef>,
    pub app_initializer: Option<ObjRef>,
    pub startup_initialized: Vec<String>,
    pub startup_instances: HashMap<String, Value>,
    pub file_provider_uris: Vec<String>,
    pub cancellation: HashMap<ObjRef, CancelHost>,
    pub biometric_can_authenticate: i32,
    pub biometric_outcome: BiometricOutcome,
    pub biometric_requests: Vec<String>,
    pub prompt_builders: HashMap<ObjRef, PromptInfo>,
    pub prompt_infos: HashMap<ObjRef, PromptInfo>,
    pub biometric_prompts: HashMap<ObjRef, Option<ObjRef>>,
    pub custom_tab_launches: Vec<String>,
    pub multidex_installs: i32,
    pub futures: HashMap<ObjRef, FutureHost>,
}

impl Default for AndroidxState {
    fn default() -> Self {
        Self {
            lifecycles: HashMap::new(),
            lifecycle_state_objs: HashMap::new(),
            lifecycle_event_objs: HashMap::new(),
            state_name_by_obj: HashMap::new(),
            event_name_by_obj: HashMap::new(),
            livedata: HashMap::new(),
            vm_stores: HashMap::new(),
            vm_providers: HashMap::new(),
            saved_state: HashMap::new(),
            array_maps: HashMap::new(),
            array_sets: HashMap::new(),
            sparse: HashMap::new(),
            lru: HashMap::new(),
            pref_keys: HashMap::new(),
            preferences: HashMap::new(),
            work_data: HashMap::new(),
            nav: HashMap::new(),
            nav_dest_ids: HashMap::new(),
            last_nav_controller: None,
            app_initializer: None,
            startup_initialized: Vec::new(),
            startup_instances: HashMap::new(),
            file_provider_uris: Vec::new(),
            cancellation: HashMap::new(),
            biometric_can_authenticate: BIOMETRIC_ERROR_NO_HARDWARE,
            biometric_outcome: BiometricOutcome::Error(
                BIOMETRIC_ERROR_HW_NOT_PRESENT,
                "No biometric hardware (RustyDroid shim default; program via Engine::androidx_set_biometric)".to_string(),
            ),
            biometric_requests: Vec::new(),
            prompt_builders: HashMap::new(),
            prompt_infos: HashMap::new(),
            biometric_prompts: HashMap::new(),
            custom_tab_launches: Vec::new(),
            multidex_installs: 0,
            futures: HashMap::new(),
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────
fn throw(class: &str, msg: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(class, msg.into()))
}
fn npe(what: &str) -> VmExit {
    throw("Ljava/lang/NullPointerException;", what.to_string())
}
fn sarg(vm: &Engine, args: &[Value], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Value::Obj(r)) => vm.heap.as_str(*r).ok().map(str::to_string),
        _ => None,
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
fn larg(args: &[Value], i: usize) -> i64 {
    args.get(i).and_then(|v| v.as_long().ok()).unwrap_or(0)
}
fn alloc(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.to_string(), Vec::new())?)
}
fn astr(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(intrinsics::alloc_string(vm, s)?))
}
fn dex_exists(vm: &mut Engine, class: &str, name: &str, sig: &str) -> bool {
    vm.cp.resolve_method(class, name, sig).is_some()
}
/// Class descriptor carried by a `java.lang.Class` object (m8 pattern).
fn class_desc_of(vm: &Engine, class_obj: Option<ObjRef>) -> Option<String> {
    let c = class_obj?;
    match vm.heap.get_field(c, "class_name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}
fn eq(vm: &Engine, a: &Value, b: &Value) -> Result<bool, VmExit> {
    intrinsics::values_equal(&vm.heap, a, b)
}
fn make_string_set(vm: &mut Engine, items: &[String]) -> Result<Value, VmExit> {
    let mut elems = Vec::with_capacity(items.len());
    for s in items {
        elems.push(Value::Obj(intrinsics::alloc_string(vm, s.clone())?));
    }
    let r = alloc(vm, "Ljava/util/HashSet;")?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}
fn make_map(vm: &mut Engine, entries: Vec<(Value, Value)>) -> Result<Value, VmExit> {
    let r = alloc(vm, "Ljava/util/HashMap;")?;
    vm.fw.objects.insert(r, HostObj::Map { entries });
    Ok(Value::Obj(r))
}
/// Host-map entries of a guest `java.util.Map` object, when host-backed.
fn read_host_map(vm: &Engine, r: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    }
}
fn uri_string(vm: &Engine, r: ObjRef) -> String {
    vm.fw.m8.uris.get(&r).cloned().unwrap_or_default()
}

// ── Engine agent surface ────────────────────────────────────────────────
impl Engine {
    pub fn androidx_lifecycle_state(&self, registry: ObjRef) -> Option<String> {
        self.fw
            .androidx
            .lifecycles
            .get(&registry)
            .map(|h| h.state.clone())
    }
    pub fn androidx_livedata_value(&self, r: ObjRef) -> Option<Value> {
        self.fw.androidx.livedata.get(&r).map(|h| h.value.clone())
    }
    pub fn androidx_vm_store_keys(&self, store: ObjRef) -> Vec<String> {
        self.fw
            .androidx
            .vm_stores
            .get(&store)
            .map(|m| m.iter().map(|(k, _)| k.clone()).collect())
            .unwrap_or_default()
    }
    pub fn androidx_preferences(&self, r: ObjRef) -> Option<Vec<(String, Value)>> {
        self.fw.androidx.preferences.get(&r).cloned()
    }
    pub fn androidx_work_data(&self, r: ObjRef) -> Option<Vec<(String, Value)>> {
        self.fw.androidx.work_data.get(&r).cloned()
    }
    pub fn androidx_nav_stack(&self, controller: ObjRef) -> Option<Vec<i32>> {
        self.fw
            .androidx
            .nav
            .get(&controller)
            .map(|n| n.stack.clone())
    }
    pub fn androidx_last_nav_controller(&self) -> Option<ObjRef> {
        self.fw.androidx.last_nav_controller
    }
    pub fn androidx_startup_initialized(&self) -> &[String] {
        &self.fw.androidx.startup_initialized
    }
    pub fn androidx_file_provider_uris(&self) -> &[String] {
        &self.fw.androidx.file_provider_uris
    }
    /// Programs the virtual biometric device: what `canAuthenticate`
    /// returns and how the next `authenticate()` calls end.
    pub fn androidx_set_biometric(&mut self, can_authenticate: i32, outcome: BiometricOutcome) {
        self.fw.androidx.biometric_can_authenticate = can_authenticate;
        self.fw.androidx.biometric_outcome = outcome;
    }
    pub fn androidx_biometric_requests(&self) -> &[String] {
        &self.fw.androidx.biometric_requests
    }
    pub fn androidx_custom_tab_launches(&self) -> &[String] {
        &self.fw.androidx.custom_tab_launches
    }
    pub fn androidx_multidex_installs(&self) -> i32 {
        self.fw.androidx.multidex_installs
    }
    /// Creates an incomplete host `ListenableFuture` the agent (or a host
    /// API) completes later with [`Engine::androidx_complete_future`].
    pub fn androidx_new_future(&mut self) -> Result<ObjRef, VmExit> {
        let r = alloc(self, LISTENABLE_FUTURE)?;
        self.fw.androidx.futures.insert(r, FutureHost::default());
        Ok(r)
    }
    /// Completes a host future and runs its listeners inline.
    pub fn androidx_complete_future(&mut self, r: ObjRef, result: Value) -> Result<bool, VmExit> {
        self.with_host_roots(|vm| complete_future(vm, r, result, None))
    }
    pub fn androidx_future_error(&mut self, r: ObjRef, message: &str) -> Result<bool, VmExit> {
        let message = message.to_string();
        self.with_host_roots(|vm| complete_future(vm, r, Value::Null, Some(message)))
    }
}

fn complete_future(
    vm: &mut Engine,
    r: ObjRef,
    result: Value,
    error: Option<String>,
) -> Result<bool, VmExit> {
    let listeners = {
        let Some(f) = vm.fw.androidx.futures.get_mut(&r) else {
            return Ok(false);
        };
        if f.done {
            return Ok(false);
        }
        f.done = true;
        f.result = result;
        f.error = error;
        std::mem::take(&mut f.listeners)
    };
    for l in listeners {
        let _ = crate::m8::call_dex(vm, l, "run", "()V", Vec::new())?;
    }
    Ok(true)
}

// ── dispatch estático ───────────────────────────────────────────────────
pub fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_static_rooted(vm, class, name, sig, args))
}

fn call_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        // The VM loads every classes*.dex natively; install() is an
        // explicitly recorded no-op (never the real zip extraction).
        (MULTIDEX, "install") => {
            vm.fw.androidx.multidex_installs += 1;
            Ok(Some(Value::Null))
        }
        (FILE_PROVIDER, "getUriForFile") => file_provider_uri(vm, args).map(Some),
        (APP_INITIALIZER, "getInstance") => {
            if let Some(r) = vm.fw.androidx.app_initializer {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, APP_INITIALIZER)?;
            vm.fw.androidx.app_initializer = Some(r);
            Ok(Some(Value::Obj(r)))
        }
        (LIFECYCLE_STATE, "valueOf") | (LIFECYCLE_EVENT, "valueOf") => {
            if dex_exists(vm, class, name, sig) {
                return Ok(None);
            }
            let wanted = sarg(vm, args, 0).ok_or_else(|| npe("Enum.valueOf(null)"))?;
            let valid: &[&str] = if class == LIFECYCLE_STATE {
                &LIFECYCLE_STATES
            } else {
                &LIFECYCLE_EVENTS
            };
            if !valid.contains(&wanted.as_str()) {
                return Err(throw(
                    "Ljava/lang/IllegalArgumentException;",
                    format!("No enum constant {class}.{wanted}"),
                ));
            }
            if class == LIFECYCLE_STATE {
                state_obj(vm, &wanted).map(|r| Some(Value::Obj(r)))
            } else {
                event_obj(vm, &wanted).map(|r| Some(Value::Obj(r)))
            }
        }
        (BIOMETRIC_MANAGER, "from") => {
            if dex_exists(vm, class, name, sig) {
                return Ok(None);
            }
            alloc(vm, BIOMETRIC_MANAGER).map(|r| Some(Value::Obj(r)))
        }
        (CONTEXT_COMPAT, "checkSelfPermission") | (ACTIVITY_COMPAT, "checkSelfPermission") => {
            if dex_exists(vm, class, name, sig) {
                return Ok(None);
            }
            let Some(perm) = sarg(vm, args, 1) else {
                return Ok(Some(Value::Int(-1)));
            };
            let granted = match &vm.granted_permissions {
                None => true,
                Some(set) => set.contains(&perm),
            };
            Ok(Some(Value::Int(if granted { 0 } else { -1 })))
        }
        _ if class.starts_with("Landroidx/datastore/preferences/core/Preferences")
            && (name.ends_with("PreferencesKey") || name == "emptyPreferences") =>
        {
            if dex_exists(vm, class, name, sig) {
                return Ok(None);
            }
            if name == "emptyPreferences" {
                let r = alloc(vm, PREFERENCES)?;
                vm.fw.androidx.preferences.insert(r, Vec::new());
                return Ok(Some(Value::Obj(r)));
            }
            // stringPreferencesKey/intPreferencesKey/... (Key<T> factories)
            let key_name = sarg(vm, args, 0).ok_or_else(|| npe("Preferences.Key(null name)"))?;
            let r = alloc(vm, PREFERENCES_KEY)?;
            let name_obj = intrinsics::alloc_string(vm, key_name.clone())?;
            let _ = vm.heap.put_field(r, "name", Value::Obj(name_obj));
            vm.fw.androidx.pref_keys.insert(r, key_name);
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

fn file_provider_uri(vm: &mut Engine, args: &[Value]) -> Result<Value, VmExit> {
    let authority = sarg(vm, args, 1).ok_or_else(|| npe("FileProvider authority null"))?;
    let path = match oarg(args, 2) {
        Some(f) => match vm.heap.get_field(f, "path") {
            Ok(Value::Obj(s)) => vm.heap.as_str(s).unwrap_or("").to_string(),
            _ => String::new(),
        },
        None => return Err(npe("FileProvider file null")),
    };
    // RustyDroid does not parse the provider XML paths; the minted URI
    // keeps the guest path under a fixed shim root and is recorded.
    let clean = path.trim_start_matches('/');
    let url = format!("content://{authority}/rustydroid/{clean}");
    let r = alloc(vm, "Landroid/net/Uri;")?;
    vm.fw.m8.uris.insert(r, url.clone());
    vm.fw.androidx.file_provider_uris.push(url);
    Ok(Value::Obj(r))
}

// ── lifecycle ───────────────────────────────────────────────────────────
fn state_ordinal(name: &str) -> i32 {
    LIFECYCLE_STATES
        .iter()
        .position(|s| *s == name)
        .map(|i| i as i32)
        .unwrap_or(1)
}
fn event_target(event: &str) -> &'static str {
    match event {
        "ON_CREATE" | "ON_STOP" => "CREATED",
        "ON_START" | "ON_PAUSE" => "STARTED",
        "ON_RESUME" => "RESUMED",
        "ON_DESTROY" => "DESTROYED",
        _ => "INITIALIZED",
    }
}
fn up_event(from: &str) -> Option<&'static str> {
    match from {
        "INITIALIZED" => Some("ON_CREATE"),
        "CREATED" => Some("ON_START"),
        "STARTED" => Some("ON_RESUME"),
        _ => None,
    }
}
fn down_event(from: &str) -> Option<&'static str> {
    match from {
        "RESUMED" => Some("ON_PAUSE"),
        "STARTED" => Some("ON_STOP"),
        "CREATED" | "INITIALIZED" => Some("ON_DESTROY"),
        _ => None,
    }
}
fn enum_obj(vm: &mut Engine, class: &str, name: &str, ordinal: i32) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, class)?;
    let n = intrinsics::alloc_string(vm, name.to_string())?;
    let _ = vm.heap.put_field(r, "name", Value::Obj(n));
    let _ = vm.heap.put_field(r, "ordinal", Value::Int(ordinal));
    Ok(r)
}
fn state_obj(vm: &mut Engine, name: &str) -> Result<ObjRef, VmExit> {
    if let Some(r) = vm.fw.androidx.lifecycle_state_objs.get(name) {
        return Ok(*r);
    }
    let r = enum_obj(vm, LIFECYCLE_STATE, name, state_ordinal(name))?;
    vm.fw
        .androidx
        .lifecycle_state_objs
        .insert(name.to_string(), r);
    vm.fw.androidx.state_name_by_obj.insert(r, name.to_string());
    Ok(r)
}
fn event_obj(vm: &mut Engine, name: &str) -> Result<ObjRef, VmExit> {
    if let Some(r) = vm.fw.androidx.lifecycle_event_objs.get(name) {
        return Ok(*r);
    }
    let ord = LIFECYCLE_EVENTS
        .iter()
        .position(|e| *e == name)
        .map(|i| i as i32)
        .unwrap_or(0);
    let r = enum_obj(vm, LIFECYCLE_EVENT, name, ord)?;
    vm.fw
        .androidx
        .lifecycle_event_objs
        .insert(name.to_string(), r);
    vm.fw.androidx.event_name_by_obj.insert(r, name.to_string());
    Ok(r)
}
fn read_state_name(vm: &Engine, v: Option<ObjRef>) -> Option<String> {
    let r = v?;
    if let Some(n) = vm.fw.androidx.state_name_by_obj.get(&r) {
        return Some(n.clone());
    }
    // DEX-created enum: java.lang.Enum name field.
    match vm.heap.get_field(r, "name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}
fn read_event_name(vm: &Engine, v: Option<ObjRef>) -> Option<String> {
    let r = v?;
    if let Some(n) = vm.fw.androidx.event_name_by_obj.get(&r) {
        return Some(n.clone());
    }
    match vm.heap.get_field(r, "name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}

/// Move a registry step-by-step to `target`, notifying observers with
/// each intermediate event (AndroidX `sync` semantics).
fn lifecycle_sync_to(vm: &mut Engine, recv: ObjRef, target: &str) -> Result<(), VmExit> {
    for _ in 0..8 {
        let (cur, owner, observers) = {
            let Some(h) = vm.fw.androidx.lifecycles.get(&recv) else {
                return Ok(());
            };
            (h.state.clone(), h.owner, h.observers.clone())
        };
        if cur == target || cur == "DESTROYED" {
            return Ok(());
        }
        let going_up = state_ordinal(target) > state_ordinal(&cur);
        let event = if going_up {
            up_event(&cur)
        } else {
            down_event(&cur)
        };
        let Some(event) = event else { return Ok(()) };
        let next = event_target(event).to_string();
        if let Some(h) = vm.fw.androidx.lifecycles.get_mut(&recv) {
            h.state = next;
        }
        let ev = event_obj(vm, event)?;
        if let Some(owner_ref) = owner {
            for obs in observers {
                let _ = crate::m8::call_dex(
                    vm,
                    obs,
                    "onStateChanged",
                    "(Landroidx/lifecycle/LifecycleOwner;Landroidx/lifecycle/Lifecycle$Event;)V",
                    vec![Value::Obj(owner_ref), Value::Obj(ev)],
                )?;
            }
        }
    }
    Ok(())
}

fn lifecycle(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Canonical State/Event objects answer their own enum methods.
    if let Some(state) = vm.fw.androidx.state_name_by_obj.get(&recv).cloned() {
        return Ok(Some(match name {
            "isAtLeast" => {
                let other = read_state_name(vm, oarg(args, 0)).unwrap_or_default();
                Value::Int((state_ordinal(&state) >= state_ordinal(&other)) as i32)
            }
            "name" | "toString" => astr(vm, state)?,
            "ordinal" => Value::Int(state_ordinal(&state)),
            _ => return Ok(None),
        }));
    }
    if let Some(event) = vm.fw.androidx.event_name_by_obj.get(&recv).cloned() {
        return Ok(Some(match name {
            "getTargetState" => Value::Obj(state_obj(vm, event_target(&event))?),
            "name" | "toString" => astr(vm, event)?,
            "ordinal" => Value::Int(
                LIFECYCLE_EVENTS
                    .iter()
                    .position(|e| *e == event)
                    .map(|i| i as i32)
                    .unwrap_or(0),
            ),
            _ => return Ok(None),
        }));
    }
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        let owner = oarg(args, 0);
        let h = vm
            .fw
            .androidx
            .lifecycles
            .entry(recv)
            .or_insert_with(|| LifecycleHost {
                owner,
                state: "INITIALIZED".to_string(),
                observers: Vec::new(),
            });
        if h.owner.is_none() {
            h.owner = owner;
        }
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.lifecycles.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "getCurrentState" => {
            let state = vm.fw.androidx.lifecycles[&recv].state.clone();
            Value::Obj(state_obj(vm, &state)?)
        }
        "setCurrentState" | "markState" => {
            let Some(target) = read_state_name(vm, oarg(args, 0)) else {
                return Err(npe("Lifecycle state null"));
            };
            lifecycle_sync_to(vm, recv, &target)?;
            Value::Null
        }
        "handleLifecycleEvent" => {
            let Some(event) = read_event_name(vm, oarg(args, 0)) else {
                return Err(npe("Lifecycle event null"));
            };
            lifecycle_sync_to(vm, recv, event_target(&event))?;
            Value::Null
        }
        "addObserver" => {
            let Some(obs) = oarg(args, 0) else {
                return Err(npe("addObserver(null)"));
            };
            let (cur, owner) = {
                let h = vm
                    .fw
                    .androidx
                    .lifecycles
                    .get_mut(&recv)
                    .ok_or_else(|| npe("lifecycle"))?;
                if !h.observers.contains(&obs) {
                    h.observers.push(obs);
                }
                (h.state.clone(), h.owner)
            };
            // Bring the new observer up to the current state (up-events).
            if let Some(owner_ref) = owner {
                let mut s = "INITIALIZED".to_string();
                while state_ordinal(&s) < state_ordinal(&cur) {
                    let Some(ev_name) = up_event(&s) else { break };
                    let ev = event_obj(vm, ev_name)?;
                    let _ = crate::m8::call_dex(
                        vm,
                        obs,
                        "onStateChanged",
                        "(Landroidx/lifecycle/LifecycleOwner;Landroidx/lifecycle/Lifecycle$Event;)V",
                        vec![Value::Obj(owner_ref), Value::Obj(ev)],
                    )?;
                    s = event_target(ev_name).to_string();
                }
            }
            Value::Null
        }
        "removeObserver" => {
            if let Some(obs) = oarg(args, 0) {
                if let Some(h) = vm.fw.androidx.lifecycles.get_mut(&recv) {
                    h.observers.retain(|o| *o != obs);
                }
            }
            Value::Null
        }
        "getObserverCount" => Value::Int(vm.fw.androidx.lifecycles[&recv].observers.len() as i32),
        _ => return Ok(None),
    }))
}

// ── LiveData ────────────────────────────────────────────────────────────
fn owner_registry(vm: &mut Engine, owner: ObjRef) -> Option<ObjRef> {
    match crate::m8::call_dex(
        vm,
        owner,
        "getLifecycle",
        "()Landroidx/lifecycle/Lifecycle;",
        Vec::new(),
    ) {
        Ok(Some(Value::Obj(r))) => Some(r),
        _ => None,
    }
}
fn observer_active(vm: &Engine, o: &LiveObserver) -> bool {
    match o.owner {
        None => true, // observeForever
        Some(_) => match o.registry.and_then(|r| vm.fw.androidx.lifecycles.get(&r)) {
            Some(h) => state_ordinal(&h.state) >= state_ordinal("STARTED"),
            None => false,
        },
    }
}
/// Teto de rondas de re-despacho por disparo (o AOSP não tem teto
/// porque o main thread + watchdog o limitam; aqui o teto é a
/// alternativa honesta a um loop/abort do hospedeiro).
const MAX_LIVEDATA_DISPATCH_ROUNDS: usize = 256;

fn livedata_dispatch(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    {
        let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) else {
            return Ok(());
        };
        if h.dispatching {
            // setValue aninhado (dentro de um onChanged): invalida e
            // deixa a ronda exterior re-despachar com o valor novo.
            h.dispatch_invalidated = true;
            return Ok(());
        }
        h.dispatching = true;
    }
    let result = livedata_dispatch_rounds(vm, recv);
    if let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) {
        h.dispatching = false;
        h.dispatch_invalidated = false;
    }
    result
}

fn livedata_dispatch_rounds(vm: &mut Engine, recv: ObjRef) -> Result<(), VmExit> {
    for _ in 0..MAX_LIVEDATA_DISPATCH_ROUNDS {
        if let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) {
            h.dispatch_invalidated = false;
        }
        // Deliver the current value to every active observer that has
        // not seen this version; inactive keep their last version.
        let snapshot: Vec<(ObjRef, Value)> = {
            let Some(h) = vm.fw.androidx.livedata.get(&recv) else {
                return Ok(());
            };
            h.observers
                .iter()
                .filter(|o| o.last_version < h.version && observer_active(vm, o))
                .map(|o| (o.observer, h.value.clone()))
                .collect()
        };
        if let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) {
            let version = h.version;
            for o in h.observers.iter_mut() {
                if snapshot.iter().any(|(obs, _)| *obs == o.observer) {
                    o.last_version = version;
                }
            }
        }
        for (obs, val) in snapshot {
            let _ = crate::m8::call_dex(vm, obs, "onChanged", "(Ljava/lang/Object;)V", vec![val])?;
        }
        let invalidated = vm
            .fw
            .androidx
            .livedata
            .get(&recv)
            .is_some_and(|h| h.dispatch_invalidated);
        if !invalidated {
            break;
        }
    }
    Ok(())
}
fn livedata(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw.androidx.livedata.entry(recv).or_insert_with(|| {
            let mut h = LiveDataHost::default();
            if !args.is_empty() {
                h.value = args[0].clone();
                h.version = 0;
            }
            h
        });
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.livedata.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "getValue" => vm.fw.androidx.livedata[&recv].value.clone(),
        // Single-thread deterministic VM: postValue IS setValue here.
        "setValue" | "postValue" => {
            {
                let h = vm
                    .fw
                    .androidx
                    .livedata
                    .get_mut(&recv)
                    .ok_or_else(|| npe("livedata"))?;
                h.value = args.first().cloned().unwrap_or(Value::Null);
                h.version += 1;
            }
            livedata_dispatch(vm, recv)?;
            Value::Null
        }
        "observe" => {
            let (Some(owner), Some(obs)) = (oarg(args, 0), oarg(args, 1)) else {
                return Err(npe("observe(null)"));
            };
            let registry = owner_registry(vm, owner);
            {
                let h = vm
                    .fw
                    .androidx
                    .livedata
                    .get_mut(&recv)
                    .ok_or_else(|| npe("livedata"))?;
                if !h.observers.iter().any(|o| o.observer == obs) {
                    h.observers.push(LiveObserver {
                        owner: Some(owner),
                        registry,
                        observer: obs,
                        last_version: -1,
                    });
                }
            }
            livedata_dispatch(vm, recv)?;
            Value::Null
        }
        "observeForever" => {
            let Some(obs) = oarg(args, 0) else {
                return Err(npe("observeForever(null)"));
            };
            {
                let h = vm
                    .fw
                    .androidx
                    .livedata
                    .get_mut(&recv)
                    .ok_or_else(|| npe("livedata"))?;
                if !h.observers.iter().any(|o| o.observer == obs) {
                    h.observers.push(LiveObserver {
                        owner: None,
                        registry: None,
                        observer: obs,
                        last_version: -1,
                    });
                }
            }
            livedata_dispatch(vm, recv)?;
            Value::Null
        }
        "removeObserver" => {
            if let Some(obs) = oarg(args, 0) {
                if let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) {
                    h.observers.retain(|o| o.observer != obs);
                }
            }
            Value::Null
        }
        "removeObservers" => {
            if let Some(owner) = oarg(args, 0) {
                if let Some(h) = vm.fw.androidx.livedata.get_mut(&recv) {
                    h.observers.retain(|o| o.owner != Some(owner));
                }
            }
            Value::Null
        }
        "hasObservers" => Value::Int((!vm.fw.androidx.livedata[&recv].observers.is_empty()) as i32),
        "hasActiveObservers" => {
            let h = &vm.fw.androidx.livedata[&recv];
            Value::Int(h.observers.iter().any(|o| observer_active(vm, o)) as i32)
        }
        _ => return Ok(None),
    }))
}

// ── ViewModel ───────────────────────────────────────────────────────────
fn vm_store(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw.androidx.vm_stores.insert(recv, Vec::new());
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.vm_stores.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "put" => {
            let key = sarg(vm, args, 0).ok_or_else(|| npe("ViewModelStore.put(null key)"))?;
            let val = oarg(args, 1).ok_or_else(|| npe("ViewModelStore.put(null viewModel)"))?;
            let m = vm
                .fw
                .androidx
                .vm_stores
                .get_mut(&recv)
                .ok_or_else(|| npe("store"))?;
            match m.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = val,
                None => m.push((key, val)),
            }
            Value::Null
        }
        "get" => {
            let key = sarg(vm, args, 0).unwrap_or_default();
            match vm.fw.androidx.vm_stores[&recv]
                .iter()
                .find(|(k, _)| *k == key)
            {
                Some((_, v)) => Value::Obj(*v),
                None => Value::Null,
            }
        }
        "keys" => {
            let keys: Vec<String> = vm.fw.androidx.vm_stores[&recv]
                .iter()
                .map(|(k, _)| k.clone())
                .collect();
            make_string_set(vm, &keys)?
        }
        "clear" => {
            let cleared: Vec<ObjRef> = {
                let m = vm
                    .fw
                    .androidx
                    .vm_stores
                    .get_mut(&recv)
                    .ok_or_else(|| npe("store"))?;
                m.drain(..).map(|(_, v)| v).collect()
            };
            for vm_ref in cleared {
                let _ = crate::m8::call_dex(vm, vm_ref, "onCleared", "()V", Vec::new())?;
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

fn vm_provider(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        // (ViewModelStore, Factory) / (ViewModelStoreOwner, Factory) /
        // (ViewModelStoreOwner) — keep object args in order.
        let first = oarg(args, 0);
        let second = oarg(args, 1);
        vm.fw.androidx.vm_providers.insert(recv, (first, second));
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.vm_providers.contains_key(&recv) || name != "get" {
        return Ok(None);
    }
    let (owner_or_store, factory) = vm.fw.androidx.vm_providers[&recv];
    // Resolve the store: direct store, or owner.getViewModelStore().
    let mut store = match owner_or_store {
        Some(r) if vm.fw.androidx.vm_stores.contains_key(&r) => Some(r),
        Some(r) => match crate::m8::call_dex(
            vm,
            r,
            "getViewModelStore",
            "()Landroidx/lifecycle/ViewModelStore;",
            Vec::new(),
        )? {
            Some(Value::Obj(s)) => Some(s),
            _ => None,
        },
        None => None,
    };
    if let Some(s) = store {
        vm.fw.androidx.vm_stores.entry(s).or_default();
    } else {
        let s = alloc(vm, VIEW_MODEL_STORE)?;
        vm.fw.androidx.vm_stores.insert(s, Vec::new());
        store = Some(s);
    }
    let store = store.ok_or_else(|| npe("ViewModelStore"))?;
    let (key, class_desc) = if args.len() >= 2 {
        let k = sarg(vm, args, 0).unwrap_or_default();
        (k, class_desc_of(vm, oarg(args, 1)))
    } else {
        let desc = class_desc_of(vm, oarg(args, 0));
        let canonical = desc
            .clone()
            .map(|d| {
                d.trim_start_matches('L')
                    .trim_end_matches(';')
                    .replace('/', ".")
            })
            .unwrap_or_default();
        (
            format!("androidx.lifecycle.ViewModelProvider.DefaultKey:{canonical}"),
            desc,
        )
    };
    if let Some((_, existing)) = vm.fw.androidx.vm_stores[&store]
        .iter()
        .find(|(k, _)| *k == key)
    {
        return Ok(Some(Value::Obj(*existing)));
    }
    let created = match (factory, oarg(args, args.len().saturating_sub(1))) {
        (Some(f), Some(cls)) => match crate::m8::call_dex(
            vm,
            f,
            "create",
            "(Ljava/lang/Class;)Landroidx/lifecycle/ViewModel;",
            vec![Value::Obj(cls)],
        )? {
            Some(Value::Obj(c)) => Some(c),
            _ => None,
        },
        _ => None,
    };
    let created = match created {
        Some(c) => c,
        None => {
            let desc = class_desc.ok_or_else(|| npe("ViewModel class"))?;
            alloc(vm, &desc)?
        }
    };
    vm.fw
        .androidx
        .vm_stores
        .get_mut(&store)
        .ok_or_else(|| npe("store"))?
        .push((key, created));
    Ok(Some(Value::Obj(created)))
}

// ── SavedStateHandle ────────────────────────────────────────────────────
fn saved_state(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        let mut entries: Vec<(String, Value)> = Vec::new();
        if let Some(map_ref) = oarg(args, 0) {
            for (k, v) in read_host_map(vm, map_ref) {
                if let Value::Obj(kr) = k {
                    if let Ok(ks) = vm.heap.as_str(kr) {
                        entries.push((ks.to_string(), v));
                    }
                }
            }
        }
        vm.fw.androidx.saved_state.insert(recv, entries);
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.saved_state.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "get" => {
            let key = sarg(vm, args, 0).unwrap_or_default();
            vm.fw.androidx.saved_state[&recv]
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null)
        }
        "set" => {
            let key = sarg(vm, args, 0).ok_or_else(|| npe("SavedStateHandle.set(null)"))?;
            let val = args.get(1).cloned().unwrap_or(Value::Null);
            let m = vm
                .fw
                .androidx
                .saved_state
                .get_mut(&recv)
                .ok_or_else(|| npe("handle"))?;
            match m.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = val,
                None => m.push((key, val)),
            }
            Value::Null
        }
        "contains" => {
            let key = sarg(vm, args, 0).unwrap_or_default();
            Value::Int(
                vm.fw.androidx.saved_state[&recv]
                    .iter()
                    .any(|(k, _)| *k == key) as i32,
            )
        }
        "remove" => {
            let key = sarg(vm, args, 0).unwrap_or_default();
            let m = vm
                .fw
                .androidx
                .saved_state
                .get_mut(&recv)
                .ok_or_else(|| npe("handle"))?;
            match m.iter().position(|(k, _)| *k == key) {
                Some(i) => m.remove(i).1,
                None => Value::Null,
            }
        }
        "keys" => {
            let keys: Vec<String> = vm.fw.androidx.saved_state[&recv]
                .iter()
                .map(|(k, _)| k.clone())
                .collect();
            make_string_set(vm, &keys)?
        }
        _ => return Ok(None),
    }))
}

// ── collection ──────────────────────────────────────────────────────────
fn collection(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let kind = if vm.fw.androidx.array_maps.contains_key(&recv)
        || mref == ARRAY_MAP
        || mref == SIMPLE_ARRAY_MAP
    {
        "map"
    } else if vm.fw.androidx.array_sets.contains_key(&recv) || mref == ARRAY_SET {
        "set"
    } else if vm.fw.androidx.sparse.contains_key(&recv)
        || mref == SPARSE_ARRAY_COMPAT
        || mref == LONG_SPARSE_ARRAY
    {
        "sparse"
    } else if vm.fw.androidx.lru.contains_key(&recv) || mref == LRU_CACHE {
        "lru"
    } else {
        return Ok(None);
    };
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        match kind {
            "map" => {
                // ArrayMap(Map) copy constructor with a host map.
                let mut entries = Vec::new();
                if let Some(src) = vm.fw.androidx.array_maps.get(&recv).cloned() {
                    entries = src;
                } else if let Some(src_ref) = oarg(args, 0) {
                    if let Some(src) = vm.fw.androidx.array_maps.get(&src_ref).cloned() {
                        entries = src;
                    } else {
                        entries = read_host_map(vm, src_ref);
                    }
                }
                vm.fw.androidx.array_maps.insert(recv, entries);
            }
            "set" => {
                vm.fw.androidx.array_sets.entry(recv).or_default();
            }
            "sparse" => {
                vm.fw.androidx.sparse.entry(recv).or_default();
            }
            _ => {
                vm.fw.androidx.lru.insert(
                    recv,
                    LruHost {
                        max_size: iarg(args, 0).max(0),
                        ..LruHost::default()
                    },
                );
            }
        }
        return Ok(Some(Value::Null));
    }
    match kind {
        "map" => array_map(vm, name, recv, args),
        "set" => array_set(vm, name, recv, args),
        "sparse" => sparse_array(vm, mref, name, recv, args),
        _ => lru_cache(vm, name, recv, args),
    }
}

fn array_map(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.fw.androidx.array_maps.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "size" => Value::Int(vm.fw.androidx.array_maps[&recv].len() as i32),
        "isEmpty" => Value::Int(vm.fw.androidx.array_maps[&recv].is_empty() as i32),
        "clear" => {
            vm.fw
                .androidx
                .array_maps
                .get_mut(&recv)
                .ok_or_else(|| npe("map"))?
                .clear();
            Value::Null
        }
        "put" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            let mut old = Value::Null;
            let entries = vm.fw.androidx.array_maps[&recv].clone();
            let mut found = None;
            for (i, (ek, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    found = Some(i);
                    break;
                }
            }
            let m = vm
                .fw
                .androidx
                .array_maps
                .get_mut(&recv)
                .ok_or_else(|| npe("map"))?;
            match found {
                Some(i) => old = std::mem::replace(&mut m[i].1, v),
                None => m.push((k, v)),
            }
            old
        }
        "get" | "getOrDefault" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.array_maps[&recv].clone();
            let mut out = if name == "getOrDefault" {
                args.get(1).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            for (ek, ev) in &entries {
                if eq(vm, ek, &k)? {
                    out = ev.clone();
                    break;
                }
            }
            out
        }
        "remove" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.array_maps[&recv].clone();
            let mut idx = None;
            for (i, (ek, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    idx = Some(i);
                    break;
                }
            }
            match idx {
                Some(i) => {
                    vm.fw
                        .androidx
                        .array_maps
                        .get_mut(&recv)
                        .ok_or_else(|| npe("map"))?
                        .remove(i)
                        .1
                }
                None => Value::Null,
            }
        }
        "containsKey" | "containsValue" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.array_maps[&recv].clone();
            let mut found = false;
            for (ek, ev) in &entries {
                let probe = if name == "containsKey" { ek } else { ev };
                if eq(vm, probe, &k)? {
                    found = true;
                    break;
                }
            }
            Value::Int(found as i32)
        }
        "indexOfKey" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.array_maps[&recv].clone();
            let mut idx = -1;
            for (i, (ek, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    idx = i as i32;
                    break;
                }
            }
            Value::Int(idx)
        }
        "keyAt" | "valueAt" => {
            let i = iarg(args, 0);
            let m = &vm.fw.androidx.array_maps[&recv];
            if i < 0 || i as usize >= m.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", m.len()),
                ));
            }
            let e = &m[i as usize];
            if name == "keyAt" {
                e.0.clone()
            } else {
                e.1.clone()
            }
        }
        "removeAt" => {
            let i = iarg(args, 0);
            let m = vm
                .fw
                .androidx
                .array_maps
                .get_mut(&recv)
                .ok_or_else(|| npe("map"))?;
            if i < 0 || i as usize >= m.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", m.len()),
                ));
            }
            m.remove(i as usize).1
        }
        "setValueAt" => {
            let i = iarg(args, 0);
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            let m = vm
                .fw
                .androidx
                .array_maps
                .get_mut(&recv)
                .ok_or_else(|| npe("map"))?;
            if i < 0 || i as usize >= m.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", m.len()),
                ));
            }
            std::mem::replace(&mut m[i as usize].1, v)
        }
        _ => return Ok(None),
    }))
}

fn array_set(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.fw.androidx.array_sets.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "size" => Value::Int(vm.fw.androidx.array_sets[&recv].len() as i32),
        "isEmpty" => Value::Int(vm.fw.androidx.array_sets[&recv].is_empty() as i32),
        "clear" => {
            vm.fw
                .androidx
                .array_sets
                .get_mut(&recv)
                .ok_or_else(|| npe("set"))?
                .clear();
            Value::Null
        }
        "add" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            let elems = vm.fw.androidx.array_sets[&recv].clone();
            let mut present = false;
            for e in &elems {
                if eq(vm, e, &v)? {
                    present = true;
                    break;
                }
            }
            if !present {
                vm.fw
                    .androidx
                    .array_sets
                    .get_mut(&recv)
                    .ok_or_else(|| npe("set"))?
                    .push(v);
            }
            Value::Int((!present) as i32)
        }
        "remove" | "contains" | "indexOf" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            let elems = vm.fw.androidx.array_sets[&recv].clone();
            let mut idx = None;
            for (i, e) in elems.iter().enumerate() {
                if eq(vm, e, &v)? {
                    idx = Some(i);
                    break;
                }
            }
            match name {
                "remove" => {
                    if let Some(i) = idx {
                        vm.fw
                            .androidx
                            .array_sets
                            .get_mut(&recv)
                            .ok_or_else(|| npe("set"))?
                            .remove(i);
                    }
                    Value::Int(idx.is_some() as i32)
                }
                "contains" => Value::Int(idx.is_some() as i32),
                _ => Value::Int(idx.map(|i| i as i32).unwrap_or(-1)),
            }
        }
        "valueAt" => {
            let i = iarg(args, 0);
            let s = &vm.fw.androidx.array_sets[&recv];
            if i < 0 || i as usize >= s.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", s.len()),
                ));
            }
            s[i as usize].clone()
        }
        "removeAt" => {
            let i = iarg(args, 0);
            let s = vm
                .fw
                .androidx
                .array_sets
                .get_mut(&recv)
                .ok_or_else(|| npe("set"))?;
            if i < 0 || i as usize >= s.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", s.len()),
                ));
            }
            s.remove(i as usize)
        }
        _ => return Ok(None),
    }))
}

fn sparse_array(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.fw.androidx.sparse.contains_key(&recv) {
        return Ok(None);
    }
    let long_keys =
        mref == LONG_SPARSE_ARRAY || vm.heap.class_of(recv).unwrap_or("") == LONG_SPARSE_ARRAY;
    let key0 = if long_keys {
        larg(args, 0)
    } else {
        iarg(args, 0) as i64
    };
    Ok(Some(match name {
        "size" => Value::Int(vm.fw.androidx.sparse[&recv].len() as i32),
        "isEmpty" => Value::Int(vm.fw.androidx.sparse[&recv].is_empty() as i32),
        "clear" => {
            vm.fw
                .androidx
                .sparse
                .get_mut(&recv)
                .ok_or_else(|| npe("sparse"))?
                .clear();
            Value::Null
        }
        "put" | "append" | "set" => {
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            vm.fw
                .androidx
                .sparse
                .get_mut(&recv)
                .ok_or_else(|| npe("sparse"))?
                .insert(key0, v);
            Value::Null
        }
        "get" => {
            let dflt = args.get(1).cloned().unwrap_or(Value::Null);
            vm.fw.androidx.sparse[&recv]
                .get(&key0)
                .cloned()
                .unwrap_or(dflt)
        }
        "delete" | "remove" => {
            vm.fw
                .androidx
                .sparse
                .get_mut(&recv)
                .ok_or_else(|| npe("sparse"))?
                .remove(&key0);
            Value::Null
        }
        "removeAt" => {
            let i = iarg(args, 0);
            let keys: Vec<i64> = vm.fw.androidx.sparse[&recv].keys().copied().collect();
            if i >= 0 && (i as usize) < keys.len() {
                let k = keys[i as usize];
                vm.fw
                    .androidx
                    .sparse
                    .get_mut(&recv)
                    .ok_or_else(|| npe("sparse"))?
                    .remove(&k);
            }
            Value::Null
        }
        "keyAt" => {
            let i = iarg(args, 0);
            let keys: Vec<i64> = vm.fw.androidx.sparse[&recv].keys().copied().collect();
            if i < 0 || i as usize >= keys.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", keys.len()),
                ));
            }
            if long_keys {
                Value::Long(keys[i as usize])
            } else {
                Value::Int(keys[i as usize] as i32)
            }
        }
        "valueAt" => {
            let i = iarg(args, 0);
            let vals: Vec<Value> = vm.fw.androidx.sparse[&recv].values().cloned().collect();
            if i < 0 || i as usize >= vals.len() {
                return Err(throw(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    format!("length={}; index={i}", vals.len()),
                ));
            }
            vals[i as usize].clone()
        }
        "indexOfKey" => {
            let keys: Vec<i64> = vm.fw.androidx.sparse[&recv].keys().copied().collect();
            Value::Int(
                keys.iter()
                    .position(|k| *k == key0)
                    .map(|i| i as i32)
                    .unwrap_or(-1),
            )
        }
        "indexOfValue" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            let vals: Vec<Value> = vm.fw.androidx.sparse[&recv].values().cloned().collect();
            let mut idx = -1;
            for (i, ev) in vals.iter().enumerate() {
                if eq(vm, ev, &v)? {
                    idx = i as i32;
                    break;
                }
            }
            Value::Int(idx)
        }
        "containsKey" => Value::Int(vm.fw.androidx.sparse[&recv].contains_key(&key0) as i32),
        "containsValue" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            let vals: Vec<Value> = vm.fw.androidx.sparse[&recv].values().cloned().collect();
            let mut found = false;
            for ev in &vals {
                if eq(vm, ev, &v)? {
                    found = true;
                    break;
                }
            }
            Value::Int(found as i32)
        }
        _ => return Ok(None),
    }))
}

fn lru_size_of(vm: &mut Engine, recv: ObjRef, k: &Value, v: &Value) -> i32 {
    match crate::m8::call_dex(
        vm,
        recv,
        "sizeOf",
        "(Ljava/lang/Object;Ljava/lang/Object;)I",
        vec![k.clone(), v.clone()],
    ) {
        Ok(Some(Value::Int(n))) if n > 0 => n,
        _ => 1,
    }
}
fn lru_trim(vm: &mut Engine, recv: ObjRef, max: i32) {
    loop {
        let evict = {
            let Some(h) = vm.fw.androidx.lru.get_mut(&recv) else {
                return;
            };
            if h.size <= max || h.entries.is_empty() {
                return;
            }
            let (k, v, s) = h.entries.remove(0);
            h.size -= s;
            h.eviction_count += 1;
            (k, v)
        };
        let _ = crate::m8::call_dex(
            vm,
            recv,
            "entryRemoved",
            "(ZLjava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)V",
            vec![Value::Int(1), evict.0, evict.1, Value::Null],
        );
    }
}
fn lru_cache(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !vm.fw.androidx.lru.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "get" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.lru[&recv].entries.clone();
            let mut hit = None;
            for (i, (ek, _, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    hit = Some(i);
                    break;
                }
            }
            let h = vm
                .fw
                .androidx
                .lru
                .get_mut(&recv)
                .ok_or_else(|| npe("lru"))?;
            match hit {
                Some(i) => {
                    h.hit_count += 1;
                    let e = h.entries.remove(i);
                    let v = e.1.clone();
                    h.entries.push(e);
                    v
                }
                None => {
                    h.miss_count += 1;
                    Value::Null
                }
            }
        }
        "put" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            if matches!(k, Value::Null) || matches!(v, Value::Null) {
                return Err(npe("LruCache.put(null)"));
            }
            let sz = lru_size_of(vm, recv, &k, &v);
            let entries = vm.fw.androidx.lru[&recv].entries.clone();
            let mut prev_idx = None;
            for (i, (ek, _, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    prev_idx = Some(i);
                    break;
                }
            }
            let (old, max) = {
                let h = vm
                    .fw
                    .androidx
                    .lru
                    .get_mut(&recv)
                    .ok_or_else(|| npe("lru"))?;
                h.put_count += 1;
                let old = match prev_idx {
                    Some(i) => {
                        let old = h.entries.remove(i);
                        h.size -= old.2;
                        old.1
                    }
                    None => Value::Null,
                };
                h.entries.push((k, v, sz));
                h.size += sz;
                (old, h.max_size)
            };
            lru_trim(vm, recv, max);
            old
        }
        "remove" => {
            let k = args.first().cloned().unwrap_or(Value::Null);
            let entries = vm.fw.androidx.lru[&recv].entries.clone();
            let mut idx = None;
            for (i, (ek, _, _)) in entries.iter().enumerate() {
                if eq(vm, ek, &k)? {
                    idx = Some(i);
                    break;
                }
            }
            let h = vm
                .fw
                .androidx
                .lru
                .get_mut(&recv)
                .ok_or_else(|| npe("lru"))?;
            match idx {
                Some(i) => {
                    let e = h.entries.remove(i);
                    h.size -= e.2;
                    e.1
                }
                None => Value::Null,
            }
        }
        "size" => Value::Int(vm.fw.androidx.lru[&recv].size),
        "maxSize" => Value::Int(vm.fw.androidx.lru[&recv].max_size),
        "hitCount" => Value::Int(vm.fw.androidx.lru[&recv].hit_count),
        "missCount" => Value::Int(vm.fw.androidx.lru[&recv].miss_count),
        "putCount" => Value::Int(vm.fw.androidx.lru[&recv].put_count),
        "evictionCount" => Value::Int(vm.fw.androidx.lru[&recv].eviction_count),
        "evictAll" => {
            lru_trim(vm, recv, 0);
            Value::Null
        }
        "trimToSize" => {
            let max = iarg(args, 0);
            lru_trim(vm, recv, max);
            Value::Null
        }
        "resize" => {
            let max = iarg(args, 0).max(0);
            if let Some(h) = vm.fw.androidx.lru.get_mut(&recv) {
                h.max_size = max;
            }
            lru_trim(vm, recv, max);
            Value::Null
        }
        "snapshot" => {
            let entries: Vec<(Value, Value)> = vm.fw.androidx.lru[&recv]
                .entries
                .iter()
                .map(|(k, v, _)| (k.clone(), v.clone()))
                .collect();
            // LinkedHashMap keeps the access order like AOSP.
            let r = alloc(vm, "Ljava/util/LinkedHashMap;")?;
            vm.fw.objects.insert(r, HostObj::Map { entries });
            Value::Obj(r)
        }
        _ => return Ok(None),
    }))
}
// ── DataStore Preferences (camada de valores) ───────────────────────────
fn pref_key_name(vm: &Engine, key: Option<ObjRef>) -> Option<String> {
    let r = key?;
    if let Some(n) = vm.fw.androidx.pref_keys.get(&r) {
        return Some(n.clone());
    }
    match vm.heap.get_field(r, "name") {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}
fn preferences(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Key objects: name + equality by name (host-created or DEX-created).
    if mref == PREFERENCES_KEY || vm.fw.androidx.pref_keys.contains_key(&recv) {
        return Ok(Some(match name {
            "getName" => astr(vm, pref_key_name(vm, Some(recv)).unwrap_or_default())?,
            "toString" => {
                let n = pref_key_name(vm, Some(recv)).unwrap_or_default();
                astr(vm, format!("Key {n}"))?
            }
            _ => return Ok(None),
        }));
    }
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        // MutablePreferences(Map, boolean) / Preferences() host forms.
        let mut entries = Vec::new();
        if let Some(src) = oarg(args, 0) {
            if let Some(m) = vm.fw.androidx.preferences.get(&src).cloned() {
                entries = m;
            } else {
                for (k, v) in read_host_map(vm, src) {
                    if let Some(kn) = match &k {
                        Value::Obj(kr) => pref_key_name(vm, Some(*kr))
                            .or_else(|| vm.heap.as_str(*kr).ok().map(str::to_string)),
                        _ => None,
                    } {
                        entries.push((kn, v));
                    }
                }
            }
        }
        vm.fw.androidx.preferences.insert(recv, entries);
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.preferences.contains_key(&recv) {
        return Ok(None);
    }
    let mutable =
        vm.heap.class_of(recv).unwrap_or("") == MUTABLE_PREFERENCES || mref == MUTABLE_PREFERENCES;
    Ok(Some(match name {
        "get" => {
            let Some(kn) = pref_key_name(vm, oarg(args, 0)) else {
                return Ok(Some(Value::Null));
            };
            vm.fw.androidx.preferences[&recv]
                .iter()
                .find(|(k, _)| *k == kn)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null)
        }
        "contains" => {
            let kn = pref_key_name(vm, oarg(args, 0)).unwrap_or_default();
            Value::Int(
                vm.fw.androidx.preferences[&recv]
                    .iter()
                    .any(|(k, _)| *k == kn) as i32,
            )
        }
        "asMap" => {
            let stored = vm.fw.androidx.preferences[&recv].clone();
            let mut entries = Vec::with_capacity(stored.len());
            for (kn, v) in stored {
                let kr = alloc(vm, PREFERENCES_KEY)?;
                let no = intrinsics::alloc_string(vm, kn.clone())?;
                let _ = vm.heap.put_field(kr, "name", Value::Obj(no));
                vm.fw.androidx.pref_keys.insert(kr, kn);
                entries.push((Value::Obj(kr), v));
            }
            make_map(vm, entries)?
        }
        "toMutablePreferences" => {
            let entries = vm.fw.androidx.preferences[&recv].clone();
            let r = alloc(vm, MUTABLE_PREFERENCES)?;
            vm.fw.androidx.preferences.insert(r, entries);
            Value::Obj(r)
        }
        "toPreferences" => {
            let entries = vm.fw.androidx.preferences[&recv].clone();
            let r = alloc(vm, PREFERENCES)?;
            vm.fw.androidx.preferences.insert(r, entries);
            Value::Obj(r)
        }
        "set" if mutable => {
            let kn =
                pref_key_name(vm, oarg(args, 0)).ok_or_else(|| npe("Preferences.set(null key)"))?;
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            let m = vm
                .fw
                .androidx
                .preferences
                .get_mut(&recv)
                .ok_or_else(|| npe("prefs"))?;
            match m.iter_mut().find(|(k, _)| *k == kn) {
                Some(e) => e.1 = v,
                None => m.push((kn, v)),
            }
            Value::Null
        }
        "remove" if mutable => {
            let kn = pref_key_name(vm, oarg(args, 0)).unwrap_or_default();
            let m = vm
                .fw
                .androidx
                .preferences
                .get_mut(&recv)
                .ok_or_else(|| npe("prefs"))?;
            match m.iter().position(|(k, _)| *k == kn) {
                Some(i) => m.remove(i).1,
                None => Value::Null,
            }
        }
        "clear" if mutable => {
            vm.fw
                .androidx
                .preferences
                .get_mut(&recv)
                .ok_or_else(|| npe("prefs"))?
                .clear();
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ── WorkManager Data ────────────────────────────────────────────────────
fn work_data(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_builder = mref == WORK_DATA_BUILDER
        || vm
            .heap
            .class_of(recv)
            .unwrap_or("")
            .contains("Data$Builder");
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw.androidx.work_data.entry(recv).or_default();
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.work_data.contains_key(&recv) {
        return Ok(None);
    }
    if is_builder {
        if name == "build" {
            let entries = vm.fw.androidx.work_data[&recv].clone();
            let r = alloc(vm, WORK_DATA)?;
            vm.fw.androidx.work_data.insert(r, entries);
            return Ok(Some(Value::Obj(r)));
        }
        if let Some(field) = name.strip_prefix("put") {
            let _ = field;
            let key = sarg(vm, args, 0).ok_or_else(|| npe("Data.Builder key null"))?;
            let v = args.get(1).cloned().unwrap_or(Value::Null);
            let m = vm
                .fw
                .androidx
                .work_data
                .get_mut(&recv)
                .ok_or_else(|| npe("data"))?;
            match m.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = v,
                None => m.push((key, v)),
            }
            return Ok(Some(Value::Obj(recv)));
        }
        return Ok(None);
    }
    let lookup = |vm: &Engine, key: &str| -> Option<Value> {
        vm.fw.androidx.work_data[&recv]
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    Ok(Some(match name {
        "getString" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default()).unwrap_or(Value::Null),
        "getInt" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default())
            .and_then(|v| v.as_int().ok())
            .map(Value::Int)
            .unwrap_or(Value::Int(iarg(args, 1))),
        "getLong" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default())
            .and_then(|v| v.as_long().ok())
            .map(Value::Long)
            .unwrap_or(Value::Long(larg(args, 1))),
        "getBoolean" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default())
            .and_then(|v| v.as_int().ok())
            .map(Value::Int)
            .unwrap_or(Value::Int(iarg(args, 1))),
        "getFloat" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default())
            .and_then(|v| v.as_float().ok())
            .map(Value::Float)
            .unwrap_or(Value::Float(
                args.get(1).and_then(|v| v.as_float().ok()).unwrap_or(0.0),
            )),
        "getDouble" => lookup(vm, &sarg(vm, args, 0).unwrap_or_default())
            .and_then(|v| v.as_double().ok())
            .map(Value::Double)
            .unwrap_or(Value::Double(
                args.get(1).and_then(|v| v.as_double().ok()).unwrap_or(0.0),
            )),
        "getStringArray" | "getIntArray" | "getLongArray" | "getBooleanArray" | "getFloatArray"
        | "getDoubleArray" => {
            lookup(vm, &sarg(vm, args, 0).unwrap_or_default()).unwrap_or(Value::Null)
        }
        "getKeyValueMap" => {
            let stored = vm.fw.androidx.work_data[&recv].clone();
            let mut entries = Vec::with_capacity(stored.len());
            for (k, v) in stored {
                entries.push((astr(vm, k)?, v));
            }
            make_map(vm, entries)?
        }
        "hasKeyWithValueOfType" => {
            let key = sarg(vm, args, 0).unwrap_or_default();
            let found = lookup(vm, &key);
            let cls = class_desc_of(vm, oarg(args, 1)).unwrap_or_default();
            let ok = match (found, cls.as_str()) {
                (Some(Value::Obj(r)), c) => c.contains("String") && vm.heap.as_str(r).is_ok(),
                (Some(Value::Int(_)), c) => c.contains("Integer") || c.contains("Boolean"),
                (Some(Value::Long(_)), c) => c.contains("Long"),
                (Some(Value::Float(_)), c) => c.contains("Float"),
                (Some(Value::Double(_)), c) => c.contains("Double"),
                _ => false,
            };
            Value::Int(ok as i32)
        }
        _ => return Ok(None),
    }))
}

// ── Navigation ──────────────────────────────────────────────────────────
fn nav_destination_obj(vm: &mut Engine, id: i32) -> Result<Value, VmExit> {
    let r = alloc(vm, NAV_DESTINATION)?;
    vm.fw.androidx.nav_dest_ids.insert(r, id);
    let _ = vm.heap.put_field(r, "id", Value::Int(id));
    Ok(Value::Obj(r))
}
fn navigation(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref == NAV_DESTINATION || vm.fw.androidx.nav_dest_ids.contains_key(&recv) {
        let id = vm.fw.androidx.nav_dest_ids.get(&recv).copied().unwrap_or(0);
        return Ok(Some(match name {
            "getId" => Value::Int(id),
            "getLabel" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw.androidx.nav.insert(recv, NavHost::default());
        vm.fw.androidx.last_nav_controller = Some(recv);
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.nav.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "setGraph" => {
            let graph = iarg(args, 0);
            let h = vm
                .fw
                .androidx
                .nav
                .get_mut(&recv)
                .ok_or_else(|| npe("nav"))?;
            h.graph_id = graph;
            h.stack.clear();
            if graph != 0 {
                h.stack.push(graph);
            }
            vm.fw.androidx.last_nav_controller = Some(recv);
            Value::Null
        }
        "getGraph" => {
            let graph = vm.fw.androidx.nav[&recv].graph_id;
            if graph == 0 {
                return Err(throw(
                    "Ljava/lang/IllegalStateException;",
                    "NavController graph was not set (RustyDroid host model)",
                ));
            }
            let r = alloc(vm, "Landroidx/navigation/NavGraph;")?;
            vm.fw.androidx.nav_dest_ids.insert(r, graph);
            let _ = vm.heap.put_field(r, "id", Value::Int(graph));
            Value::Obj(r)
        }
        "navigate" => {
            let dest = iarg(args, 0);
            let h = vm
                .fw
                .androidx
                .nav
                .get_mut(&recv)
                .ok_or_else(|| npe("nav"))?;
            h.stack.push(dest);
            vm.fw.androidx.last_nav_controller = Some(recv);
            Value::Null
        }
        "popBackStack" => {
            let h = vm
                .fw
                .androidx
                .nav
                .get_mut(&recv)
                .ok_or_else(|| npe("nav"))?;
            if args.is_empty() {
                if h.stack.len() > 1 {
                    h.stack.pop();
                    Value::Int(1)
                } else {
                    Value::Int(0)
                }
            } else {
                let target = iarg(args, 0);
                let inclusive = iarg(args, 1) != 0;
                match h.stack.iter().rposition(|d| *d == target) {
                    Some(pos) => {
                        let new_len = if inclusive { pos } else { pos + 1 };
                        let changed = new_len < h.stack.len();
                        h.stack.truncate(new_len.max(1.min(h.stack.len())));
                        Value::Int(changed as i32)
                    }
                    None => Value::Int(0),
                }
            }
        }
        "navigateUp" => {
            let h = vm
                .fw
                .androidx
                .nav
                .get_mut(&recv)
                .ok_or_else(|| npe("nav"))?;
            if h.stack.len() > 1 {
                h.stack.pop();
                Value::Int(1)
            } else {
                Value::Int(0)
            }
        }
        "getCurrentDestination" => {
            let cur = vm.fw.androidx.nav[&recv].stack.last().copied();
            match cur {
                Some(id) => nav_destination_obj(vm, id)?,
                None => Value::Null,
            }
        }
        "getCurrentBackStackEntry" => {
            let cur = vm.fw.androidx.nav[&recv].stack.last().copied();
            match cur {
                Some(id) => {
                    let r = alloc(vm, "Landroidx/navigation/NavBackStackEntry;")?;
                    vm.fw.androidx.nav_dest_ids.insert(r, id);
                    let dest = nav_destination_obj(vm, id)?;
                    let _ = vm.heap.put_field(r, "destination", dest);
                    Value::Obj(r)
                }
                None => Value::Null,
            }
        }
        _ => return Ok(None),
    }))
}

// ── Startup ─────────────────────────────────────────────────────────────
fn startup(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        vm.fw.androidx.app_initializer = Some(recv);
        return Ok(Some(Value::Null));
    }
    Ok(Some(match name {
        "initializeComponent" => {
            let Some(desc) = class_desc_of(vm, oarg(args, 0)) else {
                return Err(npe("initializeComponent(null)"));
            };
            if let Some(existing) = vm.fw.androidx.startup_instances.get(&desc) {
                return Ok(Some(existing.clone()));
            }
            let instance = alloc(vm, &desc)?;
            // Initializer.create(Context) via DEX when the app ships it.
            let created = match crate::m8::call_dex(
                vm,
                instance,
                "create",
                "(Landroid/content/Context;)Ljava/lang/Object;",
                vec![Value::Null],
            )? {
                Some(v) => v,
                None => Value::Obj(instance),
            };
            vm.fw
                .androidx
                .startup_instances
                .insert(desc.clone(), created.clone());
            vm.fw.androidx.startup_initialized.push(desc);
            created
        }
        "isEagerlyInitialized" => {
            let desc = class_desc_of(vm, oarg(args, 0)).unwrap_or_default();
            Value::Int(vm.fw.androidx.startup_instances.contains_key(&desc) as i32)
        }
        _ => return Ok(None),
    }))
}

// ── CancellationSignal ──────────────────────────────────────────────────
fn cancellation(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw
            .androidx
            .cancellation
            .insert(recv, CancelHost::default());
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.cancellation.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "cancel" => {
            let listener = {
                let h = vm
                    .fw
                    .androidx
                    .cancellation
                    .get_mut(&recv)
                    .ok_or_else(|| npe("signal"))?;
                if h.canceled {
                    None
                } else {
                    h.canceled = true;
                    h.listener
                }
            };
            if let Some(l) = listener {
                let _ = crate::m8::call_dex(vm, l, "onCancel", "()V", Vec::new())?;
            }
            Value::Null
        }
        "isCanceled" => Value::Int(vm.fw.androidx.cancellation[&recv].canceled as i32),
        "throwIfCanceled" => {
            if vm.fw.androidx.cancellation[&recv].canceled {
                return Err(throw(
                    "Landroid/os/OperationCanceledException;",
                    "The operation was canceled (RustyDroid CancellationSignal)",
                ));
            }
            Value::Null
        }
        "setOnCancelListener" => {
            let listener = oarg(args, 0);
            let already = {
                let h = vm
                    .fw
                    .androidx
                    .cancellation
                    .get_mut(&recv)
                    .ok_or_else(|| npe("signal"))?;
                h.listener = listener;
                h.canceled
            };
            if already {
                if let Some(l) = listener {
                    let _ = crate::m8::call_dex(vm, l, "onCancel", "()V", Vec::new())?;
                }
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ── Biometric ───────────────────────────────────────────────────────────
fn biometric(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref == BIOMETRIC_MANAGER || vm.heap.class_of(recv).unwrap_or("") == BIOMETRIC_MANAGER {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "canAuthenticate" => Value::Int(vm.fw.androidx.biometric_can_authenticate),
            _ => return Ok(None),
        }));
    }
    if mref == PROMPT_INFO_BUILDER || vm.fw.androidx.prompt_builders.contains_key(&recv) {
        if name == "<init>" {
            if dex_exists(vm, mref, name, sig) {
                return Ok(None);
            }
            vm.fw
                .androidx
                .prompt_builders
                .insert(recv, PromptInfo::default());
            return Ok(Some(Value::Null));
        }
        if !vm.fw.androidx.prompt_builders.contains_key(&recv) {
            return Ok(None);
        }
        return Ok(Some(match name {
            "setTitle" => {
                let s = sarg(vm, args, 0).unwrap_or_default();
                vm.fw
                    .androidx
                    .prompt_builders
                    .get_mut(&recv)
                    .ok_or_else(|| npe("builder"))?
                    .title = s;
                Value::Obj(recv)
            }
            "setSubtitle" => {
                let s = sarg(vm, args, 0).unwrap_or_default();
                vm.fw
                    .androidx
                    .prompt_builders
                    .get_mut(&recv)
                    .ok_or_else(|| npe("builder"))?
                    .subtitle = s;
                Value::Obj(recv)
            }
            "setDescription" => {
                let s = sarg(vm, args, 0).unwrap_or_default();
                vm.fw
                    .androidx
                    .prompt_builders
                    .get_mut(&recv)
                    .ok_or_else(|| npe("builder"))?
                    .description = s;
                Value::Obj(recv)
            }
            "setNegativeButtonText" => {
                let s = sarg(vm, args, 0).unwrap_or_default();
                vm.fw
                    .androidx
                    .prompt_builders
                    .get_mut(&recv)
                    .ok_or_else(|| npe("builder"))?
                    .negative = s;
                Value::Obj(recv)
            }
            "setConfirmationRequired"
            | "setDeviceCredentialAllowed"
            | "setAllowedAuthenticators" => Value::Obj(recv),
            "build" => {
                let info = vm.fw.androidx.prompt_builders[&recv].clone();
                if info.title.is_empty() {
                    return Err(throw(
                        "Ljava/lang/IllegalArgumentException;",
                        "Title must be set and non-empty (BiometricPrompt.PromptInfo)",
                    ));
                }
                let r = alloc(vm, PROMPT_INFO)?;
                vm.fw.androidx.prompt_infos.insert(r, info);
                Value::Obj(r)
            }
            _ => return Ok(None),
        }));
    }
    if mref == PROMPT_INFO || vm.fw.androidx.prompt_infos.contains_key(&recv) {
        let Some(info) = vm.fw.androidx.prompt_infos.get(&recv).cloned() else {
            return Ok(None);
        };
        return Ok(Some(match name {
            "getTitle" => astr(vm, info.title)?,
            "getSubtitle" => astr(vm, info.subtitle)?,
            "getDescription" => astr(vm, info.description)?,
            "getNegativeButtonText" => astr(vm, info.negative)?,
            _ => return Ok(None),
        }));
    }
    // BiometricPrompt itself.
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        let callback = args.iter().rev().find_map(|a| match a {
            Value::Obj(r) => Some(*r),
            _ => None,
        });
        vm.fw.androidx.biometric_prompts.insert(recv, callback);
        return Ok(Some(Value::Null));
    }
    if !vm.fw.androidx.biometric_prompts.contains_key(&recv) {
        return Ok(None);
    }
    Ok(Some(match name {
        "authenticate" => {
            let title = oarg(args, 0)
                .and_then(|r| vm.fw.androidx.prompt_infos.get(&r))
                .map(|i| i.title.clone())
                .unwrap_or_default();
            vm.fw.androidx.biometric_requests.push(title);
            let callback = vm.fw.androidx.biometric_prompts[&recv];
            let outcome = vm.fw.androidx.biometric_outcome.clone();
            if let Some(cb) = callback {
                match outcome {
                    BiometricOutcome::Success => {
                        let result = alloc(vm, AUTH_RESULT)?;
                        let _ = crate::m8::call_dex(
                            vm,
                            cb,
                            "onAuthenticationSucceeded",
                            "(Landroidx/biometric/BiometricPrompt$AuthenticationResult;)V",
                            vec![Value::Obj(result)],
                        )?;
                    }
                    BiometricOutcome::Failed => {
                        let _ = crate::m8::call_dex(
                            vm,
                            cb,
                            "onAuthenticationFailed",
                            "()V",
                            Vec::new(),
                        )?;
                    }
                    BiometricOutcome::Error(code, msg) => {
                        let msg_v = astr(vm, msg)?;
                        let _ = crate::m8::call_dex(
                            vm,
                            cb,
                            "onAuthenticationError",
                            "(ILjava/lang/CharSequence;)V",
                            vec![Value::Int(code), msg_v],
                        )?;
                    }
                }
            }
            Value::Null
        }
        "cancelAuthentication" => Value::Null,
        _ => return Ok(None),
    }))
}

// ── Custom Tabs ─────────────────────────────────────────────────────────
fn custom_tabs(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref == CUSTOM_TABS_BUILDER {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "build" => Value::Obj(alloc(vm, CUSTOM_TABS_INTENT)?),
            // Visual setters are recorded as no-ops on the builder chain.
            _ if !dex_exists(vm, mref, name, sig) => Value::Obj(recv),
            _ => return Ok(None),
        }));
    }
    Ok(Some(match name {
        "<init>" => Value::Null,
        "launchUrl" => {
            let url = oarg(args, 1).map(|r| uri_string(vm, r)).unwrap_or_default();
            if !url.is_empty() {
                vm.fw.m8.deep_links.push(url.clone());
            }
            vm.fw.androidx.custom_tab_launches.push(url);
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ── ListenableFuture ────────────────────────────────────────────────────
fn future(
    vm: &mut Engine,
    mref: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        if dex_exists(vm, mref, name, sig) {
            return Ok(None);
        }
        vm.fw.androidx.futures.insert(recv, FutureHost::default());
        return Ok(Some(Value::Null));
    }
    // Futures allocated bare by other shims (e.g. WorkManager Operation)
    // behave as already-completed with a null result.
    let known = vm.fw.androidx.futures.contains_key(&recv);
    Ok(Some(match name {
        "isDone" => Value::Int((!known || vm.fw.androidx.futures[&recv].done) as i32),
        "isCancelled" => Value::Int((known && vm.fw.androidx.futures[&recv].cancelled) as i32),
        "get" => {
            if !known {
                return Ok(Some(Value::Null));
            }
            let f = vm.fw.androidx.futures[&recv].clone();
            if f.cancelled {
                return Err(throw(
                    "Ljava/util/concurrent/CancellationException;",
                    "ListenableFuture was cancelled (RustyDroid)",
                ));
            }
            if let Some(e) = f.error {
                return Err(throw("Ljava/util/concurrent/ExecutionException;", e));
            }
            if !f.done {
                // Deterministic sync VM cannot block: incomplete futures
                // surface as a typed guest exception, never a hang.
                return Err(throw(
                    "Ljava/util/concurrent/TimeoutException;",
                    "ListenableFuture not complete (RustyDroid sync model; complete it via the agent API)",
                ));
            }
            f.result
        }
        "addListener" => {
            let Some(listener) = oarg(args, 0) else {
                return Err(npe("addListener(null)"));
            };
            // Executor argument intentionally ignored: listeners run inline.
            let run_now = if known {
                let f = vm
                    .fw
                    .androidx
                    .futures
                    .get_mut(&recv)
                    .ok_or_else(|| npe("future"))?;
                if f.done {
                    true
                } else {
                    f.listeners.push(listener);
                    false
                }
            } else {
                true
            };
            if run_now {
                let _ = crate::m8::call_dex(vm, listener, "run", "()V", Vec::new())?;
            }
            Value::Null
        }
        "cancel" => {
            if !known {
                return Ok(Some(Value::Int(0)));
            }
            let f = vm
                .fw
                .androidx
                .futures
                .get_mut(&recv)
                .ok_or_else(|| npe("future"))?;
            if f.done {
                Value::Int(0)
            } else {
                f.done = true;
                f.cancelled = true;
                Value::Int(1)
            }
        }
        "set"
            if mref == SETTABLE_FUTURE
                || vm.heap.class_of(recv).unwrap_or("") == SETTABLE_FUTURE =>
        {
            let v = args.first().cloned().unwrap_or(Value::Null);
            Value::Int(complete_future(vm, recv, v, None)? as i32)
        }
        "setException"
            if mref == SETTABLE_FUTURE
                || vm.heap.class_of(recv).unwrap_or("") == SETTABLE_FUTURE =>
        {
            Value::Int(
                complete_future(vm, recv, Value::Null, Some("guest exception".to_string()))? as i32,
            )
        }
        _ => return Ok(None),
    }))
}

// ── dispatch de instância ───────────────────────────────────────────────
pub fn call_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    vm.with_host_roots(|vm| call_instance_rooted(vm, mref_class, name, sig, recv, args))
}

fn call_instance_rooted(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let recv_class = vm.heap.class_of(recv).unwrap_or("").to_string();
    // Snapshot the tracking flags first so the immutable borrow ends
    // before the &mut family calls below.
    let (
        has_lifecycle,
        has_livedata,
        has_store,
        has_provider,
        has_saved,
        has_collection,
        has_prefs,
        has_work_data,
        has_nav,
        has_cancel,
        has_biometric,
        has_future,
    ) = {
        let ax = &vm.fw.androidx;
        let tracked = ax.lifecycles.contains_key(&recv)
            || ax.state_name_by_obj.contains_key(&recv)
            || ax.event_name_by_obj.contains_key(&recv)
            || ax.livedata.contains_key(&recv)
            || ax.vm_stores.contains_key(&recv)
            || ax.vm_providers.contains_key(&recv)
            || ax.saved_state.contains_key(&recv)
            || ax.array_maps.contains_key(&recv)
            || ax.array_sets.contains_key(&recv)
            || ax.sparse.contains_key(&recv)
            || ax.lru.contains_key(&recv)
            || ax.preferences.contains_key(&recv)
            || ax.pref_keys.contains_key(&recv)
            || ax.work_data.contains_key(&recv)
            || ax.nav.contains_key(&recv)
            || ax.nav_dest_ids.contains_key(&recv)
            || ax.cancellation.contains_key(&recv)
            || ax.prompt_builders.contains_key(&recv)
            || ax.prompt_infos.contains_key(&recv)
            || ax.biometric_prompts.contains_key(&recv)
            || ax.futures.contains_key(&recv);
        if !tracked
            && !mref_class.starts_with("Landroidx/")
            && !recv_class.starts_with("Landroidx/")
            && mref_class != LISTENABLE_FUTURE
            && mref_class != SETTABLE_FUTURE
            && recv_class != LISTENABLE_FUTURE
            && recv_class != SETTABLE_FUTURE
        {
            return Ok(None);
        }
        (
            ax.lifecycles.contains_key(&recv)
                || ax.state_name_by_obj.contains_key(&recv)
                || ax.event_name_by_obj.contains_key(&recv),
            ax.livedata.contains_key(&recv),
            ax.vm_stores.contains_key(&recv),
            ax.vm_providers.contains_key(&recv),
            ax.saved_state.contains_key(&recv),
            ax.array_maps.contains_key(&recv)
                || ax.array_sets.contains_key(&recv)
                || ax.sparse.contains_key(&recv)
                || ax.lru.contains_key(&recv),
            ax.preferences.contains_key(&recv) || ax.pref_keys.contains_key(&recv),
            ax.work_data.contains_key(&recv),
            ax.nav.contains_key(&recv) || ax.nav_dest_ids.contains_key(&recv),
            ax.cancellation.contains_key(&recv),
            ax.prompt_builders.contains_key(&recv)
                || ax.prompt_infos.contains_key(&recv)
                || ax.biometric_prompts.contains_key(&recv),
            ax.futures.contains_key(&recv),
        )
    };

    if mref_class == LIFECYCLE_REGISTRY
        || mref_class == LIFECYCLE
        || mref_class == LIFECYCLE_STATE
        || mref_class == LIFECYCLE_EVENT
        || has_lifecycle
    {
        return lifecycle(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == LIVEDATA
        || mref_class == MUTABLE_LIVEDATA
        || mref_class == MEDIATOR_LIVEDATA
        || has_livedata
    {
        return livedata(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == VIEW_MODEL_STORE || has_store {
        return vm_store(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == VIEW_MODEL_PROVIDER || has_provider {
        return vm_provider(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == SAVED_STATE_HANDLE || has_saved {
        return saved_state(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == ARRAY_MAP
        || mref_class == SIMPLE_ARRAY_MAP
        || mref_class == ARRAY_SET
        || mref_class == SPARSE_ARRAY_COMPAT
        || mref_class == LONG_SPARSE_ARRAY
        || mref_class == LRU_CACHE
        || has_collection
    {
        return collection(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == PREFERENCES
        || mref_class == MUTABLE_PREFERENCES
        || mref_class == PREFERENCES_KEY
        || has_prefs
    {
        return preferences(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == WORK_DATA || mref_class == WORK_DATA_BUILDER || has_work_data {
        return work_data(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == NAV_CONTROLLER || mref_class == NAV_DESTINATION || has_nav {
        return navigation(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == APP_INITIALIZER {
        return startup(vm, name, recv, args);
    }
    if mref_class == CANCELLATION_SIGNAL || has_cancel {
        return cancellation(vm, mref_class, name, sig, recv, args);
    }
    if mref_class.starts_with("Landroidx/biometric/") || has_biometric {
        return biometric(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == CUSTOM_TABS_INTENT || mref_class == CUSTOM_TABS_BUILDER {
        return custom_tabs(vm, mref_class, name, sig, recv, args);
    }
    if mref_class == LISTENABLE_FUTURE
        || mref_class == SETTABLE_FUTURE
        || recv_class == LISTENABLE_FUTURE
        || recv_class == SETTABLE_FUTURE
        || has_future
    {
        return future(vm, mref_class, name, sig, recv, args);
    }
    Ok(None)
}
