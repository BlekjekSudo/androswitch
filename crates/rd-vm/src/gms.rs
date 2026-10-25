//! Apêndice E — Google Play Services / Firebase no modo **"sem GMS"
//! honesto** (registro: `data/gms.toml`, validado por
//! `rd-framework::fd_gms`, que exige `mode = "no-gms-honest"` e
//! `network = "none"` em todos os serviços).
//!
//! Não há processo GMS, conta Google nem backend real — e este módulo
//! nunca finge o contrário. As APIs existem como shims host com
//! respostas estruturadas, determinísticas e **programáveis pelo
//! agente** via `Engine::gms_*`: disponibilidade, fix de GPS, usuários
//! fake, catálogo de billing, token FCM, valores de Remote Config,
//! tokens de integridade rotulados como fake. Eventos são
//! **capturados** localmente (Analytics, Crashlytics, Ads, compras,
//! mensagens) e consultáveis via `Engine::gms_*` — nada sai da sandbox,
//! nada toca a rede. Firestore/RTDB são stores locais em memória
//! (record/replay NET-03): escritas disparam listeners sincronamente,
//! sem sincronização externa. ML Kit continua `not_implemented` no
//! registro e não é despachado aqui.
//!
//! Desvios documentados (nunca silenciosos): VM single-thread
//! determinística — Tasks nascem completas ou completam inline,
//! listeners rodam na hora (o `Executor` dos overloads é ignorado),
//! `getResult()`/`Tasks.await` nunca bloqueiam (Task incompleta lança
//! `IllegalStateException`, como a API real não-bloqueante),
//! `requestLocationUpdates` entrega uma vez no registro (e a cada
//! `Engine::gms_push_location`), o fluxo de billing entrega a compra
//! ao listener antes de retornar, `FirebaseApp.getInstance()` cria o
//! app default com opções fake quando ausente (simula o
//! `FirebaseInitProvider`; `Engine::gms_set_strict_firebase_init`
//! restaura a exceção real), Geo/Settings/Transactions do Firestore e
//! `toObject(Class)` reflexivo continuam fora (None → erro tipado).
//! Crashes reais entram no Crashlytics via
//! `Engine::gms_report_uncaught_crash`, chamado pelo harness quando
//! uma execução de topo termina em exceção não tratada — sem hook no
//! loop do interpretador nesta rodada.

use std::collections::HashMap;

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::framework::HostObj;
use crate::heap::ObjRef;
use crate::intrinsics;
use crate::value::Value;

// ── classes ─────────────────────────────────────────────────────────────
pub const TASK: &str = "Lcom/google/android/gms/tasks/Task;";
pub const TASK_SOURCE: &str = "Lcom/google/android/gms/tasks/TaskCompletionSource;";
pub const TASKS: &str = "Lcom/google/android/gms/tasks/Tasks;";
pub const AVAILABILITY: &str = "Lcom/google/android/gms/common/GoogleApiAvailability;";
pub const CONNECTION_RESULT: &str = "Lcom/google/android/gms/common/ConnectionResult;";
pub const STATUS: &str = "Lcom/google/android/gms/common/api/Status;";
pub const API_EXCEPTION: &str = "Lcom/google/android/gms/common/api/ApiException;";
pub const GOOGLE_API_CLIENT: &str = "Lcom/google/android/gms/common/api/GoogleApiClient;";
pub const GOOGLE_API_CLIENT_BUILDER: &str =
    "Lcom/google/android/gms/common/api/GoogleApiClient$Builder;";
pub const LOCATION_SERVICES: &str = "Lcom/google/android/gms/location/LocationServices;";
pub const FUSED_CLIENT: &str = "Lcom/google/android/gms/location/FusedLocationProviderClient;";
pub const LOCATION_REQUEST: &str = "Lcom/google/android/gms/location/LocationRequest;";
pub const LOCATION_REQUEST_BUILDER: &str =
    "Lcom/google/android/gms/location/LocationRequest$Builder;";
pub const LOCATION_RESULT: &str = "Lcom/google/android/gms/location/LocationResult;";
pub const LOCATION_AVAILABILITY: &str = "Lcom/google/android/gms/location/LocationAvailability;";
pub const LOCATION: &str = "Landroid/location/Location;";
pub const MAPS_INITIALIZER: &str = "Lcom/google/android/gms/maps/MapsInitializer;";
pub const GOOGLE_MAP: &str = "Lcom/google/android/gms/maps/GoogleMap;";
pub const SUPPORT_MAP_FRAGMENT: &str = "Lcom/google/android/gms/maps/SupportMapFragment;";
pub const MAP_VIEW: &str = "Lcom/google/android/gms/maps/MapView;";
pub const LATLNG: &str = "Lcom/google/android/gms/maps/model/LatLng;";
pub const CAMERA_UPDATE: &str = "Lcom/google/android/gms/maps/CameraUpdate;";
pub const CAMERA_UPDATE_FACTORY: &str = "Lcom/google/android/gms/maps/CameraUpdateFactory;";
pub const CAMERA_POSITION: &str = "Lcom/google/android/gms/maps/model/CameraPosition;";
pub const CAMERA_POSITION_BUILDER: &str =
    "Lcom/google/android/gms/maps/model/CameraPosition$Builder;";
pub const MARKER_OPTIONS: &str = "Lcom/google/android/gms/maps/model/MarkerOptions;";
pub const MARKER: &str = "Lcom/google/android/gms/maps/model/Marker;";
pub const UI_SETTINGS: &str = "Lcom/google/android/gms/maps/UiSettings;";
pub const BILLING_CLIENT: &str = "Lcom/android/billingclient/api/BillingClient;";
pub const BILLING_CLIENT_BUILDER: &str = "Lcom/android/billingclient/api/BillingClient$Builder;";
pub const BILLING_RESULT: &str = "Lcom/android/billingclient/api/BillingResult;";
pub const SKU_DETAILS: &str = "Lcom/android/billingclient/api/SkuDetails;";
pub const SKU_PARAMS_BUILDER: &str = "Lcom/android/billingclient/api/SkuDetailsParams$Builder;";
pub const SKU_PARAMS: &str = "Lcom/android/billingclient/api/SkuDetailsParams;";
pub const PRODUCT_DETAILS: &str = "Lcom/android/billingclient/api/ProductDetails;";
pub const ONE_TIME_OFFER: &str =
    "Lcom/android/billingclient/api/ProductDetails$OneTimePurchaseOfferDetails;";
pub const QUERY_PRODUCTS_PARAMS: &str = "Lcom/android/billingclient/api/QueryProductDetailsParams;";
pub const QUERY_PRODUCTS_BUILDER: &str =
    "Lcom/android/billingclient/api/QueryProductDetailsParams$Builder;";
pub const QP_PRODUCT: &str = "Lcom/android/billingclient/api/QueryProductDetailsParams$Product;";
pub const QP_PRODUCT_BUILDER: &str =
    "Lcom/android/billingclient/api/QueryProductDetailsParams$Product$Builder;";
pub const PURCHASE: &str = "Lcom/android/billingclient/api/Purchase;";
pub const PURCHASES_RESULT: &str = "Lcom/android/billingclient/api/Purchase$PurchasesResult;";
pub const FLOW_PARAMS: &str = "Lcom/android/billingclient/api/BillingFlowParams;";
pub const FLOW_PARAMS_BUILDER: &str = "Lcom/android/billingclient/api/BillingFlowParams$Builder;";
pub const FLOW_PD_PARAMS: &str =
    "Lcom/android/billingclient/api/BillingFlowParams$ProductDetailsParams;";
pub const FLOW_PD_BUILDER: &str =
    "Lcom/android/billingclient/api/BillingFlowParams$ProductDetailsParams$Builder;";
pub const ACK_PARAMS: &str = "Lcom/android/billingclient/api/AcknowledgePurchaseParams;";
pub const ACK_BUILDER: &str = "Lcom/android/billingclient/api/AcknowledgePurchaseParams$Builder;";
pub const CONSUME_PARAMS: &str = "Lcom/android/billingclient/api/ConsumeParams;";
pub const CONSUME_BUILDER: &str = "Lcom/android/billingclient/api/ConsumeParams$Builder;";
pub const QUERY_PURCHASES_PARAMS: &str = "Lcom/android/billingclient/api/QueryPurchasesParams;";
pub const QUERY_PURCHASES_BUILDER: &str =
    "Lcom/android/billingclient/api/QueryPurchasesParams$Builder;";
pub const GOOGLE_SIGNIN: &str = "Lcom/google/android/gms/auth/api/signin/GoogleSignIn;";
pub const SIGNIN_CLIENT: &str = "Lcom/google/android/gms/auth/api/signin/GoogleSignInClient;";
pub const SIGNIN_ACCOUNT: &str = "Lcom/google/android/gms/auth/api/signin/GoogleSignInAccount;";
pub const SIGNIN_OPTIONS_BUILDER: &str =
    "Lcom/google/android/gms/auth/api/signin/GoogleSignInOptions$Builder;";
pub const MOBILE_ADS: &str = "Lcom/google/android/gms/ads/MobileAds;";
pub const AD_VIEW: &str = "Lcom/google/android/gms/ads/AdView;";
pub const AD_REQUEST_BUILDER: &str = "Lcom/google/android/gms/ads/AdRequest$Builder;";
pub const AD_REQUEST: &str = "Lcom/google/android/gms/ads/AdRequest;";
pub const INTERSTITIAL: &str = "Lcom/google/android/gms/ads/interstitial/InterstitialAd;";
pub const REWARDED: &str = "Lcom/google/android/gms/ads/rewarded/RewardedAd;";
pub const LOAD_AD_ERROR: &str = "Lcom/google/android/gms/ads/LoadAdError;";
pub const AD_ERROR: &str = "Lcom/google/android/gms/ads/AdError;";
pub const INIT_STATUS: &str = "Lcom/google/android/gms/ads/initialization/InitializationStatus;";
pub const REWARD_ITEM: &str = "Lcom/google/android/gms/ads/rewarded/RewardItem;";
pub const FITNESS: &str = "Lcom/google/android/gms/fitness/Fitness;";
pub const DATA_READ_RESPONSE: &str = "Lcom/google/android/gms/fitness/result/DataReadResponse;";
pub const DATA_SET: &str = "Lcom/google/android/gms/fitness/data/DataSet;";
pub const DATA_POINT: &str = "Lcom/google/android/gms/fitness/data/DataPoint;";
pub const FIT_VALUE: &str = "Lcom/google/android/gms/fitness/data/Value;";
pub const CAST_CONTEXT: &str = "Lcom/google/android/gms/cast/framework/CastContext;";
pub const SESSION_MANAGER: &str = "Lcom/google/android/gms/cast/framework/SessionManager;";
pub const CAST_SESSION: &str = "Lcom/google/android/gms/cast/framework/CastSession;";
pub const NEARBY: &str = "Lcom/google/android/gms/nearby/Nearby;";
pub const DISCOVERED_ENDPOINT: &str =
    "Lcom/google/android/gms/nearby/connection/DiscoveredEndpointInfo;";
pub const CONNECTION_INFO: &str = "Lcom/google/android/gms/nearby/connection/ConnectionInfo;";
pub const PAYLOAD: &str = "Lcom/google/android/gms/nearby/connection/Payload;";
pub const NEARBY_MESSAGE: &str = "Lcom/google/android/gms/nearby/messages/Message;";
pub const SAFETYNET: &str = "Lcom/google/android/gms/safetynet/SafetyNet;";
pub const SAFETYNET_CLIENT: &str = "Lcom/google/android/gms/safetynet/SafetyNetClient;";
pub const ATTESTATION_RESPONSE: &str =
    "Lcom/google/android/gms/safetynet/SafetyNetApi$AttestationResponse;";
pub const INTEGRITY_FACTORY: &str =
    "Lcom/google/android/play/core/integrity/IntegrityManagerFactory;";
pub const INTEGRITY_TOKEN_REQUEST: &str =
    "Lcom/google/android/play/core/integrity/IntegrityTokenRequest;";
pub const INTEGRITY_TOKEN_BUILDER: &str =
    "Lcom/google/android/play/core/integrity/IntegrityTokenRequest$Builder;";
pub const INTEGRITY_TOKEN_RESPONSE: &str =
    "Lcom/google/android/play/core/integrity/IntegrityTokenResponse;";
pub const FIREBASE_APP: &str = "Lcom/google/firebase/FirebaseApp;";
pub const FIREBASE_OPTIONS: &str = "Lcom/google/firebase/FirebaseOptions;";
pub const FIREBASE_OPTIONS_BUILDER: &str = "Lcom/google/firebase/FirebaseOptions$Builder;";
pub const FIREBASE_ANALYTICS: &str = "Lcom/google/firebase/analytics/FirebaseAnalytics;";
pub const FIREBASE_AUTH: &str = "Lcom/google/firebase/auth/FirebaseAuth;";
pub const FIREBASE_USER: &str = "Lcom/google/firebase/auth/FirebaseUser;";
pub const AUTH_RESULT: &str = "Lcom/google/firebase/auth/AuthResult;";
pub const GET_TOKEN_RESULT: &str = "Lcom/google/firebase/auth/GetTokenResult;";
pub const FIREBASE_MESSAGING: &str = "Lcom/google/firebase/messaging/FirebaseMessaging;";
pub const REMOTE_MESSAGE: &str = "Lcom/google/firebase/messaging/RemoteMessage;";
pub const REMOTE_MESSAGE_BUILDER: &str = "Lcom/google/firebase/messaging/RemoteMessage$Builder;";
pub const REMOTE_NOTIFICATION: &str = "Lcom/google/firebase/messaging/RemoteMessage$Notification;";
pub const FIREBASE_CRASHLYTICS: &str = "Lcom/google/firebase/crashlytics/FirebaseCrashlytics;";
pub const CKAV_BUILDER: &str = "Lcom/google/firebase/crashlytics/CustomKeysAndValues$Builder;";
pub const CKAV: &str = "Lcom/google/firebase/crashlytics/CustomKeysAndValues;";
pub const FIRESTORE: &str = "Lcom/google/firebase/firestore/FirebaseFirestore;";
pub const COLLECTION_REF: &str = "Lcom/google/firebase/firestore/CollectionReference;";
pub const DOC_REF: &str = "Lcom/google/firebase/firestore/DocumentReference;";
pub const DOC_SNAP: &str = "Lcom/google/firebase/firestore/DocumentSnapshot;";
pub const QUERY_SNAP: &str = "Lcom/google/firebase/firestore/QuerySnapshot;";
pub const FS_QUERY: &str = "Lcom/google/firebase/firestore/Query;";
pub const SET_OPTIONS: &str = "Lcom/google/firebase/firestore/SetOptions;";
pub const LISTENER_REG: &str = "Lcom/google/firebase/firestore/ListenerRegistration;";
pub const FIREBASE_DATABASE: &str = "Lcom/google/firebase/database/FirebaseDatabase;";
pub const DATABASE_REF: &str = "Lcom/google/firebase/database/DatabaseReference;";
pub const DATA_SNAPSHOT: &str = "Lcom/google/firebase/database/DataSnapshot;";
pub const REMOTE_CONFIG: &str = "Lcom/google/firebase/remoteconfig/FirebaseRemoteConfig;";
pub const REMOTE_CONFIG_VALUE: &str =
    "Lcom/google/firebase/remoteconfig/FirebaseRemoteConfigValue;";
pub const FIREBASE_STORAGE: &str = "Lcom/google/firebase/storage/FirebaseStorage;";
pub const STORAGE_REF: &str = "Lcom/google/firebase/storage/StorageReference;";
pub const UPLOAD_TASK: &str = "Lcom/google/firebase/storage/UploadTask;";
pub const UPLOAD_SNAP: &str = "Lcom/google/firebase/storage/UploadTask$TaskSnapshot;";
pub const LIST_RESULT: &str = "Lcom/google/firebase/storage/ListResult;";
pub const ANDROID_URI: &str = "Landroid/net/Uri;";

/// Prefixo de todas as classes que este shim conhece. Fora dele (e sem
/// estado host), o dispatch devolve `None` sem custo.
const GMS_PREFIXES: [&str; 4] = [
    "Lcom/google/android/gms/",
    "Lcom/google/firebase/",
    "Lcom/android/billingclient/",
    "Lcom/google/android/play/core/integrity/",
];

const ERROR_STRINGS: [&str; 22] = [
    "SUCCESS",
    "SERVICE_MISSING",
    "SERVICE_VERSION_UPDATE_REQUIRED",
    "SERVICE_DISABLED",
    "SIGN_IN_REQUIRED",
    "INVALID_ACCOUNT",
    "RESOLUTION_REQUIRED",
    "NETWORK_ERROR",
    "INTERNAL_ERROR",
    "SERVICE_INVALID",
    "DEVELOPER_ERROR",
    "LICENSE_CHECK_FAILED",
    "CANCELED",
    "TIMEOUT",
    "INTERRUPTED",
    "API_UNAVAILABLE",
    "SIGN_IN_FAILED",
    "SERVICE_UPDATING",
    "SERVICE_MISSING_PERMISSION",
    "RESTRICTED",
    "API_VERSION_UPDATE_REQUIRED",
    "AUTH_FAILURE",
];

// ── estado ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct TaskHost {
    pub complete: bool,
    pub successful: bool,
    pub canceled: bool,
    pub result: Value,
    /// Objeto de exceção do convidado (classe em `exception_class` é só
    /// para relatório quando o objeto não carrega classe legível).
    pub exception: Option<Value>,
    pub success_listeners: Vec<ObjRef>,
    pub failure_listeners: Vec<ObjRef>,
    pub complete_listeners: Vec<ObjRef>,
    pub canceled_listeners: Vec<ObjRef>,
}

