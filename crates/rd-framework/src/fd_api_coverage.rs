//! fd-api-coverage — Apêndice C (`android.*` por domínio) as versioned data.
//!
//! `data/api-coverage.toml` is the source of truth for which framework
//! domain/module owns an Android package/class prefix and how honest the
//! current implementation status is. It is intentionally **not** an execution
//! coverage report: static DEX references are classified here, while runtime
//! method execution remains the MCP `get_coverage` surface.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rd_apk::{RdError, RdResult};
use serde::{Deserialize, Serialize};

pub const MODULE_ID: &str = "fd-api-coverage";
pub const EMBEDDED_API_COVERAGE_TOML: &str = include_str!("../../../data/api-coverage.toml");

/// Appendix C domain IDs, in spec order. `ApiCoverageTable::validate`
/// requires exactly this set so a hand-edited table cannot silently drop a
/// functional domain.
pub const APPENDIX_C_DOMAINS: [&str; 17] = [
    "app-lifecycle",
    "view-ui",
    "graphics",
    "text",
    "os-ipc",
    "database",
    "net",
    "media",
    "camera",
    "hardware",
    "telephony",
    "connectivity",
    "security",
    "speech-ml",
    "services-ext",
    "system-ui",
    "misc",
];

const DOMAIN_MODULES: [&str; 8] = [
    "fd-ui",
    "fd-content",
    "fd-os",
    "fd-net",
    "fd-media",
    "fd-hardware",
    "fd-system-services",
    "fd-permissions",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiStatus {
    Supported,
    Partial,
    Stub,
    NotImplemented,
}

impl ApiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ApiStatus::Supported => "supported",
            ApiStatus::Partial => "partial",
            ApiStatus::Stub => "stub",
            ApiStatus::NotImplemented => "not_implemented",
        }
    }

    /// Anything short of a supported registered subset is an explicit gap for
    /// planning tools. Partial/stub still execute subsets; they are not hidden.
    pub fn is_gap(self) -> bool {
        self != ApiStatus::Supported
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiDomain {
    pub id: String,
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiSurface {
    pub id: String,
    /// Android package or class prefix, e.g. `android.view` or
    /// `android.os.Bundle`. Lookup uses the longest matching prefix, so class
    /// overrides can be narrower than their package.
    pub prefix: String,
    pub domain: String,
    pub status: ApiStatus,
    /// Spec framework domain (`fd-ui`, `fd-content`, ...).
    pub domain_module: String,
    /// Concrete implementing module, or `none` for `not_implemented`.
    pub module: String,
    #[serde(default)]
    pub code: Vec<String>,
    pub milestone: String,
    #[serde(default = "api_floor_default")]
    pub since_api: u32,
    #[serde(default = "api_ceiling_default")]
    pub until_api: u32,
    pub notes: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
    #[serde(default)]
    pub key_apis: Vec<String>,
}

impl ApiSurface {
    pub fn is_gap(&self) -> bool {
        self.status.is_gap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCoverageTable {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "api_floor_default")]
    pub api_floor: u32,
    #[serde(default = "api_ceiling_default")]
    pub api_ceiling: u32,
    #[serde(default)]
    pub domain: Vec<ApiDomain>,
    #[serde(default)]
    pub surface: Vec<ApiSurface>,
}

fn one() -> u32 {
    1
}
fn default_source() -> String {
    "RustyDroid.md Appendix C".to_string()
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
    prefix.starts_with("android.")
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

/// Normalize a DEX descriptor (`Landroid/os/Build$VERSION;`), slash type
/// (`android/os/Build`) or dotted FQCN to dotted form. Returns `None` for
/// non-Android types.
pub fn normalize_android_type(raw: &str) -> Option<String> {
    let mut s = raw.trim().trim_end_matches(';').to_string();
    if let Some(inner) = s.strip_prefix('L') {
        // Only treat a leading L as a descriptor marker when it looks like one;
        // dotted FQCNs never start with a bare `L` package segment here.
        if inner.contains('/') {
            s = inner.to_string();
        }
    }
    let dotted = s.replace('/', ".");
    dotted.starts_with("android.").then_some(dotted)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiClassification {
    pub fqcn: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_name: Option<String>,
    /// `supported|partial|stub|not_implemented|unknown|non_android`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiDomainCoverage {
    pub domain: String,
    pub total: usize,
    pub by_status: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCoverageSummary {
    pub total_types: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_domain: BTreeMap<String, ApiDomainCoverage>,
    pub unknown_types: Vec<String>,
    pub gap_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiDomainSummary {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub surfaces: usize,
    pub by_status: BTreeMap<String, usize>,
    pub prefixes: Vec<String>,
    pub modules: Vec<String>,
}

impl ApiCoverageTable {
    pub fn from_toml(contents: &str) -> RdResult<Self> {
        let table: Self = toml::from_str(contents).map_err(|e| {
            RdError::new("DATA_ERROR", format!("api-coverage.toml: {e}"), MODULE_ID)
        })?;
        table.validate()?;
        Ok(table)
    }

    pub fn embedded() -> RdResult<Self> {
        Self::from_toml(EMBEDDED_API_COVERAGE_TOML)
    }

    /// Carrega `api-coverage.toml` do disco se existir, senão o embutido.
    /// The CLI layers explicit `--data` must-exist semantics on top of this.
    pub fn load(dir: Option<&std::path::Path>) -> RdResult<Self> {
        match dir {
            Some(d) => {
                let path = d.join("api-coverage.toml");
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
                "api-coverage schema_version {} unsupported (expected 1)",
                self.schema_version
            )));
        }
        if self.source.trim().is_empty() {
            return Err(data_err("api-coverage source is empty"));
        }
        if self.api_floor != 26 || self.api_ceiling != 36 || self.api_floor > self.api_ceiling {
            return Err(data_err(format!(
                "api-coverage API window {}..={} violates the Appendix A/C 26..=36 contract",
                self.api_floor, self.api_ceiling
            )));
        }

        let mut domains: HashMap<&str, &ApiDomain> = HashMap::new();
        for d in &self.domain {
            if d.id.trim().is_empty() || d.name.trim().is_empty() || d.summary.trim().is_empty() {
                return Err(data_err(
                    "api-coverage domain id/name/summary must be non-empty",
                ));
            }
            if domains.insert(d.id.as_str(), d).is_some() {
                return Err(data_err(format!(
                    "duplicate api-coverage domain id {}",
                    d.id
                )));
            }
        }
        let expected: HashSet<&str> = APPENDIX_C_DOMAINS.iter().copied().collect();
        let actual: HashSet<&str> = domains.keys().copied().collect();
        if actual != expected {
            let missing: Vec<&str> = APPENDIX_C_DOMAINS
                .iter()
                .copied()
                .filter(|id| !actual.contains(id))
                .collect();
            let extra: Vec<&str> = actual.difference(&expected).copied().collect();
            return Err(data_err(format!(
                "api-coverage Appendix C domain set mismatch: missing={missing:?} extra={extra:?}"
            )));
        }
        if self.surface.is_empty() {
            return Err(data_err("api-coverage has no surfaces"));
        }

        let mut ids = HashSet::new();
        let mut prefixes = HashSet::new();
        let mut surfaces_by_domain: HashMap<&str, usize> = HashMap::new();
        for s in &self.surface {
            if s.id.trim().is_empty() || !ids.insert(s.id.as_str()) {
                return Err(data_err(format!(
                    "duplicate or empty api-coverage surface id {:?}",
                    s.id
                )));
            }
            if !valid_prefix(&s.prefix) || !prefixes.insert(s.prefix.as_str()) {
                return Err(data_err(format!(
                    "duplicate or invalid api-coverage prefix {:?} on {}",
                    s.prefix, s.id
                )));
            }
            if !domains.contains_key(s.domain.as_str()) {
                return Err(data_err(format!(
                    "{} references unknown domain {}",
                    s.id, s.domain
                )));
            }
            *surfaces_by_domain.entry(s.domain.as_str()).or_default() += 1;
            if !DOMAIN_MODULES.contains(&s.domain_module.as_str()) {
                return Err(data_err(format!(
                    "{} has unknown domain_module {}",
                    s.id, s.domain_module
                )));
            }
            if s.milestone.trim().is_empty() || s.notes.trim().is_empty() {
                return Err(data_err(format!("{} has empty milestone/notes", s.id)));
            }
            if s.since_api < self.api_floor
                || s.until_api > self.api_ceiling
                || s.since_api > s.until_api
            {
                return Err(data_err(format!(
                    "{} API window {}..={} is outside {}..={} or inverted",
                    s.id, s.since_api, s.until_api, self.api_floor, self.api_ceiling
                )));
            }
            for path in s.code.iter().chain(s.evidence.iter()) {
                if !valid_repo_path(path) {
                    return Err(data_err(format!("{} has invalid repo path {}", s.id, path)));
                }
            }
            for api in &s.key_apis {
                if normalize_android_type(api).as_deref() != Some(api.as_str())
                    || !prefix_matches(&s.prefix, api)
                {
                    return Err(data_err(format!(
                        "{} key_api {} does not belong to prefix {}",
                        s.id, api, s.prefix
                    )));
                }
            }
            match s.status {
                ApiStatus::Supported => {
                    if s.module == "none" || s.code.is_empty() || s.evidence.is_empty() {
                        return Err(data_err(format!(
                            "{} is supported but lacks module/code/evidence",
                            s.id
                        )));
                    }
                    if s.gap_id.is_some() {
                        return Err(data_err(format!(
                            "{} is supported but carries a gap_id",
                            s.id
                        )));
                    }
                }
                ApiStatus::Partial | ApiStatus::Stub => {
                    if s.module == "none" || s.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is {:?} but lacks an implementing module/code",
                            s.id, s.status
                        )));
                    }
                    if s.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is {:?} but has no gap_id",
                            s.id, s.status
                        )));
                    }
                }
                ApiStatus::NotImplemented => {
                    if s.module != "none" || !s.code.is_empty() {
                        return Err(data_err(format!(
                            "{} is not_implemented but claims module/code",
                            s.id
                        )));
                    }
                    if s.gap_id.as_deref().map_or(true, |g| g.trim().is_empty()) {
                        return Err(data_err(format!(
                            "{} is not_implemented but has no gap_id",
                            s.id
                        )));
                    }
                }
            }
            if s.module != "none"
                && !(s.module.starts_with("rd-vm/")
                    || s.module.starts_with("rd-framework/")
                    || s.module == "rd-render"
                    || s.module == "rd-agent"
                    || s.module == "rd-apk"
                    || s.module == "rd-cli")
            {
                return Err(data_err(format!(
                    "{} has unknown implementing module {}",
                    s.id, s.module
                )));
            }
        }
        for id in APPENDIX_C_DOMAINS {
            if surfaces_by_domain.get(id).copied().unwrap_or(0) == 0 {
                return Err(data_err(format!("Appendix C domain {id} has no surfaces")));
            }
        }
        Ok(())
    }

    pub fn domain(&self, id: &str) -> Option<&ApiDomain> {
        self.domain.iter().find(|d| d.id == id)
    }

    pub fn surfaces_for_domain<'a>(
        &'a self,
        domain: &'a str,
    ) -> impl Iterator<Item = &'a ApiSurface> + 'a {
        self.surface.iter().filter(move |s| s.domain == domain)
    }

    pub fn surface_by_id_or_prefix(&self, query: &str) -> Option<&ApiSurface> {
        self.surface
            .iter()
            .find(|s| s.id == query || s.prefix == query)
    }

    /// Longest-prefix lookup for a type, class or `package.Class.method` ID.
    pub fn lookup(&self, raw: &str) -> Option<&ApiSurface> {
        let fqcn = normalize_android_type(raw)?;
        self.surface
            .iter()
            .filter(|s| prefix_matches(&s.prefix, &fqcn))
            .max_by_key(|s| s.prefix.len())
    }

    pub fn classify(&self, raw: &str) -> ApiClassification {
        let Some(fqcn) = normalize_android_type(raw) else {
            return ApiClassification {
                fqcn: raw.trim().trim_end_matches(';').replace('/', "."),
                surface_id: None,
                prefix: None,
                domain: None,
                domain_name: None,
                status: "non_android".to_string(),
                module: None,
                domain_module: None,
                gap_id: None,
            };
        };
        match self.lookup(&fqcn) {
            Some(s) => ApiClassification {
                fqcn,
                surface_id: Some(s.id.clone()),
                prefix: Some(s.prefix.clone()),
                domain: Some(s.domain.clone()),
                domain_name: self.domain(&s.domain).map(|d| d.name.clone()),
                status: s.status.as_str().to_string(),
                module: Some(s.module.clone()),
                domain_module: Some(s.domain_module.clone()),
                gap_id: s.gap_id.clone(),
            },
            None => ApiClassification {
                fqcn,
                surface_id: None,
                prefix: None,
                domain: None,
                domain_name: None,
                status: "unknown".to_string(),
                module: None,
                domain_module: None,
                gap_id: Some("gap-android-unknown".to_string()),
            },
        }
    }

    pub fn summarize_types<'a, I>(&self, types: I) -> ApiCoverageSummary
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut total_types = 0usize;
        let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_domain: BTreeMap<String, ApiDomainCoverage> = BTreeMap::new();
        let mut unknown_types = Vec::new();
        let mut gap_ids = BTreeSet::new();
        for raw in types {
            let c = self.classify(raw);
            total_types += 1;
            *by_status.entry(c.status.clone()).or_default() += 1;
            if c.status == "unknown" {
                unknown_types.push(c.fqcn.clone());
            }
            if let Some(gap) = c.gap_id.clone() {
                gap_ids.insert(gap);
            }
            if let Some(domain) = c.domain.clone() {
                let entry = by_domain
                    .entry(domain.clone())
                    .or_insert(ApiDomainCoverage {
                        domain,
                        total: 0,
                        by_status: BTreeMap::new(),
                    });
                entry.total += 1;
                *entry.by_status.entry(c.status.clone()).or_default() += 1;
            }
        }
        ApiCoverageSummary {
            total_types,
            by_status,
            by_domain,
            unknown_types,
            gap_ids: gap_ids.into_iter().collect(),
        }
    }

    pub fn status_counts(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for s in &self.surface {
            *out.entry(s.status.as_str().to_string()).or_default() += 1;
        }
        out
    }

    pub fn domain_summaries(&self) -> Vec<ApiDomainSummary> {
        APPENDIX_C_DOMAINS
            .iter()
            .filter_map(|id| self.domain(id))
            .map(|d| {
                let mut by_status = BTreeMap::new();
                let mut prefixes = Vec::new();
                let mut modules = BTreeSet::new();
                let mut surfaces = 0usize;
                for s in self.surfaces_for_domain(&d.id) {
                    surfaces += 1;
                    *by_status.entry(s.status.as_str().to_string()).or_default() += 1;
                    prefixes.push(s.prefix.clone());
                    modules.insert(s.module.clone());
                }
                prefixes.sort();
                ApiDomainSummary {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    summary: d.summary.clone(),
                    surfaces,
                    by_status,
                    prefixes,
                    modules: modules.into_iter().collect(),
                }
            })
            .collect()
    }

    /// Appendix C table for `RustyDroid.md` (regenerated by xtask).
    pub fn appendix_c_markdown_table(&self) -> String {
        let mut out = String::new();
        out.push_str("| Domínio | Superfícies/pacotes principais | Status das superfícies | Módulos |\n|---|---|---|---|\n");
        for summary in self.domain_summaries() {
            let prefixes = summary
                .prefixes
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let status = ["supported", "partial", "stub", "not_implemented"]
                .iter()
                .filter_map(|k| summary.by_status.get(*k).map(|n| format!("{k} {n}")))
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
                md_cell(&prefixes),
                md_cell(&status),
                md_cell(&modules)
            ));
        }
        out
    }

    /// Full generated report for `docs/api-coverage.md`.
    pub fn markdown_report(&self) -> String {
        let counts = self.status_counts();
        let get = |k: &str| counts.get(k).copied().unwrap_or(0);
        let mut out = String::new();
        out.push_str("# Android API coverage (Apêndice C)\n\n");
        out.push_str("<!-- GERADO por `cargo xtask api-coverage-report` a partir de `data/api-coverage.toml` — não editar à mão. -->\n\n");
        out.push_str(&format!(
            "Fonte: `{}`. Janela: API {}–{}. Superfícies registradas: **{}** em **{}** domínios (supported {}, partial {}, stub {}, not_implemented {}).\n\n",
            self.source,
            self.api_floor,
            self.api_ceiling,
            self.surface.len(),
            self.domain.len(),
            get("supported"),
            get("partial"),
            get("stub"),
            get("not_implemented")
        ));
        out.push_str("**Semântica honesta:** `supported` significa modelo host com estado para o subconjunto documentado em `notes`/`key_apis`, não o pacote AOSP inteiro. `partial` executa um subconjunto real; `stub` é hierarquia/fallback; `not_implemented` não tem modelo host. Presença em DEX ou numa hierarquia stub não é cobertura de execução — para execução use MCP `get_coverage`.\n\n");
        out.push_str("## Resumo por domínio\n\n");
        out.push_str(&self.appendix_c_markdown_table());
        out.push_str("\n## Superfícies\n\n");
        out.push_str("| Prefixo | Domínio | Status | Módulo | Marco | Gap | Notas |\n|---|---|---|---|---|---|---|\n");
        for domain in APPENDIX_C_DOMAINS {
            let mut surfaces: Vec<&ApiSurface> = self.surfaces_for_domain(domain).collect();
            surfaces.sort_by(|a, b| a.prefix.cmp(&b.prefix));
            for s in surfaces {
                out.push_str(&format!(
                    "| `{}` | {} | {} | `{}` (`{}`) | {} | {} | {} |\n",
                    s.prefix,
                    md_cell(domain),
                    s.status.as_str(),
                    s.module,
                    s.domain_module,
                    md_cell(&s.milestone),
                    md_cell(s.gap_id.as_deref().unwrap_or("—")),
                    md_cell(&s.notes)
                ));
            }
        }
        out.push_str("\n## Como atualizar\n\n1. Ao mudar uma API Android, atualize a linha correspondente em `data/api-coverage.toml` no mesmo PR.\n2. Rode `rd api-coverage check --json` (ou os testes de `rd-framework`).\n3. Regenere este arquivo e a tabela da spec com `cargo xtask api-coverage-report` e `cargo xtask api-coverage-spec`.\n\nPara o critério literal de conclusão e os bloqueadores por superfície, veja [appendix-c-literal-completion.md](appendix-c-literal-completion.md).\n");
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
    fn embedded_appendix_c_is_complete_and_conservative() {
        let table = ApiCoverageTable::embedded().unwrap();
        assert_eq!(table.domain.len(), 17);
        assert!(
            table.surface.len() >= 100,
            "surfaces={}",
            table.surface.len()
        );
        // Whole packages are never claimed as fully supported; supported is
        // reserved for narrow documented subsets.
        for s in table
            .surface
            .iter()
            .filter(|s| s.status == ApiStatus::Supported)
        {
            assert!(
                s.prefix.rsplit('.').next().is_some_and(|last| {
                    last.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                }),
                "supported surface must be class-narrow: {}",
                s.prefix
            );
        }
        let counts = table.status_counts();
        assert!(counts["not_implemented"] > counts["supported"]);
    }

    #[test]
    fn embedded_code_and_evidence_paths_exist() {
        let table = ApiCoverageTable::embedded().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        for s in &table.surface {
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
        let table = ApiCoverageTable::embedded().unwrap();
        assert_eq!(
            table.lookup("Landroid/os/Build$VERSION;").unwrap().id,
            "android-os-build"
        );
        assert_eq!(
            table.lookup("android.provider.Settings$Global").unwrap().id,
            "android-provider-settings"
        );
        // textclassifier belongs to Speech/ML even though it is nested under android.view.
        let c = table.classify("android.view.textclassifier.TextClassifier");
        assert_eq!(c.domain.as_deref(), Some("speech-ml"));
        assert_eq!(c.status, "partial");
        let c = table.classify("android.database.sqlite.SQLiteDatabase");
        assert_eq!(c.status, "partial");
        assert_eq!(
            c.gap_id.as_deref(),
            Some("gap-android-database-sqlite-literal")
        );
    }

    #[test]
    fn unknown_android_types_are_explicit_gaps() {
        let table = ApiCoverageTable::embedded().unwrap();
        let summary = table.summarize_types(["android.app.Activity", "android.future.NewApi"]);
        assert_eq!(summary.total_types, 2);
        assert_eq!(summary.by_status["partial"], 1);
        assert_eq!(summary.by_status["unknown"], 1);
        assert_eq!(
            summary.unknown_types,
            vec!["android.future.NewApi".to_string()]
        );
        assert!(summary.gap_ids.iter().any(|g| g == "gap-android-unknown"));
    }

    #[test]
    fn validation_rejects_overclaiming_supported() {
        let mut table = ApiCoverageTable::embedded().unwrap();
        table.surface[0].status = ApiStatus::Supported;
        let err = table.validate().unwrap_err();
        assert_eq!(err.code, "DATA_ERROR");
        assert!(err.cause.contains("gap_id"), "{}", err.cause);
    }
}
