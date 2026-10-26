//! fd-jetpack — Apêndice D (AndroidX/Jetpack, shims por módulo) as data.
//!
//! `data/jetpack.toml` is the source of truth for which module owns an
//! AndroidX/third-party library, which strategy applies (`dex-passthrough`,
//! `host-shim`, `host-native`) and how honest the current status is. It is
//! intentionally **not** an execution coverage report: a library shipping
//! inside an APK's DEX is not coverage. Runtime execution remains the MCP
//! `get_coverage` surface, exactly as in Appendix C.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rd_apk::{RdError, RdResult};
use serde::{Deserialize, Serialize};

use crate::fd_api_coverage::ApiStatus;

pub const MODULE_ID: &str = "fd-jetpack";
pub const EMBEDDED_JETPACK_TOML: &str = include_str!("../../../data/jetpack.toml");

/// Appendix D category IDs, in spec order. [`JetpackTable::validate`]
/// requires exactly this set so a hand-edited table cannot silently drop
/// a whole category of the spec table.
pub const APPENDIX_D_CATEGORIES: [&str; 6] = [
    "foundation",
    "architecture",
    "ui",
    "behavior",
    "di-async",
    "third-party",
];

/// Every library named by the Appendix D spec table, by registry ID.
/// Extra rows beyond the spec table (today: `material-components`,
/// `guava-listenablefuture`) are allowed, but none of these may vanish.
/// Firebase rows of the spec's 3rd-party line live in Appendix E
/// (`data/gms.toml`) — single-owner per prefix, never double-registered.
pub const APPENDIX_D_LIBRARIES: [&str; 44] = [
    // Foundation
    "androidx-appcompat",
    "androidx-core",
    "androidx-activity",
    "androidx-annotation",
    "androidx-multidex",
    "androidx-startup",
    // Architecture (lifecycle split per module, as the spec requires)
    "androidx-lifecycle-runtime",
    "androidx-lifecycle-livedata",
    "androidx-lifecycle-viewmodel",
    "androidx-lifecycle-savedstate",
    "androidx-room",
    "androidx-paging",
    "androidx-work",
    "androidx-navigation",
    "androidx-datastore",
    "androidx-collection",
    "androidx-palette",
    // UI
    "androidx-compose",
    "androidx-recyclerview",
    "androidx-viewpager2",
    "androidx-constraintlayout",
    "androidx-coordinatorlayout",
    "androidx-drawerlayout",
    "androidx-swiperefreshlayout",
    "androidx-cardview",
    "androidx-fragment",
    "androidx-browser",
    "androidx-emoji2",
    // Behavior
    "androidx-media3",
    "androidx-camerax",
    "androidx-biometric",
    "androidx-security-crypto",
    "androidx-credentials",
    "androidx-photopicker",
    // DI/async
    "hilt-dagger",
    "kotlinx-coroutines",
    "rxjava",
    // 3rd-party comuns (Firebase → Appendix E)
    "okhttp",
    "retrofit",
    "moshi",
    "gson",
    "coil",
    "glide",
    "picasso",
];

