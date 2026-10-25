//! fd-gms — Apêndice E (Google Play Services / Firebase, modo "sem GMS"
//! honesto) as versioned data.
//!
//! `data/gms.toml` is the source of truth for which module shims a Play
//! Services / Firebase surface, what the agent can program, and how honest
//! the current status is. The honest mode is **validated**, not just
//! documented: the table mode must be `no-gms-honest` and every service
//! must declare `network = "none"`. There is no GMS process, Google
//! account or real backend; `supported`/`partial` measure the local shim
//! with state, never parity with Google's servers.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rd_apk::{RdError, RdResult};
use serde::{Deserialize, Serialize};

use crate::fd_api_coverage::ApiStatus;

pub const MODULE_ID: &str = "fd-gms";
pub const EMBEDDED_GMS_TOML: &str = include_str!("../../../data/gms.toml");
pub const HONEST_MODE: &str = "no-gms-honest";

/// Appendix E category IDs. [`GmsTable::validate`] requires exactly this
/// set so a hand-edited table cannot silently drop a group of services.
pub const APPENDIX_E_CATEGORIES: [&str; 3] = ["gms-core", "gms-services", "firebase"];

/// Every service named by Appendix E (the Auth bullet covers both Google
/// Sign-In and Firebase Auth providers). Extra rows beyond the spec
/// bullets (today: `firebase-remoteconfig`, `firebase-storage`, `mlkit`)
/// are allowed, but none of these may vanish.
pub const APPENDIX_E_SERVICES: [&str; 18] = [
    "gms-common",
    "gms-tasks",
    "gms-location",
    "gms-maps",
    "gms-billing",
    "gms-auth",
    "gms-ads",
    "gms-fit-health",
    "gms-cast",
    "gms-nearby",
    "gms-integrity",
    "firebase-core",
    "firebase-analytics",
    "firebase-auth",
    "firebase-messaging",
    "firebase-crashlytics",
    "firebase-firestore",
    "firebase-rtdb",
];