impl Default for TaskHost {
    fn default() -> Self {
        Self {
            complete: false,
            successful: false,
            canceled: false,
            result: Value::Null,
            exception: None,
            success_listeners: Vec::new(),
            failure_listeners: Vec::new(),
            complete_listeners: Vec::new(),
            canceled_listeners: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Pending {
    Cont {
        cont: ObjRef,
        success_only: bool,
        flatten: bool,
        out: ObjRef,
    },
    Flatten {
        out: ObjRef,
    },
    WhenAll {
        out: ObjRef,
        all: Vec<ObjRef>,
        success_list: bool,
        /// `whenAllComplete`: nunca falha/cancela por causa dos membros;
        /// conclui com sucesso vendendo a lista das próprias Tasks.
        never_fail: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FbOptionsCfg {
    pub api_key: String,
    pub application_id: String,
    pub project_id: String,
    pub database_url: String,
    pub storage_bucket: String,
    pub gcm_sender_id: String,
}

impl FbOptionsCfg {
    fn fake_default() -> Self {
        Self {
            api_key: "FakeApiKeyForRustyDroidShim00000000".to_string(),
            application_id: "1:000000000000:android:rustydroidfake0000".to_string(),
            project_id: "rustydroid-fake-project".to_string(),
            database_url: "https://rustydroid-fake-project.firebaseio.com".to_string(),
            storage_bucket: "rustydroid-fake-project.appspot.com".to_string(),
            gcm_sender_id: "000000000000".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnalyticsEvent {
    pub name: String,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct AuthUserRec {
    pub uid: String,
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub anonymous: bool,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GmsAccountCfg {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub id_token: String,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteMsg {
    pub from: String,
    pub to: String,
    pub data: Vec<(String, String)>,
    pub title: String,
    pub body: String,
    pub message_id: String,
    pub collapse_key: String,
    pub ttl: i32,
    pub sent_time: i64,
}

#[derive(Debug, Clone, Default)]
pub struct CrashRecord {
    pub kind: String,
    pub class: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct FsRef {
    pub path: String,
    pub is_doc: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FsQuery {
    pub collection: String,
    pub filters: Vec<(String, Value)>,
    pub order_by: Option<String>,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct FsDocSnap {
    pub path: String,
    pub exists: bool,
    pub fields: Vec<(String, Value)>,
}

#[derive(Debug, Clone, Default)]
pub struct FixCfg {
    pub lat: f64,
    pub lng: f64,
    pub accuracy_m: f32,
    pub altitude_m: f64,
    pub bearing_deg: f32,
    pub speed_mps: f32,
    pub time_ms: i64,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct LocReq {
    pub interval_ms: i64,
    pub priority: i32,
}

impl Default for LocReq {
    fn default() -> Self {
        Self {
            interval_ms: 10_000,
            priority: 100, // PRIORITY_BALANCED_POWER_ACCURACY
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkuCfg {
    pub sku: String,
    pub typ: String, // "inapp" | "subs"
    pub price: String,
    pub price_micros: i64,
    pub currency: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct PurchaseRec {
    pub token: String,
    pub products: Vec<String>,
    pub state: i32, // 1 = PURCHASED
    pub acknowledged: bool,
    pub consumed: bool,
    pub order_id: String,
    pub purchase_time: i64,
}

#[derive(Debug, Clone, Default)]
pub enum BillingOutcome {
    #[default]
    Purchase,
    Canceled,
    Error(i32),
}

#[derive(Debug, Clone, Default)]
pub struct BillingClientHost {
    pub listener: Option<ObjRef>,
    pub connected: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BillingBuilder {
    pub listener: Option<ObjRef>,
    pub sku_type: String,
    pub skus: Vec<String>,
    pub product_ids: Vec<String>,
    pub token: String,
    pub sku_details: Option<ObjRef>,
}

#[derive(Debug, Clone, Default)]
pub struct AdViewHost {
    pub unit_id: String,
    pub listener: Option<ObjRef>,
    pub loaded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FullScreenAd {
    pub unit_id: String,
    pub callback: Option<ObjRef>,
    pub shown: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CameraPos {
    pub lat: f64,
    pub lng: f64,
    pub zoom: f32,
    pub bearing: f32,
    pub tilt: f32,
}

#[derive(Debug, Clone)]
pub enum CameraUpd {
    LatLng(f64, f64, Option<f32>),
    ZoomTo(f32),
    ZoomBy(f32),
}

#[derive(Debug, Clone, Default)]
pub struct MapHost {
    pub camera: CameraPos,
    pub markers: Vec<ObjRef>,
    pub map_type: i32,
    pub marker_click_listener: Option<ObjRef>,
    pub ui_settings: Option<ObjRef>,
    pub lifecycle: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MarkerCfg {
    pub lat: f64,
    pub lng: f64,
    pub title: String,
    pub snippet: String,
    pub id: String,
}

/// Evento de listener pendente na fila de entrega (ver `fire_guarded`).
#[derive(Debug, Clone)]
pub enum PendingFire {
    Firestore(String),
    Rtdb(String),
    Auth,
}

#[derive(Debug, Default)]
pub struct GmsState {
    /// Entrega de listeners em curso: escritas feitas DENTRO de um
    /// callback enfileiram em `pending_fires` em vez de recursar
    /// (sem isto, um listener que escreve no próprio path fazia a
    /// pilha do hospedeiro transbordar — SIGABRT).
    pub firing_listeners: bool,
    pub pending_fires: Vec<PendingFire>,
    pub availability_code: i32,
    pub availability_obj: Option<ObjRef>,
    pub error_notifications: i32,
    pub tasks: HashMap<ObjRef, TaskHost>,
    pub task_sources: HashMap<ObjRef, ObjRef>,
    pub pending: HashMap<ObjRef, Vec<Pending>>,
    pub statuses: HashMap<ObjRef, (i32, String)>,
    pub conn_results: HashMap<ObjRef, i32>,
    pub api_exceptions: HashMap<ObjRef, ObjRef>, // exc -> status obj
    pub api_clients: HashMap<ObjRef, BillingClientHost>,
    pub api_client_builders: HashMap<ObjRef, Vec<ObjRef>>,
    // Firebase core (`fb_strict_init=false` ⇒ getInstance auto-inicializa)
    pub fb_strict_init: bool,
    pub fb_default_options: Option<FbOptionsCfg>,
    pub fb_apps: HashMap<String, ObjRef>,
    pub fb_app_names: HashMap<ObjRef, String>,
    pub fb_app_options: HashMap<ObjRef, FbOptionsCfg>,
    pub fb_options_objs: HashMap<ObjRef, FbOptionsCfg>,
    pub fb_options_builders: HashMap<ObjRef, FbOptionsCfg>,
    pub fb_context: Option<ObjRef>,
    pub fb_initialized: Vec<String>,
    // Analytics
    pub analytics_obj: Option<ObjRef>,
    pub analytics_events: Vec<AnalyticsEvent>,
    pub analytics_user_properties: Vec<(String, String)>,
    pub analytics_user_id: Option<String>,
    pub analytics_disabled: bool,
    pub analytics_defaults: Vec<(String, String)>,
    // Firebase Auth
    pub auth_obj: Option<ObjRef>,
    pub auth_users: Vec<AuthUserRec>,
    pub auth_user_by_obj: HashMap<ObjRef, usize>,
    pub auth_current_user: Option<ObjRef>,
    pub auth_listeners: Vec<ObjRef>,
    pub auth_password_resets: Vec<String>,
    pub auth_counter: u32,
    pub auth_result_users: HashMap<ObjRef, ObjRef>,
    pub token_results: HashMap<ObjRef, String>,
    // Google Sign-In
    pub gms_account_cfg: Option<GmsAccountCfg>,
    pub gms_signed_in: bool,
    pub gms_account_by_obj: HashMap<ObjRef, GmsAccountCfg>,
    pub gms_signin_clients: Vec<ObjRef>,
    pub gms_signin_intents: i32,
    // Messaging
    pub messaging_obj: Option<ObjRef>,
    pub fcm_token: String,
    pub fcm_topics: Vec<String>,
    pub fcm_auto_init_disabled: bool,
    pub fcm_token_deleted: bool,
    pub remote_messages: HashMap<ObjRef, RemoteMsg>,
    pub rm_builders: HashMap<ObjRef, RemoteMsg>,
    pub messaging_services: Vec<ObjRef>,
    pub fcm_upstream: Vec<String>,
    pub fcm_delivered: Vec<String>,
    // Crashlytics
    pub crashlytics_obj: Option<ObjRef>,
    pub crashes: Vec<CrashRecord>,
    pub crashlytics_logs: Vec<String>,
    pub crashlytics_keys: Vec<(String, String)>,
    pub crashlytics_user_id: Option<String>,
    pub crashlytics_disabled: bool,
    pub crashlytics_dropped: i32,
    pub ckav_builders: HashMap<ObjRef, Vec<(String, String)>>,
    pub ckav_objs: HashMap<ObjRef, Vec<(String, String)>>,
    // Firestore
    pub firestore_obj: Option<ObjRef>,
    pub fs_refs: HashMap<ObjRef, FsRef>,
    pub fs_queries: HashMap<ObjRef, FsQuery>,
    pub fs_docs: HashMap<String, Vec<(String, Value)>>,
    pub fs_doc_order: Vec<String>,
    pub fs_doc_snaps: HashMap<ObjRef, FsDocSnap>,
    pub fs_query_snaps: HashMap<ObjRef, Vec<ObjRef>>,
    pub fs_doc_listeners: Vec<(String, ObjRef)>,
    pub fs_query_listeners: Vec<(FsQuery, ObjRef)>,
    pub fs_set_options: HashMap<ObjRef, bool>,
    pub fs_counter: u32,
    // RTDB
    pub rtdb_obj: Option<ObjRef>,
    pub rtdb_url: String,
    pub rtdb_refs: HashMap<ObjRef, String>,
    pub rtdb_values: HashMap<String, Value>,
    pub rtdb_listeners: Vec<(String, ObjRef)>,
    pub rtdb_snaps: HashMap<ObjRef, String>,
    pub rtdb_counter: u32,
    // Remote Config
    pub rc_obj: Option<ObjRef>,
    pub rc_server: Vec<(String, String)>,
    pub rc_pending: Vec<(String, String)>,
    pub rc_values: Vec<(String, String)>,
    pub rc_defaults: Vec<(String, String)>,
    pub rc_value_objs: HashMap<ObjRef, (String, i32)>,
    // Storage
    pub storage_obj: Option<ObjRef>,
    pub storage_refs: HashMap<ObjRef, String>,
    pub storage_blobs: HashMap<String, Vec<u8>>,
    pub storage_list_results: HashMap<ObjRef, Vec<ObjRef>>,
    pub upload_snaps: HashMap<ObjRef, i64>,
    // Location
    pub fused_client: Option<ObjRef>,
    pub location_fix: Option<FixCfg>,
    pub location_requests: Vec<(ObjRef, LocReq)>,
    pub loc_request_objs: HashMap<ObjRef, LocReq>,
    pub loc_results: HashMap<ObjRef, Vec<ObjRef>>,
    pub loc_availability: HashMap<ObjRef, bool>,
    // Billing
    pub billing_catalog: Vec<SkuCfg>,
    pub billing_purchases: Vec<PurchaseRec>,
    pub billing_clients: HashMap<ObjRef, BillingClientHost>,
    pub billing_builders: HashMap<ObjRef, BillingBuilder>,
    pub billing_results: HashMap<ObjRef, (i32, String)>,
    pub sku_details_objs: HashMap<ObjRef, SkuCfg>,
    pub product_details_objs: HashMap<ObjRef, SkuCfg>,
    pub purchase_by_obj: HashMap<ObjRef, usize>,
    pub purchases_results: HashMap<ObjRef, (i32, Vec<ObjRef>)>,
    pub billing_flow_outcome: BillingOutcome,
    pub billing_setup_code: i32,
    pub billing_counter: u32,
    // Ads (`ad_load_fail=false` ⇒ loads têm sucesso)
    pub ads_initialized: bool,
    pub ad_load_fail: bool,
    pub ad_error_code: i32,
    pub ad_events: Vec<String>,
    pub ad_views: HashMap<ObjRef, AdViewHost>,
    pub full_screen_ads: HashMap<ObjRef, FullScreenAd>,
    pub ad_errors: HashMap<ObjRef, (i32, String)>,
    pub reward_items: HashMap<ObjRef, (i32, String)>,
    // Maps
    pub latlngs: HashMap<ObjRef, (f64, f64)>,
    pub camera_updates: HashMap<ObjRef, CameraUpd>,
    pub camera_positions: HashMap<ObjRef, CameraPos>,
    pub camera_pos_builders: HashMap<ObjRef, CameraPos>,
    pub google_maps: HashMap<ObjRef, MapHost>,
    pub map_by_fragment: HashMap<ObjRef, ObjRef>,
    pub marker_options: HashMap<ObjRef, MarkerCfg>,
    pub markers: HashMap<ObjRef, MarkerCfg>,
    pub marker_map: HashMap<ObjRef, ObjRef>,
    pub maps_counter: u32,
    pub maps_initialized: i32,
    // Fit
    pub fit_steps: i32,
    pub fit_clients: HashMap<ObjRef, String>,
    pub fit_inserts: i32,
    pub fit_sessions_started: Vec<String>,
    pub fit_step_objs: HashMap<ObjRef, i32>,
    // Cast
    pub cast_context: Option<ObjRef>,
    pub cast_device_name: Option<String>,
    pub cast_session_manager: Option<ObjRef>,
    pub cast_session: Option<ObjRef>,
    pub cast_listeners: Vec<ObjRef>,
    // Nearby
    pub nearby_endpoints: Vec<(String, String)>,
    pub nearby_clients: HashMap<ObjRef, String>,
    pub nearby_advertising: Option<String>,
    pub nearby_discovering: bool,
    pub nearby_connections: Vec<String>,
    pub nearby_payloads: Vec<usize>,
    pub payload_objs: HashMap<ObjRef, usize>,
    pub endpoint_info: HashMap<ObjRef, String>,
    pub nearby_messages: HashMap<ObjRef, Vec<u8>>,
    // Integridade
    pub safetynet_client: Option<ObjRef>,
    pub attestations: HashMap<ObjRef, String>,
    pub integrity_managers: Vec<ObjRef>,
    pub integrity_requests: HashMap<ObjRef, String>,
    pub integrity_responses: HashMap<ObjRef, String>,
    pub integrity_token: String,
    pub integrity_nonces: Vec<String>,
    pub attestation_jws: String,
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
fn last_oarg(args: &[Value]) -> Option<ObjRef> {
    args.iter().rev().find_map(|v| match v {
        Value::Obj(r) => Some(*r),
        _ => None,
    })
}
fn iarg(args: &[Value], i: usize) -> i32 {
    args.get(i).and_then(|v| v.as_int().ok()).unwrap_or(0)
}
fn larg(args: &[Value], i: usize) -> i64 {
    args.get(i).and_then(|v| v.as_long().ok()).unwrap_or(0)
}
fn darg(args: &[Value], i: usize) -> f64 {
    args.get(i).and_then(|v| v.as_double().ok()).unwrap_or(0.0)
}
fn farg(args: &[Value], i: usize) -> f32 {
    match args.get(i) {
        Some(Value::Float(f)) => *f,
        Some(Value::Double(d)) => *d as f32,
        _ => 0.0,
    }
}
fn alloc(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    Ok(vm.heap.alloc_instance(class.to_string(), Vec::new())?)
}
fn astr(vm: &mut Engine, s: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(intrinsics::alloc_string(vm, s)?))
}
fn put(vm: &mut Engine, r: ObjRef, field: &str, v: Value) {
    let _ = vm.heap.put_field(r, field, v);
}
fn field_str(vm: &Engine, r: ObjRef, field: &str) -> Option<String> {
    match vm.heap.get_field(r, field) {
        Ok(Value::Obj(s)) => vm.heap.as_str(s).ok().map(str::to_string),
        _ => None,
    }
}
fn make_exception(vm: &mut Engine, class: &str, msg: &str) -> Result<Value, VmExit> {
    let r = alloc(vm, class)?;
    let m = intrinsics::alloc_string(vm, msg.to_string())?;
    put(vm, r, "message", Value::Obj(m));
    Ok(Value::Obj(r))
}
fn exception_message(vm: &Engine, v: &Value) -> String {
    match v {
        Value::Obj(r) => field_str(vm, *r, "message")
            .unwrap_or_else(|| vm.heap.class_of(*r).unwrap_or("?").to_string()),
        Value::Null => "null exception".to_string(),
        other => format!("{other:?}"),
    }
}
fn box_bool(vm: &mut Engine, b: bool) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Boolean;",
        Value::Int(b as i32),
    )?))
}
fn box_long(vm: &mut Engine, n: i64) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Long;",
        Value::Long(n),
    )?))
}
fn box_double(vm: &mut Engine, n: f64) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Double;",
        Value::Double(n),
    )?))
}
#[allow(dead_code)]
fn box_int(vm: &mut Engine, n: i32) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::boxes::box_value(
        vm,
        "Ljava/lang/Integer;",
        Value::Int(n),
    )?))
}
/// Valor primitivo dentro de um wrapper (`Integer`, `Long`, ...), quando
/// o objeto é um box host; senão o próprio valor.
fn unbox(vm: &Engine, v: &Value) -> Value {
    if let Value::Obj(r) = v {
        let cls = vm.heap.class_of(*r).unwrap_or("");
        if matches!(
            cls,
            "Ljava/lang/Integer;"
                | "Ljava/lang/Long;"
                | "Ljava/lang/Double;"
                | "Ljava/lang/Float;"
                | "Ljava/lang/Boolean;"
                | "Ljava/lang/Short;"
                | "Ljava/lang/Byte;"
                | "Ljava/lang/Character;"
        ) {
            if let Ok(inner) = vm.heap.get_field(*r, "value") {
                return inner;
            }
        }
    }
    v.clone()
}
fn value_display(vm: &Engine, v: &Value) -> String {
    match unbox(vm, v) {
        Value::Int(n) => n.to_string(),
        Value::Long(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(d) => d.to_string(),
        Value::Obj(r) => vm
            .heap
            .as_str(r)
            .map(str::to_string)
            .unwrap_or_else(|_| format!("<{}#{r}>", vm.heap.class_of(r).unwrap_or("?"))),
        Value::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}
fn make_list(vm: &mut Engine, elems: Vec<Value>) -> Result<Value, VmExit> {
    let r = alloc(vm, "Ljava/util/ArrayList;")?;
    vm.fw.objects.insert(r, HostObj::List { elems });
    Ok(Value::Obj(r))
}
fn make_map(vm: &mut Engine, entries: Vec<(Value, Value)>) -> Result<Value, VmExit> {
    let r = alloc(vm, "Ljava/util/HashMap;")?;
    vm.fw.objects.insert(r, HostObj::Map { entries });
    Ok(Value::Obj(r))
}
fn read_host_map(vm: &Engine, r: ObjRef) -> Vec<(Value, Value)> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::Map { entries }) => entries.clone(),
        _ => Vec::new(),
    }
}
fn read_host_list(vm: &Engine, r: ObjRef) -> Vec<Value> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::List { elems }) => elems.clone(),
        _ => Vec::new(),
    }
}
fn read_bundle(vm: &Engine, r: ObjRef) -> Vec<(String, Value)> {
    match vm.fw.objects.get(&r) {
        Some(HostObj::Bundle { map }) => map.clone(),
        _ => Vec::new(),
    }
}
fn read_bytes(vm: &mut Engine, v: &Value) -> Vec<u8> {
    match v {
        Value::Obj(r) => match vm.heap.array_elems_mut(*r) {
            Ok(elems) => elems
                .iter()
                .map(|e| e.as_int().unwrap_or(0) as u8)
                .collect(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    }
}
fn make_bytes(vm: &mut Engine, bytes: &[u8]) -> Result<Value, VmExit> {
    let r = vm
        .heap
        .alloc_array(crate::heap::ElemKind::Int, bytes.len(), "[B")?;
    if let Ok(elems) = vm.heap.array_elems_mut(r) {
        elems.clear();
        // issue #250: [B guarda byte COM sinal (-128..=127), como o aput-byte.
        elems.extend(bytes.iter().map(|b| Value::Int(*b as i8 as i32)));
    }
    Ok(Value::Obj(r))
}
/// Invoca um callback DEX tentando assinaturas em ordem (bridges de
/// genéricos apagam para `Object`, mas fixtures podem declarar o tipo
/// concreto). `Ok(None)` = nenhum corpo DEX (objeto host/sintético).
fn call_dex_any(
    vm: &mut Engine,
    recv: ObjRef,
    name: &str,
    sigs: &[&str],
    args: Vec<Value>,
) -> Result<Option<Value>, VmExit> {
    for sig in sigs {
        if let Some(v) = crate::m8::call_dex(vm, recv, name, sig, args.clone())? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}
fn upsert(pairs: &mut Vec<(String, String)>, k: String, v: String) {
    if let Some(e) = pairs.iter_mut().find(|(ek, _)| *ek == k) {
        e.1 = v;
    } else {
        pairs.push((k, v));
    }
}
fn lookup<'a>(pairs: &'a [(String, String)], k: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(ek, _)| ek == k)
        .map(|(_, v)| v.as_str())
}
fn error_string(code: i32) -> String {
    ERROR_STRINGS
        .get(code as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("UNKNOWN({code})"))
}

// ── Tasks API (fundação de todos os serviços) ───────────────────────────
fn new_task_as(vm: &mut Engine, class: &str) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, class)?;
    vm.fw.gms.tasks.insert(
        r,
        TaskHost {
            result: Value::Null,
            ..TaskHost::default()
        },
    );
    Ok(r)
}
fn completed_task(vm: &mut Engine, result: Value) -> Result<ObjRef, VmExit> {
    let r = new_task_as(vm, TASK)?;
    settle_task(vm, r, Some(result), None, false)?;
    Ok(r)
}
fn void_task(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    completed_task(vm, Value::Null)
}
fn failed_task(vm: &mut Engine, class: &str, msg: &str) -> Result<ObjRef, VmExit> {
    let exc = make_exception(vm, class, msg)?;
    let r = new_task_as(vm, TASK)?;
    settle_task(vm, r, None, Some(exc), false)?;
    Ok(r)
}
fn task_state(vm: &Engine, r: ObjRef) -> Option<TaskHost> {
    vm.fw.gms.tasks.get(&r).cloned()
}

/// Completa uma Task (sucesso XOR exceção XOR cancelada), dispara os
/// listeners registrados e processa continuações pendentes. Idempotente:
/// Task já completa não muda (retorna false por `try_settle_task`).
fn try_settle_task(
    vm: &mut Engine,
    r: ObjRef,
    result: Option<Value>,
    exception: Option<Value>,
    canceled: bool,
) -> Result<bool, VmExit> {
    let (success_ls, failure_ls, complete_ls, canceled_ls, res, exc, ok) = {
        let Some(t) = vm.fw.gms.tasks.get_mut(&r) else {
            return Ok(false);
        };
        if t.complete {
            return Ok(false);
        }
        t.complete = true;
        t.canceled = canceled;
        t.successful = !canceled && exception.is_none();
        if let Some(v) = &result {
            t.result = v.clone();
        }
        if let Some(e) = &exception {
            t.exception = Some(e.clone());
        }
        (
            t.success_listeners.clone(),
            t.failure_listeners.clone(),
            t.complete_listeners.clone(),
            t.canceled_listeners.clone(),
            t.result.clone(),
            t.exception.clone(),
            t.successful,
        )
    };
    if canceled {
        for l in canceled_ls {
            let _ = call_dex_any(vm, l, "onCanceled", &["()V"], Vec::new())?;
        }
    } else if ok {
        for l in success_ls {
            let _ = call_dex_any(
                vm,
                l,
                "onSuccess",
                &["(Ljava/lang/Object;)V"],
                vec![res.clone()],
            )?;
        }
    } else if let Some(e) = &exc {
        for l in failure_ls {
            let _ = call_dex_any(
                vm,
                l,
                "onFailure",
                &["(Ljava/lang/Exception;)V", "(Ljava/lang/Object;)V"],
                vec![e.clone()],
            )?;
        }
    }
    for l in complete_ls {
        let _ = call_dex_any(
            vm,
            l,
            "onComplete",
            &[
                "(Lcom/google/android/gms/tasks/Task;)V",
                "(Ljava/lang/Object;)V",
            ],
            vec![Value::Obj(r)],
        )?;
    }
    let pendings = vm.fw.gms.pending.remove(&r).unwrap_or_default();
    for p in pendings {
        run_pending(vm, r, p)?;
    }
    Ok(true)
}
fn settle_task(
    vm: &mut Engine,
    r: ObjRef,
    result: Option<Value>,
    exception: Option<Value>,
    canceled: bool,
) -> Result<(), VmExit> {
    try_settle_task(vm, r, result, exception, canceled).map(|_| ())
}
fn propagation_of(vm: &Engine, src: ObjRef) -> (Option<Value>, Option<Value>, bool) {
    match vm.fw.gms.tasks.get(&src) {
        Some(t) if t.canceled => (None, None, true),
        Some(t) if t.successful => (Some(t.result.clone()), None, false),
        Some(t) => (None, t.exception.clone(), false),
        None => (None, None, false),
    }
}
fn run_pending(vm: &mut Engine, src: ObjRef, p: Pending) -> Result<(), VmExit> {
    match p {
        Pending::Flatten { out } => {
            let (res, exc, canceled) = propagation_of(vm, src);
            settle_task(vm, out, res, exc, canceled)
        }
        Pending::WhenAll {
            out,
            all,
            success_list,
            never_fail,
        } => {
            let states: Vec<Option<TaskHost>> = all.iter().map(|r| task_state(vm, *r)).collect();
            if !states
                .iter()
                .all(|s| s.as_ref().is_some_and(|t| t.complete))
            {
                // Re-registra: outro settle cuidará dela.
                vm.fw
                    .gms
                    .pending
                    .entry(src)
                    .or_default()
                    .push(Pending::WhenAll {
                        out,
                        all,
                        success_list,
                        never_fail,
                    });
                return Ok(());
            }
            if never_fail {
                let elems: Vec<Value> = all.iter().map(|r| Value::Obj(*r)).collect();
                let list = make_list(vm, elems)?;
                return settle_task(vm, out, Some(list), None, false);
            }
            if states.iter().flatten().any(|t| t.canceled) {
                return settle_task(vm, out, None, None, true);
            }
            if let Some(t) = states.iter().flatten().find(|t| !t.successful) {
                return settle_task(vm, out, None, t.exception.clone(), false);
            }
            if success_list {
                let elems: Vec<Value> = states.iter().flatten().map(|t| t.result.clone()).collect();
                let list = make_list(vm, elems)?;
                settle_task(vm, out, Some(list), None, false)
            } else {
                settle_task(vm, out, Some(Value::Null), None, false)
            }
        }
        Pending::Cont {
            cont,
            success_only,
            flatten,
            out,
        } => {
            let st = task_state(vm, src).unwrap_or_default();
            if success_only && !st.successful {
                let (res, exc, canceled) = propagation_of(vm, src);
                return settle_task(vm, out, res, exc, canceled);
            }
            let (name, sig, arg) = if success_only {
                (
                    "onSuccess",
                    "(Ljava/lang/Object;)Lcom/google/android/gms/tasks/Task;",
                    st.result.clone(),
                )
            } else {
                (
                    "then",
                    "(Lcom/google/android/gms/tasks/Task;)Ljava/lang/Object;",
                    Value::Obj(src),
                )
            };
            // continuação tipada Object→Object serve ambos os ramos.
            let alt = "(Ljava/lang/Object;)Ljava/lang/Object;";
            match call_dex_any(vm, cont, name, &[sig, alt], vec![arg]) {
                Ok(Some(ret)) => {
                    if flatten {
                        match ret {
                            Value::Obj(inner) if vm.fw.gms.tasks.contains_key(&inner) => {
                                if task_state(vm, inner).is_some_and(|t| t.complete) {
                                    let (res, exc, canceled) = propagation_of(vm, inner);
                                    settle_task(vm, out, res, exc, canceled)
                                } else {
                                    vm.fw
                                        .gms
                                        .pending
                                        .entry(inner)
                                        .or_default()
                                        .push(Pending::Flatten { out });
                                    Ok(())
                                }
                            }
                            other => settle_task(vm, out, Some(other), None, false),
                        }
                    } else {
                        settle_task(vm, out, Some(ret), None, false)
                    }
                }
                Ok(None) => {
                    let exc = make_exception(
                        vm,
                        "Ljava/lang/RuntimeException;",
                        "Continuation has no DEX implementation in this fixture (RustyDroid GMS shim)",
                    )?;
                    settle_task(vm, out, None, Some(exc), false)
                }
                Err(VmExit::Exception(t)) => {
                    let exc = match t.obj {
                        Some(o) => Value::Obj(o),
                        None => make_exception(
                            vm,
                            &t.class,
                            t.message.as_deref().unwrap_or("continuation threw"),
                        )?,
                    };
                    settle_task(vm, out, None, Some(exc), false)
                }
                Err(e) => Err(e),
            }
        }
    }
}
fn add_listener(
    vm: &mut Engine,
    task: ObjRef,
    kind: u8, // 0 success, 1 failure, 2 complete, 3 canceled
    listener: ObjRef,
) -> Result<(), VmExit> {
    let st = {
        let Some(t) = vm.fw.gms.tasks.get_mut(&task) else {
            return Ok(());
        };
        match kind {
            0 => t.success_listeners.push(listener),
            1 => t.failure_listeners.push(listener),
            2 => t.complete_listeners.push(listener),
            _ => t.canceled_listeners.push(listener),
        }
        t.clone()
    };
    if !st.complete {
        return Ok(());
    }
    match (kind, st.canceled, st.successful) {
        (0, false, true) => {
            let _ = call_dex_any(
                vm,
                listener,
                "onSuccess",
                &["(Ljava/lang/Object;)V"],
                vec![st.result.clone()],
            )?;
        }
        (1, false, false) => {
            if let Some(e) = st.exception.clone() {
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onFailure",
                    &["(Ljava/lang/Exception;)V", "(Ljava/lang/Object;)V"],
                    vec![e],
                )?;
            }
        }
        (2, _, _) => {
            let _ = call_dex_any(
                vm,
                listener,
                "onComplete",
                &[
                    "(Lcom/google/android/gms/tasks/Task;)V",
                    "(Ljava/lang/Object;)V",
                ],
                vec![Value::Obj(task)],
            )?;
        }
        (3, true, _) => {
            let _ = call_dex_any(vm, listener, "onCanceled", &["()V"], Vec::new())?;
        }
        _ => {}
    }
    Ok(())
}

fn tasks_static(vm: &mut Engine, name: &str, args: &[Value]) -> Result<Option<Value>, VmExit> {
    match name {
        "forResult" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            completed_task(vm, v).map(|r| Some(Value::Obj(r)))
        }
        "forException" => {
            let exc = args.first().cloned().unwrap_or(Value::Null);
            let r = new_task_as(vm, TASK)?;
            settle_task(vm, r, None, Some(exc), false)?;
            Ok(Some(Value::Obj(r)))
        }
        "forCanceled" => {
            let r = new_task_as(vm, TASK)?;
            settle_task(vm, r, None, None, true)?;
            Ok(Some(Value::Obj(r)))
        }
        "whenAll" | "whenAllSuccess" | "whenAllComplete" => {
            let mut tasks: Vec<ObjRef> = Vec::new();
            for a in args {
                match a {
                    Value::Obj(r) if vm.fw.gms.tasks.contains_key(r) => tasks.push(*r),
                    Value::Obj(r) => {
                        for v in read_host_list(vm, *r) {
                            if let Value::Obj(t) = v {
                                if vm.fw.gms.tasks.contains_key(&t) {
                                    tasks.push(t);
                                }
                            }
                        }
                        let elems = match vm.heap.array_elems_mut(*r) {
                            Ok(e) => e.clone(),
                            Err(_) => Vec::new(),
                        };
                        for v in elems {
                            if let Value::Obj(t) = v {
                                if vm.fw.gms.tasks.contains_key(&t) {
                                    tasks.push(t);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            let out = new_task_as(vm, TASK)?;
            let p = Pending::WhenAll {
                out,
                all: tasks.clone(),
                success_list: name == "whenAllSuccess",
                never_fail: name == "whenAllComplete",
            };
            if tasks.is_empty()
                || tasks
                    .iter()
                    .all(|r| task_state(vm, *r).is_some_and(|t| t.complete))
            {
                // Resolve já: fonte sintética qualquer serve de gatilho.
                if tasks.is_empty() {
                    // whenAllSuccess/whenAllComplete vendem a lista vazia
                    // (upstream); whenAll vende Void (null).
                    let v = if name == "whenAll" {
                        Value::Null
                    } else {
                        make_list(vm, Vec::new())?
                    };
                    settle_task(vm, out, Some(v), None, false)?;
                } else {
                    let src = tasks[0];
                    vm.fw.gms.pending.entry(src).or_default().push(p);
                    let pendings = vm.fw.gms.pending.remove(&src).unwrap_or_default();
                    for p in pendings {
                        run_pending(vm, src, p)?;
                    }
                }
            } else {
                for t in &tasks {
                    vm.fw.gms.pending.entry(*t).or_default().push(p.clone());
                }
            }
            Ok(Some(Value::Obj(out)))
        }
        "await" => {
            let Some(r) = oarg(args, 0) else {
                return Err(npe("Tasks.await(null)"));
            };
            let Some(t) = task_state(vm, r) else {
                return Ok(None);
            };
            task_result_or_throw(vm, &t, true).map(Some)
        }
        _ => Ok(None),
    }
}

fn task_result_or_throw(vm: &Engine, t: &TaskHost, wrap: bool) -> Result<Value, VmExit> {
    if !t.complete {
        return Err(throw(
            "Ljava/lang/IllegalStateException;",
            "Task is not yet complete",
        ));
    }
    if t.canceled {
        return Err(throw(
            "Ljava/util/concurrent/CancellationException;",
            "Task was cancelled",
        ));
    }
    if !t.successful {
        let msg = t
            .exception
            .as_ref()
            .map(|e| exception_message(vm, e))
            .unwrap_or_else(|| "task failed".to_string());
        return Err(throw(
            if wrap {
                "Ljava/util/concurrent/ExecutionException;"
            } else {
                // Task.getResult() upstream envolve sempre a causa em
                // RuntimeExecutionException.
                "Lcom/google/android/gms/tasks/RuntimeExecutionException;"
            },
            msg,
        ));
    }
    Ok(t.result.clone())
}

fn task_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let Some(st) = task_state(vm, recv) else {
        return Ok(None);
    };
    Ok(Some(match name {
        "isComplete" => Value::Int(st.complete as i32),
        "isSuccessful" => Value::Int((st.complete && st.successful) as i32),
        "isCanceled" => Value::Int(st.canceled as i32),
        "getResult" => task_result_or_throw(vm, &st, false)?,
        "getException" => st.exception.clone().unwrap_or(Value::Null),
        "addOnSuccessListener" => {
            if let Some(l) = last_oarg(args) {
                add_listener(vm, recv, 0, l)?;
            }
            Value::Obj(recv)
        }
        "addOnFailureListener" => {
            if let Some(l) = last_oarg(args) {
                add_listener(vm, recv, 1, l)?;
            }
            Value::Obj(recv)
        }
        "addOnCompleteListener" => {
            if let Some(l) = last_oarg(args) {
                add_listener(vm, recv, 2, l)?;
            }
            Value::Obj(recv)
        }
        "addOnCanceledListener" => {
            if let Some(l) = last_oarg(args) {
                add_listener(vm, recv, 3, l)?;
            }
            Value::Obj(recv)
        }
        "continueWith" | "continueWithTask" | "onSuccessTask" => {
            let Some(cont) = last_oarg(args) else {
                return Err(npe("continuation == null"));
            };
            let out = new_task_as(vm, TASK)?;
            let p = Pending::Cont {
                cont,
                success_only: name == "onSuccessTask",
                flatten: name != "continueWith",
                out,
            };
            if st.complete {
                run_pending(vm, recv, p)?;
            } else {
                vm.fw.gms.pending.entry(recv).or_default().push(p);
            }
            Value::Obj(out)
        }
        _ => return Ok(None),
    }))
}

fn task_source_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if name == "<init>" {
        let t = new_task_as(vm, TASK)?;
        vm.fw.gms.task_sources.insert(recv, t);
        return Ok(Some(Value::Null));
    }
    let Some(task) = vm.fw.gms.task_sources.get(&recv).copied() else {
        return Ok(None);
    };
    Ok(Some(match name {
        "getTask" => Value::Obj(task),
        "setResult" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            if !try_settle_task(vm, task, Some(v), None, false)? {
                return Err(throw(
                    "Ljava/lang/IllegalStateException;",
                    "Task is already complete",
                ));
            }
            Value::Null
        }
        "setException" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            if !try_settle_task(vm, task, None, Some(v), false)? {
                return Err(throw(
                    "Ljava/lang/IllegalStateException;",
                    "Task is already complete",
                ));
            }
            Value::Null
        }
        "trySetResult" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            Value::Int(try_settle_task(vm, task, Some(v), None, false)? as i32)
        }
        "trySetException" => {
            let v = args.first().cloned().unwrap_or(Value::Null);
            Value::Int(try_settle_task(vm, task, None, Some(v), false)? as i32)
        }
        "setCanceled" | "trySetCanceled" => {
            Value::Int(try_settle_task(vm, task, None, None, true)? as i32)
        }
        _ => return Ok(None),
    }))
}

// ── Common: disponibilidade, Status, ConnectionResult, GoogleApiClient ──
fn make_status(vm: &mut Engine, code: i32, msg: &str) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, STATUS)?;
    vm.fw.gms.statuses.insert(r, (code, msg.to_string()));
    Ok(r)
}
fn make_api_exception(vm: &mut Engine, code: i32, msg: &str) -> Result<Value, VmExit> {
    let st = make_status(vm, code, msg)?;
    let r = alloc(vm, API_EXCEPTION)?;
    let m = intrinsics::alloc_string(vm, format!("{code}: {msg}"))?;
    put(vm, r, "message", Value::Obj(m));
    vm.fw.gms.api_exceptions.insert(r, st);
    Ok(Value::Obj(r))
}