/// Ecosystem roots: a type under one of these that matches no library is
/// an explicit `unknown` gap, never silently `non_jetpack`.
const JETPACK_ROOTS: [&str; 13] = [
    "androidx.",
    "dagger.",
    "kotlinx.coroutines",
    "okhttp3",
    "retrofit2",
    "coil",
    "io.reactivex",
    "com.squareup.moshi",
    "com.squareup.picasso",
    "com.bumptech.glide",
    "com.google.gson",
    "com.google.common",
    "com.google.android.material",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JetpackStrategy {
    /// The library's DEX ships in the APK and executes on the VM; the
    /// host only provides the `android.*`/`java.*` platform underneath.
    DexPassthrough,
    /// The host intercepts listed methods with a stateful model; every
    /// other call falls through to the library DEX.
    HostShim,
    /// Implemented natively by the host; no library DEX is expected.
    HostNative,
}

impl JetpackStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            JetpackStrategy::DexPassthrough => "dex-passthrough",
            JetpackStrategy::HostShim => "host-shim",
            JetpackStrategy::HostNative => "host-native",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackCategory {
    pub id: String,
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackLibrary {
    pub id: String,
    pub name: String,
    pub category: String,
    /// Maven coordinates of the upstream artifact (`group:artifact`).
    pub coordinates: String,
    /// Package or class prefixes owned by this library. Lookup uses the
    /// longest matching prefix across the whole registry, so class-level
    /// prefixes (e.g. `androidx.lifecycle.LiveData`) can carve modules
    /// out of a shared package (`androidx.lifecycle`).
    pub prefixes: Vec<String>,
    pub strategy: JetpackStrategy,
    pub status: ApiStatus,
    /// Concrete implementing module, or `none` for `not_implemented`.
    pub module: String,
    #[serde(default)]
    pub code: Vec<String>,
    pub milestone: String,
    pub notes: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
    #[serde(default)]
    pub key_apis: Vec<String>,
}

impl JetpackLibrary {
    pub fn is_gap(&self) -> bool {
        self.status.is_gap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackTable {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "api_floor_default")]
    pub api_floor: u32,
    #[serde(default = "api_ceiling_default")]
    pub api_ceiling: u32,
    #[serde(default)]
    pub category: Vec<JetpackCategory>,
    #[serde(default)]
    pub library: Vec<JetpackLibrary>,
}

fn one() -> u32 {
    1
}
fn default_source() -> String {
    "RustyDroid.md Appendix D".to_string()
}
fn api_floor_default() -> u32 {
    26
}
fn api_ceiling_default() -> u32 {
    36
}

fn data_err(cause: impl Into<String>) -> RdError {
    RdError::new("DATA_ERROR", cause.into(), MODULE_ID)
}

fn valid_repo_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && (path.starts_with("crates/")
            || path.starts_with("data/")
            || path.starts_with("docs/")
            || path.starts_with("tests/")
            || path.starts_with("xtask/"))
}

/// A dotted package (`androidx.room`), class (`androidx.lifecycle.LiveData`)
/// or single-segment root (`dagger`, `okhttp3`, `coil`) prefix.
fn valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && !prefix.ends_with('.')
        && !prefix.contains(['/', ';', ' ', '\t', '\n'])
        && prefix.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

fn prefix_matches(prefix: &str, fqcn: &str) -> bool {
    if fqcn == prefix {
        return true;
    }
    fqcn.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('$'))
}

/// Normalize a DEX descriptor (`Landroidx/room/Room;`), slash type
/// (`androidx/room/Room`) or dotted FQCN to dotted form.
pub fn normalize_jetpack_type(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches(';').to_string();
    if let Some(inner) = s.strip_prefix('L') {
        if inner.contains('/') {
            s = inner.to_string();
        }
    }
    s.replace('/', ".")
}