/// Ecosystem roots: a type under one of these that matches no service is
/// an explicit `unknown` gap, never silently `non_gms`.
const GMS_ROOTS: [&str; 5] = [
    "com.google.android.gms.",
    "com.google.firebase.",
    "com.android.billingclient",
    "com.google.android.play.",
    "com.google.mlkit",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsCategory {
    pub id: String,
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsService {
    pub id: String,
    pub name: String,
    pub category: String,
    pub coordinates: String,
    /// Package prefixes owned by this service. Lookup uses the longest
    /// matching prefix, so `com.google.firebase.analytics` wins over the
    /// `com.google.firebase` fallback owned by Firebase core.
    pub prefixes: Vec<String>,
    pub status: ApiStatus,
    /// Concrete implementing module, or `none` for `not_implemented`.
    pub module: String,
    #[serde(default)]
    pub code: Vec<String>,
    pub milestone: String,
    /// Honest-mode invariant: must be exactly `"none"`.
    pub network: String,
    /// What the agent can program/capture through the shim. Required —
    /// a GMS surface without a stated programmable contract is not a shim.
    pub programmable: String,
    pub notes: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
    #[serde(default)]
    pub key_apis: Vec<String>,
}

impl GmsService {
    pub fn is_gap(&self) -> bool {
        self.status.is_gap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsTable {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "api_floor_default")]
    pub api_floor: u32,
    #[serde(default = "api_ceiling_default")]
    pub api_ceiling: u32,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub category: Vec<GmsCategory>,
    #[serde(default)]
    pub service: Vec<GmsService>,
}

fn one() -> u32 {
    1
}
fn default_source() -> String {
    "RustyDroid.md Appendix E".to_string()
}
fn default_mode() -> String {
    HONEST_MODE.to_string()
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

/// Normalize a DEX descriptor/slash type/dotted FQCN to dotted form.
pub fn normalize_gms_type(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches(';').to_string();
    if let Some(inner) = s.strip_prefix('L') {
        if inner.contains('/') {
            s = inner.to_string();
        }
    }
    s.replace('/', ".")
}

fn is_gms_root(fqcn: &str) -> bool {
    GMS_ROOTS.iter().any(|r| fqcn.starts_with(r))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsClassification {
    pub fqcn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    /// `supported|partial|stub|not_implemented|unknown|non_gms`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Always `"none"` for matched services (honest no-GMS mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsCategoryCoverage {
    pub category: String,
    pub total: usize,
    pub by_status: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsSummary {
    pub total_types: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_category: BTreeMap<String, GmsCategoryCoverage>,
    pub unknown_types: Vec<String>,
    pub gap_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmsCategorySummary {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub services: usize,
    pub by_status: BTreeMap<String, usize>,
    pub prefixes: Vec<String>,
    pub modules: Vec<String>,
}

impl GmsTable {
    pub fn from_toml(contents: &str) -> RdResult<Self> {
        let table: Self = toml::from_str(contents)
            .map_err(|e| RdError::new("DATA_ERROR", format!("gms.toml: {e}"), MODULE_ID))?;
        table.validate()?;
        Ok(table)
    }

    pub fn embedded() -> RdResult<Self> {
        Self::from_toml(EMBEDDED_GMS_TOML)
    }

    /// Loads `gms.toml` from disk when present, else the embedded one.
    pub fn load(dir: Option<&std::path::Path>) -> RdResult<Self> {
        match dir {
            Some(d) => {
                let path = d.join("gms.toml");
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
                "gms schema_version {} unsupported (expected 1)",
                self.schema_version
            )));
        }
        if self.source.trim().is_empty() {
            return Err(data_err("gms source is empty"));
        }
        if self.mode != HONEST_MODE {
            return Err(data_err(format!(
                "gms mode {:?} violates the honest no-GMS contract (expected {HONEST_MODE:?})",
                self.mode
            )));
        }
        if self.api_floor != 26 || self.api_ceiling != 36 || self.api_floor > self.api_ceiling {
            return Err(data_err(format!(
                "gms API window {}..={} violates the Appendix A/E 26..=36 contract",
                self.api_floor, self.api_ceiling
            )));
        }

        let mut categories: HashMap<&str, &GmsCategory> = HashMap::new();
        for c in &self.category {
            if c.id.trim().is_empty() || c.name.trim().is_empty() || c.summary.trim().is_empty() {
                return Err(data_err("gms category id/name/summary must be non-empty"));
            }
            if categories.insert(c.id.as_str(), c).is_some() {
                return Err(data_err(format!("duplicate gms category id {}", c.id)));
            }
        }
        let expected: HashSet<&str> = APPENDIX_E_CATEGORIES.iter().copied().collect();
        let actual: HashSet<&str> = categories.keys().copied().collect();
        if actual != expected {
            let missing: Vec<&str> = APPENDIX_E_CATEGORIES
                .iter()
                .copied()
                .filter(|id| !actual.contains(id))
                .collect();
            let extra: Vec<&str> = actual.difference(&expected).copied().collect();
            return Err(data_err(format!(
                "gms Appendix E category set mismatch: missing={missing:?} extra={extra:?}"
            )));
        }
        if self.service.is_empty() {
            return Err(data_err("gms registry has no services"));
        }

        let mut ids = HashSet::new();
        let mut prefixes = HashSet::new();
        let mut svcs_by_category: HashMap<&str, usize> = HashMap::new();
        for svc in &self.service {
            if svc.id.trim().is_empty() || !ids.insert(svc.id.as_str()) {
                return Err(data_err(format!(
                    "duplicate or empty gms service id {:?}",
                    svc.id
                )));
            }
            if svc.name.trim().is_empty() || svc.coordinates.trim().is_empty() {
                return Err(data_err(format!("{} has empty name/coordinates", svc.id)));
            }
            if svc.network != "none" {
                return Err(data_err(format!(
                    "{} declares network={:?}; the honest no-GMS mode requires \"none\"",
                    svc.id, svc.network
                )));
            }
            if svc.programmable.trim().is_empty() {
                return Err(data_err(format!(
                    "{} has no programmable contract (what can the agent program/capture?)",
                    svc.id
                )));
            }
            if svc.prefixes.is_empty() {
                return Err(data_err(format!("{} has no prefixes", svc.id)));
            }
            for p in &svc.prefixes {
                if !valid_prefix(p) || !prefixes.insert(p.as_str()) {
                    return Err(data_err(format!(
                        "duplicate or invalid gms prefix {:?} on {}",
                        p, svc.id
                    )));
                }
                // A prefix is in-ecosystem when it equals a root
                // (`com.google.firebase`, `com.android.billingclient`)
                // or sits under one (`com.google.firebase.analytics`).
                let in_ecosystem = GMS_ROOTS
                    .iter()
                    .any(|r| p.as_str() == r.trim_end_matches('.') || p.starts_with(r));
                if !in_ecosystem {
                    return Err(data_err(format!(
                        "{} prefix {:?} is outside the GMS/Firebase ecosystem roots",
                        svc.id, p
                    )));
                }
            }
            if !categories.contains_key(svc.category.as_str()) {
                return Err(data_err(format!(
                    "{} references unknown category {}",
                    svc.id, svc.category
                )));
            }
            *svcs_by_category.entry(svc.category.as_str()).or_default() += 1;
            if svc.milestone.trim().is_empty() || svc.notes.trim().is_empty() {
                return Err(data_err(format!("{} has empty milestone/notes", svc.id)));
            }
            for path in svc.code.iter().chain(svc.evidence.iter()) {
                if !valid_repo_path(path) {
                    return Err(data_err(format!(
                        "{} has invalid repo path {}",
                        svc.id, path
                    )));
                }
            }
            for api in &svc.key_apis {
                let dotted = normalize_gms_type(api);
                if dotted != *api || !svc.prefixes.iter().any(|p| prefix_matches(p, api)) {
                    return Err(data_err(format!(
                        "{} key_api {} does not belong to prefixes {:?}",
                        svc.id, api, svc.prefixes
                    )));
                }
            }
            match svc.status {
                ApiStatus::Supported => {
                    if svc.module == "none"
                        || svc.code.is_empty()
                        || svc.evidence.is_empty()
                        || svc.key_apis.is_empty()
                    {
                        return Err(data_err(format!(
                            "{} is supported but lacks module/code/evidence/key_apis",
                            svc.id
                        )));
                    }
                    if svc.gap_id.is_some() {
                        return Err(data_err(format!(
                            "{} is supported but carries a gap_id",
                            svc.id
                        )));
                    }
                }
                ApiStatus::Partial | ApiStatus::Stub => {
                    if svc.module == "none" || svc.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is {:?} but lacks an implementing module/code",
                            svc.id, svc.status
                        )));
                    }
                    if svc.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is {:?} but has no gap_id",
                            svc.id, svc.status
                        )));
                    }
                }
                ApiStatus::NotImplemented => {
                    if svc.module != "none" || !svc.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is not_implemented but claims module/code",
                            svc.id
                        )));
                    }
                    if svc.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is not_implemented but has no gap_id",
                            svc.id
                        )));
                    }
                }
            }
            if svc.module != "none"
                && !(svc.module.starts_with("rd-vm/")
                    || svc.module.starts_with("rd-framework/")
                    || svc.module == "rd-render"
                    || svc.module == "rd-agent"
                    || svc.module == "rd-apk"
                    || svc.module == "rd-cli")
            {
                return Err(data_err(format!(
                    "{} has unknown implementing module {}",
                    svc.id, svc.module
                )));
            }
        }
        for id in APPENDIX_E_CATEGORIES {
            if svcs_by_category.get(id).copied().unwrap_or(0) == 0 {
                return Err(data_err(format!(
                    "Appendix E category {id} has no services"
                )));
            }
        }
        for id in APPENDIX_E_SERVICES {
            if !ids.contains(id) {
                return Err(data_err(format!(
                    "Appendix E spec service {id} is missing from the registry"
                )));
            }
        }
        Ok(())
    }

    pub fn category(&self, id: &str) -> Option<&GmsCategory> {
        self.category.iter().find(|c| c.id == id)
    }

    pub fn services_for_category<'a>(
        &'a self,
        category: &'a str,
    ) -> impl Iterator<Item = &'a GmsService> + 'a {
        self.service.iter().filter(move |s| s.category == category)
    }

    pub fn service_by_id_or_prefix(&self, query: &str) -> Option<&GmsService> {
        self.service
            .iter()
            .find(|s| s.id == query || s.prefixes.iter().any(|p| p == query))
    }

    /// Longest-prefix lookup across every service prefix.
    pub fn lookup(&self, raw: &str) -> Option<(&GmsService, &str)> {
        let fqcn = normalize_gms_type(raw);
        self.service
            .iter()
            .flat_map(|s| s.prefixes.iter().map(move |p| (s, p.as_str())))
            .filter(|(_, p)| prefix_matches(p, &fqcn))
            .max_by_key(|(_, p)| p.len())
    }

    pub fn classify(&self, raw: &str) -> GmsClassification {
        let fqcn = normalize_gms_type(raw);
        match self.lookup(raw) {
            Some((svc, prefix)) => GmsClassification {
                fqcn,
                service_id: Some(svc.id.clone()),
                prefix: Some(prefix.to_string()),
                category: Some(svc.category.clone()),
                category_name: self.category(&svc.category).map(|c| c.name.clone()),
                status: svc.status.as_str().to_string(),
                module: Some(svc.module.clone()),
                network: Some(svc.network.clone()),
                gap_id: svc.gap_id.clone(),
            },
            None => GmsClassification {
                status: if is_gms_root(&fqcn) {
                    "unknown".to_string()
                } else {
                    "non_gms".to_string()
                },
                gap_id: is_gms_root(&fqcn).then(|| "gap-gms-unknown".to_string()),
                fqcn,
                service_id: None,
                prefix: None,
                category: None,
                category_name: None,
                module: None,
                network: None,
            },
        }
    }

    pub fn summarize_types<I, S>(&self, types: I) -> GmsSummary
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_category: BTreeMap<String, GmsCategoryCoverage> = BTreeMap::new();
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
                    .or_insert_with(|| GmsCategoryCoverage {
                        category: cat.clone(),
                        total: 0,
                        by_status: BTreeMap::new(),
                    });
                e.total += 1;
                *e.by_status.entry(c.status.clone()).or_default() += 1;
            }
        }
        GmsSummary {
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
        for s in &self.service {
            *counts.get_mut(s.status.as_str()).expect("known status") += 1;
        }
        counts
    }

    pub fn category_summaries(&self) -> Vec<GmsCategorySummary> {
        APPENDIX_E_CATEGORIES
            .iter()
            .filter_map(|id| self.category(id))
            .map(|c| {
                let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
                let mut prefixes = Vec::new();
                let mut modules: BTreeSet<String> = BTreeSet::new();
                let mut services = 0usize;
                for s in self.services_for_category(&c.id) {
                    services += 1;
                    *by_status.entry(s.status.as_str().to_string()).or_default() += 1;
                    prefixes.extend(s.prefixes.iter().cloned());
                    modules.insert(s.module.clone());
                }
                prefixes.sort();
                GmsCategorySummary {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    summary: c.summary.clone(),
                    services,
                    by_status,
                    prefixes,
                    modules: modules.into_iter().collect(),
                }
            })
            .collect()
    }

    /// Appendix E table for `RustyDroid.md` (regenerated by xtask).
    pub fn appendix_e_markdown_table(&self) -> String {
        let mut out = String::new();
        out.push_str("| Categoria | Serviços (shim sem GMS) | Status dos serviços | Módulos |\n|---|---|---|---|\n");
        for summary in self.category_summaries() {
            let names = self
                .services_for_category(&summary.id)
                .map(|s| s.name.clone())
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

    /// Full generated report for `docs/gms.md`.
    pub fn markdown_report(&self) -> String {
        let counts = self.status_counts();
        let get = |k: &str| counts.get(k).copied().unwrap_or(0);
        let mut out = String::new();
        out.push_str(
            "# Google Play Services / Firebase — modo \"sem GMS\" honesto (Apêndice E)\n\n",
        );
        out.push_str("<!-- GERADO por `cargo xtask gms-report` a partir de `data/gms.toml` — não editar à mão. -->\n\n");
        out.push_str(&format!(
            "Fonte: `{}`. Modo: `{}`. Janela: API {}–{}. Serviços registrados: **{}** em **{}** categorias (supported {}, partial {}, stub {}, not_implemented {}).\n\n",
            self.source,
            self.mode,
            self.api_floor,
            self.api_ceiling,
            self.service.len(),
            self.category.len(),
            get("supported"),
            get("partial"),
            get("stub"),
            get("not_implemented")
        ));
        out.push_str("**Modo honesto (validado):** não há processo GMS, conta Google nem backend real. Todo serviço declara `network = \"none\"`; as respostas são shims locais estruturados, determinísticos e programáveis pelo agente, e eventos são capturados localmente — nunca enviados. `supported`/`partial` medem o shim para o subconjunto documentado, jamais paridade com os servidores do Google. `GoogleApiAvailability` programável com default `SUCCESS` significa apenas \"o shim responde\". Firestore/RTDB são stores locais (record/replay NET-03), sem sincronização.\n\n");
        out.push_str("## Resumo por categoria\n\n");
        out.push_str(&self.appendix_e_markdown_table());
        out.push_str("\n## Serviços\n\n");
        out.push_str("| Serviço | Coordenadas | Prefixos | Status | Módulo | Marco | Gap | Programável pelo agente | Notas |\n|---|---|---|---|---|---|---|---|---|\n");
        for cat in APPENDIX_E_CATEGORIES {
            let mut svcs: Vec<&GmsService> = self.services_for_category(cat).collect();
            svcs.sort_by(|a, b| a.name.cmp(&b.name));
            for s in svcs {
                out.push_str(&format!(
                    "| {} | `{}` | {} | {} | `{}` | {} | {} | {} | {} |\n",
                    md_cell(&s.name),
                    s.coordinates,
                    md_cell(
                        &s.prefixes
                            .iter()
                            .map(|p| format!("`{p}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    s.status.as_str(),
                    s.module,
                    md_cell(&s.milestone),
                    md_cell(s.gap_id.as_deref().unwrap_or("—")),
                    md_cell(&s.programmable),
                    md_cell(&s.notes)
                ));
            }
        }
        out.push_str("\n## Como atualizar\n\n1. Ao mudar um shim GMS/Firebase, atualize a linha correspondente em `data/gms.toml` no mesmo PR (mantendo `network = \"none\"` e o contrato `programmable`).\n2. Rode `rd gms check --json` (ou os testes de `rd-framework`).\n3. Regenere este arquivo e a tabela da spec com `cargo xtask gms-report` e `cargo xtask gms-spec`.\n\nContexto e ledger: [appendix-d-e.md](appendix-d-e.md).\n");
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
    fn embedded_appendix_e_is_complete_honest_and_conservative() {
        let table = GmsTable::embedded().unwrap();
        assert_eq!(table.mode, HONEST_MODE);
        assert_eq!(table.category.len(), 3);
        assert!(
            table.service.len() >= APPENDIX_E_SERVICES.len(),
            "services={}",
            table.service.len()
        );
        for id in APPENDIX_E_SERVICES {
            assert!(table.service.iter().any(|s| s.id == id), "missing {id}");
        }
        // Honest mode: no service may ever claim a network/backend.
        assert!(table.service.iter().all(|s| s.network == "none"));
        assert!(table
            .service
            .iter()
            .all(|s| !s.programmable.trim().is_empty()));
        let counts = table.status_counts();
        assert!(
            counts["supported"] <= counts["partial"] + counts["not_implemented"],
            "supported must stay a documented-shim minority: {counts:?}"
        );
    }

    #[test]
    fn embedded_code_and_evidence_paths_exist() {
        let table = GmsTable::embedded().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for s in &table.service {
            for path in s.code.iter().chain(s.evidence.iter()) {
                assert!(
                    root.join(path).exists(),
                    "{} references missing {}",
                    s.id,
                    path
                );
            }
        }
    }

    #[test]
    fn lookup_uses_longest_prefix_and_normalizes_descriptors() {
        let table = GmsTable::embedded().unwrap();
        // Analytics is longer than the firebase-core fallback prefix.
        assert_eq!(
            table
                .lookup("Lcom/google/firebase/analytics/FirebaseAnalytics;")
                .unwrap()
                .0
                .id,
            "firebase-analytics"
        );
        assert_eq!(
            table
                .lookup("com.google.firebase.FirebaseApp")
                .unwrap()
                .0
                .id,
            "firebase-core"
        );
        assert_eq!(
            table
                .lookup("com.google.android.gms.tasks.Task")
                .unwrap()
                .0
                .id,
            "gms-tasks"
        );
        // Billing lives outside com.google.android.gms.* entirely.
        assert_eq!(
            table
                .lookup("com.android.billingclient.api.BillingClient")
                .unwrap()
                .0
                .id,
            "gms-billing"
        );
        // Gson must NOT classify as GMS (it is Appendix D third-party).
        let c = table.classify("com.google.gson.Gson");
        assert_eq!(c.status, "non_gms");
        assert!(c.service_id.is_none());
        // Material is neither GMS nor Firebase.
        assert_eq!(
            table
                .classify("com.google.android.material.tabs.TabLayout")
                .status,
            "non_gms"
        );
    }

    #[test]
    fn unknown_gms_types_are_explicit_gaps() {
        let table = GmsTable::embedded().unwrap();
        let summary = table.summarize_types([
            "com.google.firebase.messaging.FirebaseMessaging",
            "com.google.android.gms.future.NewApi",
            "java.lang.String",
        ]);
        assert_eq!(summary.total_types, 3);
        assert_eq!(summary.by_status["unknown"], 1);
        assert_eq!(summary.by_status["non_gms"], 1);
        assert!(summary.gap_ids.iter().any(|g| g == "gap-gms-unknown"));
    }

    #[test]
    fn validation_rejects_dishonest_mode_and_network_claims() {
        let mut table = GmsTable::embedded().unwrap();
        table.mode = "real-gms".to_string();
        let err = table.validate().unwrap_err();
        assert_eq!(err.code, "DATA_ERROR");
        assert!(err.cause.contains("honest"), "{}", err.cause);

        let mut table = GmsTable::embedded().unwrap();
        table.service[0].network = "https://googleapis.com".to_string();
        let err = table.validate().unwrap_err();
        assert!(err.cause.contains("network"), "{}", err.cause);

        let mut table = GmsTable::embedded().unwrap();
        table.service.retain(|s| s.id != "gms-tasks");
        let err = table.validate().unwrap_err();
        assert!(err.cause.contains("gms-tasks"), "{}", err.cause);
    }
}
