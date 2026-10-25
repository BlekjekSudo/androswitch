//! Modelo tipado completo do AndroidManifest (Apêndice F da spec).
//!
//! Construído a partir da árvore AXML genérica; mantém o raw tree acessível
//! (`Apk::manifest_raw`) porque **o manifest é a verdade** (Lei 2) — nada se perde.

use crate::arsc::Arsc;
use crate::axml::{AttrValue, AxmlDocument, XmlAttribute, XmlElement};
use crate::error::RdResult;
use serde::Serialize;

pub const NS_ANDROID_HINT: &str = "schemas.android.com/apk/res/android";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    #[default]
    Activity,
    ActivityAlias,
    Service,
    Receiver,
    Provider,
}

impl std::fmt::Display for ComponentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ComponentKind::Activity => "activity",
            ComponentKind::ActivityAlias => "activity-alias",
            ComponentKind::Service => "service",
            ComponentKind::Receiver => "receiver",
            ComponentKind::Provider => "provider",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsesPermission {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sdk: Option<u32>,
    /// Declarado via `uses-permission-sdk-23`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sdk23: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeclaredPermission {
    pub name: String,
    /// `android:protectionLevel` DECODIFICADO (ex.: `"signature|privileged"`).
    /// issue #69: antes era a string numérica crua ("18"), ilegível para
    /// humanos e agentes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protection_level: Option<String>,
    /// Valor numérico bruto do `protectionLevel` (ex.: `18`), preservado.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protection_level_raw: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UsesFeature {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// glEsVersion decodificada como (major, minor).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gl_es_version: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct NativeLibrary {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MetaData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IntentData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl IntentData {
    pub fn is_empty(&self) -> bool {
        self.scheme.is_none()
            && self.host.is_none()
            && self.port.is_none()
            && self.path.is_none()
            && self.path_prefix.is_none()
            && self.path_pattern.is_none()
            && self.mime_type.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IntentFilter {
    pub actions: Vec<String>,
    pub categories: Vec<String>,
    pub data: Vec<IntentData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_verify: bool,
    /// Ordem de declaração (estável para o agente).
    pub order: usize,
}

impl IntentFilter {
    pub fn has_action(&self, action: &str) -> bool {
        self.actions.iter().any(|a| a == action)
    }
    pub fn has_category(&self, category: &str) -> bool {
        self.categories.iter().any(|c| c == category)
    }
    pub fn is_launcher(&self) -> bool {
        self.has_action("android.intent.action.MAIN")
            && (self.has_category("android.intent.category.LAUNCHER")
                || self.has_category("android.intent.category.LEANBACK_LAUNCHER"))
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Component {
    pub kind: ComponentKind,
    /// Classe resolvida (pacote prefixado quando relativo `.Foo`).
    pub class_name: String,
    /// Valor crudo de android:name.
    pub name_raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_activity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorities: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_uri_permissions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_permission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_permission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_orientation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_changes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct_boot_aware: Option<bool>,
    pub intent_filters: Vec<IntentFilter>,
    pub meta_data: Vec<MetaData>,
    /// `<property>` (API 31+).
    pub properties: Vec<MetaData>,
    /// Computed: tem intent-filter MAIN+LAUNCHER.
    pub is_launcher: bool,
    /// Computed: exported efetivo (declaração ou implícito por ter intent-filter).
    pub exported_effective: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Application {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Label resolvida (string crua ou via resources.arsc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_icon_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_res: Option<u32>,
    /// Nome qualificado do tema resolvido via arsc (ex.: @style/Theme.Material).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_backup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses_cleartext_traffic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_security_config_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_extraction_rules_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_backup_content_res: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_accelerated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debuggable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_native_libs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_legacy_external_storage: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_code: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_component_factory: Option<String>,
    /// `<meta-data>` declaradas direto no `<application>` (não em componentes).
    /// Lei nº 2 (manifest é a verdade): TWA/PWA declara `asset_statements`
    /// aqui; serviços declaram chaves de configuração global aqui. O motor de
    /// detecção de engines inventaria este conjunto.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub meta_data: Vec<MetaData>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Instrumentation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functional_test: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_profiling: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Queries {
    pub packages: Vec<String>,
    pub intents: Vec<IntentFilter>,
    pub providers: Vec<String>,
    /// `<signature/>` — contagem (elemento vazio).
    pub signature_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Manifest {
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split: Option<String>,
    /// versionCode (32 bits baixos + major<<32 quando presente).
    pub version_code: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_code_major: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_sdk: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_sdk_codename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_build_version_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_build_version_name: Option<String>,
    /// Default da plataforma: 1.
    pub min_sdk: u32,
    /// Default da plataforma: minSdk.
    pub target_sdk: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sdk: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_location: Option<String>,
    pub uses_permissions: Vec<UsesPermission>,
    /// `<permission>` declarados pelo próprio app.
    pub declared_permissions: Vec<DeclaredPermission>,
    pub uses_features: Vec<UsesFeature>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub supports_screens: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub compatible_screens: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub uses_configurations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub supports_gl_textures: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub native_libraries: Vec<NativeLibrary>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub used_libraries: Vec<String>,
    pub application: Application,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub instrumentation: Vec<Instrumentation>,
    #[serde(default, skip_serializing_if = "Queries::is_empty")]
    pub queries: Queries,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_package: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub adopt_permissions: Vec<String>,
}

impl Queries {
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
            && self.intents.is_empty()
            && self.providers.is_empty()
            && self.signature_count == 0
    }
}

// ─── construção a partir da árvore AXML ─────────────────────────────────────

/// Converte `.Foo` em `pkg.Foo`; nomes absolutos passam direto.
pub fn resolve_class(package: &str, raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('.') {
        format!("{package}.{rest}")
    } else if raw.contains('.') {
        raw.to_string()
    } else {
        format!("{package}.{raw}")
    }
}

fn attr_str(el: &XmlElement, name: &str) -> Option<String> {
    el.android_attr(name).map(|a| a.text())
}

fn attr_bool(el: &XmlElement, name: &str) -> Option<bool> {
    el.android_attr(name).and_then(XmlAttribute::as_bool)
}

fn attr_u32(el: &XmlElement, name: &str) -> Option<u32> {
    el.android_attr(name).and_then(XmlAttribute::as_u32)
}

fn attr_i32(el: &XmlElement, name: &str) -> Option<i32> {
    el.android_attr(name).and_then(XmlAttribute::as_i32)
}

/// Constrói o Manifest a partir do AXML + ARSC opcional (para resolver labels).
pub fn build(doc: &AxmlDocument, arsc: Option<&Arsc>) -> RdResult<Manifest> {
    let root = &doc.root;
    if root.name != "manifest" {
        return Err(crate::RdError::invalid_format(format!(
            "manifest: root element is <{}>, expected <manifest>",
            root.name
        )));
    }
    let package = root
        .attr("package")
        .map(|a| a.text())
        .ok_or_else(|| crate::RdError::invalid_format("manifest: missing package attribute"))?;
    if package.is_empty() {
        return Err(crate::RdError::invalid_format(
            "manifest: empty package attribute",
        ));
    }

    let mut m = Manifest {
        package: package.clone(),
        split: attr_str(root, "split"),
        ..Default::default()
    };

    // versionCode pode ser major/minor (API 28): code = minor | major<<32
    let vc = root.android_attr("versionCode").and_then(|a| a.as_u32());
    let vcm = attr_u32(root, "versionCodeMajor");
    if let Some(code) = vc {
        m.version_code = Some(match vcm {
            Some(maj) => ((maj as u64) << 32) | code as u64,
            None => code as u64,
        });
    }
    m.version_code_major = vcm;
    m.version_name = attr_str(root, "versionName");
    m.compile_sdk = attr_u32(root, "compileSdkVersion");
    m.compile_sdk_codename = attr_str(root, "compileSdkVersionCodename");
    m.platform_build_version_code = root
        .attr("platformBuildVersionCode")
        .and_then(|a| a.as_u32());
    m.platform_build_version_name = root.attr("platformBuildVersionName").map(|a| a.text());
    m.min_sdk = attr_u32(root, "minSdkVersion").unwrap_or(1);
    m.target_sdk = attr_u32(root, "targetSdkVersion").unwrap_or(m.min_sdk);
    m.max_sdk = attr_u32(root, "maxSdkVersion");
    // local canônico: elemento filho <uses-sdk> (AGP/aapt) — prevalece sobre attrs do root
    if let Some(uses_sdk) = root.children_named("uses-sdk").next() {
        if let Some(v) = attr_u32(uses_sdk, "minSdkVersion") {
            m.min_sdk = v;
        }
        if let Some(v) = attr_u32(uses_sdk, "targetSdkVersion") {
            m.target_sdk = v;
        }
        if let Some(v) = attr_u32(uses_sdk, "maxSdkVersion") {
            m.max_sdk = Some(v);
        }
    }
    m.shared_user_id = attr_str(root, "sharedUserId");
    m.install_location = attr_str(root, "installLocation");
    m.original_package = root
        .children_named("original-package")
        .next()
        .and_then(|e| attr_str(e, "name"));
    m.adopt_permissions = root
        .children_named("adopt-permission")
        .filter_map(|e| attr_str(e, "name"))
        .collect();

    for uses in root.children_named("uses-permission") {
        if let Some(name) = attr_str(uses, "name") {
            m.uses_permissions.push(UsesPermission {
                name,
                max_sdk: attr_u32(uses, "maxSdkVersion"),
                sdk23: false,
            });
        }
    }
    for uses in root.children_named("uses-permission-sdk-23") {
        if let Some(name) = attr_str(uses, "name") {
            m.uses_permissions.push(UsesPermission {
                name,
                max_sdk: attr_u32(uses, "maxSdkVersion"),
                sdk23: true,
            });
        }
    }

    for perm in root.children_named("permission") {
        if let Some(name) = attr_str(perm, "name") {
            let (pl_raw, pl_decoded) = attr_str(perm, "protectionLevel")
                .map(|raw| decode_protection_level(&raw))
                .unwrap_or((None, None));
            m.declared_permissions.push(DeclaredPermission {
                name,
                protection_level: pl_decoded,
                protection_level_raw: pl_raw,
                group: attr_str(perm, "permissionGroup"),
                label: attr_str(perm, "label"),
                description: attr_str(perm, "description"),
            });
        }
    }

    for feat in root.children_named("uses-feature") {
        let required = attr_bool(feat, "required").unwrap_or(true);
        if let Some(gles) = attr_str(feat, "glEsVersion") {
            m.uses_features.push(UsesFeature {
                name: None,
                gl_es_version: Some(gles),
                required,
            });
        } else if let Some(name) = attr_str(feat, "name") {
            m.uses_features.push(UsesFeature {
                name: Some(name),
                gl_es_version: None,
                required,
            });
        } else {
            // glEsVersion pode vir codificada como int (major<<16|minor)
            if let Some(code) = attr_u32(feat, "glEsVersion") {
                m.uses_features.push(UsesFeature {
                    name: None,
                    gl_es_version: Some(format!("{}.{}", code >> 16, code & 0xFFFF)),
                    required,
                });
            }
        }
    }

    m.supports_screens = root
        .children_named("supports-screens")
        .flat_map(|e| e.attrs.iter().map(|a| format!("{}={}", a.name, a.text())))
        .collect();
    m.compatible_screens = root
        .children_named("compatible-screens")
        .flat_map(|e| {
            e.children_named("screen").map(|s| {
                format!(
                    "{}/{}",
                    attr_str(s, "screenSize").unwrap_or_default(),
                    attr_str(s, "screenDensity").unwrap_or_default()
                )
            })
        })
        .collect();
    m.uses_configurations = root
        .children_named("uses-configuration")
        .map(|e| {
            e.attrs
                .iter()
                .map(|a| format!("{}={}", a.name, a.text()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    m.supports_gl_textures = root
        .children_named("supports-gl-texture")
        .filter_map(|e| attr_str(e, "name"))
        .collect();
    m.native_libraries = root
        .children_named("uses-native-library")
        .filter_map(|e| {
            attr_str(e, "name").map(|name| NativeLibrary {
                name,
                required: attr_bool(e, "required").unwrap_or(true),
            })
        })
        .collect();
    m.used_libraries = root
        .children_named("uses-library")
        .filter_map(|e| attr_str(e, "name"))
        .collect();

    if let Some(app) = root.children_named("application").next() {
        m.application = build_application(app, &package, arsc);
    }

    for instr in root.children_named("instrumentation") {
        m.instrumentation.push(Instrumentation {
            name: attr_str(instr, "name"),
            target_package: attr_str(instr, "targetPackage"),
            functional_test: attr_bool(instr, "functionalTest"),
            handle_profiling: attr_bool(instr, "handleProfiling"),
        });
    }

    if let Some(queries) = root.children_named("queries").next() {
        let mut q = Queries::default();
        for pkg in queries.children_named("package") {
            if let Some(name) = attr_str(pkg, "name") {
                q.packages.push(name);
            }
        }
        for intent in queries.children_named("intent") {
            q.intents.push(build_intent_filter(intent, q.intents.len()));
        }
        for prov in queries.children_named("provider") {
            if let Some(auth) = attr_str(prov, "authorities") {
                q.providers.push(auth);
            }
        }
        q.signature_count = queries.children_named("signature").count();
        m.queries = q;
    }

    Ok(m)
}

/// Converte referência em texto: string resolvida, ou nome qualificado
/// (`@string/app_name`) quando o texto não é resolúvel (config ausente).
fn resolve_label(
    value: &AttrValue,
    raw: Option<&str>,
    arsc: Option<&Arsc>,
) -> (Option<String>, Option<u32>) {
    match value {
        AttrValue::Reference(res) => {
            let resolved =
                arsc.and_then(|a| a.resolve_string(*res).or_else(|| a.resolve_name(*res)));
            (resolved, Some(*res))
        }
        // issue #35: AXML craftado/stripped pode ter TYPE_STRING com raw=None —
        // o valor decodificado (AttrValue::String) não pode ser descartado
        _ => (
            raw.map(str::to_owned)
                .or_else(|| value.as_string().map(str::to_owned)),
            None,
        ),
    }
}

fn build_application(app: &XmlElement, package: &str, arsc: Option<&Arsc>) -> Application {
    let (label, label_res) = app
        .android_attr("label")
        .map(|a| resolve_label(&a.value, a.raw.as_deref(), arsc))
        .unwrap_or((None, None));

    let mut a = Application {
        name: attr_str(app, "name").map(|n| resolve_class(package, &n)),
        label,
        label_res,
        icon_res: attr_u32(app, "icon"),
        round_icon_res: attr_u32(app, "roundIcon"),
        theme_res: attr_u32(app, "theme"),
        theme_name: None,
        allow_backup: attr_bool(app, "allowBackup"),
        uses_cleartext_traffic: attr_bool(app, "usesCleartextTraffic"),
        network_security_config_res: attr_u32(app, "networkSecurityConfig"),
        data_extraction_rules_res: attr_u32(app, "dataExtractionRules"),
        full_backup_content_res: attr_u32(app, "fullBackupContent"),
        hardware_accelerated: attr_bool(app, "hardwareAccelerated"),
        debuggable: attr_bool(app, "debuggable"),
        extract_native_libs: attr_bool(app, "extractNativeLibs"),
        request_legacy_external_storage: attr_bool(app, "requestLegacyExternalStorage"),
        has_code: attr_bool(app, "hasCode"),
        process: attr_str(app, "process"),
        app_component_factory: attr_str(app, "appComponentFactory"),
        meta_data: collect_meta_data(app),
        components: Vec::new(),
    };

    // nome do tema resolvido via arsc quando possível
    if let (Some(tres), Some(arsc)) = (a.theme_res, arsc) {
        a.theme_name = arsc.resolve_name(tres);
    }

    for child in &app.children {
        let kind = match child.name.as_str() {
            "activity" => Some(ComponentKind::Activity),
            "activity-alias" => Some(ComponentKind::ActivityAlias),
            "service" => Some(ComponentKind::Service),
            "receiver" => Some(ComponentKind::Receiver),
            "provider" => Some(ComponentKind::Provider),
            _ => None,
        };
        if let Some(kind) = kind {
            a.components.push(build_component(child, kind, package));
        }
    }

    a
}

fn build_component(el: &XmlElement, kind: ComponentKind, package: &str) -> Component {
    let name_raw = attr_str(el, "name").unwrap_or_default();
    let filters: Vec<IntentFilter> = el
        .children_named("intent-filter")
        .enumerate()
        .map(|(i, f)| build_intent_filter(f, i))
        .collect();
    let is_launcher = filters.iter().any(IntentFilter::is_launcher);
    // exported: declaração explícita vence; sem declaração, a regra da
    // plataforma (comportamento legado, preservado em apps targetSdk < 31) é
    // true se o componente tem QUALQUER <intent-filter> — não só MAIN/LAUNCHER
    // (receiver BOOT_COMPLETED, activity deep-link VIEW e service com filtro
    // custom são exported por default; issue #11)
    let exported_declared = attr_bool(el, "exported");
    let exported_effective = exported_declared.unwrap_or(!filters.is_empty());

    Component {
        kind,
        class_name: if name_raw.is_empty() {
            String::new()
        } else {
            resolve_class(package, &name_raw)
        },
        name_raw,
        exported: exported_declared,
        enabled: attr_bool(el, "enabled"),
        permission: attr_str(el, "permission"),
        process: attr_str(el, "process"),
        target_activity: attr_str(el, "targetActivity").map(|t| resolve_class(package, &t)),
        authorities: attr_str(el, "authorities"),
        grant_uri_permissions: attr_bool(el, "grantUriPermissions"),
        read_permission: attr_str(el, "readPermission"),
        write_permission: attr_str(el, "writePermission"),
        launch_mode: attr_str(el, "launchMode"),
        screen_orientation: attr_str(el, "screenOrientation"),
        config_changes: attr_str(el, "configChanges"),
        theme: attr_str(el, "theme"),
        direct_boot_aware: attr_bool(el, "directBootAware"),
        intent_filters: filters,
        meta_data: collect_meta_data(el),
        properties: el
            .children_named("property")
            .map(|p| MetaData {
                name: attr_str(p, "name"),
                value: attr_str(p, "value"),
                resource: attr_u32(p, "resource"),
            })
            .collect(),
        is_launcher,
        exported_effective,
    }
}

fn collect_meta_data(el: &XmlElement) -> Vec<MetaData> {
    el.children_named("meta-data")
        .map(|md| MetaData {
            name: attr_str(md, "name"),
            value: attr_str(md, "value"),
            resource: attr_u32(md, "resource"),
        })
        .collect()
}

fn build_intent_filter(f: &XmlElement, order: usize) -> IntentFilter {
    let mut filter = IntentFilter {
        priority: attr_i32(f, "priority"),
        auto_verify: attr_bool(f, "autoVerify").unwrap_or(false),
        order,
        ..Default::default()
    };
    for action in f.children_named("action") {
        if let Some(name) = attr_str(action, "name") {
            filter.actions.push(name);
        }
    }
    for cat in f.children_named("category") {
        if let Some(name) = attr_str(cat, "name") {
            filter.categories.push(name);
        }
    }
    for data in f.children_named("data") {
        let d = IntentData {
            scheme: attr_str(data, "scheme"),
            host: attr_str(data, "host"),
            port: attr_str(data, "port"),
            path: attr_str(data, "path"),
            path_prefix: attr_str(data, "pathPrefix"),
            path_pattern: attr_str(data, "pathPattern"),
            mime_type: attr_str(data, "mimeType"),
        };
        if !d.is_empty() {
            filter.data.push(d);
        }
    }
    filter
}

// ─── proteção de permissões declaradas (issue #69) ─────────────────────────

/// Decodifica `android:protectionLevel` do AXML.
///
/// O atributo é um bitfield: base em `0x0–0x3` (`PROTECTION_MASK_BASE`) mais
/// flags a partir de `0x10` — o AOSP faz o mesmo em
/// `PermissionInfo.protectionToString`. Devolve `(bruto, decodificado)`;
/// quando o valor já vem em forma textual (`"signature"`), não há bruto.
pub fn decode_protection_level(raw: &str) -> (Option<u32>, Option<String>) {
    let t = raw.trim();
    let value = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<u32>().ok()
    };
    match value {
        Some(v) => (Some(v), Some(describe_protection_level(v))),
        None if t.is_empty() => (None, None),
        // forma textual no próprio AXML — nada a decodificar
        None => (None, Some(t.to_string())),
    }
}

/// Texto de um `protectionLevel` numérico: base + flags, nas constantes de
/// `android.content.pm.PermissionInfo` / `R.attr.protectionLevel`. Bits não
/// conhecidos são preservados em hexadecimal (nunca perdidos, nunca inventados).
pub fn describe_protection_level(v: u32) -> String {
    let base = match v & 0xF {
        0 => "normal",
        1 => "dangerous",
        2 => "signature",
        3 => "signatureOrSystem",
        4 => "internal",
        other => return format!("0x{other:x}"),
    };
    // 0x10 é `PROTECTION_FLAG_PRIVILEGED` (nome atual; o antigo
    // `PROTECTION_FLAG_SYSTEM` era o mesmo bit e está deprecated desde API 23).
    const FLAGS: &[(u32, &str)] = &[
        (0x10, "privileged"),
        (0x20, "development"),
        (0x40, "appop"),
        (0x80, "pre23"),
        (0x100, "installer"),
        (0x200, "verifier"),
        (0x400, "preinstalled"),
        (0x800, "setup"),
        (0x1000, "instant"),
        (0x2000, "runtimeOnly"),
        (0x4000, "oem"),
        (0x8000, "vendorPrivileged"),
        (0x1_0000, "systemTextClassifier"),
    ];
    let mut out = base.to_string();
    let mut known = 0u32;
    for (bit, name) in FLAGS {
        if v & bit != 0 {
            out.push('|');
            out.push_str(name);
        }
        known |= bit;
    }
    let rest = v & !0xF & !known;
    if rest != 0 {
        out.push_str(&format!("|0x{rest:x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axml::{AttrValue, NS_ANDROID};

    fn android_attr(name: &str, value: AttrValue) -> XmlAttribute {
        XmlAttribute {
            ns: Some(NS_ANDROID.to_string()),
            name: name.to_string(),
            raw: None,
            value,
            res_id: None,
        }
    }

    fn plain_attr(name: &str, value: &str) -> XmlAttribute {
        XmlAttribute {
            ns: None,
            name: name.to_string(),
            raw: None,
            value: AttrValue::String(value.to_string()),
            res_id: None,
        }
    }

    /// issue #69: `protectionLevel` é um bitfield — 18 = 0x12 = signature (0x2)
    /// | privileged (0x10). Antes o valor era exposto cru ("18").
    #[test]
    fn protection_level_is_decoded_and_keeps_the_raw_value() {
        // caso real do F-Droid: org.fdroid.fdroid.permission.UPDATE_REPOS
        assert_eq!(
            decode_protection_level("18"),
            (Some(18), Some("signature|privileged".to_string()))
        );
        // DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION
        assert_eq!(
            decode_protection_level("2"),
            (Some(2), Some("signature".to_string()))
        );
        // bases
        assert_eq!(
            decode_protection_level("0"),
            (Some(0), Some("normal".to_string()))
        );
        assert_eq!(
            decode_protection_level("1"),
            (Some(1), Some("dangerous".to_string()))
        );
        assert_eq!(
            decode_protection_level("3"),
            (Some(3), Some("signatureOrSystem".to_string()))
        );
        // hex + flag de desenvolvimento (50 = 0x32)
        assert_eq!(
            decode_protection_level("0x32"),
            (
                Some(50),
                Some("signature|privileged|development".to_string())
            )
        );
        // forma textual no próprio AXML: sem bruto, texto preservado
        assert_eq!(
            decode_protection_level("signature"),
            (None, Some("signature".to_string()))
        );
        // bits desconhecidos são preservados em hexadecimal (nunca perdidos)
        assert_eq!(
            decode_protection_level("0x100002"),
            (Some(0x100002), Some("signature|0x100000".to_string()))
        );
        // vazio
        assert_eq!(decode_protection_level("  "), (None, None));
    }

    /// manifest mínimo com um componente arbitrário dentro de <application>.
    fn doc_with_component(
        kind: &str,
        attrs: Vec<XmlAttribute>,
        filters: Vec<Vec<&str>>,
    ) -> AxmlDocument {
        let mut comp = XmlElement {
            name: kind.to_string(),
            attrs,
            children: Vec::new(),
            text: None,
        };
        for actions in filters {
            let filter = XmlElement {
                name: "intent-filter".to_string(),
                attrs: Vec::new(),
                children: actions
                    .into_iter()
                    .map(|a| XmlElement {
                        name: "action".to_string(),
                        attrs: vec![android_attr("name", AttrValue::String(a.to_string()))],
                        children: Vec::new(),
                        text: None,
                    })
                    .collect(),
                text: None,
            };
            comp.children.push(filter);
        }
        AxmlDocument {
            root: XmlElement {
                name: "manifest".to_string(),
                attrs: vec![
                    plain_attr("package", "com.t.app"),
                    android_attr("targetSdkVersion", AttrValue::Int(30)),
                ],
                children: vec![XmlElement {
                    name: "application".to_string(),
                    attrs: Vec::new(),
                    children: vec![comp],
                    text: None,
                }],
                text: None,
            },
            ..Default::default()
        }
    }

    /// Issue #11: receiver com filtro BOOT_COMPLETED (sem android:exported) é
    /// exported por default na plataforma — não apenas componentes LAUNCHER.
    #[test]
    fn exported_default_is_any_intent_filter_not_only_launcher() {
        let doc = doc_with_component(
            "receiver",
            vec![android_attr(
                "name",
                AttrValue::String("com.t.app.BootRc".into()),
            )],
            vec![vec!["android.intent.action.BOOT_COMPLETED"]],
        );
        let m = build(&doc, None).expect("manifest");
        let c = &m.application.components[0];
        assert!(c.exported.is_none());
        assert!(
            c.exported_effective,
            "receiver com BOOT_COMPLETED deve ser exported efetivo"
        );

        // activity com deep-link VIEW (não launcher) → mesmo default
        let doc = doc_with_component(
            "activity",
            vec![android_attr(
                "name",
                AttrValue::String("com.t.app.Deep".into()),
            )],
            vec![vec!["android.intent.action.VIEW"]],
        );
        let m = build(&doc, None).expect("manifest");
        assert!(m.application.components[0].exported_effective);

        // sem filtro nenhum → default false
        let doc = doc_with_component(
            "activity",
            vec![android_attr(
                "name",
                AttrValue::String("com.t.app.Plain".into()),
            )],
            vec![],
        );
        let m = build(&doc, None).expect("manifest");
        assert!(!m.application.components[0].exported_effective);
    }

    /// Declaração explícita vence o default (qualquer que seja o valor).
    #[test]
    fn explicit_exported_declaration_wins() {
        let doc = doc_with_component(
            "receiver",
            vec![
                android_attr("name", AttrValue::String("com.t.app.Rc".into())),
                android_attr("exported", AttrValue::Bool(false)),
            ],
            vec![vec!["android.intent.action.BOOT_COMPLETED"]],
        );
        let m = build(&doc, None).expect("manifest");
        let c = &m.application.components[0];
        assert_eq!(c.exported, Some(false));
        assert!(!c.exported_effective, "declaração explícita false vence");

        let doc = doc_with_component(
            "activity",
            vec![
                android_attr("name", AttrValue::String("com.t.app.A".into())),
                android_attr("exported", AttrValue::Bool(true)),
            ],
            vec![],
        );
        let m = build(&doc, None).expect("manifest");
        let c = &m.application.components[0];
        assert_eq!(c.exported, Some(true));
        assert!(c.exported_effective, "declaração explícita true vence");
    }

    /// Gap APK-Tester engine-32 (TWA): `<meta-data>` declarada direto no
    /// `<application>` (não em componente) precisa ser modelada — Lei nº 2
    /// (manifest é a verdade). Um TWA real declara `asset_statements` aqui.
    #[test]
    fn application_level_meta_data_is_collected() {
        let mut doc = doc_with_component(
            "activity",
            vec![android_attr(
                "name",
                AttrValue::String("com.t.app.Main".into()),
            )],
            vec![vec!["android.intent.action.MAIN"]],
        );
        let app = &mut doc.root.children[0];
        app.children.push(XmlElement {
            name: "meta-data".to_string(),
            attrs: vec![
                android_attr("name", AttrValue::String("asset_statements".into())),
                android_attr("resource", AttrValue::Int(0x7f_00_00_01)),
            ],
            children: Vec::new(),
            text: None,
        });
        app.children.push(XmlElement {
            name: "meta-data".to_string(),
            attrs: vec![
                android_attr(
                    "name",
                    AttrValue::String("android.support.customtabs.trusted.DEFAULT_URL".into()),
                ),
                android_attr(
                    "value",
                    AttrValue::String("https://twa.example.invalid/".into()),
                ),
            ],
            children: Vec::new(),
            text: None,
        });
        let m = build(&doc, None).expect("manifest");
        // entries no nível application…
        assert_eq!(m.application.meta_data.len(), 2);
        assert_eq!(
            m.application.meta_data[0].name.as_deref(),
            Some("asset_statements")
        );
        assert_eq!(m.application.meta_data[0].resource, Some(0x7f_00_00_01));
        assert_eq!(
            m.application.meta_data[1].name.as_deref(),
            Some("android.support.customtabs.trusted.DEFAULT_URL")
        );
        assert_eq!(
            m.application.meta_data[1].value.as_deref(),
            Some("https://twa.example.invalid/")
        );
        // …e NÃO vazam para o componente (que tem as suas próprias)
        assert!(m.application.components[0].meta_data.is_empty());
        // serialização: campo omitido quando vazio (golden vectors intactos)
        let empty = build(
            &doc_with_component(
                "activity",
                vec![android_attr(
                    "name",
                    AttrValue::String("com.t.app.B".into()),
                )],
                vec![],
            ),
            None,
        )
        .unwrap();
        let json = serde_json::to_value(&empty.application).unwrap();
        assert!(
            json.get("meta_data").is_none(),
            "meta_data vazio não deve aparecer no JSON"
        );
    }
}