fn is_jetpack_root(fqcn: &str) -> bool {
    JETPACK_ROOTS.iter().any(|r| fqcn.starts_with(r))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackClassification {
    pub fqcn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    /// `supported|partial|stub|not_implemented|unknown|non_jetpack`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackCategoryCoverage {
    pub category: String,
    pub total: usize,
    pub by_status: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackSummary {
    pub total_types: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_category: BTreeMap<String, JetpackCategoryCoverage>,
    pub unknown_types: Vec<String>,
    pub gap_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JetpackCategorySummary {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub libraries: usize,
    pub by_status: BTreeMap<String, usize>,
    pub prefixes: Vec<String>,
    pub modules: Vec<String>,
}

impl JetpackTable {
    pub fn from_toml(contents: &str) -> RdResult<Self> {
        let table: Self = toml::from_str(contents)
            .map_err(|e| RdError::new("DATA_ERROR", format!("jetpack.toml: {e}"), MODULE_ID))?;
        table.validate()?;
        Ok(table)
    }

    pub fn embedded() -> RdResult<Self> {
        Self::from_toml(EMBEDDED_JETPACK_TOML)
    }

    /// Loads `jetpack.toml` from disk when present, else the embedded one.
    pub fn load(dir: Option<&std::path::Path>) -> RdResult<Self> {
        match dir {
            Some(d) => {
                let path = d.join("jetpack.toml");
                if path.exists() {
                    let s = std::fs::read_to_string(&path)
                        .map_err(|e| RdError::io(&e, format!("reading {}", path.display())))?;
                    Self::from_toml(&s)
                } else {
                    Self::embedded()
                }
            }
            None => Self::embedded(),
        }
    }

    pub fn validate(&self) -> RdResult<()> {
        if self.schema_version != 1 {
            return Err(data_err(format!(
                "jetpack schema_version {} unsupported (expected 1)",
                self.schema_version
            )));
        }
        if self.source.trim().is_empty() {
            return Err(data_err("jetpack source is empty"));
        }
        if self.api_floor != 26 || self.api_ceiling != 36 || self.api_floor > self.api_ceiling {
            return Err(data_err(format!(
                "jetpack API window {}..={} violates the Appendix A/D 26..=36 contract",
                self.api_floor, self.api_ceiling
            )));
        }

        let mut categories: HashMap<&str, &JetpackCategory> = HashMap::new();
        for c in &self.category {
            if c.id.trim().is_empty() || c.name.trim().is_empty() || c.summary.trim().is_empty() {
                return Err(data_err(
                    "jetpack category id/name/summary must be non-empty",
                ));
            }
            if categories.insert(c.id.as_str(), c).is_some() {
                return Err(data_err(format!("duplicate jetpack category id {}", c.id)));
            }
        }
        let expected: HashSet<&str> = APPENDIX_D_CATEGORIES.iter().copied().collect();
        let actual: HashSet<&str> = categories.keys().copied().collect();
        if actual != expected {
            let missing: Vec<&str> = APPENDIX_D_CATEGORIES
                .iter()
                .copied()
                .filter(|id| !actual.contains(id))
                .collect();
            let extra: Vec<&str> = actual.difference(&expected).copied().collect();
            return Err(data_err(format!(
                "jetpack Appendix D category set mismatch: missing={missing:?} extra={extra:?}"
            )));
        }
        if self.library.is_empty() {
            return Err(data_err("jetpack registry has no libraries"));
        }

        let mut ids = HashSet::new();
        let mut prefixes = HashSet::new();
        let mut libs_by_category: HashMap<&str, usize> = HashMap::new();
        for lib in &self.library {
            if lib.id.trim().is_empty() || !ids.insert(lib.id.as_str()) {
                return Err(data_err(format!(
                    "duplicate or empty jetpack library id {:?}",
                    lib.id
                )));
            }
            if lib.name.trim().is_empty() || lib.coordinates.trim().is_empty() {
                return Err(data_err(format!("{} has empty name/coordinates", lib.id)));
            }
            if lib.prefixes.is_empty() {
                return Err(data_err(format!("{} has no prefixes", lib.id)));
            }
            for p in &lib.prefixes {
                if !valid_prefix(p) || !prefixes.insert(p.as_str()) {
                    return Err(data_err(format!(
                        "duplicate or invalid jetpack prefix {:?} on {}",
                        p, lib.id
                    )));
                }
            }
            if !categories.contains_key(lib.category.as_str()) {
                return Err(data_err(format!(
                    "{} references unknown category {}",
                    lib.id, lib.category
                )));
            }
            *libs_by_category.entry(lib.category.as_str()).or_default() += 1;
            if lib.milestone.trim().is_empty() || lib.notes.trim().is_empty() {
                return Err(data_err(format!("{} has empty milestone/notes", lib.id)));
            }
            for path in lib.code.iter().chain(lib.evidence.iter()) {
                if !valid_repo_path(path) {
                    return Err(data_err(format!(
                        "{} has invalid repo path {}",
                        lib.id, path
                    )));
                }
            }
            for api in &lib.key_apis {
                let dotted = normalize_jetpack_type(api);
                if dotted != *api || !lib.prefixes.iter().any(|p| prefix_matches(p, api)) {
                    return Err(data_err(format!(
                        "{} key_api {} does not belong to prefixes {:?}",
                        lib.id, api, lib.prefixes
                    )));
                }
            }
            match lib.status {
                ApiStatus::Supported => {
                    if lib.module == "none"
                        || lib.code.is_empty()
                        || lib.evidence.is_empty()
                        || lib.key_apis.is_empty()
                    {
                        return Err(data_err(format!(
                            "{} is supported but lacks module/code/evidence/key_apis",
                            lib.id
                        )));
                    }
                    if lib.gap_id.is_some() {
                        return Err(data_err(format!(
                            "{} is supported but carries a gap_id",
                            lib.id
                        )));
                    }
                }
                ApiStatus::Partial | ApiStatus::Stub => {
                    if lib.module == "none" || lib.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is {:?} but lacks an implementing module/code",
                            lib.id, lib.status
                        )));
                    }
                    if lib.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is {:?} but has no gap_id",
                            lib.id, lib.status
                        )));
                    }
                }
                ApiStatus::NotImplemented => {
                    if lib.module != "none" || !lib.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is not_implemented but claims module/code",
                            lib.id
                        )));
                    }
                    if lib.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is not_implemented but has no gap_id",
                            lib.id
                        )));
                    }
                }
            }
            if lib.module != "none"
                && !(lib.module.starts_with("rd-vm/")
                    || lib.module.starts_with("rd-framework/")
                    || lib.module == "rd-render"
                    || lib.module == "rd-agent"
                    || lib.module == "rd-apk"
                    || lib.module == "rd-cli")
            {
                return Err(data_err(format!(
                    "{} has unknown implementing module {}",
                    lib.id, lib.module
                )));
            }
        }
        for id in APPENDIX_D_CATEGORIES {
            if libs_by_category.get(id).copied().unwrap_or(0) == 0 {
                return Err(data_err(format!(
                    "Appendix D category {id} has no libraries"
                )));
            }
        }
        for id in APPENDIX_D_LIBRARIES {
            if !ids.contains(id) {
                return Err(data_err(format!(
                    "Appendix D spec library {id} is missing from the registry"
                )));
            }
        }
        Ok(())
    }

    pub fn category(&self, id: &str) -> Option<&JetpackCategory> {
        self.category.iter().find(|c| c.id == id)
    }

    pub fn libraries_for_category<'a>(
        &'a self,
        category: &'a str,
    ) -> impl Iterator<Item = &'a JetpackLibrary> + 'a {
        self.library.iter().filter(move |l| l.category == category)
    }

    pub fn library_by_id_or_prefix(&self, query: &str) -> Option<&JetpackLibrary> {
        self.library
            .iter()
            .find(|l| l.id == query || l.prefixes.iter().any(|p| p == query))
    }

    /// Longest-prefix lookup across every library prefix.
    pub fn lookup(&self, raw: &str) -> Option<(&JetpackLibrary, &str)> {
        let fqcn = normalize_jetpack_type(raw);
        self.library
            .iter()
            .flat_map(|l| l.prefixes.iter().map(move |p| (l, p.as_str())))
            .filter(|(_, p)| prefix_matches(p, &fqcn))
            .max_by_key(|(_, p)| p.len())
    }

    pub fn classify(&self, raw: &str) -> JetpackClassification {
        let fqcn = normalize_jetpack_type(raw);
        match self.lookup(raw) {
            Some((lib, prefix)) => JetpackClassification {
                fqcn,
                library_id: Some(lib.id.clone()),
                prefix: Some(prefix.to_string()),
                category: Some(lib.category.clone()),
                category_name: self.category(&lib.category).map(|c| c.name.clone()),
                status: lib.status.as_str().to_string(),
                module: Some(lib.module.clone()),
                strategy: Some(lib.strategy.as_str().to_string()),
                gap_id: lib.gap_id.clone(),
            },
            None => JetpackClassification {
                status: if is_jetpack_root(&fqcn) {
                    "unknown".to_string()
                } else {
                    "non_jetpack".to_string()
                },
                gap_id: is_jetpack_root(&fqcn).then(|| "gap-jetpack-unknown".to_string()),
                fqcn,
                library_id: None,
                prefix: None,
                category: None,
                category_name: None,
                module: None,
                strategy: None,
            },
        }
    }

    pub fn summarize_types<I, S>(&self, types: I) -> JetpackSummary
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_category: BTreeMap<String, JetpackCategoryCoverage> = BTreeMap::new();
        let mut unknown_types = Vec::new();
        let mut gap_ids: BTreeSet<String> = BTreeSet::new();
        let mut total = 0usize;
        for t in types {
            let c = self.classify(t.as_ref());
            total += 1;
            *by_status.entry(c.status.clone()).or_default() += 1;
            if c.status == "unknown" {
                unknown_types.push(c.fqcn.clone());
            }
            if let Some(g) = &c.gap_id {
                gap_ids.insert(g.clone());
            }
            if let Some(cat) = &c.category {
                let e = by_category
                    .entry(cat.clone())
                    .or_insert_with(|| JetpackCategoryCoverage {
                        category: cat.clone(),
                        total: 0,
                        by_status: BTreeMap::new(),
                    });
                e.total += 1;
                *e.by_status.entry(c.status.clone()).or_default() += 1;
            }
        }
        JetpackSummary {
            total_types: total,
            by_status,
            by_category,
            unknown_types,
            gap_ids: gap_ids.into_iter().collect(),
        }
    }

    pub fn status_counts(&self) -> BTreeMap<String, usize> {
        let mut counts: BTreeMap<String, usize> = [
            ("supported".to_string(), 0),
            ("partial".to_string(), 0),
            ("stub".to_string(), 0),
            ("not_implemented".to_string(), 0),
        ]
        .into_iter()
        .collect();
        for l in &self.library {
            *counts.get_mut(l.status.as_str()).expect("known status") += 1;
        }
        counts
    }

    pub fn category_summaries(&self) -> Vec<JetpackCategorySummary> {
        APPENDIX_D_CATEGORIES
            .iter()
            .filter_map(|id| self.category(id))
            .map(|c| {
                let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
                let mut prefixes = Vec::new();
                let mut modules: BTreeSet<String> = BTreeSet::new();
                let mut libraries = 0usize;
                for l in self.libraries_for_category(&c.id) {
                    libraries += 1;
                    *by_status.entry(l.status.as_str().to_string()).or_default() += 1;
                    prefixes.extend(l.prefixes.iter().cloned());
                    modules.insert(l.module.clone());
                }
                prefixes.sort();
                JetpackCategorySummary {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    summary: c.summary.clone(),
                    libraries,
                    by_status,
                    prefixes,
                    modules: modules.into_iter().collect(),
                }
            })
            .collect()
    }

    /// Appendix D table for `RustyDroid.md` (regenerated by xtask).
    pub fn appendix_d_markdown_table(&self) -> String {
        let mut out = String::new();
        out.push_str("| Categoria | Bibliotecas (implementação nativa ou shim) | Status das bibliotecas | Módulos |\n|---|---|---|---|\n");
        for summary in self.category_summaries() {
            let names = self
                .libraries_for_category(&summary.id)
                .map(|l| l.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            let status = ["supported", "partial", "stub", "not_implemented"]
                .iter()
                .filter_map(|k| {
                    summary
                        .by_status
                        .get(*k)
                        .filter(|n| **n > 0)
                        .map(|n| format!("{k} {n}"))
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let modules = summary
                .modules
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                md_cell(&summary.name),
                md_cell(&names),
                md_cell(&status),
                md_cell(&modules)
            ));
        }
        out
    }

    /// Full generated report for `docs/jetpack.md`.
    pub fn markdown_report(&self) -> String {
        let counts = self.status_counts();
        let get = |k: &str| counts.get(k).copied().unwrap_or(0);
        let mut out = String::new();
        out.push_str("# AndroidX/Jetpack — shims por módulo (Apêndice D)\n\n");
        out.push_str("<!-- GERADO por `cargo xtask jetpack-report` a partir de `data/jetpack.toml` — não editar à mão. -->\n\n");
        out.push_str(&format!(
            "Fonte: `{}`. Janela: API {}–{}. Bibliotecas registradas: **{}** em **{}** categorias (supported {}, partial {}, stub {}, not_implemented {}).\n\n",
            self.source,
            self.api_floor,
            self.api_ceiling,
            self.library.len(),
            self.category.len(),
            get("supported"),
            get("partial"),
            get("stub"),
            get("not_implemented")
        ));
        out.push_str("**Semântica honesta:** `supported` é modelo host com estado para o subconjunto documentado em `notes`/`key_apis`, nunca a biblioteca inteira. `partial` executa um subconjunto real (DEX sobre a plataforma host e/ou shim com estado); `stub` é só reconhecimento; `not_implemented` não tem modelo. Estratégias: `dex-passthrough` (o DEX da biblioteca executa na VM), `host-shim` (host intercepta métodos listados; resto cai no DEX), `host-native` (só host). Presença da biblioteca num APK não é cobertura de execução — para execução use MCP `get_coverage`. Firebase fica no Apêndice E ([gms.md](gms.md)).\n\n");
        out.push_str("## Resumo por categoria\n\n");
        out.push_str(&self.appendix_d_markdown_table());
        out.push_str("\n## Bibliotecas\n\n");
        out.push_str("| Biblioteca | Coordenadas | Prefixos | Estratégia | Status | Módulo | Marco | Gap | Notas |\n|---|---|---|---|---|---|---|---|---|\n");
        for cat in APPENDIX_D_CATEGORIES {
            let mut libs: Vec<&JetpackLibrary> = self.libraries_for_category(cat).collect();
            libs.sort_by(|a, b| a.name.cmp(&b.name));
            for l in libs {
                out.push_str(&format!(
                    "| {} | `{}` | {} | {} | {} | `{}` | {} | {} | {} |\n",
                    md_cell(&l.name),
                    l.coordinates,
                    md_cell(
                        &l.prefixes
                            .iter()
                            .map(|p| format!("`{p}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    l.strategy.as_str(),
                    l.status.as_str(),
                    l.module,
                    md_cell(&l.milestone),
                    md_cell(l.gap_id.as_deref().unwrap_or("—")),
                    md_cell(&l.notes)
                ));
            }
        }
        out.push_str("\n## Como atualizar\n\n1. Ao mudar um shim AndroidX, atualize a linha correspondente em `data/jetpack.toml` no mesmo PR.\n2. Rode `rd jetpack check --json` (ou os testes de `rd-framework`).\n3. Regenere este arquivo e a tabela da spec com `cargo xtask jetpack-report` e `cargo xtask jetpack-spec`.\n\nContexto e ledger: [appendix-d-e.md](appendix-d-e.md).\n");
        out
    }
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn embedded_appendix_d_is_complete_and_conservative() {
        let table = JetpackTable::embedded().unwrap();
        assert_eq!(table.category.len(), 6);
        assert!(
            table.library.len() >= APPENDIX_D_LIBRARIES.len(),
            "libraries={}",
            table.library.len()
        );
        for id in APPENDIX_D_LIBRARIES {
            assert!(table.library.iter().any(|l| l.id == id), "missing {id}");
        }
        // Firebase prefixes are Appendix E single-owner; never duplicated here.
        for l in &table.library {
            for p in &l.prefixes {
                assert!(
                    !p.starts_with("com.google.firebase"),
                    "firebase prefix leaked into D: {} {p}",
                    l.id
                );
            }
        }
        let counts = table.status_counts();
        assert!(
            counts["supported"] <= counts["partial"] + counts["not_implemented"],
            "supported must stay a documented-subset minority: {counts:?}"
        );
    }

    #[test]
    fn embedded_code_and_evidence_paths_exist() {
        let table = JetpackTable::embedded().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for l in &table.library {
            for path in l.code.iter().chain(l.evidence.iter()) {
                assert!(
                    root.join(path).exists(),
                    "{} references missing {}",
                    l.id,
                    path
                );
            }
        }
    }

    #[test]
    fn lookup_uses_longest_prefix_and_normalizes_descriptors() {
        let table = JetpackTable::embedded().unwrap();
        // Class-level prefixes carve LiveData/ViewModel out of androidx.lifecycle.
        assert_eq!(
            table
                .lookup("Landroidx/lifecycle/MutableLiveData;")
                .unwrap()
                .0
                .id,
            "androidx-lifecycle-livedata"
        );
        assert_eq!(
            table
                .lookup("androidx.lifecycle.ViewModelStore")
                .unwrap()
                .0
                .id,
            "androidx-lifecycle-viewmodel"
        );
        assert_eq!(
            table
                .lookup("androidx.lifecycle.LifecycleRegistry")
                .unwrap()
                .0
                .id,
            "androidx-lifecycle-runtime"
        );
        assert_eq!(
            table
                .lookup("androidx.lifecycle.SavedStateHandle")
                .unwrap()
                .0
                .id,
            "androidx-lifecycle-savedstate"
        );
        // Gson is third-party JSON, never GMS.
        assert_eq!(table.lookup("com.google.gson.Gson").unwrap().0.id, "gson");
        assert_eq!(
            table.lookup("Lokhttp3/OkHttpClient;").unwrap().0.id,
            "okhttp"
        );
        assert_eq!(
            table
                .lookup("dagger.hilt.android.HiltAndroidApp")
                .unwrap()
                .0
                .id,
            "hilt-dagger"
        );
        // coil vs coil3 are distinct roots of the same library row.
        assert_eq!(table.lookup("coil3.ImageLoader").unwrap().0.id, "coil");
        // Photo-picker contracts are narrower than the whole activity library.
        let c = table
            .classify("androidx.activity.result.contracts.ActivityResultContracts$PickVisualMedia");
        assert_eq!(c.library_id.as_deref(), Some("androidx-photopicker"));
        let c = table.classify("androidx.activity.ComponentActivity");
        assert_eq!(c.library_id.as_deref(), Some("androidx-activity"));
    }

    #[test]
    fn unknown_jetpack_types_are_explicit_gaps() {
        let table = JetpackTable::embedded().unwrap();
        let summary = table.summarize_types([
            "androidx.room.Room",
            "androidx.future.NewLib",
            "java.lang.String",
        ]);
        assert_eq!(summary.total_types, 3);
        assert_eq!(summary.by_status["unknown"], 1);
        assert_eq!(summary.by_status["non_jetpack"], 1);
        assert_eq!(
            summary.unknown_types,
            vec!["androidx.future.NewLib".to_string()]
        );
        assert!(summary.gap_ids.iter().any(|g| g == "gap-jetpack-unknown"));
    }

    #[test]
    fn validation_rejects_overclaiming_and_dropped_spec_libraries() {
        // A not_implemented library flipped to supported lacks everything.
        let mut table = JetpackTable::embedded().unwrap();
        table.library[0].status = ApiStatus::Supported;
        let err = table.validate().unwrap_err();
        assert_eq!(err.code, "DATA_ERROR");

        // A partial library (module/code/evidence present) flipped to
        // supported still carries its gap_id — that alone must reject it.
        let mut table = JetpackTable::embedded().unwrap();
        let room = table
            .library
            .iter_mut()
            .find(|l| l.id == "androidx-room")
            .unwrap();
        room.status = ApiStatus::Supported;
        assert!(room.gap_id.is_some());
        let err = table.validate().unwrap_err();
        assert!(err.cause.contains("gap_id"), "{}", err.cause);

        let mut table = JetpackTable::embedded().unwrap();
        table.library.retain(|l| l.id != "androidx-room");
        let err = table.validate().unwrap_err();
        assert!(err.cause.contains("androidx-room"), "{}", err.cause);
    }
}