fn common_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Status
    if let Some((code, msg)) = vm.fw.gms.statuses.get(&recv).cloned() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getStatusCode" => Value::Int(code),
            "getStatus" => Value::Obj(recv),
            "getStatusMessage" => {
                if msg.is_empty() {
                    Value::Null
                } else {
                    astr(vm, msg)?
                }
            }
            "isSuccess" => Value::Int((code == 0) as i32),
            "isCanceled" => Value::Int((code == 16) as i32),
            "isInterrupted" => Value::Int((code == 14) as i32),
            "hasResolution" => Value::Int(0),
            _ => return Ok(None),
        }));
    }
    if (mref_class == STATUS || vm.heap.class_of(recv).unwrap_or("") == STATUS) && name == "<init>"
    {
        let code = iarg(args, 0);
        let msg = sarg(vm, args, 1).unwrap_or_default();
        vm.fw.gms.statuses.insert(recv, (code, msg));
        return Ok(Some(Value::Null));
    }
    // ApiException
    if let Some(st) = vm.fw.gms.api_exceptions.get(&recv).copied() {
        let (code, _) = vm
            .fw
            .gms
            .statuses
            .get(&st)
            .cloned()
            .unwrap_or((0, String::new()));
        return Ok(Some(match name {
            "getStatusCode" => Value::Int(code),
            "getStatus" => Value::Obj(st),
            _ => return Ok(None),
        }));
    }
    if mref_class == API_EXCEPTION && name == "<init>" {
        if let Some(st) = oarg(args, 0) {
            vm.fw.gms.api_exceptions.insert(recv, st);
            let (code, msg) = vm
                .fw
                .gms
                .statuses
                .get(&st)
                .cloned()
                .unwrap_or((0, String::new()));
            let m = intrinsics::alloc_string(vm, format!("{code}: {msg}"))?;
            put(vm, recv, "message", Value::Obj(m));
        }
        return Ok(Some(Value::Null));
    }
    // ConnectionResult
    if let Some(code) = vm.fw.gms.conn_results.get(&recv).copied() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getErrorCode" => Value::Int(code),
            "isSuccess" => Value::Int((code == 0) as i32),
            "hasResolution" => Value::Int(0),
            "getErrorMessage" => astr(vm, error_string(code))?,
            _ => return Ok(None),
        }));
    }
    if mref_class == CONNECTION_RESULT && name == "<init>" {
        vm.fw.gms.conn_results.insert(recv, iarg(args, 0));
        return Ok(Some(Value::Null));
    }
    // GoogleApiAvailability (instância)
    if vm.fw.gms.availability_obj == Some(recv) || mref_class == AVAILABILITY {
        let code = vm.fw.gms.availability_code;
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "isGooglePlayServicesAvailable" => Value::Int(code),
            "isUserResolvableError" => Value::Int(matches!(iarg(args, 0), 1 | 2 | 3 | 18) as i32),
            "getErrorString" => astr(vm, error_string(iarg(args, 0)))?,
            "makeGooglePlayServicesAvailable" => {
                if code == 0 {
                    Value::Obj(void_task(vm)?)
                } else {
                    let exc = make_api_exception(vm, code, &error_string(code))?;
                    let t = new_task_as(vm, TASK)?;
                    settle_task(vm, t, None, Some(exc), false)?;
                    Value::Obj(t)
                }
            }
            "showErrorNotification" => {
                vm.fw.gms.error_notifications += 1;
                Value::Null
            }
            "showErrorDialogFragment" => Value::Int(0),
            _ => return Ok(None),
        }));
    }
    // GoogleApiClient + Builder
    if vm.fw.gms.api_client_builders.contains_key(&recv) {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "build" => {
                let cbs = vm
                    .fw
                    .gms
                    .api_client_builders
                    .get(&recv)
                    .cloned()
                    .unwrap_or_default();
                let client = alloc(vm, GOOGLE_API_CLIENT)?;
                // Reusa BillingClientHost pelo campo de listener não; guarda callbacks à parte.
                vm.fw
                    .gms
                    .api_clients
                    .insert(client, BillingClientHost::default());
                vm.fw.gms.api_client_builders.insert(client, cbs);
                Value::Obj(client)
            }
            _ => {
                // addApi/addConnectionCallbacks/...: coleta objetos (callbacks) e devolve this.
                let extra: Vec<ObjRef> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Obj(r) => Some(*r),
                        _ => None,
                    })
                    .collect();
                vm.fw
                    .gms
                    .api_client_builders
                    .entry(recv)
                    .or_default()
                    .extend(extra);
                Value::Obj(recv)
            }
        }));
    }
    if mref_class == GOOGLE_API_CLIENT_BUILDER && name == "<init>" {
        vm.fw.gms.api_client_builders.insert(recv, Vec::new());
        return Ok(Some(Value::Null));
    }
    if vm.fw.gms.api_clients.contains_key(&recv) {
        let connected = vm
            .fw
            .gms
            .api_clients
            .get(&recv)
            .is_some_and(|c| c.connected);
        return Ok(Some(match name {
            "connect" => {
                let ok = vm.fw.gms.availability_code == 0;
                if let Some(c) = vm.fw.gms.api_clients.get_mut(&recv) {
                    c.connected = ok;
                }
                if ok {
                    let cbs = vm
                        .fw
                        .gms
                        .api_client_builders
                        .get(&recv)
                        .cloned()
                        .unwrap_or_default();
                    for cb in cbs {
                        let _ = call_dex_any(
                            vm,
                            cb,
                            "onConnected",
                            &["(Landroid/os/Bundle;)V"],
                            vec![Value::Null],
                        )?;
                    }
                }
                Value::Null
            }
            "disconnect" => {
                if let Some(c) = vm.fw.gms.api_clients.get_mut(&recv) {
                    c.connected = false;
                }
                Value::Null
            }
            "isConnected" => Value::Int(connected as i32),
            "isConnecting" => Value::Int(0),
            "reconnect" => Value::Null,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

// ── Firebase core ───────────────────────────────────────────────────────
fn default_fb_options(vm: &Engine) -> FbOptionsCfg {
    vm.fw
        .gms
        .fb_default_options
        .clone()
        .unwrap_or_else(FbOptionsCfg::fake_default)
}
fn ensure_default_app(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    if let Some(app) = vm.fw.gms.fb_apps.get("[DEFAULT]").copied() {
        return Ok(app);
    }
    if vm.fw.gms.fb_strict_init {
        return Err(throw(
            "Ljava/lang/IllegalStateException;",
            "Default FirebaseApp is not initialized in this process. Call FirebaseApp.initializeApp(Context) first.",
        ));
    }
    let opts = default_fb_options(vm);
    create_fb_app(vm, "[DEFAULT]", opts)
}
fn create_fb_app(vm: &mut Engine, name: &str, opts: FbOptionsCfg) -> Result<ObjRef, VmExit> {
    if let Some(app) = vm.fw.gms.fb_apps.get(name).copied() {
        return Ok(app);
    }
    let app = alloc(vm, FIREBASE_APP)?;
    vm.fw.gms.fb_apps.insert(name.to_string(), app);
    vm.fw.gms.fb_app_names.insert(app, name.to_string());
    vm.fw.gms.fb_app_options.insert(app, opts);
    vm.fw.gms.fb_initialized.push(name.to_string());
    Ok(app)
}
fn options_obj(vm: &mut Engine, cfg: &FbOptionsCfg) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, FIREBASE_OPTIONS)?;
    vm.fw.gms.fb_options_objs.insert(r, cfg.clone());
    Ok(r)
}
fn firebase_core_static(
    vm: &mut Engine,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "initializeApp" => {
            if let Some(ctx) = oarg(args, 0) {
                vm.fw.gms.fb_context = Some(ctx);
            }
            let opts = oarg(args, 1)
                .and_then(|r| vm.fw.gms.fb_options_objs.get(&r).cloned())
                .unwrap_or_else(|| default_fb_options(vm));
            let name = sarg(vm, args, 2).unwrap_or_else(|| "[DEFAULT]".to_string());
            let app = create_fb_app(vm, &name, opts)?;
            Ok(Some(Value::Obj(app)))
        }
        "getInstance" => {
            let name = sarg(vm, args, 0);
            match name {
                None => ensure_default_app(vm).map(|a| Some(Value::Obj(a))),
                Some(n) => match vm.fw.gms.fb_apps.get(&n).copied() {
                    Some(a) => Ok(Some(Value::Obj(a))),
                    None => Err(throw(
                        "Ljava/lang/IllegalStateException;",
                        format!("FirebaseApp with name {n} doesn't exist."),
                    )),
                },
            }
        }
        "getApps" => {
            let elems: Vec<Value> = vm
                .fw
                .gms
                .fb_app_names
                .keys()
                .map(|r| Value::Obj(*r))
                .collect();
            make_list(vm, elems).map(Some)
        }
        _ => Ok(None),
    }
}
fn firebase_core_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(app_name) = vm.fw.gms.fb_app_names.get(&recv).cloned() {
        return Ok(Some(match name {
            "getName" => astr(vm, app_name)?,
            "getOptions" => {
                let cfg = vm
                    .fw
                    .gms
                    .fb_app_options
                    .get(&recv)
                    .cloned()
                    .unwrap_or_else(|| default_fb_options(vm));
                Value::Obj(options_obj(vm, &cfg)?)
            }
            "getApplicationContext" => vm.fw.gms.fb_context.map(Value::Obj).unwrap_or(Value::Null),
            "delete" => {
                vm.fw.gms.fb_apps.remove(&app_name);
                vm.fw.gms.fb_app_names.remove(&recv);
                vm.fw.gms.fb_app_options.remove(&recv);
                Value::Null
            }
            "setAutomaticResourceManagementEnabled" => Value::Null,
            "isDataCollectionDefaultEnabled" => Value::Int(0),
            "setDataCollectionDefaultEnabled" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some(cfg) = vm.fw.gms.fb_options_objs.get(&recv).cloned() {
        return Ok(Some(match name {
            "getApiKey" => astr(vm, cfg.api_key)?,
            "getApplicationId" => astr(vm, cfg.application_id)?,
            "getProjectId" => astr(vm, cfg.project_id)?,
            "getDatabaseUrl" => astr(vm, cfg.database_url)?,
            "getStorageBucket" => astr(vm, cfg.storage_bucket)?,
            "getGcmSenderId" => astr(vm, cfg.gcm_sender_id)?,
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.fb_options_builders.contains_key(&recv) || mref_class == FIREBASE_OPTIONS_BUILDER {
        if name == "<init>" {
            vm.fw
                .gms
                .fb_options_builders
                .insert(recv, FbOptionsCfg::default());
            return Ok(Some(Value::Null));
        }
        if name == "build" {
            let cfg = vm
                .fw
                .gms
                .fb_options_builders
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            return options_obj(vm, &cfg).map(|r| Some(Value::Obj(r)));
        }
        let v = sarg(vm, args, 0).unwrap_or_default();
        let cfg = vm.fw.gms.fb_options_builders.entry(recv).or_default();
        match name {
            "setApiKey" => cfg.api_key = v,
            "setApplicationId" => cfg.application_id = v,
            "setProjectId" => cfg.project_id = v,
            "setDatabaseUrl" => cfg.database_url = v,
            "setStorageBucket" => cfg.storage_bucket = v,
            "setGcmSenderId" => cfg.gcm_sender_id = v,
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    Ok(None)
}

// ── Analytics ───────────────────────────────────────────────────────────
fn analytics_instance(
    vm: &mut Engine,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    Ok(Some(match name {
        "<init>" => Value::Null,
        "logEvent" => {
            let ev = sarg(vm, args, 0).unwrap_or_default();
            let params = match oarg(args, 1) {
                Some(b) => read_bundle(vm, b)
                    .into_iter()
                    .map(|(k, v)| (k, value_display(vm, &v)))
                    .collect(),
                None => Vec::new(),
            };
            if !vm.fw.gms.analytics_disabled {
                let mut all = vm.fw.gms.analytics_defaults.clone();
                for (k, v) in params {
                    upsert(&mut all, k, v);
                }
                vm.fw.gms.analytics_events.push(AnalyticsEvent {
                    name: ev,
                    params: all,
                });
            }
            Value::Null
        }
        "setUserProperty" => {
            if let (Some(k), v) = (sarg(vm, args, 0), sarg(vm, args, 1)) {
                match v {
                    Some(val) => upsert(&mut vm.fw.gms.analytics_user_properties, k, val),
                    None => vm
                        .fw
                        .gms
                        .analytics_user_properties
                        .retain(|(ek, _)| *ek != k),
                }
            }
            Value::Null
        }
        "setUserId" => {
            vm.fw.gms.analytics_user_id = sarg(vm, args, 0);
            Value::Null
        }
        "setAnalyticsCollectionEnabled" => {
            vm.fw.gms.analytics_disabled = iarg(args, 0) == 0;
            Value::Null
        }
        "setDefaultEventParameters" => {
            vm.fw.gms.analytics_defaults = match oarg(args, 0) {
                Some(b) => read_bundle(vm, b)
                    .into_iter()
                    .map(|(k, v)| (k, value_display(vm, &v)))
                    .collect(),
                None => Vec::new(),
            };
            Value::Null
        }
        "getAppInstanceId" => {
            let id = astr(vm, "rustydroid-app-instance-id".to_string())?;
            Value::Obj(completed_task(vm, id)?)
        }
        "getSessionId" => {
            let v = box_long(vm, 1)?;
            Value::Obj(completed_task(vm, v)?)
        }
        "resetAnalyticsData" => {
            vm.fw.gms.analytics_events.clear();
            vm.fw.gms.analytics_user_properties.clear();
            vm.fw.gms.analytics_user_id = None;
            Value::Null
        }
        "setConsent" | "setCurrentScreen" | "setSessionTimeoutDuration" => Value::Null,
        _ => return Ok(None),
    }))
}

// ── Firebase Auth ───────────────────────────────────────────────────────
fn mint_fb_user(vm: &mut Engine, idx: usize) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, FIREBASE_USER)?;
    vm.fw.gms.auth_user_by_obj.insert(r, idx);
    Ok(r)
}
fn auth_succeed(vm: &mut Engine, idx: usize) -> Result<Value, VmExit> {
    let user = mint_fb_user(vm, idx)?;
    vm.fw.gms.auth_current_user = Some(user);
    let result = alloc(vm, AUTH_RESULT)?;
    vm.fw.gms.auth_result_users.insert(result, user);
    fire_auth_listeners(vm)?;
    Ok(Value::Obj(completed_task(vm, Value::Obj(result))?))
}
fn auth_fail(vm: &mut Engine, class: &str, msg: &str) -> Result<Value, VmExit> {
    Ok(Value::Obj(failed_task(vm, class, msg)?))
}
fn fire_auth_listeners(vm: &mut Engine) -> Result<(), VmExit> {
    fire_guarded(vm, PendingFire::Auth)
}

fn fire_auth_listeners_now(vm: &mut Engine) -> Result<(), VmExit> {
    let Some(auth) = vm.fw.gms.auth_obj else {
        return Ok(());
    };
    let listeners = vm.fw.gms.auth_listeners.clone();
    for l in listeners {
        let _ = call_dex_any(
            vm,
            l,
            "onAuthStateChanged",
            &["(Lcom/google/firebase/auth/FirebaseAuth;)V"],
            vec![Value::Obj(auth)],
        )?;
    }
    Ok(())
}
fn fb_auth_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // FirebaseUser / AuthResult / GetTokenResult primeiro (objetos-filhos).
    if let Some(user) = vm.fw.gms.auth_result_users.get(&recv).copied() {
        return Ok(Some(match name {
            "getUser" => Value::Obj(user),
            "getAdditionalUserInfo" => Value::Null,
            "getCredential" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some(tok) = vm.fw.gms.token_results.get(&recv).cloned() {
        return Ok(Some(match name {
            "getToken" => astr(vm, tok)?,
            "getAuthTimestamp" => Value::Long(1_700_000_000_000),
            "getIssuedAtTimestamp" => Value::Long(1_700_000_000_000),
            "getExpirationTimestamp" => Value::Long(1_700_003_600_000),
            "getSignInProvider" => astr(vm, "fake".to_string())?,
            "getClaims" => make_map(vm, Vec::new())?,
            _ => return Ok(None),
        }));
    }
    if let Some(idx) = vm.fw.gms.auth_user_by_obj.get(&recv).copied() {
        let rec = vm.fw.gms.auth_users.get(idx).cloned().unwrap_or_default();
        return Ok(Some(match name {
            "getUid" => astr(vm, rec.uid)?,
            "getEmail" => {
                if rec.email.is_empty() {
                    Value::Null
                } else {
                    astr(vm, rec.email)?
                }
            }
            "getDisplayName" => {
                if rec.display_name.is_empty() {
                    Value::Null
                } else {
                    astr(vm, rec.display_name)?
                }
            }
            "isAnonymous" => Value::Int(rec.anonymous as i32),
            "isEmailVerified" => Value::Int(0),
            "getProviderId" => astr(vm, "firebase".to_string())?,
            "getIdToken" => {
                let tok = format!("fake-id-token-{}", rec.uid);
                let r = alloc(vm, GET_TOKEN_RESULT)?;
                vm.fw.gms.token_results.insert(r, tok);
                Value::Obj(completed_task(vm, Value::Obj(r))?)
            }
            "reload" => Value::Obj(void_task(vm)?),
            "delete" => {
                if let Some(u) = vm.fw.gms.auth_users.get_mut(idx) {
                    u.deleted = true;
                }
                if vm.fw.gms.auth_current_user == Some(recv) {
                    vm.fw.gms.auth_current_user = None;
                    fire_auth_listeners(vm)?;
                }
                Value::Obj(void_task(vm)?)
            }
            "sendEmailVerification" => Value::Obj(void_task(vm)?),
            _ => return Ok(None),
        }));
    }
    Ok(Some(match name {
        "<init>" => Value::Null,
        "getCurrentUser" => vm
            .fw
            .gms
            .auth_current_user
            .map(Value::Obj)
            .unwrap_or(Value::Null),
        "signInAnonymously" => {
            vm.fw.gms.auth_counter += 1;
            let n = vm.fw.gms.auth_counter;
            vm.fw.gms.auth_users.push(AuthUserRec {
                uid: format!("anon-{n}"),
                anonymous: true,
                ..Default::default()
            });
            let idx = vm.fw.gms.auth_users.len() - 1;
            auth_succeed(vm, idx)?
        }
        "createUserWithEmailAndPassword" => {
            let email = sarg(vm, args, 0).unwrap_or_default();
            let pw = sarg(vm, args, 1).unwrap_or_default();
            if vm
                .fw
                .gms
                .auth_users
                .iter()
                .any(|u| !u.deleted && u.email == email)
            {
                auth_fail(
                    vm,
                    "Lcom/google/firebase/auth/FirebaseAuthUserCollisionException;",
                    "The email address is already in use by another account.",
                )?
            } else if pw.chars().count() < 6 {
                auth_fail(
                    vm,
                    "Lcom/google/firebase/auth/FirebaseAuthWeakPasswordException;",
                    "The given password is invalid. [ Password should be at least 6 characters ]",
                )?
            } else {
                vm.fw.gms.auth_counter += 1;
                let n = vm.fw.gms.auth_counter;
                vm.fw.gms.auth_users.push(AuthUserRec {
                    uid: format!("uid-{n}"),
                    email,
                    password: pw,
                    ..Default::default()
                });
                let idx = vm.fw.gms.auth_users.len() - 1;
                auth_succeed(vm, idx)?
            }
        }
        "signInWithEmailAndPassword" => {
            let email = sarg(vm, args, 0).unwrap_or_default();
            let pw = sarg(vm, args, 1).unwrap_or_default();
            let found = vm
                .fw
                .gms
                .auth_users
                .iter()
                .position(|u| !u.deleted && u.email == email);
            match found {
                Some(idx) if vm.fw.gms.auth_users[idx].password == pw => auth_succeed(vm, idx)?,
                _ => auth_fail(
                    vm,
                    "Lcom/google/firebase/auth/FirebaseAuthInvalidCredentialsException;",
                    "The supplied auth credential is incorrect, malformed or has expired.",
                )?,
            }
        }
        "signInWithCredential" | "signInWithCustomToken" => {
            vm.fw.gms.auth_counter += 1;
            let n = vm.fw.gms.auth_counter;
            vm.fw.gms.auth_users.push(AuthUserRec {
                uid: format!("cred-{n}"),
                ..Default::default()
            });
            let idx = vm.fw.gms.auth_users.len() - 1;
            auth_succeed(vm, idx)?
        }
        "signOut" => {
            vm.fw.gms.auth_current_user = None;
            fire_auth_listeners(vm)?;
            Value::Null
        }
        "addAuthStateListener" => {
            if let Some(l) = oarg(args, 0) {
                if !vm.fw.gms.auth_listeners.contains(&l) {
                    vm.fw.gms.auth_listeners.push(l);
                }
                let _ = call_dex_any(
                    vm,
                    l,
                    "onAuthStateChanged",
                    &["(Lcom/google/firebase/auth/FirebaseAuth;)V"],
                    vec![Value::Obj(recv)],
                )?;
            }
            Value::Null
        }
        "removeAuthStateListener" => {
            if let Some(l) = oarg(args, 0) {
                vm.fw.gms.auth_listeners.retain(|x| *x != l);
            }
            Value::Null
        }
        "sendPasswordResetEmail" => {
            let email = sarg(vm, args, 0).unwrap_or_default();
            if vm
                .fw
                .gms
                .auth_users
                .iter()
                .any(|u| !u.deleted && u.email == email)
            {
                vm.fw.gms.auth_password_resets.push(email);
                Value::Obj(void_task(vm)?)
            } else {
                auth_fail(
                    vm,
                    "Lcom/google/firebase/auth/FirebaseAuthInvalidUserException;",
                    "There is no user record corresponding to this identifier.",
                )?
            }
        }
        "getAccessToken" => {
            // API legada rara; sem sessão, devolve nulo via DEX (None).
            return Ok(None);
        }
        _ => return Ok(None),
    }))
}

// ── Google Sign-In ──────────────────────────────────────────────────────
fn mint_gms_account(vm: &mut Engine, cfg: &GmsAccountCfg) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, SIGNIN_ACCOUNT)?;
    vm.fw.gms.gms_account_by_obj.insert(r, cfg.clone());
    Ok(r)
}
fn gms_auth_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(cfg) = vm.fw.gms.gms_account_by_obj.get(&recv).cloned() {
        return Ok(Some(match name {
            "getId" => astr(vm, cfg.id)?,
            "getEmail" => astr(vm, cfg.email)?,
            "getDisplayName" => astr(vm, cfg.display_name)?,
            "getIdToken" => {
                if cfg.id_token.is_empty() {
                    Value::Null
                } else {
                    astr(vm, cfg.id_token)?
                }
            }
            "getGivenName" | "getFamilyName" | "getPhotoUrl" | "getServerAuthCode" => Value::Null,
            "getAccount" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.gms_signin_clients.contains(&recv) {
        return Ok(Some(match name {
            "silentSignIn" => {
                if vm.fw.gms.gms_signed_in {
                    match vm.fw.gms.gms_account_cfg.clone() {
                        Some(cfg) => {
                            let acct = mint_gms_account(vm, &cfg)?;
                            Value::Obj(completed_task(vm, Value::Obj(acct))?)
                        }
                        None => {
                            let exc = make_api_exception(vm, 4, "SIGN_IN_REQUIRED")?;
                            let t = new_task_as(vm, TASK)?;
                            settle_task(vm, t, None, Some(exc), false)?;
                            Value::Obj(t)
                        }
                    }
                } else {
                    let exc = make_api_exception(vm, 4, "SIGN_IN_REQUIRED")?;
                    let t = new_task_as(vm, TASK)?;
                    settle_task(vm, t, None, Some(exc), false)?;
                    Value::Obj(t)
                }
            }
            "signOut" | "revokeAccess" => {
                vm.fw.gms.gms_signed_in = false;
                Value::Obj(void_task(vm)?)
            }
            "getSignInIntent" => {
                vm.fw.gms.gms_signin_intents += 1;
                let r = alloc(vm, "Landroid/content/Intent;")?;
                vm.fw.objects.insert(
                    r,
                    HostObj::Intent {
                        component: Some(
                            "com.google.android.gms.auth.api.signin.internal.SignInHubActivity"
                                .to_string(),
                        ),
                        extras: Vec::new(),
                    },
                );
                Value::Obj(r)
            }
            _ => return Ok(None),
        }));
    }
    if mref_class == SIGNIN_OPTIONS_BUILDER || mref_class.contains("GoogleSignInOptions") {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "build" => Value::Obj(alloc(
                vm,
                "Lcom/google/android/gms/auth/api/signin/GoogleSignInOptions;",
            )?),
            _ => Value::Obj(recv),
        }));
    }
    let _ = args;
    Ok(None)
}

// ── Messaging (FCM) ─────────────────────────────────────────────────────
fn fcm_token(vm: &Engine) -> String {
    if vm.fw.gms.fcm_token.is_empty() {
        "rustydroid-fcm-token-0001".to_string()
    } else {
        vm.fw.gms.fcm_token.clone()
    }
}
fn mint_remote_message(vm: &mut Engine, msg: RemoteMsg) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, REMOTE_MESSAGE)?;
    vm.fw.gms.remote_messages.insert(r, msg);
    Ok(r)
}
fn messaging_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if vm.fw.gms.rm_builders.contains_key(&recv) || mref_class == REMOTE_MESSAGE_BUILDER {
        if name == "<init>" {
            let to = sarg(vm, args, 0).unwrap_or_default();
            vm.fw.gms.rm_builders.insert(
                recv,
                RemoteMsg {
                    to,
                    ..Default::default()
                },
            );
            return Ok(Some(Value::Null));
        }
        if name == "build" {
            let msg = vm
                .fw
                .gms
                .rm_builders
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            return mint_remote_message(vm, msg).map(|r| Some(Value::Obj(r)));
        }
        let arg0 = sarg(vm, args, 0).unwrap_or_default();
        let arg1 = sarg(vm, args, 1).unwrap_or_default();
        let data_pairs: Option<Vec<(String, String)>> = oarg(args, 0).map(|m| {
            read_host_map(vm, m)
                .into_iter()
                .map(|(k, v)| (value_display(vm, &k), value_display(vm, &v)))
                .collect()
        });
        let b = vm.fw.gms.rm_builders.entry(recv).or_default();
        match name {
            "addData" => {
                b.data.retain(|(ek, _)| *ek != arg0);
                b.data.push((arg0, arg1));
            }
            "setData" => {
                if let Some(pairs) = data_pairs {
                    b.data = pairs;
                }
            }
            "setMessageId" => b.message_id = arg0,
            "setCollapseKey" => b.collapse_key = arg0,
            "setTtl" => b.ttl = iarg(args, 0),
            "setMessageType" => {}
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if let Some(msg) = vm.fw.gms.remote_messages.get(&recv).cloned() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getFrom" => {
                if msg.from.is_empty() {
                    Value::Null
                } else {
                    astr(vm, msg.from)?
                }
            }
            "getTo" => {
                if msg.to.is_empty() {
                    Value::Null
                } else {
                    astr(vm, msg.to)?
                }
            }
            "getMessageId" => astr(vm, msg.message_id)?,
            "getCollapseKey" => {
                if msg.collapse_key.is_empty() {
                    Value::Null
                } else {
                    astr(vm, msg.collapse_key)?
                }
            }
            "getSentTime" => Value::Long(msg.sent_time),
            "getTtl" => Value::Int(msg.ttl),
            "getData" => {
                let mut entries = Vec::with_capacity(msg.data.len());
                for (k, v) in &msg.data {
                    let ko = intrinsics::alloc_string(vm, k.clone())?;
                    let vo = intrinsics::alloc_string(vm, v.clone())?;
                    entries.push((Value::Obj(ko), Value::Obj(vo)));
                }
                make_map(vm, entries)?
            }
            "getNotification" => {
                if msg.title.is_empty() && msg.body.is_empty() {
                    Value::Null
                } else {
                    let n = alloc(vm, REMOTE_NOTIFICATION)?;
                    // Guarda o par em campos do heap (lidos abaixo).
                    let t = intrinsics::alloc_string(vm, msg.title.clone())?;
                    let b = intrinsics::alloc_string(vm, msg.body.clone())?;
                    put(vm, n, "rd_title", Value::Obj(t));
                    put(vm, n, "rd_body", Value::Obj(b));
                    Value::Obj(n)
                }
            }
            "getTitle" => match field_str(vm, recv, "rd_title") {
                Some(t) => astr(vm, t)?,
                None => Value::Null,
            },
            "getBody" => match field_str(vm, recv, "rd_body") {
                Some(b) => astr(vm, b)?,
                None => Value::Null,
            },
            _ => return Ok(None),
        }));
    }
    if mref_class == REMOTE_MESSAGE && name == "<init>" {
        // RemoteMessage(Bundle): chaves google.* viram metadados, resto é data.
        let mut msg = RemoteMsg::default();
        if let Some(b) = oarg(args, 0) {
            for (k, v) in read_bundle(vm, b) {
                let val = value_display(vm, &v);
                match k.as_str() {
                    "google.message_id" | "message_id" => msg.message_id = val,
                    "from" => msg.from = val,
                    "google.collapse_key" | "collapse_key" => msg.collapse_key = val,
                    "google.notification.title" | "gcm.notification.title" => msg.title = val,
                    "google.notification.body" | "gcm.notification.body" => msg.body = val,
                    _ => {
                        if !k.starts_with("google.") && !k.starts_with("gcm.") {
                            msg.data.push((k, val));
                        }
                    }
                }
            }
        }
        vm.fw.gms.remote_messages.insert(recv, msg);
        return Ok(Some(Value::Null));
    }
    if mref_class == REMOTE_NOTIFICATION {
        return Ok(Some(match name {
            "getTitle" => match field_str(vm, recv, "rd_title") {
                Some(t) => astr(vm, t)?,
                None => Value::Null,
            },
            "getBody" => match field_str(vm, recv, "rd_body") {
                Some(b) => astr(vm, b)?,
                None => Value::Null,
            },
            _ => return Ok(None),
        }));
    }
    Ok(Some(match name {
        "<init>" => Value::Null,
        "getToken" => {
            vm.fw.gms.fcm_token_deleted = false;
            let tok = astr(vm, fcm_token(vm))?;
            Value::Obj(completed_task(vm, tok)?)
        }
        "deleteToken" => {
            vm.fw.gms.fcm_token_deleted = true;
            Value::Obj(void_task(vm)?)
        }
        "subscribeToTopic" => {
            if let Some(t) = sarg(vm, args, 0) {
                if !vm.fw.gms.fcm_topics.contains(&t) {
                    vm.fw.gms.fcm_topics.push(t);
                }
            }
            Value::Obj(void_task(vm)?)
        }
        "unsubscribeFromTopic" => {
            if let Some(t) = sarg(vm, args, 0) {
                vm.fw.gms.fcm_topics.retain(|x| *x != t);
            }
            Value::Obj(void_task(vm)?)
        }
        "setAutoInitEnabled" => {
            vm.fw.gms.fcm_auto_init_disabled = iarg(args, 0) == 0;
            Value::Null
        }
        "isAutoInitEnabled" => Value::Int(!vm.fw.gms.fcm_auto_init_disabled as i32),
        "send" => {
            if let Some(m) = oarg(args, 0) {
                let id = vm
                    .fw
                    .gms
                    .remote_messages
                    .get(&m)
                    .map(|x| x.message_id.clone())
                    .unwrap_or_default();
                vm.fw.gms.fcm_upstream.push(id);
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ── Crashlytics ─────────────────────────────────────────────────────────
fn crashlytics_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if vm.fw.gms.ckav_builders.contains_key(&recv) || mref_class == CKAV_BUILDER {
        if name == "<init>" {
            vm.fw.gms.ckav_builders.insert(recv, Vec::new());
            return Ok(Some(Value::Null));
        }
        if name == "build" {
            let pairs = vm
                .fw
                .gms
                .ckav_builders
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            let r = alloc(vm, CKAV)?;
            vm.fw.gms.ckav_objs.insert(r, pairs);
            return Ok(Some(Value::Obj(r)));
        }
        if let Some(rest) = name.strip_prefix("put") {
            let k = sarg(vm, args, 0).unwrap_or_default();
            let v = args
                .get(1)
                .map(|a| value_display(vm, a))
                .unwrap_or_default();
            let _ = rest;
            upsert(vm.fw.gms.ckav_builders.entry(recv).or_default(), k, v);
            return Ok(Some(Value::Obj(recv)));
        }
        return Ok(None);
    }
    Ok(Some(match name {
        "<init>" => Value::Null,
        "recordException" => {
            if vm.fw.gms.crashlytics_disabled {
                vm.fw.gms.crashlytics_dropped += 1;
                return Ok(Some(Value::Null));
            }
            let (class, message) = match args.first() {
                Some(v @ Value::Obj(r)) => (
                    vm.heap.class_of(*r).unwrap_or("?").to_string(),
                    exception_message(vm, v),
                ),
                _ => ("?".to_string(), String::new()),
            };
            vm.fw.gms.crashes.push(CrashRecord {
                kind: "recorded".to_string(),
                class,
                message,
            });
            Value::Null
        }
        "log" => {
            if vm.fw.gms.crashlytics_disabled {
                vm.fw.gms.crashlytics_dropped += 1;
            } else if let Some(s) = sarg(vm, args, 0) {
                vm.fw.gms.crashlytics_logs.push(s);
            }
            Value::Null
        }
        "setUserId" => {
            vm.fw.gms.crashlytics_user_id = sarg(vm, args, 0);
            Value::Null
        }
        "setCustomKey" => {
            if let Some(k) = sarg(vm, args, 0) {
                let v = args
                    .get(1)
                    .map(|a| value_display(vm, a))
                    .unwrap_or_default();
                upsert(&mut vm.fw.gms.crashlytics_keys, k, v);
            }
            Value::Null
        }
        "setCustomKeys" => {
            if let Some(r) = oarg(args, 0) {
                let pairs = vm.fw.gms.ckav_objs.get(&r).cloned().unwrap_or_default();
                for (k, v) in pairs {
                    upsert(&mut vm.fw.gms.crashlytics_keys, k, v);
                }
            }
            Value::Null
        }
        "setCrashlyticsCollectionEnabled" => {
            if let Some(v) = args.first() {
                let enabled = match unbox(vm, v) {
                    Value::Int(n) => n != 0,
                    Value::Null => true, // null Boolean = usa o manifest
                    _ => true,
                };
                vm.fw.gms.crashlytics_disabled = !enabled;
            }
            Value::Null
        }
        "isCrashlyticsCollectionEnabled" => Value::Int(!vm.fw.gms.crashlytics_disabled as i32),
        "checkForUnsentReports" => {
            let v = box_bool(vm, false)?;
            Value::Obj(completed_task(vm, v)?)
        }
        "sendUnsentReports" | "deleteUnsentReports" => Value::Null,
        "didCrashOnPreviousExecution" => Value::Int(0),
        _ => return Ok(None),
    }))
}

// ── Firestore ───────────────────────────────────────────────────────────
fn fs_normalize(path: &str) -> String {
    path.trim_matches('/').to_string()
}
fn fs_join(base: &str, rel: &str) -> String {
    if base.is_empty() {
        fs_normalize(rel)
    } else {
        fs_normalize(&format!("{}/{}", base, rel.trim_matches('/')))
    }
}
fn fs_last(path: &str) -> String {
    path.rsplit('/').next().unwrap_or("").to_string()
}
fn fs_parent(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((p, _)) => p.to_string(),
        None => String::new(),
    }
}
fn fs_ref_obj(vm: &mut Engine, class: &str, path: String, is_doc: bool) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, class)?;
    vm.fw.gms.fs_refs.insert(r, FsRef { path, is_doc });
    Ok(r)
}
fn fs_doc_snap_obj(vm: &mut Engine, path: &str) -> Result<ObjRef, VmExit> {
    let fields = vm.fw.gms.fs_docs.get(path).cloned();
    let r = alloc(vm, DOC_SNAP)?;
    vm.fw.gms.fs_doc_snaps.insert(
        r,
        FsDocSnap {
            path: path.to_string(),
            exists: fields.is_some(),
            fields: fields.unwrap_or_default(),
        },
    );
    Ok(r)
}
fn fs_run_query(vm: &mut Engine, q: &FsQuery) -> Result<ObjRef, VmExit> {
    let mut paths: Vec<String> = vm
        .fw
        .gms
        .fs_doc_order
        .iter()
        .filter(|p| fs_parent(p) == q.collection)
        .cloned()
        .collect();
    for (field, want) in &q.filters {
        let mut kept = Vec::new();
        for p in paths {
            let got = vm
                .fw
                .gms
                .fs_docs
                .get(&p)
                .and_then(|fs| fs.iter().find(|(k, _)| k == field).map(|(_, v)| v.clone()));
            let keep = match got {
                Some(g) => intrinsics::values_equal(&vm.heap, &g, want).unwrap_or(false),
                None => false,
            };
            if keep {
                kept.push(p);
            }
        }
        paths = kept;
    }
    if let Some(order) = &q.order_by {
        let order = order.clone();
        let docs = vm.fw.gms.fs_docs.clone();
        paths.sort_by(|a, b| {
            let av = docs
                .get(a)
                .and_then(|fs| fs.iter().find(|(k, _)| k == &order).map(|(_, v)| v));
            let bv = docs
                .get(b)
                .and_then(|fs| fs.iter().find(|(k, _)| k == &order).map(|(_, v)| v));
            fs_cmp(av, bv)
        });
    }
    if let Some(n) = q.limit {
        paths.truncate(n.max(0) as usize);
    }
    let mut snaps = Vec::with_capacity(paths.len());
    for p in &paths {
        snaps.push(fs_doc_snap_obj(vm, p)?);
    }
    let r = alloc(vm, QUERY_SNAP)?;
    vm.fw.gms.fs_query_snaps.insert(r, snaps);
    Ok(r)
}
fn fs_cmp(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let key = |v: Option<&Value>| -> (u8, f64, String) {
        match v {
            None | Some(Value::Null) => (0, 0.0, String::new()),
            Some(Value::Int(n)) => (1, *n as f64, String::new()),
            Some(Value::Long(n)) => (1, *n as f64, String::new()),
            Some(Value::Float(f)) => (1, *f as f64, String::new()),
            Some(Value::Double(d)) => (1, *d, String::new()),
            Some(other) => (2, 0.0, format!("{other:?}")),
        }
    };
    let (ka, kb) = (key(a), key(b));
    ka.0.cmp(&kb.0)
        .then(ka.1.partial_cmp(&kb.1).unwrap_or(Equal))
        .then(ka.2.cmp(&kb.2))
}
/// Ponto único de entrega de listeners (Firestore/RTDB/Auth) com guarda
/// de reentrância: a entrega é síncrona, por isso um callback que escreve
/// (e dispara outro evento) NÃO pode recursir nesta pilha — o evento
/// aninhado entra na fila e a chamada exterior drena-a iterativamente,
/// com um teto de eventos por disparo para travar ping-pong infinito
/// entre listeners (o excesso é descartado, nunca entregue com atraso
/// escondido: o teto é alto o suficiente para fluxos reais e o descarte
/// é a alternativa honesta a um abort do processo).
const MAX_FIRES_PER_TRIGGER: usize = 256;

fn fire_guarded(vm: &mut Engine, ev: PendingFire) -> Result<(), VmExit> {
    if vm.fw.gms.firing_listeners {
        vm.fw.gms.pending_fires.push(ev);
        return Ok(());
    }
    vm.fw.gms.firing_listeners = true;
    let result = fire_drain(vm, ev);
    vm.fw.gms.firing_listeners = false;
    vm.fw.gms.pending_fires.clear();
    result
}

fn fire_drain(vm: &mut Engine, first: PendingFire) -> Result<(), VmExit> {
    let mut queue = std::collections::VecDeque::from([first]);
    let mut budget = MAX_FIRES_PER_TRIGGER;
    while let Some(ev) = queue.pop_front() {
        match &ev {
            PendingFire::Firestore(path) => fs_fire_listeners_now(vm, path)?,
            PendingFire::Rtdb(path) => rtdb_fire_now(vm, path)?,
            PendingFire::Auth => fire_auth_listeners_now(vm)?,
        }
        let pending = std::mem::take(&mut vm.fw.gms.pending_fires);
        queue.extend(pending);
        budget = budget.saturating_sub(1);
        if budget == 0 {
            break;
        }
    }
    Ok(())
}

fn fs_fire_listeners(vm: &mut Engine, changed_doc: &str) -> Result<(), VmExit> {
    fire_guarded(vm, PendingFire::Firestore(changed_doc.to_string()))
}

fn fs_fire_listeners_now(vm: &mut Engine, changed_doc: &str) -> Result<(), VmExit> {
    let doc_listeners: Vec<ObjRef> = vm
        .fw
        .gms
        .fs_doc_listeners
        .iter()
        .filter(|(p, _)| p == changed_doc)
        .map(|(_, l)| *l)
        .collect();
    for l in doc_listeners {
        let snap = fs_doc_snap_obj(vm, changed_doc)?;
        let _ = call_dex_any(
            vm,
            l,
            "onEvent",
            &[
                "(Lcom/google/firebase/firestore/DocumentSnapshot;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                "(Ljava/lang/Object;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
            ],
            vec![Value::Obj(snap), Value::Null],
        )?;
    }
    let queries: Vec<(FsQuery, ObjRef)> = vm
        .fw
        .gms
        .fs_query_listeners
        .iter()
        .filter(|(q, _)| q.collection == fs_parent(changed_doc))
        .cloned()
        .collect();
    for (q, l) in queries {
        let snap = fs_run_query(vm, &q)?;
        let _ = call_dex_any(
            vm,
            l,
            "onEvent",
            &[
                "(Lcom/google/firebase/firestore/QuerySnapshot;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                "(Ljava/lang/Object;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
            ],
            vec![Value::Obj(snap), Value::Null],
        )?;
    }
    Ok(())
}
fn fs_write_doc(
    vm: &mut Engine,
    path: &str,
    fields: Vec<(String, Value)>,
    merge: bool,
) -> Result<(), VmExit> {
    let is_new = !vm.fw.gms.fs_docs.contains_key(path);
    if merge && !is_new {
        let cur = vm.fw.gms.fs_docs.entry(path.to_string()).or_default();
        for (k, v) in fields {
            if let Some(e) = cur.iter_mut().find(|(ek, _)| *ek == k) {
                e.1 = v;
            } else {
                cur.push((k, v));
            }
        }
    } else {
        vm.fw.gms.fs_docs.insert(path.to_string(), fields);
    }
    if is_new {
        vm.fw.gms.fs_doc_order.push(path.to_string());
    }
    fs_fire_listeners(vm, path)
}
fn fs_fields_from_map(vm: &Engine, m: ObjRef) -> Vec<(String, Value)> {
    read_host_map(vm, m)
        .into_iter()
        .map(|(k, v)| (value_display(vm, &k), v))
        .collect()
}
fn fs_query_from(vm: &mut Engine, q: FsQuery) -> Result<Value, VmExit> {
    let r = alloc(vm, FS_QUERY)?;
    vm.fw.gms.fs_queries.insert(r, q);
    Ok(Value::Obj(r))
}

fn firestore_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    // Snapshots
    if let Some(snaps) = vm.fw.gms.fs_query_snaps.get(&recv).cloned() {
        return Ok(Some(match name {
            "getDocuments" => make_list(vm, snaps.iter().map(|s| Value::Obj(*s)).collect())?,
            "size" => Value::Int(snaps.len() as i32),
            "isEmpty" => Value::Int(snaps.is_empty() as i32),
            "getDocumentChanges" => make_list(vm, Vec::new())?,
            _ => return Ok(None),
        }));
    }
    if let Some(snap) = vm.fw.gms.fs_doc_snaps.get(&recv).cloned() {
        let getf = |vm: &Engine, f: &str| -> Value {
            let _ = vm;
            snap.fields
                .iter()
                .find(|(k, _)| k == f)
                .map(|(_, v)| v.clone())
                .unwrap_or(Value::Null)
        };
        return Ok(Some(match name {
            "exists" => Value::Int(snap.exists as i32),
            "getId" => astr(vm, fs_last(&snap.path))?,
            "getReference" => Value::Obj(fs_ref_obj(vm, DOC_REF, snap.path.clone(), true)?),
            "get" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                getf(vm, &f)
            }
            "getString" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                match getf(vm, &f) {
                    v @ Value::Obj(_) => v,
                    Value::Null => Value::Null,
                    other => astr(vm, value_display(vm, &other))?,
                }
            }
            "getLong" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                match unbox(vm, &getf(vm, &f)) {
                    Value::Long(n) => box_long(vm, n)?,
                    Value::Int(n) => box_long(vm, n as i64)?,
                    Value::Double(d) => box_long(vm, d as i64)?,
                    Value::Float(x) => box_long(vm, x as i64)?,
                    _ => Value::Null,
                }
            }
            "getDouble" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                match unbox(vm, &getf(vm, &f)) {
                    Value::Double(d) => box_double(vm, d)?,
                    Value::Float(x) => box_double(vm, x as f64)?,
                    Value::Long(n) => box_double(vm, n as f64)?,
                    Value::Int(n) => box_double(vm, n as f64)?,
                    _ => Value::Null,
                }
            }
            "getBoolean" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                match unbox(vm, &getf(vm, &f)) {
                    Value::Int(n) => box_bool(vm, n != 0)?,
                    _ => Value::Null,
                }
            }
            "getData" => {
                if !snap.exists {
                    Value::Null
                } else {
                    let mut entries = Vec::with_capacity(snap.fields.len());
                    for (k, v) in &snap.fields {
                        let ko = intrinsics::alloc_string(vm, k.clone())?;
                        entries.push((Value::Obj(ko), v.clone()));
                    }
                    make_map(vm, entries)?
                }
            }
            "contains" => {
                let f = sarg(vm, args, 0).unwrap_or_default();
                Value::Int(snap.fields.iter().any(|(k, _)| *k == f) as i32)
            }
            _ => return Ok(None),
        }));
    }
    // Query objects
    if let Some(q) = vm.fw.gms.fs_queries.get(&recv).cloned() {
        return Ok(Some(match name {
            "whereEqualTo" => {
                let mut q2 = q.clone();
                let f = sarg(vm, args, 0).unwrap_or_default();
                let v = args.get(1).cloned().unwrap_or(Value::Null);
                q2.filters.push((f, v));
                fs_query_from(vm, q2)?
            }
            "orderBy" => {
                let mut q2 = q.clone();
                q2.order_by = sarg(vm, args, 0);
                fs_query_from(vm, q2)?
            }
            "limit" => {
                let mut q2 = q.clone();
                q2.limit = Some(larg(args, 0) as i32);
                fs_query_from(vm, q2)?
            }
            "get" => {
                let snap = fs_run_query(vm, &q)?;
                Value::Obj(completed_task(vm, Value::Obj(snap))?)
            }
            "addSnapshotListener" => {
                if let Some(l) = last_oarg(args) {
                    vm.fw.gms.fs_query_listeners.push((q.clone(), l));
                    let snap = fs_run_query(vm, &q)?;
                    let _ = call_dex_any(
                        vm,
                        l,
                        "onEvent",
                        &[
                            "(Lcom/google/firebase/firestore/QuerySnapshot;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                            "(Ljava/lang/Object;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                        ],
                        vec![Value::Obj(snap), Value::Null],
                    )?;
                }
                Value::Obj(alloc(vm, LISTENER_REG)?)
            }
            _ => return Ok(None),
        }));
    }
    // Refs (documento/coleção) e a própria instância Firestore.
    if let Some(r) = vm.fw.gms.fs_refs.get(&recv).cloned() {
        if r.is_doc {
            return Ok(Some(match name {
                "getId" => astr(vm, fs_last(&r.path))?,
                "getPath" => astr(vm, r.path.clone())?,
                "getParent" => {
                    Value::Obj(fs_ref_obj(vm, COLLECTION_REF, fs_parent(&r.path), false)?)
                }
                "collection" => {
                    let rel = sarg(vm, args, 0).unwrap_or_default();
                    Value::Obj(fs_ref_obj(
                        vm,
                        COLLECTION_REF,
                        fs_join(&r.path, &rel),
                        false,
                    )?)
                }
                "set" => {
                    let Some(m) = oarg(args, 0) else {
                        return Err(npe("DocumentReference.set(null)"));
                    };
                    let merge = oarg(args, 1)
                        .and_then(|o| vm.fw.gms.fs_set_options.get(&o).copied())
                        .unwrap_or(false);
                    let fields = fs_fields_from_map(vm, m);
                    fs_write_doc(vm, &r.path, fields, merge)?;
                    Value::Obj(void_task(vm)?)
                }
                "update" => {
                    if !vm.fw.gms.fs_docs.contains_key(&r.path) {
                        let exc = make_exception(
                            vm,
                            "Lcom/google/firebase/firestore/FirebaseFirestoreException;",
                            &format!("No document to update: {}", r.path),
                        )?;
                        let t = new_task_as(vm, TASK)?;
                        settle_task(vm, t, None, Some(exc), false)?;
                        Value::Obj(t)
                    } else {
                        match args.first() {
                            Some(Value::Obj(m)) if sarg(vm, args, 0).is_none() => {
                                let fields = fs_fields_from_map(vm, *m);
                                fs_write_doc(vm, &r.path, fields, true)?;
                            }
                            _ => {
                                // update(String, Object, Object...) — forma comum: pares.
                                let mut fields = Vec::new();
                                let mut i = 0;
                                while i + 1 < args.len() {
                                    if let Some(k) = sarg(vm, args, i) {
                                        fields.push((k, args[i + 1].clone()));
                                    }
                                    i += 2;
                                }
                                fs_write_doc(vm, &r.path, fields, true)?;
                            }
                        }
                        Value::Obj(void_task(vm)?)
                    }
                }
                "delete" => {
                    vm.fw.gms.fs_docs.remove(&r.path);
                    vm.fw.gms.fs_doc_order.retain(|p| *p != r.path);
                    fs_fire_listeners(vm, &r.path)?;
                    Value::Obj(void_task(vm)?)
                }
                "get" => {
                    let snap = fs_doc_snap_obj(vm, &r.path)?;
                    Value::Obj(completed_task(vm, Value::Obj(snap))?)
                }
                "addSnapshotListener" => {
                    if let Some(l) = last_oarg(args) {
                        vm.fw.gms.fs_doc_listeners.push((r.path.clone(), l));
                        let snap = fs_doc_snap_obj(vm, &r.path)?;
                        let _ = call_dex_any(
                            vm,
                            l,
                            "onEvent",
                            &[
                                "(Lcom/google/firebase/firestore/DocumentSnapshot;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                                "(Ljava/lang/Object;Lcom/google/firebase/firestore/FirebaseFirestoreException;)V",
                            ],
                            vec![Value::Obj(snap), Value::Null],
                        )?;
                    }
                    Value::Obj(alloc(vm, LISTENER_REG)?)
                }
                _ => return Ok(None),
            }));
        }
        return Ok(Some(match name {
            "getId" => astr(vm, fs_last(&r.path))?,
            "getPath" => astr(vm, r.path.clone())?,
            "getParent" => {
                let parent = fs_parent(&r.path);
                if parent.is_empty() {
                    Value::Null
                } else {
                    Value::Obj(fs_ref_obj(vm, DOC_REF, parent, true)?)
                }
            }
            "document" => {
                let rel = sarg(vm, args, 0);
                let path = match rel {
                    Some(p) if !p.is_empty() => fs_join(&r.path, &p),
                    _ => {
                        vm.fw.gms.fs_counter += 1;
                        let n = vm.fw.gms.fs_counter;
                        fs_join(&r.path, &format!("doc-{n:020}"))
                    }
                };
                Value::Obj(fs_ref_obj(vm, DOC_REF, path, true)?)
            }
            "add" => {
                let Some(m) = oarg(args, 0) else {
                    return Err(npe("CollectionReference.add(null)"));
                };
                let fields = fs_fields_from_map(vm, m);
                vm.fw.gms.fs_counter += 1;
                let n = vm.fw.gms.fs_counter;
                let path = fs_join(&r.path, &format!("doc-{n:020}"));
                fs_write_doc(vm, &path, fields, false)?;
                let doc = fs_ref_obj(vm, DOC_REF, path, true)?;
                Value::Obj(completed_task(vm, Value::Obj(doc))?)
            }
            "whereEqualTo" | "orderBy" | "limit" | "get" | "addSnapshotListener" => {
                // Coleção É uma Query: converte e despacha pelo caminho da query.
                let q = FsQuery {
                    collection: r.path.clone(),
                    ..Default::default()
                };
                let qo = alloc(vm, FS_QUERY)?;
                vm.fw.gms.fs_queries.insert(qo, q);
                return firestore_instance(vm, name, qo, args);
            }
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.firestore_obj == Some(recv) {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "collection" => {
                let p = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(fs_ref_obj(vm, COLLECTION_REF, fs_normalize(&p), false)?)
            }
            "document" => {
                let p = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(fs_ref_obj(vm, DOC_REF, fs_normalize(&p), true)?)
            }
            "collectionGroup" => {
                let p = sarg(vm, args, 0).unwrap_or_default();
                let q = FsQuery {
                    collection: fs_normalize(&p),
                    ..Default::default()
                };
                fs_query_from(vm, q)?
            }
            "getFirestoreSettings"
            | "setFirestoreSettings"
            | "enableNetwork"
            | "disableNetwork"
            | "clearPersistence" => match name {
                "enableNetwork" | "disableNetwork" | "clearPersistence" => {
                    Value::Obj(void_task(vm)?)
                }
                _ => Value::Null,
            },
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

// ── RTDB ────────────────────────────────────────────────────────────────
fn rtdb_join(base: &str, rel: &str) -> String {
    fs_join(base, rel)
}
fn rtdb_resolve(vm: &Engine, path: &str) -> Option<Value> {
    if let Some(v) = vm.fw.gms.rtdb_values.get(path) {
        return Some(v.clone());
    }
    // Desce mapas armazenados em ancestrais.
    let mut p = path.to_string();
    loop {
        let parent = fs_parent(&p);
        if parent == p {
            return None;
        }
        if let Some(v) = vm.fw.gms.rtdb_values.get(&parent) {
            let rest = path[parent.len()..].trim_start_matches('/');
            let mut cur = v.clone();
            for seg in rest.split('/').filter(|s| !s.is_empty()) {
                match cur {
                    Value::Obj(r) => {
                        let entries = read_host_map(vm, r);
                        let mut found = None;
                        for (k, val) in entries {
                            if value_display(vm, &k) == seg {
                                found = Some(val);
                                break;
                            }
                        }
                        cur = found?;
                    }
                    _ => return None,
                }
            }
            return Some(cur);
        }
        if parent.is_empty() {
            return None;
        }
        p = parent;
    }
}
fn rtdb_snap_obj(vm: &mut Engine, path: &str) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, DATA_SNAPSHOT)?;
    vm.fw.gms.rtdb_snaps.insert(r, path.to_string());
    Ok(r)
}
fn rtdb_ref_obj(vm: &mut Engine, path: String) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, DATABASE_REF)?;
    vm.fw.gms.rtdb_refs.insert(r, path);
    Ok(r)
}
fn rtdb_fire(vm: &mut Engine, changed: &str) -> Result<(), VmExit> {
    fire_guarded(vm, PendingFire::Rtdb(changed.to_string()))
}

fn rtdb_fire_now(vm: &mut Engine, changed: &str) -> Result<(), VmExit> {
    let affected: Vec<(String, ObjRef)> = vm
        .fw
        .gms
        .rtdb_listeners
        .iter()
        .filter(|(p, _)| {
            p == changed
                || changed.starts_with(&format!("{p}/"))
                || p.starts_with(&format!("{changed}/"))
                || p.is_empty()
        })
        .cloned()
        .collect();
    for (path, l) in affected {
        let snap = rtdb_snap_obj(vm, &path)?;
        let _ = call_dex_any(
            vm,
            l,
            "onDataChange",
            &["(Lcom/google/firebase/database/DataSnapshot;)V"],
            vec![Value::Obj(snap)],
        )?;
    }
    Ok(())
}
fn rtdb_set(vm: &mut Engine, path: &str, value: Value) -> Result<(), VmExit> {
    // Remove descendentes (setValue substitui a subárvore).
    let prefix = format!("{path}/");
    vm.fw.gms.rtdb_values.retain(|k, _| !k.starts_with(&prefix));
    vm.fw.gms.rtdb_values.insert(path.to_string(), value);
    rtdb_fire(vm, path)
}

fn rtdb_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(path) = vm.fw.gms.rtdb_snaps.get(&recv).cloned() {
        let val = rtdb_resolve(vm, &path);
        return Ok(Some(match name {
            "exists" => Value::Int(val.is_some() as i32),
            "getKey" => {
                if path.is_empty() {
                    Value::Null
                } else {
                    astr(vm, fs_last(&path))?
                }
            }
            "getValue" => val.clone().unwrap_or(Value::Null),
            "child" => {
                let rel = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(rtdb_snap_obj(vm, &rtdb_join(&path, &rel))?)
            }
            "hasChild" => {
                let rel = sarg(vm, args, 0).unwrap_or_default();
                Value::Int(rtdb_resolve(vm, &rtdb_join(&path, &rel)).is_some() as i32)
            }
            "hasChildren" => Value::Int(matches!(val, Some(Value::Obj(_))) as i32),
            "getChildrenCount" => match &val {
                Some(Value::Obj(r)) => Value::Long(read_host_map(vm, *r).len() as i64),
                _ => Value::Long(0),
            },
            "getChildren" => {
                let mut kids = Vec::new();
                if let Some(Value::Obj(r)) = &val {
                    let keys: Vec<String> = read_host_map(vm, *r)
                        .into_iter()
                        .map(|(k, _)| value_display(vm, &k))
                        .collect();
                    for k in keys {
                        kids.push(Value::Obj(rtdb_snap_obj(vm, &rtdb_join(&path, &k))?));
                    }
                }
                make_list(vm, kids)?
            }
            "getRef" => Value::Obj(rtdb_ref_obj(vm, path.clone())?),
            "getPriority" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some(path) = vm.fw.gms.rtdb_refs.get(&recv).cloned() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "child" => {
                let rel = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(rtdb_ref_obj(vm, rtdb_join(&path, &rel))?)
            }
            "getKey" => {
                if path.is_empty() {
                    Value::Null
                } else {
                    astr(vm, fs_last(&path))?
                }
            }
            "getParent" => {
                if path.is_empty() {
                    Value::Null
                } else {
                    Value::Obj(rtdb_ref_obj(vm, fs_parent(&path))?)
                }
            }
            "getRoot" => Value::Obj(rtdb_ref_obj(vm, String::new())?),
            "getReference" => Value::Obj(recv),
            "push" => {
                vm.fw.gms.rtdb_counter += 1;
                let n = vm.fw.gms.rtdb_counter;
                Value::Obj(rtdb_ref_obj(
                    vm,
                    rtdb_join(&path, &format!("-rustydroid{n:012}")),
                )?)
            }
            "setValue" => {
                let v = args.first().cloned().unwrap_or(Value::Null);
                rtdb_set(vm, &path, v)?;
                // CompletionListener opcional no 2º arg.
                if let Some(l) = oarg(args, 1) {
                    let _ = call_dex_any(
                        vm,
                        l,
                        "onComplete",
                        &[
                            "(Lcom/google/firebase/database/DatabaseError;Lcom/google/firebase/database/DatabaseReference;)V",
                        ],
                        vec![Value::Null, Value::Obj(recv)],
                    )?;
                }
                Value::Obj(void_task(vm)?)
            }
            "removeValue" => {
                rtdb_set(vm, &path, Value::Null)?;
                vm.fw.gms.rtdb_values.remove(&path);
                Value::Obj(void_task(vm)?)
            }
            "updateChildren" => {
                if let Some(m) = oarg(args, 0) {
                    let entries: Vec<(String, Value)> = read_host_map(vm, m)
                        .into_iter()
                        .map(|(k, v)| (value_display(vm, &k), v))
                        .collect();
                    for (k, v) in entries {
                        let full = rtdb_join(&path, &k);
                        rtdb_set(vm, &full, v)?;
                    }
                }
                Value::Obj(void_task(vm)?)
            }
            "get" => {
                let snap = rtdb_snap_obj(vm, &path)?;
                Value::Obj(completed_task(vm, Value::Obj(snap))?)
            }
            "addValueEventListener" => {
                if let Some(l) = oarg(args, 0) {
                    vm.fw.gms.rtdb_listeners.push((path.clone(), l));
                    let snap = rtdb_snap_obj(vm, &path)?;
                    let _ = call_dex_any(
                        vm,
                        l,
                        "onDataChange",
                        &["(Lcom/google/firebase/database/DataSnapshot;)V"],
                        vec![Value::Obj(snap)],
                    )?;
                    Value::Obj(l)
                } else {
                    Value::Null
                }
            }
            "addListenerForSingleValueEvent" => {
                if let Some(l) = oarg(args, 0) {
                    let snap = rtdb_snap_obj(vm, &path)?;
                    let _ = call_dex_any(
                        vm,
                        l,
                        "onDataChange",
                        &["(Lcom/google/firebase/database/DataSnapshot;)V"],
                        vec![Value::Obj(snap)],
                    )?;
                }
                Value::Null
            }
            "removeEventListener" => {
                if let Some(l) = oarg(args, 0) {
                    vm.fw.gms.rtdb_listeners.retain(|(_, x)| *x != l);
                }
                Value::Null
            }
            "toString" => {
                let base = vm.fw.gms.rtdb_url.clone();
                astr(vm, format!("{}/{}", base.trim_end_matches('/'), path))?
            }
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.rtdb_obj == Some(recv) {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getReference" => {
                let rel = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(rtdb_ref_obj(vm, fs_normalize(&rel))?)
            }
            "getReferenceFromUrl" => {
                let url = sarg(vm, args, 0).unwrap_or_default();
                let rel = url
                    .trim_start_matches("https://")
                    .split_once('/')
                    .map(|(_, p)| p.to_string())
                    .unwrap_or_default();
                Value::Obj(rtdb_ref_obj(vm, fs_normalize(&rel))?)
            }
            "goOnline" | "goOffline" | "purgeOutstandingWrites" => Value::Null,
            "setPersistenceEnabled" => Value::Null,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

// ── Remote Config ───────────────────────────────────────────────────────
fn rc_lookup(vm: &Engine, key: &str) -> (String, i32) {
    // source: 1 = VALUE_SOURCE_REMOTE? A API: STATIC=0, DEFAULT=1,
    // REMOTE=2 (FirebaseRemoteConfigValue). Usamos essa numeração.
    if let Some(v) = lookup(&vm.fw.gms.rc_values, key) {
        (v.to_string(), 2)
    } else if let Some(v) = lookup(&vm.fw.gms.rc_defaults, key) {
        (v.to_string(), 1)
    } else {
        (String::new(), 0)
    }
}
fn remote_config_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some((val, source)) = vm.fw.gms.rc_value_objs.get(&recv).cloned() {
        return Ok(Some(match name {
            "asString" => astr(vm, val)?,
            "asLong" => Value::Long(val.trim().parse::<i64>().unwrap_or(0)),
            "asDouble" => Value::Double(val.trim().parse::<f64>().unwrap_or(0.0)),
            "asBoolean" => {
                Value::Int(matches!(val.trim().to_ascii_lowercase().as_str(), "true" | "1") as i32)
            }
            "getSource" => Value::Int(source),
            _ => return Ok(None),
        }));
    }
    Ok(Some(match name {
        "<init>" => Value::Null,
        "setDefaultsAsync" => {
            if let Some(m) = oarg(args, 0) {
                let entries: Vec<(String, String)> = read_host_map(vm, m)
                    .into_iter()
                    .map(|(k, v)| (value_display(vm, &k), value_display(vm, &v)))
                    .collect();
                for (k, v) in entries {
                    upsert(&mut vm.fw.gms.rc_defaults, k, v);
                }
            }
            Value::Obj(void_task(vm)?)
        }
        "fetch" => {
            vm.fw.gms.rc_pending = vm.fw.gms.rc_server.clone();
            Value::Obj(void_task(vm)?)
        }
        "activate" => {
            let mut changed = false;
            let pending = std::mem::take(&mut vm.fw.gms.rc_pending);
            for (k, v) in pending {
                if lookup(&vm.fw.gms.rc_values, &k) != Some(v.as_str()) {
                    changed = true;
                }
                upsert(&mut vm.fw.gms.rc_values, k, v);
            }
            {
                let v = box_bool(vm, changed)?;
                Value::Obj(completed_task(vm, v)?)
            }
        }
        "fetchAndActivate" => {
            // fetch seguido de activate (mesma lógica, sem recursão DEX).
            vm.fw.gms.rc_pending = vm.fw.gms.rc_server.clone();
            let mut changed = false;
            let pending = std::mem::take(&mut vm.fw.gms.rc_pending);
            for (k, v) in pending {
                if lookup(&vm.fw.gms.rc_values, &k) != Some(v.as_str()) {
                    changed = true;
                }
                upsert(&mut vm.fw.gms.rc_values, k, v);
            }
            {
                let v = box_bool(vm, changed)?;
                Value::Obj(completed_task(vm, v)?)
            }
        }
        "getString" => {
            let k = sarg(vm, args, 0).unwrap_or_default();
            astr(vm, rc_lookup(vm, &k).0)?
        }
        "getLong" => {
            let k = sarg(vm, args, 0).unwrap_or_default();
            Value::Long(rc_lookup(vm, &k).0.trim().parse::<i64>().unwrap_or(0))
        }
        "getDouble" => {
            let k = sarg(vm, args, 0).unwrap_or_default();
            Value::Double(rc_lookup(vm, &k).0.trim().parse::<f64>().unwrap_or(0.0))
        }
        "getBoolean" => {
            let k = sarg(vm, args, 0).unwrap_or_default();
            let v = rc_lookup(vm, &k).0;
            Value::Int(matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1") as i32)
        }
        "getValue" => {
            let k = sarg(vm, args, 0).unwrap_or_default();
            let (v, source) = rc_lookup(vm, &k);
            let r = alloc(vm, REMOTE_CONFIG_VALUE)?;
            vm.fw.gms.rc_value_objs.insert(r, (v, source));
            Value::Obj(r)
        }
        "getAll" => {
            let keys: Vec<String> = vm
                .fw
                .gms
                .rc_values
                .iter()
                .map(|(k, _)| k.clone())
                .chain(vm.fw.gms.rc_defaults.iter().map(|(k, _)| k.clone()))
                .collect();
            let mut entries = Vec::new();
            let mut seen: Vec<String> = Vec::new();
            for k in keys {
                if seen.contains(&k) {
                    continue;
                }
                seen.push(k.clone());
                let ko = intrinsics::alloc_string(vm, k.clone())?;
                let (v, source) = rc_lookup(vm, &k);
                let vo = alloc(vm, REMOTE_CONFIG_VALUE)?;
                vm.fw.gms.rc_value_objs.insert(vo, (v, source));
                entries.push((Value::Obj(ko), Value::Obj(vo)));
            }
            make_map(vm, entries)?
        }
        "getKeysByPrefix" => {
            let prefix = sarg(vm, args, 0).unwrap_or_default();
            let keys: Vec<String> = vm
                .fw
                .gms
                .rc_values
                .iter()
                .chain(vm.fw.gms.rc_defaults.iter())
                .map(|(k, _)| k.clone())
                .filter(|k| k.starts_with(&prefix))
                .collect();
            let mut elems = Vec::new();
            for k in keys {
                let ko = intrinsics::alloc_string(vm, k)?;
                elems.push(Value::Obj(ko));
            }
            let r = alloc(vm, "Ljava/util/HashSet;")?;
            vm.fw.objects.insert(r, HostObj::List { elems });
            Value::Obj(r)
        }
        "getInfo" => Value::Null,
        "setConfigSettingsAsync" => Value::Obj(void_task(vm)?),
        "ensureInitialized" => Value::Obj(void_task(vm)?),
        "reset" => {
            vm.fw.gms.rc_values.clear();
            vm.fw.gms.rc_pending.clear();
            Value::Obj(void_task(vm)?)
        }
        _ => return Ok(None),
    }))
}

// ── Storage ─────────────────────────────────────────────────────────────
fn storage_ref_obj(vm: &mut Engine, path: String) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, STORAGE_REF)?;
    vm.fw.gms.storage_refs.insert(r, path);
    Ok(r)
}
fn storage_bucket(vm: &Engine) -> String {
    let opts = default_fb_options(vm);
    if opts.storage_bucket.is_empty() {
        "rustydroid-fake-project.appspot.com".to_string()
    } else {
        opts.storage_bucket
    }
}
fn storage_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(n) = vm.fw.gms.upload_snaps.get(&recv).copied() {
        return Ok(Some(match name {
            "getBytesTransferred" => Value::Long(n),
            "getTotalByteCount" => Value::Long(n),
            "getStorage" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some(items) = vm.fw.gms.storage_list_results.get(&recv).cloned() {
        return Ok(Some(match name {
            "getItems" => make_list(vm, items.iter().map(|r| Value::Obj(*r)).collect())?,
            "getPrefixes" => make_list(vm, Vec::new())?,
            _ => return Ok(None),
        }));
    }
    if let Some(path) = vm.fw.gms.storage_refs.get(&recv).cloned() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "child" => {
                let rel = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(storage_ref_obj(vm, fs_join(&path, &rel))?)
            }
            "getParent" => {
                if path.is_empty() {
                    Value::Null
                } else {
                    Value::Obj(storage_ref_obj(vm, fs_parent(&path))?)
                }
            }
            "getRoot" => Value::Obj(storage_ref_obj(vm, String::new())?),
            "getName" => astr(vm, fs_last(&path))?,
            "getPath" => astr(vm, format!("/{path}"))?,
            "getBucket" => astr(vm, storage_bucket(vm))?,
            "putBytes" => {
                let bytes = match args.first() {
                    Some(v) => read_bytes(vm, v),
                    None => Vec::new(),
                };
                let len = bytes.len() as i64;
                vm.fw.gms.storage_blobs.insert(path.clone(), bytes);
                let snap = alloc(vm, UPLOAD_SNAP)?;
                vm.fw.gms.upload_snaps.insert(snap, len);
                let task = new_task_as(vm, UPLOAD_TASK)?;
                settle_task(vm, task, Some(Value::Obj(snap)), None, false)?;
                Value::Obj(task)
            }
            "getBytes" => {
                let max = larg(args, 0);
                match vm.fw.gms.storage_blobs.get(&path).cloned() {
                    Some(b) if (b.len() as i64) <= max || max <= 0 => {
                        let arr = make_bytes(vm, &b)?;
                        Value::Obj(completed_task(vm, arr)?)
                    }
                    Some(_) => Value::Obj(failed_task(
                        vm,
                        "Lcom/google/firebase/storage/StorageException;",
                        "Download size exceeds max allowed size (RustyDroid shim)",
                    )?),
                    None => Value::Obj(failed_task(
                        vm,
                        "Lcom/google/firebase/storage/StorageException;",
                        "Object does not exist at location (RustyDroid shim store)",
                    )?),
                }
            }
            "getDownloadUrl" => {
                if !vm.fw.gms.storage_blobs.contains_key(&path) {
                    Value::Obj(failed_task(
                        vm,
                        "Lcom/google/firebase/storage/StorageException;",
                        "Object does not exist at location (RustyDroid shim store)",
                    )?)
                } else {
                    let url = format!(
                        "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media&token=rustydroid-fake",
                        storage_bucket(vm),
                        path.replace('/', "%2F")
                    );
                    let uri = alloc(vm, ANDROID_URI)?;
                    vm.fw.m8.uris.insert(uri, url);
                    Value::Obj(completed_task(vm, Value::Obj(uri))?)
                }
            }
            "delete" => {
                if vm.fw.gms.storage_blobs.remove(&path).is_some() {
                    Value::Obj(void_task(vm)?)
                } else {
                    Value::Obj(failed_task(
                        vm,
                        "Lcom/google/firebase/storage/StorageException;",
                        "Object does not exist at location (RustyDroid shim store)",
                    )?)
                }
            }
            "listAll" => {
                let prefix = if path.is_empty() {
                    String::new()
                } else {
                    format!("{path}/")
                };
                let mut items = Vec::new();
                let keys: Vec<String> = vm
                    .fw
                    .gms
                    .storage_blobs
                    .keys()
                    .filter(|k| prefix.is_empty() || k.starts_with(&prefix))
                    .cloned()
                    .collect();
                for k in keys {
                    items.push(storage_ref_obj(vm, k)?);
                }
                let r = alloc(vm, LIST_RESULT)?;
                vm.fw.gms.storage_list_results.insert(r, items);
                Value::Obj(completed_task(vm, Value::Obj(r))?)
            }
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.storage_obj == Some(recv) {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getReference" => {
                let p = sarg(vm, args, 0).unwrap_or_default();
                Value::Obj(storage_ref_obj(vm, fs_normalize(&p))?)
            }
            "getBucket" => astr(vm, storage_bucket(vm))?,
            "getApp" => Value::Obj(ensure_default_app(vm)?),
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

// ── Location (fused) ────────────────────────────────────────────────────
fn mint_location(vm: &mut Engine, fix: &FixCfg) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, LOCATION)?;
    let provider = intrinsics::alloc_string(
        vm,
        if fix.provider.is_empty() {
            "fused".to_string()
        } else {
            fix.provider.clone()
        },
    )?;
    // Campos lidos pelo modelo host de android.location.Location
    // (android_stub_surfaces: mLatitudeDegrees/-Longitude/-Accuracy...).
    put(vm, r, "mProvider", Value::Obj(provider));
    put(vm, r, "mLatitudeDegrees", Value::Double(fix.lat));
    put(vm, r, "mLongitudeDegrees", Value::Double(fix.lng));
    put(
        vm,
        r,
        "mHorizontalAccuracyMeters",
        Value::Float(fix.accuracy_m),
    );
    put(vm, r, "mAltitudeMeters", Value::Double(fix.altitude_m));
    put(vm, r, "mSpeedMetersPerSecond", Value::Float(fix.speed_mps));
    put(vm, r, "mBearingDegrees", Value::Float(fix.bearing_deg));
    put(vm, r, "mTimeMs", Value::Long(fix.time_ms));
    put(
        vm,
        r,
        "mElapsedRealtimeNs",
        Value::Long(fix.time_ms * 1_000_000),
    );
    // HAS_ALTITUDE|HAS_SPEED|HAS_BEARING|HAS_HORIZONTAL_ACCURACY
    put(vm, r, "mFieldsMask", Value::Int(0b1111));
    Ok(r)
}
fn location_task(vm: &mut Engine) -> Result<Value, VmExit> {
    match vm.fw.gms.location_fix.clone() {
        Some(fix) => {
            let loc = mint_location(vm, &fix)?;
            Ok(Value::Obj(completed_task(vm, Value::Obj(loc))?))
        }
        None => Ok(Value::Obj(completed_task(vm, Value::Null)?)),
    }
}
fn loc_result_obj(vm: &mut Engine, locations: Vec<ObjRef>) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, LOCATION_RESULT)?;
    vm.fw.gms.loc_results.insert(r, locations);
    Ok(r)
}
fn deliver_location(vm: &mut Engine, callback: ObjRef, fix: &FixCfg) -> Result<(), VmExit> {
    let loc = mint_location(vm, fix)?;
    let result = loc_result_obj(vm, vec![loc])?;
    let _ = call_dex_any(
        vm,
        callback,
        "onLocationResult",
        &["(Lcom/google/android/gms/location/LocationResult;)V"],
        vec![Value::Obj(result)],
    )?;
    Ok(())
}
fn location_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(locs) = vm.fw.gms.loc_results.get(&recv).cloned() {
        return Ok(Some(match name {
            "getLocations" => make_list(vm, locs.iter().map(|r| Value::Obj(*r)).collect())?,
            "getLastLocation" => locs.last().map(|r| Value::Obj(*r)).unwrap_or(Value::Null),
            _ => return Ok(None),
        }));
    }
    if let Some(avail) = vm.fw.gms.loc_availability.get(&recv).copied() {
        return Ok(Some(match name {
            "isLocationAvailable" => Value::Int(avail as i32),
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.loc_request_objs.contains_key(&recv)
        || mref_class == LOCATION_REQUEST
        || mref_class == LOCATION_REQUEST_BUILDER
    {
        if name == "build" {
            let req = vm
                .fw
                .gms
                .loc_request_objs
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            let r = alloc(vm, LOCATION_REQUEST)?;
            vm.fw.gms.loc_request_objs.insert(r, req);
            return Ok(Some(Value::Obj(r)));
        }
        let req = vm.fw.gms.loc_request_objs.entry(recv).or_default();
        match name {
            "<init>" => {
                if let Some(n) = args.first().and_then(|v| v.as_long().ok()) {
                    req.interval_ms = n;
                }
                return Ok(Some(Value::Null));
            }
            "setPriority" => req.priority = iarg(args, 0),
            "setInterval" | "setMinUpdateIntervalMillis" | "setDurationMillis" => {
                req.interval_ms = larg(args, 0)
            }
            "setFastestInterval"
            | "setMaxWaitTime"
            | "setNumUpdates"
            | "setExpirationDuration"
            | "setSmallestDisplacement"
            | "setMaxUpdates"
            | "setGranularity"
            | "setWaitForAccurateLocation" => {}
            "getPriority" => return Ok(Some(Value::Int(req.priority))),
            "getInterval" | "getIntervalMillis" | "getMinUpdateIntervalMillis" => {
                return Ok(Some(Value::Long(req.interval_ms)))
            }
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if vm.fw.gms.fused_client == Some(recv) {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getLastLocation" | "getCurrentLocation" => location_task(vm)?,
            "getLocationAvailability" => {
                let avail = vm.fw.gms.location_fix.is_some();
                let r = alloc(vm, LOCATION_AVAILABILITY)?;
                vm.fw.gms.loc_availability.insert(r, avail);
                Value::Obj(completed_task(vm, Value::Obj(r))?)
            }
            "requestLocationUpdates" => {
                let req = oarg(args, 0)
                    .and_then(|r| vm.fw.gms.loc_request_objs.get(&r).cloned())
                    .unwrap_or_default();
                let callback = args.iter().skip(1).find_map(|v| match v {
                    Value::Obj(r) => Some(*r),
                    _ => None,
                });
                if let Some(cb) = callback {
                    vm.fw.gms.location_requests.push((cb, req));
                    if let Some(fix) = vm.fw.gms.location_fix.clone() {
                        deliver_location(vm, cb, &fix)?;
                    }
                }
                Value::Obj(void_task(vm)?)
            }
            "removeLocationUpdates" => {
                if let Some(cb) = oarg(args, 0) {
                    vm.fw.gms.location_requests.retain(|(c, _)| *c != cb);
                }
                Value::Obj(void_task(vm)?)
            }
            "flushLocations" | "setMockMode" => Value::Obj(void_task(vm)?),
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

// ── Billing ─────────────────────────────────────────────────────────────
fn make_billing_result(vm: &mut Engine, code: i32, msg: &str) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, BILLING_RESULT)?;
    vm.fw.gms.billing_results.insert(r, (code, msg.to_string()));
    Ok(r)
}
fn sku_obj(vm: &mut Engine, cfg: &SkuCfg, modern: bool) -> Result<ObjRef, VmExit> {
    if modern {
        let r = alloc(vm, PRODUCT_DETAILS)?;
        vm.fw.gms.product_details_objs.insert(r, cfg.clone());
        Ok(r)
    } else {
        let r = alloc(vm, SKU_DETAILS)?;
        vm.fw.gms.sku_details_objs.insert(r, cfg.clone());
        Ok(r)
    }
}
fn purchase_obj(vm: &mut Engine, idx: usize) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, PURCHASE)?;
    vm.fw.gms.purchase_by_obj.insert(r, idx);
    Ok(r)
}
fn billing_builder_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let is_builder = vm.fw.gms.billing_builders.contains_key(&recv)
        || mref_class.ends_with("$Builder;")
            && mref_class.starts_with("Lcom/android/billingclient/");
    if !is_builder {
        return Ok(None);
    }
    if name == "<init>" {
        vm.fw
            .gms
            .billing_builders
            .insert(recv, BillingBuilder::default());
        return Ok(Some(Value::Null));
    }
    if name == "build" {
        let b = vm
            .fw
            .gms
            .billing_builders
            .get(&recv)
            .cloned()
            .unwrap_or_default();
        let built_class = match mref_class {
            BILLING_CLIENT_BUILDER => {
                let c = alloc(vm, BILLING_CLIENT)?;
                vm.fw.gms.billing_clients.insert(
                    c,
                    BillingClientHost {
                        listener: b.listener,
                        connected: false,
                    },
                );
                return Ok(Some(Value::Obj(c)));
            }
            SKU_PARAMS_BUILDER => SKU_PARAMS,
            QUERY_PRODUCTS_BUILDER => QUERY_PRODUCTS_PARAMS,
            QP_PRODUCT_BUILDER => QP_PRODUCT,
            FLOW_PARAMS_BUILDER => FLOW_PARAMS,
            FLOW_PD_BUILDER => FLOW_PD_PARAMS,
            ACK_BUILDER => ACK_PARAMS,
            CONSUME_BUILDER => CONSUME_PARAMS,
            QUERY_PURCHASES_BUILDER => QUERY_PURCHASES_PARAMS,
            _ => return Ok(Some(Value::Obj(recv))),
        };
        let r = alloc(vm, built_class)?;
        vm.fw.gms.billing_builders.insert(r, b);
        return Ok(Some(Value::Obj(r)));
    }
    match name {
        "setListener" => {
            vm.fw.gms.billing_builders.entry(recv).or_default().listener = oarg(args, 0);
        }
        "setType" | "setProductType" => {
            let t = sarg(vm, args, 0).unwrap_or_default();
            vm.fw.gms.billing_builders.entry(recv).or_default().sku_type = t;
        }
        "setSkusList" => {
            let skus = match oarg(args, 0) {
                Some(l) => read_host_list(vm, l)
                    .iter()
                    .map(|v| value_display(vm, v))
                    .collect(),
                None => Vec::new(),
            };
            vm.fw.gms.billing_builders.entry(recv).or_default().skus = skus;
        }
        "setProductId" => {
            let id = sarg(vm, args, 0).unwrap_or_default();
            let b = vm.fw.gms.billing_builders.entry(recv).or_default();
            b.skus = vec![id.clone()];
            b.product_ids = vec![id];
        }
        "setProductList" => {
            let mut ids = Vec::new();
            if let Some(l) = oarg(args, 0) {
                for v in read_host_list(vm, l) {
                    if let Value::Obj(r) = v {
                        if let Some(pb) = vm.fw.gms.billing_builders.get(&r) {
                            ids.extend(pb.product_ids.clone());
                        }
                    }
                }
            }
            vm.fw
                .gms
                .billing_builders
                .entry(recv)
                .or_default()
                .product_ids = ids;
        }
        "setSkuDetails" => {
            vm.fw
                .gms
                .billing_builders
                .entry(recv)
                .or_default()
                .sku_details = oarg(args, 0);
        }
        "setProductDetails" => {
            if let Some(r) = oarg(args, 0) {
                let id = vm
                    .fw
                    .gms
                    .product_details_objs
                    .get(&r)
                    .map(|c| c.sku.clone());
                let b = vm.fw.gms.billing_builders.entry(recv).or_default();
                b.sku_details = Some(r);
                if let Some(id) = id {
                    b.product_ids = vec![id];
                }
            }
        }
        "setProductDetailsParamsList" => {
            let mut ids = Vec::new();
            if let Some(l) = oarg(args, 0) {
                for v in read_host_list(vm, l) {
                    if let Value::Obj(r) = v {
                        if let Some(pb) = vm.fw.gms.billing_builders.get(&r) {
                            ids.extend(pb.product_ids.clone());
                            ids.extend(pb.skus.clone());
                        }
                    }
                }
            }
            ids.dedup();
            vm.fw
                .gms
                .billing_builders
                .entry(recv)
                .or_default()
                .product_ids = ids;
        }
        "setPurchaseToken" => {
            let t = sarg(vm, args, 0).unwrap_or_default();
            vm.fw.gms.billing_builders.entry(recv).or_default().token = t;
        }
        _ => {} // demais setters aceitos (retorna this)
    }
    Ok(Some(Value::Obj(recv)))
}

fn billing_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(v) = billing_builder_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some((code, msg)) = vm.fw.gms.billing_results.get(&recv).cloned() {
        return Ok(Some(match name {
            "getResponseCode" => Value::Int(code),
            "getDebugMessage" => astr(vm, msg)?,
            _ => return Ok(None),
        }));
    }
    if let Some(cfg) = vm
        .fw
        .gms
        .sku_details_objs
        .get(&recv)
        .or_else(|| vm.fw.gms.product_details_objs.get(&recv))
        .cloned()
    {
        return Ok(Some(match name {
            "getSku" | "getProductId" => astr(vm, cfg.sku)?,
            "getType" | "getProductType" => astr(vm, cfg.typ)?,
            "getPrice" | "getFormattedPrice" => astr(vm, cfg.price)?,
            "getPriceAmountMicros" => Value::Long(cfg.price_micros),
            "getPriceCurrencyCode" => astr(vm, cfg.currency)?,
            "getTitle" => astr(vm, cfg.title)?,
            "getDescription" => astr(vm, cfg.description)?,
            "getOriginalJson" => astr(vm, format!(
                "{{\"productId\":\"{}\",\"type\":\"{}\",\"price\":\"{}\",\"price_amount_micros\":{},\"price_currency_code\":\"{}\",\"title\":\"{}\",\"description\":\"{}\"}}",
                cfg.sku, cfg.typ, cfg.price, cfg.price_micros, cfg.currency, cfg.title, cfg.description))?,
            "getOneTimePurchaseOfferDetails" => {
                let r = alloc(vm, ONE_TIME_OFFER)?;
                vm.fw.gms.product_details_objs.insert(r, cfg.clone());
                Value::Obj(r)
            }
            "getSubscriptionOfferDetails" => make_list(vm, Vec::new())?,
            _ => return Ok(None),
        }));
    }
    if let Some(idx) = vm.fw.gms.purchase_by_obj.get(&recv).copied() {
        let rec = vm
            .fw
            .gms
            .billing_purchases
            .get(idx)
            .cloned()
            .unwrap_or_default();
        return Ok(Some(match name {
            "getProducts" | "getSkus" => {
                let mut elems = Vec::with_capacity(rec.products.len());
                for p in &rec.products {
                    elems.push(Value::Obj(intrinsics::alloc_string(vm, p.clone())?));
                }
                make_list(vm, elems)?
            }
            "getPurchaseToken" => astr(vm, rec.token)?,
            "getPurchaseState" => Value::Int(rec.state),
            "isAcknowledged" => Value::Int(rec.acknowledged as i32),
            "getOrderId" => astr(vm, rec.order_id)?,
            "getPurchaseTime" => Value::Long(rec.purchase_time),
            "getOriginalJson" => astr(vm, format!(
                "{{\"orderId\":\"{}\",\"purchaseToken\":\"{}\",\"purchaseState\":{},\"acknowledged\":{}}}",
                rec.order_id, rec.token, rec.state, rec.acknowledged))?,
            "getSignature" => astr(vm, "rustydroid-fake-signature".to_string())?,
            "isAutoRenewing" => Value::Int(0),
            "getAccountIdentifiers" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some((code, purchases)) = vm.fw.gms.purchases_results.get(&recv).cloned() {
        return Ok(Some(match name {
            "getResponseCode" => Value::Int(code),
            "getBillingResult" => Value::Obj(make_billing_result(vm, code, "")?),
            "getPurchasesList" => {
                make_list(vm, purchases.iter().map(|r| Value::Obj(*r)).collect())?
            }
            _ => return Ok(None),
        }));
    }
    if !vm.fw.gms.billing_clients.contains_key(&recv) {
        return Ok(None);
    }
    let host = vm
        .fw
        .gms
        .billing_clients
        .get(&recv)
        .cloned()
        .unwrap_or_default();
    Ok(Some(match name {
        "<init>" => Value::Null,
        "startConnection" => {
            let code = vm.fw.gms.billing_setup_code;
            if let Some(c) = vm.fw.gms.billing_clients.get_mut(&recv) {
                c.connected = code == 0;
            }
            if let Some(listener) = oarg(args, 0) {
                let res = make_billing_result(vm, code, "")?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onBillingSetupFinished",
                    &["(Lcom/android/billingclient/api/BillingResult;)V"],
                    vec![Value::Obj(res)],
                )?;
            }
            Value::Null
        }
        "endConnection" => {
            if let Some(c) = vm.fw.gms.billing_clients.get_mut(&recv) {
                c.connected = false;
            }
            Value::Null
        }
        "isReady" => Value::Int(host.connected as i32),
        "getConnectionState" => Value::Int(if host.connected { 2 } else { 0 }),
        "isFeatureSupported" => Value::Obj(make_billing_result(vm, 0, "")?),
        "querySkuDetailsAsync" => {
            let b = oarg(args, 0)
                .and_then(|r| vm.fw.gms.billing_builders.get(&r).cloned())
                .unwrap_or_default();
            let wanted: Vec<SkuCfg> = vm
                .fw
                .gms
                .billing_catalog
                .iter()
                .filter(|c| b.skus.is_empty() || b.skus.contains(&c.sku))
                .filter(|c| b.sku_type.is_empty() || c.typ == b.sku_type)
                .cloned()
                .collect();
            let mut elems = Vec::with_capacity(wanted.len());
            for cfg in &wanted {
                elems.push(Value::Obj(sku_obj(vm, cfg, false)?));
            }
            if let Some(listener) = oarg(args, 1) {
                let res = make_billing_result(vm, 0, "")?;
                let list = make_list(vm, elems)?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onSkuDetailsResponse",
                    &["(Lcom/android/billingclient/api/BillingResult;Ljava/util/List;)V"],
                    vec![Value::Obj(res), list],
                )?;
            }
            Value::Null
        }
        "queryProductDetailsAsync" => {
            let b = oarg(args, 0)
                .and_then(|r| vm.fw.gms.billing_builders.get(&r).cloned())
                .unwrap_or_default();
            let wanted: Vec<SkuCfg> = vm
                .fw
                .gms
                .billing_catalog
                .iter()
                .filter(|c| b.product_ids.is_empty() || b.product_ids.contains(&c.sku))
                .cloned()
                .collect();
            let mut elems = Vec::with_capacity(wanted.len());
            for cfg in &wanted {
                elems.push(Value::Obj(sku_obj(vm, cfg, true)?));
            }
            if let Some(listener) = oarg(args, 1) {
                let res = make_billing_result(vm, 0, "")?;
                let list = make_list(vm, elems)?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onProductDetailsResponse",
                    &["(Lcom/android/billingclient/api/BillingResult;Ljava/util/List;)V"],
                    vec![Value::Obj(res), list],
                )?;
            }
            Value::Null
        }
        "launchBillingFlow" => {
            if !host.connected {
                return Ok(Some(Value::Obj(make_billing_result(
                    vm,
                    -1,
                    "Service disconnected (RustyDroid billing shim)",
                )?)));
            }
            let b = oarg(args, 1)
                .and_then(|r| vm.fw.gms.billing_builders.get(&r).cloned())
                .unwrap_or_default();
            let mut products = b.product_ids.clone();
            if products.is_empty() {
                if let Some(sd) = b.sku_details {
                    if let Some(cfg) = vm
                        .fw
                        .gms
                        .sku_details_objs
                        .get(&sd)
                        .or_else(|| vm.fw.gms.product_details_objs.get(&sd))
                    {
                        products = vec![cfg.sku.clone()];
                    }
                }
            }
            if products.is_empty() {
                products = b.skus.clone();
            }
            let known = !products.is_empty()
                && products
                    .iter()
                    .all(|p| vm.fw.gms.billing_catalog.iter().any(|c| &c.sku == p));
            let listener = host.listener;
            let fire = |vm: &mut Engine,
                        code: i32,
                        msg: &str,
                        purchase: Option<Value>|
             -> Result<(), VmExit> {
                if let Some(l) = listener {
                    let res = make_billing_result(vm, code, msg)?;
                    let list = match purchase {
                        Some(p) => make_list(vm, vec![p])?,
                        None => Value::Null,
                    };
                    let _ = call_dex_any(
                        vm,
                        l,
                        "onPurchasesUpdated",
                        &["(Lcom/android/billingclient/api/BillingResult;Ljava/util/List;)V"],
                        vec![Value::Obj(res), list],
                    )?;
                }
                Ok(())
            };
            if !known {
                fire(vm, 4, "Item unavailable", None)?;
                Value::Obj(make_billing_result(vm, 4, "Item unavailable")?)
            } else {
                match vm.fw.gms.billing_flow_outcome.clone() {
                    BillingOutcome::Canceled => fire(vm, 1, "User cancelled", None)?,
                    BillingOutcome::Error(code) => {
                        fire(vm, code, "Billing error (programmed)", None)?
                    }
                    BillingOutcome::Purchase => {
                        vm.fw.gms.billing_counter += 1;
                        let n = vm.fw.gms.billing_counter;
                        vm.fw.gms.billing_purchases.push(PurchaseRec {
                            token: format!("rustydroid-purchase-token-{n}"),
                            products: products.clone(),
                            state: 1,
                            acknowledged: false,
                            consumed: false,
                            order_id: format!("GPA.rustydroid-fake-{n}"),
                            purchase_time: 1_700_000_000_000 + vm.fw.clock() as i64,
                        });
                        let idx = vm.fw.gms.billing_purchases.len() - 1;
                        let pobj = purchase_obj(vm, idx)?;
                        fire(vm, 0, "", Some(Value::Obj(pobj)))?;
                    }
                }
                Value::Obj(make_billing_result(vm, 0, "")?)
            }
        }
        "queryPurchases" => {
            let typ = sarg(vm, args, 0).unwrap_or_default();
            let idxs: Vec<usize> = vm
                .fw
                .gms
                .billing_purchases
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    !p.consumed
                        && (typ.is_empty()
                            || p.products.iter().all(|id| {
                                vm.fw
                                    .gms
                                    .billing_catalog
                                    .iter()
                                    .any(|c| &c.sku == id && c.typ == typ)
                            }))
                })
                .map(|(i, _)| i)
                .collect();
            let mut objs = Vec::with_capacity(idxs.len());
            for i in idxs {
                objs.push(purchase_obj(vm, i)?);
            }
            let r = alloc(vm, PURCHASES_RESULT)?;
            vm.fw.gms.purchases_results.insert(r, (0, objs));
            Value::Obj(r)
        }
        "queryPurchasesAsync" => {
            let idxs: Vec<usize> = vm
                .fw
                .gms
                .billing_purchases
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.consumed)
                .map(|(i, _)| i)
                .collect();
            let mut elems = Vec::with_capacity(idxs.len());
            for i in idxs {
                elems.push(Value::Obj(purchase_obj(vm, i)?));
            }
            if let Some(listener) = last_oarg(args) {
                let res = make_billing_result(vm, 0, "")?;
                let list = make_list(vm, elems)?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onQueryPurchasesResponse",
                    &["(Lcom/android/billingclient/api/BillingResult;Ljava/util/List;)V"],
                    vec![Value::Obj(res), list],
                )?;
            }
            Value::Null
        }
        "acknowledgePurchase" => {
            let token = oarg(args, 0)
                .and_then(|r| vm.fw.gms.billing_builders.get(&r))
                .map(|b| b.token.clone())
                .unwrap_or_default();
            let code = match vm
                .fw
                .gms
                .billing_purchases
                .iter_mut()
                .find(|p| p.token == token)
            {
                Some(p) => {
                    p.acknowledged = true;
                    0
                }
                None => 5,
            };
            if let Some(listener) = oarg(args, 1) {
                let res = make_billing_result(vm, code, "")?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onAcknowledgePurchaseResponse",
                    &["(Lcom/android/billingclient/api/BillingResult;)V"],
                    vec![Value::Obj(res)],
                )?;
            }
            Value::Null
        }
        "consumeAsync" => {
            let token = oarg(args, 0)
                .and_then(|r| vm.fw.gms.billing_builders.get(&r))
                .map(|b| b.token.clone())
                .unwrap_or_default();
            let code = match vm
                .fw
                .gms
                .billing_purchases
                .iter_mut()
                .find(|p| p.token == token)
            {
                Some(p) => {
                    p.consumed = true;
                    0
                }
                None => 5,
            };
            if let Some(listener) = oarg(args, 1) {
                let res = make_billing_result(vm, code, "")?;
                let tok = astr(vm, token)?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onConsumeResponse",
                    &["(Lcom/android/billingclient/api/BillingResult;Ljava/lang/String;)V"],
                    vec![Value::Obj(res), tok],
                )?;
            }
            Value::Null
        }
        _ => return Ok(None),
    }))
}

// ── Ads ─────────────────────────────────────────────────────────────────
fn make_ad_error(vm: &mut Engine, code: i32, msg: &str, load: bool) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, if load { LOAD_AD_ERROR } else { AD_ERROR })?;
    vm.fw.gms.ad_errors.insert(r, (code, msg.to_string()));
    Ok(r)
}
fn ads_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some((code, msg)) = vm.fw.gms.ad_errors.get(&recv).cloned() {
        return Ok(Some(match name {
            "getCode" => Value::Int(code),
            "getMessage" => astr(vm, msg)?,
            "getDomain" => astr(vm, "com.google.android.gms.ads".to_string())?,
            _ => return Ok(None),
        }));
    }
    if vm.heap.class_of(recv).unwrap_or("") == INIT_STATUS {
        return Ok(Some(match name {
            "getAdapterStatusMap" => make_map(vm, Vec::new())?,
            _ => return Ok(None),
        }));
    }
    if let Some((amount, typ)) = vm.fw.gms.reward_items.get(&recv).cloned() {
        return Ok(Some(match name {
            "getAmount" => Value::Int(amount),
            "getType" => astr(vm, typ)?,
            _ => return Ok(None),
        }));
    }
    if mref_class == AD_REQUEST_BUILDER
        || vm.heap.class_of(recv).unwrap_or("") == AD_REQUEST_BUILDER
    {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "build" => Value::Obj(alloc(vm, AD_REQUEST)?),
            _ => Value::Obj(recv),
        }));
    }
    if vm.fw.gms.ad_views.contains_key(&recv)
        || mref_class == AD_VIEW
        || vm.heap.class_of(recv).unwrap_or("") == AD_VIEW
    {
        return Ok(Some(match name {
            "<init>" => {
                vm.fw.gms.ad_views.insert(recv, AdViewHost::default());
                Value::Null
            }
            "setAdUnitId" => {
                let id = sarg(vm, args, 0).unwrap_or_default();
                vm.fw.gms.ad_views.entry(recv).or_default().unit_id = id;
                Value::Null
            }
            "getAdUnitId" => {
                let id = vm
                    .fw
                    .gms
                    .ad_views
                    .get(&recv)
                    .map(|v| v.unit_id.clone())
                    .unwrap_or_default();
                astr(vm, id)?
            }
            "setAdListener" => {
                vm.fw.gms.ad_views.entry(recv).or_default().listener = oarg(args, 0);
                Value::Null
            }
            "loadAd" => {
                let unit = vm
                    .fw
                    .gms
                    .ad_views
                    .get(&recv)
                    .map(|v| v.unit_id.clone())
                    .unwrap_or_default();
                let listener = vm.fw.gms.ad_views.get(&recv).and_then(|v| v.listener);
                if vm.fw.gms.ad_load_fail {
                    vm.fw.gms.ad_events.push(format!("banner_fail:{unit}"));
                    if let Some(l) = listener {
                        let err = make_ad_error(
                            vm,
                            ad_error_code(vm),
                            "No fill (programmed RustyDroid ads shim)",
                            true,
                        )?;
                        let _ = call_dex_any(
                            vm,
                            l,
                            "onAdFailedToLoad",
                            &["(Lcom/google/android/gms/ads/LoadAdError;)V"],
                            vec![Value::Obj(err)],
                        )?;
                    }
                } else {
                    vm.fw.gms.ad_events.push(format!("banner_load:{unit}"));
                    if let Some(v) = vm.fw.gms.ad_views.get_mut(&recv) {
                        v.loaded = true;
                    }
                    if let Some(l) = listener {
                        let _ = call_dex_any(vm, l, "onAdLoaded", &["()V"], Vec::new())?;
                    }
                }
                Value::Null
            }
            "isLoading" => Value::Int(0),
            "pause" | "resume" | "destroy" | "setAdSize" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.full_screen_ads.contains_key(&recv) {
        let ad = vm
            .fw
            .gms
            .full_screen_ads
            .get(&recv)
            .cloned()
            .unwrap_or_default();
        return Ok(Some(match name {
            "setFullScreenContentCallback" => {
                vm.fw.gms.full_screen_ads.entry(recv).or_default().callback = oarg(args, 0);
                Value::Null
            }
            "show" => {
                vm.fw.gms.ad_events.push(format!("show:{}", ad.unit_id));
                if let Some(a) = vm.fw.gms.full_screen_ads.get_mut(&recv) {
                    a.shown = true;
                }
                if let Some(cb) = ad.callback {
                    let _ =
                        call_dex_any(vm, cb, "onAdShowedFullScreenContent", &["()V"], Vec::new())?;
                }
                if vm.heap.class_of(recv).unwrap_or("") == REWARDED {
                    let reward_listener = args
                        .iter()
                        .filter_map(|v| match v {
                            Value::Obj(r) => Some(*r),
                            _ => None,
                        })
                        .nth(1);
                    if let Some(l) = reward_listener {
                        let item = alloc(vm, REWARD_ITEM)?;
                        vm.fw
                            .gms
                            .reward_items
                            .insert(item, (10, "coins".to_string()));
                        let _ = call_dex_any(
                            vm,
                            l,
                            "onUserEarnedReward",
                            &["(Lcom/google/android/gms/ads/rewarded/RewardItem;)V"],
                            vec![Value::Obj(item)],
                        )?;
                        vm.fw.gms.ad_events.push(format!("reward:{}", ad.unit_id));
                    }
                }
                if let Some(cb) = ad.callback {
                    let _ = call_dex_any(
                        vm,
                        cb,
                        "onAdDismissedFullScreenContent",
                        &["()V"],
                        Vec::new(),
                    )?;
                }
                Value::Null
            }
            "getAdUnitId" => astr(vm, ad.unit_id)?,
            "setOnPaidEventListener" | "setImmersiveMode" => Value::Null,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}
fn ad_error_code(vm: &Engine) -> i32 {
    if vm.fw.gms.ad_error_code == 0 {
        3
    } else {
        vm.fw.gms.ad_error_code
    }
}
fn ads_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (MOBILE_ADS, "initialize") => {
            vm.fw.gms.ads_initialized = true;
            vm.fw.gms.ad_events.push("initialize".to_string());
            if let Some(listener) = oarg(args, 1) {
                let status = alloc(vm, INIT_STATUS)?;
                let _ = call_dex_any(
                    vm,
                    listener,
                    "onInitializationComplete",
                    &["(Lcom/google/android/gms/ads/initialization/InitializationStatus;)V"],
                    vec![Value::Obj(status)],
                )?;
            }
            Ok(Some(Value::Null))
        }
        (MOBILE_ADS, "getInitializationStatus") => {
            alloc(vm, INIT_STATUS).map(|r| Some(Value::Obj(r)))
        }
        (MOBILE_ADS, "setRequestConfiguration") => {
            vm.fw
                .gms
                .ad_events
                .push("request_configuration".to_string());
            Ok(Some(Value::Null))
        }
        (INTERSTITIAL, "load") | (REWARDED, "load") => {
            let unit = sarg(vm, args, 1).unwrap_or_default();
            let callback = oarg(args, 3);
            let kind = if class == REWARDED {
                "rewarded"
            } else {
                "interstitial"
            };
            if vm.fw.gms.ad_load_fail {
                vm.fw.gms.ad_events.push(format!("{kind}_fail:{unit}"));
                if let Some(cb) = callback {
                    let err = make_ad_error(
                        vm,
                        ad_error_code(vm),
                        "No fill (programmed RustyDroid ads shim)",
                        true,
                    )?;
                    let _ = call_dex_any(
                        vm,
                        cb,
                        "onAdFailedToLoad",
                        &["(Lcom/google/android/gms/ads/LoadAdError;)V"],
                        vec![Value::Obj(err)],
                    )?;
                }
            } else {
                vm.fw.gms.ad_events.push(format!("{kind}_load:{unit}"));
                let ad = alloc(vm, class)?;
                vm.fw.gms.full_screen_ads.insert(
                    ad,
                    FullScreenAd {
                        unit_id: unit,
                        callback: None,
                        shown: false,
                    },
                );
                if let Some(cb) = callback {
                    let sig = if class == REWARDED {
                        "(Lcom/google/android/gms/ads/rewarded/RewardedAd;)V"
                    } else {
                        "(Lcom/google/android/gms/ads/interstitial/InterstitialAd;)V"
                    };
                    let _ = call_dex_any(
                        vm,
                        cb,
                        "onAdLoaded",
                        &[sig, "(Ljava/lang/Object;)V"],
                        vec![Value::Obj(ad)],
                    )?;
                }
            }
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

// ── Maps ────────────────────────────────────────────────────────────────
fn latlng_obj(vm: &mut Engine, lat: f64, lng: f64) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, LATLNG)?;
    vm.fw.gms.latlngs.insert(r, (lat, lng));
    put(vm, r, "latitude", Value::Double(lat));
    put(vm, r, "longitude", Value::Double(lng));
    Ok(r)
}
fn camera_pos_obj(vm: &mut Engine, pos: &CameraPos) -> Result<ObjRef, VmExit> {
    let r = alloc(vm, CAMERA_POSITION)?;
    vm.fw.gms.camera_positions.insert(r, pos.clone());
    let ll = latlng_obj(vm, pos.lat, pos.lng)?;
    put(vm, r, "target", Value::Obj(ll));
    put(vm, r, "zoom", Value::Float(pos.zoom));
    put(vm, r, "bearing", Value::Float(pos.bearing));
    put(vm, r, "tilt", Value::Float(pos.tilt));
    Ok(r)
}
fn apply_camera_update(map: &mut MapHost, upd: &CameraUpd) {
    match upd {
        CameraUpd::LatLng(lat, lng, zoom) => {
            map.camera.lat = *lat;
            map.camera.lng = *lng;
            if let Some(z) = zoom {
                map.camera.zoom = *z;
            }
        }
        CameraUpd::ZoomTo(z) => map.camera.zoom = *z,
        CameraUpd::ZoomBy(d) => map.camera.zoom += *d,
    }
}
fn maps_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == LATLNG && name == "<init>" {
        let lat = darg(args, 0);
        let lng = darg(args, 1);
        vm.fw.gms.latlngs.insert(recv, (lat, lng));
        put(vm, recv, "latitude", Value::Double(lat));
        put(vm, recv, "longitude", Value::Double(lng));
        return Ok(Some(Value::Null));
    }
    if vm.fw.gms.camera_pos_builders.contains_key(&recv) || mref_class == CAMERA_POSITION_BUILDER {
        if name == "build" {
            let pos = vm
                .fw
                .gms
                .camera_pos_builders
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            return camera_pos_obj(vm, &pos).map(|r| Some(Value::Obj(r)));
        }
        let b = vm.fw.gms.camera_pos_builders.entry(recv).or_default();
        match name {
            "<init>" => {
                if let Some(p) =
                    oarg(args, 0).and_then(|r| vm.fw.gms.camera_positions.get(&r).cloned())
                {
                    *b = p;
                }
                return Ok(Some(Value::Null));
            }
            "target" => {
                if let Some(ll) = oarg(args, 0).and_then(|r| vm.fw.gms.latlngs.get(&r).copied()) {
                    b.lat = ll.0;
                    b.lng = ll.1;
                }
            }
            "zoom" => b.zoom = farg(args, 0),
            "bearing" => b.bearing = farg(args, 0),
            "tilt" => b.tilt = farg(args, 0),
            _ => return Ok(None),
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if vm.fw.gms.marker_options.contains_key(&recv) || mref_class == MARKER_OPTIONS {
        if name == "<init>" {
            vm.fw.gms.marker_options.insert(recv, MarkerCfg::default());
            return Ok(Some(Value::Null));
        }
        if name == "getPosition" {
            let cfg = vm
                .fw
                .gms
                .marker_options
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            return latlng_obj(vm, cfg.lat, cfg.lng).map(|r| Some(Value::Obj(r)));
        }
        if name == "getTitle" {
            let cfg = vm
                .fw
                .gms
                .marker_options
                .get(&recv)
                .cloned()
                .unwrap_or_default();
            return Ok(Some(if cfg.title.is_empty() {
                Value::Null
            } else {
                astr(vm, cfg.title)?
            }));
        }
        let ll_arg = oarg(args, 0).and_then(|r| vm.fw.gms.latlngs.get(&r).copied());
        let s_arg = sarg(vm, args, 0).unwrap_or_default();
        let cfg = vm.fw.gms.marker_options.entry(recv).or_default();
        match name {
            "position" => {
                if let Some(ll) = ll_arg {
                    cfg.lat = ll.0;
                    cfg.lng = ll.1;
                }
            }
            "title" => cfg.title = s_arg,
            "snippet" => cfg.snippet = s_arg,
            _ => {}
        }
        return Ok(Some(Value::Obj(recv)));
    }
    if let Some(cfg) = vm.fw.gms.markers.get(&recv).cloned() {
        return Ok(Some(match name {
            "getPosition" => Value::Obj(latlng_obj(vm, cfg.lat, cfg.lng)?),
            "getTitle" => {
                if cfg.title.is_empty() {
                    Value::Null
                } else {
                    astr(vm, cfg.title)?
                }
            }
            "getSnippet" => {
                if cfg.snippet.is_empty() {
                    Value::Null
                } else {
                    astr(vm, cfg.snippet)?
                }
            }
            "setTitle" => {
                let t = sarg(vm, args, 0).unwrap_or_default();
                if let Some(c) = vm.fw.gms.markers.get_mut(&recv) {
                    c.title = t;
                }
                Value::Null
            }
            "setPosition" => {
                if let Some(ll) = oarg(args, 0).and_then(|r| vm.fw.gms.latlngs.get(&r).copied()) {
                    if let Some(c) = vm.fw.gms.markers.get_mut(&recv) {
                        c.lat = ll.0;
                        c.lng = ll.1;
                    }
                }
                Value::Null
            }
            "getId" => astr(vm, cfg.id)?,
            "remove" => {
                if let Some(map) = vm.fw.gms.marker_map.get(&recv).copied() {
                    if let Some(m) = vm.fw.gms.google_maps.get_mut(&map) {
                        m.markers.retain(|x| *x != recv);
                    }
                }
                vm.fw.gms.markers.remove(&recv);
                Value::Null
            }
            "showInfoWindow" | "hideInfoWindow" | "setVisible" => Value::Null,
            "isVisible" => Value::Int(1),
            _ => return Ok(None),
        }));
    }
    if vm.heap.class_of(recv).unwrap_or("") == UI_SETTINGS {
        return Ok(Some(if name.starts_with("is") && args.is_empty() {
            Value::Int(1)
        } else {
            Value::Null
        }));
    }
    if vm.fw.gms.google_maps.contains_key(&recv) {
        return Ok(Some(match name {
            "moveCamera" | "animateCamera" => {
                if let Some(u) =
                    oarg(args, 0).and_then(|r| vm.fw.gms.camera_updates.get(&r).cloned())
                {
                    if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                        apply_camera_update(m, &u);
                    }
                }
                Value::Null
            }
            "getCameraPosition" => {
                let pos = vm
                    .fw
                    .gms
                    .google_maps
                    .get(&recv)
                    .map(|m| m.camera.clone())
                    .unwrap_or_default();
                Value::Obj(camera_pos_obj(vm, &pos)?)
            }
            "addMarker" => {
                let cfg = oarg(args, 0)
                    .and_then(|r| vm.fw.gms.marker_options.get(&r).cloned())
                    .unwrap_or_default();
                vm.fw.gms.maps_counter += 1;
                let n = vm.fw.gms.maps_counter;
                let marker = alloc(vm, MARKER)?;
                vm.fw.gms.markers.insert(
                    marker,
                    MarkerCfg {
                        id: format!("m{n}"),
                        ..cfg
                    },
                );
                vm.fw.gms.marker_map.insert(marker, recv);
                if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                    m.markers.push(marker);
                }
                Value::Obj(marker)
            }
            "clear" => {
                if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                    m.markers.clear();
                }
                Value::Null
            }
            "setMapType" => {
                if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                    m.map_type = iarg(args, 0);
                }
                Value::Null
            }
            "getMapType" => Value::Int(
                vm.fw
                    .gms
                    .google_maps
                    .get(&recv)
                    .map(|m| m.map_type)
                    .unwrap_or(1),
            ),
            "setOnMarkerClickListener" => {
                if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                    m.marker_click_listener = oarg(args, 0);
                }
                Value::Null
            }
            "getUiSettings" => match vm.fw.gms.google_maps.get(&recv).and_then(|m| m.ui_settings) {
                Some(r) => Value::Obj(r),
                None => {
                    let r = alloc(vm, UI_SETTINGS)?;
                    if let Some(m) = vm.fw.gms.google_maps.get_mut(&recv) {
                        m.ui_settings = Some(r);
                    }
                    Value::Obj(r)
                }
            },
            "setMyLocationEnabled"
            | "setTrafficEnabled"
            | "setBuildingsEnabled"
            | "setIndoorEnabled"
            | "setOnMapClickListener"
            | "setOnMapLoadedCallback" => Value::Null,
            "isTrafficEnabled" | "isMyLocationEnabled" => Value::Int(0),
            _ => return Ok(None),
        }));
    }
    if mref_class == SUPPORT_MAP_FRAGMENT
        || mref_class == MAP_VIEW
        || vm.heap.class_of(recv).unwrap_or("") == SUPPORT_MAP_FRAGMENT
        || vm.heap.class_of(recv).unwrap_or("") == MAP_VIEW
    {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getMapAsync" => {
                let map = match vm.fw.gms.map_by_fragment.get(&recv).copied() {
                    Some(m) if vm.fw.gms.google_maps.contains_key(&m) => m,
                    _ => {
                        let m = alloc(vm, GOOGLE_MAP)?;
                        let mut host = MapHost::default();
                        host.camera.zoom = 2.0;
                        vm.fw.gms.google_maps.insert(m, host);
                        vm.fw.gms.map_by_fragment.insert(recv, m);
                        m
                    }
                };
                if let Some(cb) = oarg(args, 0) {
                    let _ = call_dex_any(
                        vm,
                        cb,
                        "onMapReady",
                        &["(Lcom/google/android/gms/maps/GoogleMap;)V"],
                        vec![Value::Obj(map)],
                    )?;
                }
                Value::Null
            }
            "onCreate"
            | "onStart"
            | "onResume"
            | "onPause"
            | "onStop"
            | "onDestroy"
            | "onLowMemory"
            | "onSaveInstanceState" => Value::Null,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}
fn maps_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (MAPS_INITIALIZER, "initialize") => {
            let code = vm.fw.gms.availability_code;
            vm.fw.gms.maps_initialized += 1;
            Ok(Some(Value::Int(code)))
        }
        (CAMERA_UPDATE_FACTORY, "newLatLng") | (CAMERA_UPDATE_FACTORY, "newLatLngZoom") => {
            let ll = oarg(args, 0)
                .and_then(|r| vm.fw.gms.latlngs.get(&r).copied())
                .unwrap_or((0.0, 0.0));
            let zoom = if name == "newLatLngZoom" {
                Some(farg(args, 1))
            } else {
                None
            };
            let r = alloc(vm, CAMERA_UPDATE)?;
            vm.fw
                .gms
                .camera_updates
                .insert(r, CameraUpd::LatLng(ll.0, ll.1, zoom));
            Ok(Some(Value::Obj(r)))
        }
        (CAMERA_UPDATE_FACTORY, "newCameraPosition") => {
            let pos = oarg(args, 0)
                .and_then(|r| vm.fw.gms.camera_positions.get(&r).cloned())
                .unwrap_or_default();
            let r = alloc(vm, CAMERA_UPDATE)?;
            vm.fw
                .gms
                .camera_updates
                .insert(r, CameraUpd::LatLng(pos.lat, pos.lng, Some(pos.zoom)));
            Ok(Some(Value::Obj(r)))
        }
        (CAMERA_UPDATE_FACTORY, "zoomTo") => {
            let r = alloc(vm, CAMERA_UPDATE)?;
            vm.fw
                .gms
                .camera_updates
                .insert(r, CameraUpd::ZoomTo(farg(args, 0)));
            Ok(Some(Value::Obj(r)))
        }
        (CAMERA_UPDATE_FACTORY, "zoomIn")
        | (CAMERA_UPDATE_FACTORY, "zoomOut")
        | (CAMERA_UPDATE_FACTORY, "zoomBy") => {
            let d = if name == "zoomBy" {
                farg(args, 0)
            } else if name == "zoomIn" {
                1.0
            } else {
                -1.0
            };
            let r = alloc(vm, CAMERA_UPDATE)?;
            vm.fw.gms.camera_updates.insert(r, CameraUpd::ZoomBy(d));
            Ok(Some(Value::Obj(r)))
        }
        (SUPPORT_MAP_FRAGMENT, "newInstance") => {
            alloc(vm, SUPPORT_MAP_FRAGMENT).map(|r| Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

// ── Fit/Health ──────────────────────────────────────────────────────────
fn fit_instance(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(steps) = vm.fw.gms.fit_step_objs.get(&recv).copied() {
        let cls = vm.heap.class_of(recv).unwrap_or("");
        return Ok(Some(match (cls, name) {
            (DATA_READ_RESPONSE, "getDataSets") => {
                if steps == 0 {
                    make_list(vm, Vec::new())?
                } else {
                    let ds = alloc(vm, DATA_SET)?;
                    vm.fw.gms.fit_step_objs.insert(ds, steps);
                    make_list(vm, vec![Value::Obj(ds)])?
                }
            }
            (DATA_READ_RESPONSE, "getDataSet") => {
                let ds = alloc(vm, DATA_SET)?;
                vm.fw.gms.fit_step_objs.insert(ds, steps);
                Value::Obj(ds)
            }
            (DATA_SET, "getDataPoints") => {
                if steps == 0 {
                    make_list(vm, Vec::new())?
                } else {
                    let dp = alloc(vm, DATA_POINT)?;
                    vm.fw.gms.fit_step_objs.insert(dp, steps);
                    make_list(vm, vec![Value::Obj(dp)])?
                }
            }
            (DATA_POINT, "getValue") => {
                let v = alloc(vm, FIT_VALUE)?;
                vm.fw.gms.fit_step_objs.insert(v, steps);
                Value::Obj(v)
            }
            (FIT_VALUE, "asInt") => Value::Int(steps),
            (FIT_VALUE, "asFloat") => Value::Float(steps as f32),
            (FIT_VALUE, "asString") => astr(vm, steps.to_string())?,
            _ => return Ok(None),
        }));
    }
    let Some(kind) = vm.fw.gms.fit_clients.get(&recv).cloned() else {
        return Ok(None);
    };
    Ok(Some(match (kind.as_str(), name) {
        ("history", "readData") => {
            let r = alloc(vm, DATA_READ_RESPONSE)?;
            vm.fw.gms.fit_step_objs.insert(r, vm.fw.gms.fit_steps);
            Value::Obj(completed_task(vm, Value::Obj(r))?)
        }
        ("history", "insertData") | ("history", "updateData") => {
            vm.fw.gms.fit_inserts += 1;
            Value::Obj(void_task(vm)?)
        }
        ("history", "deleteData") => Value::Obj(void_task(vm)?),
        ("history", "readDailyTotal") | ("history", "readDailyTotalFromLocalDevice") => {
            let ds = alloc(vm, DATA_SET)?;
            vm.fw.gms.fit_step_objs.insert(ds, vm.fw.gms.fit_steps);
            Value::Obj(completed_task(vm, Value::Obj(ds))?)
        }
        ("recording", "subscribe") | ("recording", "unsubscribe") => Value::Obj(void_task(vm)?),
        ("recording", "listSubscriptions") => {
            let v = make_list(vm, Vec::new())?;
            Value::Obj(completed_task(vm, v)?)
        }
        ("sessions", "startSession") => {
            vm.fw.gms.fit_sessions_started.push(String::new());
            Value::Obj(void_task(vm)?)
        }
        ("sessions", "stopSession") => {
            let v = make_list(vm, Vec::new())?;
            Value::Obj(completed_task(vm, v)?)
        }
        _ => {
            let _ = args;
            return Ok(None);
        }
    }))
}

// ── Cast / Nearby / Integridade ─────────────────────────────────────────
fn cast_nearby_integrity_instance(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if vm.fw.gms.cast_context == Some(recv) {
        return Ok(Some(match name {
            "getSessionManager" => match vm.fw.gms.cast_session_manager {
                Some(r) => Value::Obj(r),
                None => {
                    let r = alloc(vm, SESSION_MANAGER)?;
                    vm.fw.gms.cast_session_manager = Some(r);
                    Value::Obj(r)
                }
            },
            "getCastState" => {
                if vm.fw.gms.cast_session.is_some() {
                    Value::Int(4)
                } else if vm.fw.gms.cast_device_name.is_some() {
                    Value::Int(2)
                } else {
                    Value::Int(1)
                } // NO_DEVICES_AVAILABLE
            }
            "addCastStateListener" => {
                if let Some(l) = oarg(args, 0) {
                    if !vm.fw.gms.cast_listeners.contains(&l) {
                        vm.fw.gms.cast_listeners.push(l);
                    }
                }
                Value::Null
            }
            "removeCastStateListener" => {
                if let Some(l) = oarg(args, 0) {
                    vm.fw.gms.cast_listeners.retain(|x| *x != l);
                }
                Value::Null
            }
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.cast_session_manager == Some(recv) || mref_class == SESSION_MANAGER {
        return Ok(Some(match name {
            "getCurrentCastSession" | "getCurrentSession" => vm
                .fw
                .gms
                .cast_session
                .map(Value::Obj)
                .unwrap_or(Value::Null),
            "addSessionManagerListener" | "removeSessionManagerListener" => Value::Null,
            "endCurrentSession" => {
                vm.fw.gms.cast_session = None;
                Value::Null
            }
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.cast_session == Some(recv) {
        return Ok(Some(match name {
            "isConnected" => Value::Int(1),
            "getSessionId" => astr(vm, "rustydroid-cast-session".to_string())?,
            "getCastDevice" => Value::Null,
            _ => return Ok(None),
        }));
    }
    if let Some(dev) = vm.fw.gms.endpoint_info.get(&recv).cloned() {
        return Ok(Some(match name {
            "getEndpointName" => astr(vm, dev)?,
            "getAuthenticationDigits" => astr(vm, "0000".to_string())?,
            "getAuthenticationToken" => astr(vm, "rustydroid-fake".to_string())?,
            "isIncomingConnection" => Value::Int(0),
            _ => return Ok(None),
        }));
    }
    if let Some(len) = vm.fw.gms.payload_objs.get(&recv).copied() {
        return Ok(Some(match name {
            "getType" => Value::Int(1), // BYTES
            "asBytes" => make_bytes(vm, &vec![0u8; len])?,
            _ => return Ok(None),
        }));
    }
    if let Some(content) = vm.fw.gms.nearby_messages.get(&recv).cloned() {
        return Ok(Some(match name {
            "<init>" => Value::Null,
            "getContent" => make_bytes(vm, &content)?,
            "getType" | "getNamespace" => astr(vm, "__reserved_namespace".to_string())?,
            _ => return Ok(None),
        }));
    }
    if mref_class == NEARBY_MESSAGE && name == "<init>" {
        let bytes = args.first().map(|v| read_bytes(vm, v)).unwrap_or_default();
        vm.fw.gms.nearby_messages.insert(recv, bytes);
        return Ok(Some(Value::Null));
    }
    if let Some(kind) = vm.fw.gms.nearby_clients.get(&recv).cloned() {
        if kind == "connections" {
            return Ok(Some(match name {
                "startDiscovery" => {
                    vm.fw.gms.nearby_discovering = true;
                    if let Some(cb) = oarg(args, 1) {
                        let endpoints = vm.fw.gms.nearby_endpoints.clone();
                        for (id, dev_name) in endpoints {
                            let info = alloc(vm, DISCOVERED_ENDPOINT)?;
                            vm.fw.gms.endpoint_info.insert(info, dev_name);
                            let idv = astr(vm, id)?;
                            let _ = call_dex_any(vm, cb, "onEndpointFound",
                                &["(Ljava/lang/String;Lcom/google/android/gms/nearby/connection/DiscoveredEndpointInfo;)V"],
                                vec![idv, Value::Obj(info)])?;
                        }
                    }
                    Value::Obj(void_task(vm)?)
                }
                "stopDiscovery" => {
                    vm.fw.gms.nearby_discovering = false;
                    Value::Null
                }
                "startAdvertising" => {
                    vm.fw.gms.nearby_advertising = sarg(vm, args, 0);
                    Value::Obj(void_task(vm)?)
                }
                "stopAdvertising" => {
                    vm.fw.gms.nearby_advertising = None;
                    Value::Null
                }
                "stopAllEndpoints" => {
                    vm.fw.gms.nearby_connections.clear();
                    Value::Null
                }
                "requestConnection" => {
                    let ids: Vec<String> = args
                        .iter()
                        .filter_map(|a| match a {
                            Value::Obj(r) => vm.heap.as_str(*r).ok().map(str::to_string),
                            _ => None,
                        })
                        .collect();
                    if let Some(id) = ids.last().cloned() {
                        vm.fw.gms.nearby_connections.push(id);
                    }
                    Value::Obj(void_task(vm)?)
                }
                "acceptConnection" | "rejectConnection" | "disconnectFromEndpoint" => {
                    Value::Obj(void_task(vm)?)
                }
                "sendPayload" => {
                    if let Some(p) = args.iter().find_map(|v| match v {
                        Value::Obj(r) => vm.fw.gms.payload_objs.get(r).copied(),
                        _ => None,
                    }) {
                        vm.fw.gms.nearby_payloads.push(p);
                    }
                    Value::Obj(void_task(vm)?)
                }
                _ => return Ok(None),
            }));
        }
        return Ok(Some(match name {
            "publish" | "unpublish" | "subscribe" | "unsubscribe" => Value::Obj(void_task(vm)?),
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.safetynet_client == Some(recv) {
        return Ok(Some(match name {
            "attest" => {
                let nonce = args.first().map(|v| read_bytes(vm, v)).unwrap_or_default();
                let jws = if !vm.fw.gms.attestation_jws.is_empty() {
                    vm.fw.gms.attestation_jws.clone()
                } else {
                    format!(
                        "rustydroid-fake-jws.nonce{}bytes.not-a-real-attestation",
                        nonce.len()
                    )
                };
                vm.fw.gms.integrity_nonces.push(format!(
                    "safetynet:{}bytes:apiKeyPresent={}",
                    nonce.len(),
                    sarg(vm, args, 1).is_some()
                ));
                let r = alloc(vm, ATTESTATION_RESPONSE)?;
                vm.fw.gms.attestations.insert(r, jws);
                Value::Obj(completed_task(vm, Value::Obj(r))?)
            }
            _ => return Ok(None),
        }));
    }
    if let Some(jws) = vm.fw.gms.attestations.get(&recv).cloned() {
        return Ok(Some(match name {
            "getJwsResult" => astr(vm, jws)?,
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.integrity_requests.contains_key(&recv) || mref_class == INTEGRITY_TOKEN_BUILDER {
        match name {
            "<init>" => {
                vm.fw.gms.integrity_requests.insert(recv, String::new());
                return Ok(Some(Value::Null));
            }
            "setNonce" => {
                let n = sarg(vm, args, 0).unwrap_or_default();
                vm.fw.gms.integrity_requests.insert(recv, n);
                return Ok(Some(Value::Obj(recv)));
            }
            "setCloudProjectNumber" => return Ok(Some(Value::Obj(recv))),
            "build" => {
                let nonce = vm
                    .fw
                    .gms
                    .integrity_requests
                    .get(&recv)
                    .cloned()
                    .unwrap_or_default();
                let r = alloc(vm, INTEGRITY_TOKEN_REQUEST)?;
                vm.fw.gms.integrity_requests.insert(r, nonce);
                return Ok(Some(Value::Obj(r)));
            }
            "nonce" => {
                let n = vm
                    .fw
                    .gms
                    .integrity_requests
                    .get(&recv)
                    .cloned()
                    .unwrap_or_default();
                return Ok(Some(astr(vm, n)?));
            }
            _ => {}
        }
    }
    if let Some(tok) = vm.fw.gms.integrity_responses.get(&recv).cloned() {
        return Ok(Some(match name {
            "token" => astr(vm, tok)?,
            _ => return Ok(None),
        }));
    }
    if vm.fw.gms.integrity_managers.contains(&recv) {
        return Ok(Some(match name {
            "requestIntegrityToken" => {
                let nonce = oarg(args, 0)
                    .and_then(|r| vm.fw.gms.integrity_requests.get(&r).cloned())
                    .unwrap_or_default();
                vm.fw
                    .gms
                    .integrity_nonces
                    .push(format!("integrity:{nonce}"));
                let token = if vm.fw.gms.integrity_token.is_empty() {
                    "rustydroid-fake-integrity-token".to_string()
                } else {
                    vm.fw.gms.integrity_token.clone()
                };
                let r = alloc(vm, INTEGRITY_TOKEN_RESPONSE)?;
                vm.fw.gms.integrity_responses.insert(r, token);
                Value::Obj(completed_task(vm, Value::Obj(r))?)
            }
            _ => return Ok(None),
        }));
    }
    Ok(None)
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

fn is_gms_class(class: &str) -> bool {
    GMS_PREFIXES.iter().any(|p| class.starts_with(p))
}

fn call_static_rooted(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !is_gms_class(class) {
        return Ok(None);
    }
    // Construtores de builders: newBuilder(...) estático em várias classes.
    if name == "newBuilder" {
        let builder_class = match class {
            BILLING_CLIENT => BILLING_CLIENT_BUILDER,
            SKU_PARAMS => SKU_PARAMS_BUILDER,
            QUERY_PRODUCTS_PARAMS => QUERY_PRODUCTS_BUILDER,
            QP_PRODUCT => QP_PRODUCT_BUILDER,
            FLOW_PARAMS => FLOW_PARAMS_BUILDER,
            FLOW_PD_PARAMS => FLOW_PD_BUILDER,
            ACK_PARAMS => ACK_BUILDER,
            CONSUME_PARAMS => CONSUME_BUILDER,
            QUERY_PURCHASES_PARAMS => QUERY_PURCHASES_BUILDER,
            INTEGRITY_TOKEN_REQUEST => INTEGRITY_TOKEN_BUILDER,
            _ => return Ok(None),
        };
        let r = alloc(vm, builder_class)?;
        if builder_class == INTEGRITY_TOKEN_BUILDER {
            vm.fw.gms.integrity_requests.insert(r, String::new());
        } else {
            vm.fw
                .gms
                .billing_builders
                .insert(r, BillingBuilder::default());
        }
        return Ok(Some(Value::Obj(r)));
    }
    match class {
        TASKS => return tasks_static(vm, name, args),
        AVAILABILITY => match name {
            "getInstance" => {
                if let Some(r) = vm.fw.gms.availability_obj {
                    return Ok(Some(Value::Obj(r)));
                }
                let r = alloc(vm, AVAILABILITY)?;
                vm.fw.gms.availability_obj = Some(r);
                return Ok(Some(Value::Obj(r)));
            }
            "getErrorString" => return astr(vm, error_string(iarg(args, 0))).map(Some),
            _ => {}
        },
        FIREBASE_APP => {
            if let Some(v) = firebase_core_static(vm, name, args)? {
                return Ok(Some(v));
            }
        }
        FIREBASE_ANALYTICS if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.analytics_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_ANALYTICS)?;
            vm.fw.gms.analytics_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        FIREBASE_AUTH if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.auth_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_AUTH)?;
            vm.fw.gms.auth_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        FIREBASE_MESSAGING if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.messaging_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_MESSAGING)?;
            vm.fw.gms.messaging_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        FIREBASE_CRASHLYTICS if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.crashlytics_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_CRASHLYTICS)?;
            vm.fw.gms.crashlytics_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        FIRESTORE if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.firestore_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIRESTORE)?;
            vm.fw.gms.firestore_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        SET_OPTIONS if name == "merge" => {
            let r = alloc(vm, SET_OPTIONS)?;
            vm.fw.gms.fs_set_options.insert(r, true);
            return Ok(Some(Value::Obj(r)));
        }
        LISTENER_REG => {}
        FIREBASE_DATABASE if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            let url = sarg(vm, args, 1)
                .or_else(|| sarg(vm, args, 0))
                .unwrap_or_else(|| default_fb_options(vm).database_url);
            if vm.fw.gms.rtdb_url.is_empty() {
                vm.fw.gms.rtdb_url = url;
            }
            if let Some(r) = vm.fw.gms.rtdb_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_DATABASE)?;
            vm.fw.gms.rtdb_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        REMOTE_CONFIG if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.rc_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, REMOTE_CONFIG)?;
            vm.fw.gms.rc_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        FIREBASE_STORAGE if name == "getInstance" => {
            let _ = ensure_default_app(vm)?;
            if let Some(r) = vm.fw.gms.storage_obj {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FIREBASE_STORAGE)?;
            vm.fw.gms.storage_obj = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        LOCATION_SERVICES if name == "getFusedLocationProviderClient" => {
            if let Some(r) = vm.fw.gms.fused_client {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, FUSED_CLIENT)?;
            vm.fw.gms.fused_client = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        GOOGLE_SIGNIN => match name {
            "getClient" => {
                let r = alloc(vm, SIGNIN_CLIENT)?;
                vm.fw.gms.gms_signin_clients.push(r);
                return Ok(Some(Value::Obj(r)));
            }
            "getLastSignedInAccount" => {
                if vm.fw.gms.gms_signed_in {
                    if let Some(cfg) = vm.fw.gms.gms_account_cfg.clone() {
                        let r = mint_gms_account(vm, &cfg)?;
                        return Ok(Some(Value::Obj(r)));
                    }
                }
                return Ok(Some(Value::Null));
            }
            "hasPermissions" => return Ok(Some(Value::Int(0))),
            _ => {}
        },
        MOBILE_ADS | INTERSTITIAL | REWARDED => {
            if let Some(v) = ads_static(vm, class, name, args)? {
                return Ok(Some(v));
            }
        }
        MAPS_INITIALIZER | CAMERA_UPDATE_FACTORY | SUPPORT_MAP_FRAGMENT => {
            if let Some(v) = maps_static(vm, class, name, args)? {
                return Ok(Some(v));
            }
        }
        LOCATION_REQUEST if name == "create" => {
            let r = alloc(vm, LOCATION_REQUEST)?;
            vm.fw.gms.loc_request_objs.insert(r, LocReq::default());
            return Ok(Some(Value::Obj(r)));
        }
        BILLING_CLIENT if name == "newBuilder" => {}
        FITNESS => {
            let kind = match name {
                "getHistoryClient" => Some("history"),
                "getRecordingClient" => Some("recording"),
                "getSessionsClient" => Some("sessions"),
                _ => None,
            };
            if let Some(kind) = kind {
                let client_class = match kind {
                    "history" => "Lcom/google/android/gms/fitness/HistoryClient;",
                    "recording" => "Lcom/google/android/gms/fitness/RecordingClient;",
                    _ => "Lcom/google/android/gms/fitness/SessionsClient;",
                };
                let r = alloc(vm, client_class)?;
                vm.fw.gms.fit_clients.insert(r, kind.to_string());
                return Ok(Some(Value::Obj(r)));
            }
        }
        CAST_CONTEXT if name == "getSharedInstance" => {
            if let Some(r) = vm.fw.gms.cast_context {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, CAST_CONTEXT)?;
            vm.fw.gms.cast_context = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        NEARBY => match name {
            "getConnectionsClient" => {
                let r = alloc(
                    vm,
                    "Lcom/google/android/gms/nearby/connection/ConnectionsClient;",
                )?;
                vm.fw
                    .gms
                    .nearby_clients
                    .insert(r, "connections".to_string());
                return Ok(Some(Value::Obj(r)));
            }
            "getMessagesClient" => {
                let r = alloc(
                    vm,
                    "Lcom/google/android/gms/nearby/messages/MessagesClient;",
                )?;
                vm.fw.gms.nearby_clients.insert(r, "messages".to_string());
                return Ok(Some(Value::Obj(r)));
            }
            _ => {}
        },
        PAYLOAD if name == "fromBytes" => {
            let bytes = args.first().map(|v| read_bytes(vm, v)).unwrap_or_default();
            let r = alloc(vm, PAYLOAD)?;
            vm.fw.gms.payload_objs.insert(r, bytes.len());
            return Ok(Some(Value::Obj(r)));
        }
        SAFETYNET if name == "getClient" => {
            if let Some(r) = vm.fw.gms.safetynet_client {
                return Ok(Some(Value::Obj(r)));
            }
            let r = alloc(vm, SAFETYNET_CLIENT)?;
            vm.fw.gms.safetynet_client = Some(r);
            return Ok(Some(Value::Obj(r)));
        }
        INTEGRITY_FACTORY if name == "create" => {
            let r = alloc(
                vm,
                "Lcom/google/android/play/core/integrity/IntegrityManager;",
            )?;
            vm.fw.gms.integrity_managers.push(r);
            return Ok(Some(Value::Obj(r)));
        }
        _ => {}
    }
    Ok(None)
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

fn tracked(vm: &Engine, recv: ObjRef) -> bool {
    let g = &vm.fw.gms;
    g.tasks.contains_key(&recv)
        || g.task_sources.contains_key(&recv)
        || g.statuses.contains_key(&recv)
        || g.conn_results.contains_key(&recv)
        || g.api_exceptions.contains_key(&recv)
        || g.api_clients.contains_key(&recv)
        || g.api_client_builders.contains_key(&recv)
        || g.fb_app_names.contains_key(&recv)
        || g.fb_options_objs.contains_key(&recv)
        || g.fb_options_builders.contains_key(&recv)
        || g.auth_user_by_obj.contains_key(&recv)
        || g.auth_result_users.contains_key(&recv)
        || g.token_results.contains_key(&recv)
        || g.gms_account_by_obj.contains_key(&recv)
        || g.gms_signin_clients.contains(&recv)
        || g.remote_messages.contains_key(&recv)
        || g.rm_builders.contains_key(&recv)
        || g.ckav_builders.contains_key(&recv)
        || g.ckav_objs.contains_key(&recv)
        || g.fs_refs.contains_key(&recv)
        || g.fs_queries.contains_key(&recv)
        || g.fs_doc_snaps.contains_key(&recv)
        || g.fs_query_snaps.contains_key(&recv)
        || g.rtdb_refs.contains_key(&recv)
        || g.rtdb_snaps.contains_key(&recv)
        || g.rc_value_objs.contains_key(&recv)
        || g.storage_refs.contains_key(&recv)
        || g.storage_list_results.contains_key(&recv)
        || g.upload_snaps.contains_key(&recv)
        || g.loc_request_objs.contains_key(&recv)
        || g.loc_results.contains_key(&recv)
        || g.loc_availability.contains_key(&recv)
        || g.billing_clients.contains_key(&recv)
        || g.billing_builders.contains_key(&recv)
        || g.billing_results.contains_key(&recv)
        || g.sku_details_objs.contains_key(&recv)
        || g.product_details_objs.contains_key(&recv)
        || g.purchase_by_obj.contains_key(&recv)
        || g.purchases_results.contains_key(&recv)
        || g.ad_views.contains_key(&recv)
        || g.full_screen_ads.contains_key(&recv)
        || g.ad_errors.contains_key(&recv)
        || g.reward_items.contains_key(&recv)
        || g.latlngs.contains_key(&recv)
        || g.camera_positions.contains_key(&recv)
        || g.camera_pos_builders.contains_key(&recv)
        || g.google_maps.contains_key(&recv)
        || g.marker_options.contains_key(&recv)
        || g.markers.contains_key(&recv)
        || g.fit_clients.contains_key(&recv)
        || g.fit_step_objs.contains_key(&recv)
        || g.nearby_clients.contains_key(&recv)
        || g.endpoint_info.contains_key(&recv)
        || g.payload_objs.contains_key(&recv)
        || g.nearby_messages.contains_key(&recv)
        || g.attestations.contains_key(&recv)
        || g.integrity_requests.contains_key(&recv)
        || g.integrity_responses.contains_key(&recv)
        || g.integrity_managers.contains(&recv)
        || Some(recv) == g.availability_obj
        || Some(recv) == g.analytics_obj
        || Some(recv) == g.auth_obj
        || Some(recv) == g.messaging_obj
        || Some(recv) == g.crashlytics_obj
        || Some(recv) == g.firestore_obj
        || Some(recv) == g.rtdb_obj
        || Some(recv) == g.rc_obj
        || Some(recv) == g.storage_obj
        || Some(recv) == g.fused_client
        || Some(recv) == g.cast_context
        || Some(recv) == g.cast_session_manager
        || Some(recv) == g.cast_session
        || Some(recv) == g.safetynet_client
}

fn call_instance_rooted(
    vm: &mut Engine,
    mref_class: &str,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let recv_class = vm.heap.class_of(recv).unwrap_or("").to_string();
    if !tracked(vm, recv) && !is_gms_class(mref_class) && !is_gms_class(&recv_class) {
        return Ok(None);
    }
    // Tasks primeiro: objetos Task/TaskCompletionSource do shim.
    if mref_class == TASK_SOURCE
        || recv_class == TASK_SOURCE
        || vm.fw.gms.task_sources.contains_key(&recv)
    {
        if let Some(v) = task_source_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.tasks.contains_key(&recv) {
        if let Some(v) = task_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if let Some(v) = common_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = firebase_core_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if vm.fw.gms.analytics_obj == Some(recv) || mref_class == FIREBASE_ANALYTICS {
        if let Some(v) = analytics_instance(vm, name, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.auth_result_users.contains_key(&recv)
        || vm.fw.gms.token_results.contains_key(&recv)
        || vm.fw.gms.auth_user_by_obj.contains_key(&recv)
        || vm.fw.gms.auth_obj == Some(recv)
        || mref_class.starts_with("Lcom/google/firebase/auth/")
    {
        if let Some(v) = fb_auth_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if let Some(v) = gms_auth_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if vm.fw.gms.remote_messages.contains_key(&recv)
        || vm.fw.gms.rm_builders.contains_key(&recv)
        || vm.fw.gms.messaging_obj == Some(recv)
        || mref_class.starts_with("Lcom/google/firebase/messaging/")
    {
        if let Some(v) = messaging_instance(vm, mref_class, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.crashlytics_obj == Some(recv)
        || vm.fw.gms.ckav_builders.contains_key(&recv)
        || mref_class.starts_with("Lcom/google/firebase/crashlytics/")
    {
        if let Some(v) = crashlytics_instance(vm, mref_class, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.fs_refs.contains_key(&recv)
        || vm.fw.gms.fs_queries.contains_key(&recv)
        || vm.fw.gms.fs_doc_snaps.contains_key(&recv)
        || vm.fw.gms.fs_query_snaps.contains_key(&recv)
        || vm.fw.gms.firestore_obj == Some(recv)
    {
        if let Some(v) = firestore_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.rtdb_refs.contains_key(&recv)
        || vm.fw.gms.rtdb_snaps.contains_key(&recv)
        || vm.fw.gms.rtdb_obj == Some(recv)
    {
        if let Some(v) = rtdb_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.rc_obj == Some(recv) || vm.fw.gms.rc_value_objs.contains_key(&recv) {
        if let Some(v) = remote_config_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if vm.fw.gms.storage_refs.contains_key(&recv)
        || vm.fw.gms.storage_obj == Some(recv)
        || vm.fw.gms.upload_snaps.contains_key(&recv)
        || vm.fw.gms.storage_list_results.contains_key(&recv)
    {
        if let Some(v) = storage_instance(vm, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if let Some(v) = location_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if mref_class.starts_with("Lcom/android/billingclient/")
        || vm.fw.gms.billing_clients.contains_key(&recv)
        || vm.fw.gms.billing_builders.contains_key(&recv)
    {
        if let Some(v) = billing_instance(vm, mref_class, name, recv, args)? {
            return Ok(Some(v));
        }
    }
    if let Some(v) = ads_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = maps_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = fit_instance(vm, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = cast_nearby_integrity_instance(vm, mref_class, name, recv, args)? {
        return Ok(Some(v));
    }
    Ok(None)
}

// ── API de programação do Engine (hospedeiro) ───────────────────────────
/// Programadores `gms_*`: configuram o comportamento do shim antes de
/// executar o código do app. Espelham `Engine::androidx_*` (Apêndice D).
impl Engine {
    /// Define o resultado de `isGooglePlayServicesAvailable`.
    pub fn gms_set_availability(&mut self, code: i32) {
        self.fw.gms.availability_code = code;
    }

    /// Define o fix de GPS vendido pelo FusedLocationClient.
    pub fn gms_set_location_fix(&mut self, lat: f64, lon: f64, accuracy_m: f32) {
        self.fw.gms.location_fix = Some(FixCfg {
            lat,
            lng: lon,
            accuracy_m,
            ..FixCfg::default()
        });
    }

    /// Remove o fix (getLastLocation volta a concluir com null).
    pub fn gms_clear_location_fix(&mut self) {
        self.fw.gms.location_fix = None;
    }

    /// Define um novo fix e entrega-o já a todos os callbacks registados
    /// com `requestLocationUpdates`.
    pub fn gms_push_location(&mut self, lat: f64, lon: f64, accuracy_m: f32) {
        self.gms_set_location_fix(lat, lon, accuracy_m);
        if let Some(fix) = self.fw.gms.location_fix.clone() {
            let callbacks: Vec<ObjRef> = self
                .fw
                .gms
                .location_requests
                .iter()
                .map(|(r, _)| *r)
                .collect();
            for cb in callbacks {
                let _ = deliver_location(self, cb, &fix);
            }
        }
    }

    /// Nº de callbacks de localização registados.
    pub fn gms_location_callback_count(&self) -> usize {
        self.fw.gms.location_requests.len()
    }

    /// Programa a conta Google vendida depois de `gms_complete_signin`.
    pub fn gms_program_google_account(&mut self, email: &str, display_name: &str) {
        self.fw.gms.gms_account_cfg = Some(GmsAccountCfg {
            id: format!("gmsid-{email}"),
            email: email.to_string(),
            display_name: display_name.to_string(),
            id_token: format!("rustydroid-gms-idtoken-{email}"),
        });
    }

    /// Completa o fluxo de entrada fora da VM: `silentSignIn` e
    /// `getLastSignedInAccount` passam a vender a conta programada (ou
    /// uma por defeito).
    pub fn gms_complete_signin(&mut self, account: Option<(&str, &str)>) {
        if let Some((email, name)) = account {
            self.gms_program_google_account(email, name);
        }
        if self.fw.gms.gms_account_cfg.is_none() {
            self.gms_program_google_account("user@noreply.github.com", "Test User");
        }
        self.fw.gms.gms_signed_in = true;
    }

    /// Termina a sessão Google (silentSignIn volta a falhar, código 4).
    pub fn gms_sign_out_google(&mut self) {
        self.fw.gms.gms_signed_in = false;
    }

    /// Exige `FirebaseApp.initializeApp` antes de qualquer
    /// `FirebaseXxx.getInstance` (comportamento upstream); por defeito o
    /// shim auto-inicializa com opções falsas (ver preâmbulo do módulo).
    pub fn gms_firebase_strict_init(&mut self, strict: bool) {
        self.fw.gms.fb_strict_init = strict;
    }

    /// Pré-regista um utilizador no Firebase Auth local.
    #[allow(clippy::field_reassign_with_default)] // campo password fora do literal, de propósito
    pub fn gms_program_firebase_user(&mut self, email: &str, pwd: &str) {
        let mut rec = AuthUserRec::default();
        rec.uid = format!("uid-{email}");
        rec.email = email.to_string();
        rec.password = pwd.to_string();
        match self.fw.gms.auth_users.iter_mut().find(|u| u.email == email) {
            Some(old) => *old = rec,
            None => self.fw.gms.auth_users.push(rec),
        }
    }

    /// Eventos de Analytics capturados.
    pub fn gms_analytics_events(&self) -> &[AnalyticsEvent] {
        &self.fw.gms.analytics_events
    }

    /// Propriedades de utilizador do Analytics.
    pub fn gms_analytics_user_props(&self) -> Vec<(String, String)> {
        self.fw.gms.analytics_user_properties.clone()
    }

    /// Define o token FCM vendido por `getToken()`.
    pub fn gms_set_fcm_token(&mut self, token: &str) {
        self.fw.gms.fcm_token = token.to_string();
    }

    /// Tópicos FCM subscritos.
    pub fn gms_fcm_topics(&self) -> Vec<String> {
        self.fw.gms.fcm_topics.clone()
    }

    /// Entrega uma RemoteMessage às instâncias registadas de
    /// FirebaseMessagingService (ou a uma nova, se a classe existir em
    /// DEX). Devolve também o objeto RemoteMessage.
    pub fn gms_deliver_remote_message(
        &mut self,
        from: &str,
        data: Vec<(String, String)>,
        notification: Option<(&str, &str)>,
    ) -> Result<Value, VmExit> {
        let (title, body) = notification
            .map(|(t, b)| (t.to_string(), b.to_string()))
            .unwrap_or_default();
        let msg = RemoteMsg {
            from: from.to_string(),
            data,
            title,
            body,
            ..RemoteMsg::default()
        };
        let obj = mint_remote_message(self, msg)?;
        let sig = "(Lcom/google/firebase/messaging/RemoteMessage;)V";
        let services = self.fw.gms.messaging_services.clone();
        if services.is_empty() {
            let class = "Lcom/google/firebase/messaging/FirebaseMessagingService;";
            if self
                .cp
                .resolve_method(class, "onMessageReceived", sig)
                .is_some()
            {
                let svc = alloc(self, class)?;
                let _ = crate::m8::call_dex(
                    self,
                    svc,
                    "onMessageReceived",
                    sig,
                    vec![Value::Obj(obj)],
                )?;
            }
        } else {
            for svc in services {
                let _ = crate::m8::call_dex(
                    self,
                    svc,
                    "onMessageReceived",
                    sig,
                    vec![Value::Obj(obj)],
                )?;
            }
        }
        Ok(Value::Obj(obj))
    }

    /// Alimenta o Crashlytics com um crash real da VM — é o "crashes
    /// reais são capturados" do spec: o hospedeiro entrega aqui o
    /// resumo do VmExit fatal.
    pub fn gms_report_uncaught_crash(&mut self, class: &str, message: &str) {
        self.fw.gms.crashes.push(CrashRecord {
            kind: "uncaught".to_string(),
            class: class.to_string(),
            message: message.to_string(),
        });
    }

    /// Sumários dos crashes registados.
    pub fn gms_crashlytics_crash_summaries(&self) -> Vec<String> {
        self.fw
            .gms
            .crashes
            .iter()
            .map(|c| format!("{} {}: {}", c.kind, c.class, c.message))
            .collect()
    }

    /// Linhas de log do Crashlytics.
    pub fn gms_crashlytics_log(&self) -> Vec<String> {
        self.fw.gms.crashlytics_logs.clone()
    }

    /// Semeia um documento do Firestore local.
    pub fn gms_firestore_seed(&mut self, doc_path: &str, fields: Vec<(String, Value)>) {
        if !self.fw.gms.fs_docs.contains_key(doc_path) {
            self.fw.gms.fs_doc_order.push(doc_path.to_string());
        }
        self.fw.gms.fs_docs.insert(doc_path.to_string(), fields);
    }

    /// Lê um documento do Firestore local.
    pub fn gms_firestore_doc(&self, doc_path: &str) -> Option<Vec<(String, Value)>> {
        self.fw.gms.fs_docs.get(doc_path).cloned()
    }

    /// Semeia um valor do RTDB.
    pub fn gms_rtdb_seed(&mut self, path: &str, value: Value) {
        self.fw
            .gms
            .rtdb_values
            .insert(path.trim_start_matches('/').to_string(), value);
    }

    /// Lê um valor do RTDB local (subárvores voltam como mapa host).
    pub fn gms_rtdb_value(&self, path: &str) -> Option<Value> {
        rtdb_resolve(self, path.trim_start_matches('/'))
    }

    /// Valores que o próximo fetch do RemoteConfig vai buscar.
    pub fn gms_remoteconfig_program(&mut self, values: Vec<(String, String)>) {
        for (k, v) in values {
            upsert(&mut self.fw.gms.rc_server, k, v);
        }
    }

    /// Semeia um blob da Cloud Storage.
    pub fn gms_storage_seed(&mut self, path: &str, bytes: Vec<u8>) {
        self.fw.gms.storage_blobs.insert(path.to_string(), bytes);
    }

    /// Lê um blob da Cloud Storage local.
    pub fn gms_storage_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.fw.gms.storage_blobs.get(path).cloned()
    }

    /// Adiciona um SKU ao catálogo de Billing (preço já formatado).
    #[allow(clippy::too_many_arguments)]
    pub fn gms_billing_add_sku(
        &mut self,
        sku: &str,
        sku_type: &str,
        price: &str,
        price_amount_micros: i64,
        currency: &str,
        title: &str,
    ) {
        let cfg = SkuCfg {
            sku: sku.to_string(),
            typ: sku_type.to_string(),
            price: price.to_string(),
            price_micros: price_amount_micros,
            currency: currency.to_string(),
            title: title.to_string(),
            description: String::new(),
        };
        match self
            .fw
            .gms
            .billing_catalog
            .iter_mut()
            .find(|c| c.sku == sku)
        {
            Some(old) => *old = cfg,
            None => self.fw.gms.billing_catalog.push(cfg),
        }
    }

    /// Programa o resultado de `launchBillingFlow`.
    pub fn gms_billing_set_outcome(&mut self, outcome: BillingOutcome) {
        self.fw.gms.billing_flow_outcome = outcome;
    }

    /// Compras atualmente vendidas.
    pub fn gms_billing_purchases(&self) -> Vec<PurchaseRec> {
        self.fw.gms.billing_purchases.clone()
    }

    /// Registo de eventos de anúncios estruturados.
    pub fn gms_ads_events(&self) -> Vec<String> {
        self.fw.gms.ad_events.clone()
    }

    /// Faz os carregamentos de anúncios falharem com o código dado
    /// (0 interno, 1 pedido inválido, 2 rede, 3 sem fill).
    pub fn gms_ads_set_failure(&mut self, code: i32) {
        self.fw.gms.ad_load_fail = true;
        self.fw.gms.ad_error_code = code;
    }

    /// Repõe o sucesso dos anúncios.
    pub fn gms_ads_clear_failure(&mut self) {
        self.fw.gms.ad_load_fail = false;
    }

    /// (lat, lng, zoom) da câmara de cada mapa vivo.
    pub fn gms_maps_cameras(&self) -> Vec<(f64, f64, f32)> {
        self.fw
            .gms
            .google_maps
            .values()
            .map(|m| (m.camera.lat, m.camera.lng, m.camera.zoom))
            .collect()
    }

    /// Marcadores vivos (posição + título).
    pub fn gms_maps_markers(&self) -> Vec<(f64, f64, String)> {
        self.fw
            .gms
            .markers
            .values()
            .map(|m| (m.lat, m.lng, m.title.clone()))
            .collect()
    }

    /// Define os passos vendidos pelo Fit HistoryClient.
    pub fn gms_fit_set_steps(&mut self, steps: i32) {
        self.fw.gms.fit_steps = steps;
    }

    /// Liga uma sessão Cast programada (getCastState → CONNECTED).
    pub fn gms_cast_connect(&mut self, device_name: &str) {
        self.fw.gms.cast_device_name = Some(device_name.to_string());
        if self.fw.gms.cast_session.is_none() {
            if let Ok(r) = alloc(self, CAST_SESSION) {
                self.fw.gms.cast_session = Some(r);
            }
        }
    }

    /// Finaliza a sessão Cast programada.
    pub fn gms_cast_disconnect(&mut self) {
        self.fw.gms.cast_device_name = None;
        self.fw.gms.cast_session = None;
    }

    /// Adiciona um endpoint descoberto do Nearby Connections.
    pub fn gms_nearby_add_endpoint(&mut self, id: &str, device_name: &str) {
        if !self.fw.gms.nearby_endpoints.iter().any(|(i, _)| i == id) {
            self.fw
                .gms
                .nearby_endpoints
                .push((id.to_string(), device_name.to_string()));
        }
    }

    /// Define o token de integridade determinístico.
    pub fn gms_set_integrity_token(&mut self, token: &str) {
        self.fw.gms.integrity_token = token.to_string();
    }

    /// Define o JWS de atestação SafetyNet devolvido.
    pub fn gms_set_attestation_jws(&mut self, jws: &str) {
        self.fw.gms.attestation_jws = jws.to_string();
    }

    /// Nonces pedidos (prefixo "integrity:" / "safetynet:" discrimina).
    pub fn gms_integrity_nonces(&self) -> Vec<String> {
        self.fw.gms.integrity_nonces.clone()
    }

    /// Nº de Tasks por concluir — diagnóstico de esperas do app.
    pub fn gms_pending_tasks(&self) -> usize {
        self.fw.gms.tasks.values().filter(|t| !t.complete).count()
    }
}

#[cfg(test)]
mod narrow_bytes_250 {
    use super::*;

    /// issue #250: o `byte[]` criado pelo host guarda valores COM sinal,
    /// a mesma invariante do aput-byte e do fill-array-data.
    #[test]
    fn make_bytes_is_signed() {
        let mut vm = Engine::new(Vec::new(), Default::default());
        let Value::Obj(r) = make_bytes(&mut vm, &[0xFF, 0x7F, 0x80, 0x00]).unwrap() else {
            panic!("make_bytes deve devolver um objeto");
        };
        let got: Vec<i32> = vm
            .heap
            .array_elems_mut(r)
            .unwrap()
            .iter()
            .map(|v| v.as_int().unwrap())
            .collect();
        assert_eq!(got, vec![-1, 127, -128, 0]);
    }
}
