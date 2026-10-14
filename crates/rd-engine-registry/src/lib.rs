//! Apêndice G: catálogo e detecção, NÃO execução de novos runtimes.
//!
//! DEX/ELF compartilhados produzem candidatos; marcadores de frameworks
//! produzem assinaturas heurísticas. Features selecionam contratos, não
//! implementações. Ver ADR 003 e `docs/engine-registry.md`.

mod contract;
mod detect;
mod report;

pub use contract::{Artifact, EngineAdapter, EngineEvent, EngineSyscall, UnavailableAdapter};
pub use detect::{DetectionReport, Diagnostic, EngineFinding, Evidence, RuleEvidence};

use rd_apk::{RdError, RdResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

pub const MODULE_ID: &str = "rd-engine-registry";
pub const SCOPE: &str = "registry-and-detection-only";
pub const ENGINE_COUNT: usize = 28;
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;
pub const CANONICAL_IDS: [&str; ENGINE_COUNT] = [
    "java",
    "kotlin",
    "kotlin-multiplatform",
    "jvm-dynamic",
    "c-ndk",
    "cpp-ndk",
    "rust-ndk",
    "go-mobile",
    "objective-c-ndk",
    "zig-llvm",
    "flutter",
    "react-native",
    "cordova",
    "twa-pwa",
    "nativescript",
    "tauri",
    "dotnet-maui",
    "unity-mono",
    "unity-il2cpp",
    "unreal",
    "godot",
    "lua-engines",
    "python-engines",
    "haxe",
    "qt-qml",
    "avalonia",
    "generated-java",
    "wasm",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageModule {
    pub id: String,
    pub feature: String,
    pub depends_on: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    /// A VM já existente executa um subconjunto DEX, pela rota rd vm/app.
    PartialDex,
    NotImplemented,
}

impl RuntimeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::PartialDex => "DEX parcial (rd-vm); adaptador do registro ausente",
            Self::NotImplemented => "NOT_IMPLEMENTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionState {
    NotDetected,
    Candidate,
    Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectionRule {
    pub state: DetectionState,
    pub all: Vec<String>,
    pub any: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineDescriptor {
    pub number: usize,
    pub id: String,
    pub name: String,
    pub artifact: String,
    pub modules: Vec<String>,
    pub strategy: String,
    /// Rota existente ou crate FUTURA. Nome no catálogo não significa instalada.
    pub adapter_crate: String,
    pub runtime_status: RuntimeStatus,
    pub notes: String,
    pub rule: Vec<DetectionRule>,
}

// Deserialize only the private document: public Registry values must pass validation.
#[derive(Debug, Clone, Serialize)]
pub struct Registry {
    pub schema_version: u32,
    pub scope: String,
    #[serde(rename = "module")]
    modules: Vec<LanguageModule>,
    #[serde(rename = "engine")]
    engines: Vec<EngineDescriptor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    schema_version: u32,
    scope: String,
    module: Vec<LanguageModule>,
    engine: Vec<EngineDescriptor>,
}

#[derive(Debug, Serialize)]
pub struct ModuleState<'a> {
    #[serde(flatten)]
    pub module: &'a LanguageModule,
    pub feature_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct EngineInfo<'a> {
    #[serde(flatten)]
    pub engine: &'a EngineDescriptor,
    pub required_modules: Vec<String>,
    pub disabled_modules: Vec<String>,
    pub feature_enabled: bool,
    pub registry_adapter_status: &'static str,
}

impl Registry {
    pub fn embedded() -> RdResult<Self> {
        Self::from_toml(include_str!("../../../data/engines.toml"))
    }

    /// Carregamento explícito: erro de arquivo/schema nunca usa fallback.
    pub fn load(path: &Path) -> RdResult<Self> {
        let size = std::fs::metadata(path)
            .map_err(|e| registry_error("IO_ERROR", format!("{}: {e}", path.display())))?
            .len();
        if size > MAX_CATALOG_BYTES as u64 {
            return Err(registry_error(
                "CATALOG_TOO_LARGE",
                "engines.toml exceeds 1 MiB",
            ));
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| registry_error("IO_ERROR", format!("{}: {e}", path.display())))?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> RdResult<Self> {
        if text.len() > MAX_CATALOG_BYTES {
            return Err(registry_error(
                "CATALOG_TOO_LARGE",
                "engines.toml exceeds 1 MiB",
            ));
        }
        let document: RegistryDocument = toml::from_str(text)
            .map_err(|e| registry_error("INVALID_ENGINE_CATALOG", e.to_string()))?;
        let mut r = Self {
            schema_version: document.schema_version,
            scope: document.scope,
            modules: document.module,
            engines: document.engine,
        };
        r.modules.sort_by(|a, b| a.id.cmp(&b.id));
        r.engines.sort_by_key(|e| e.number);
        r.validate()?;
        Ok(r)
    }

    fn validate(&self) -> RdResult<()> {
        let invalid = |s| registry_error("INVALID_ENGINE_CATALOG", s);
        if self.schema_version != 1 || self.scope != SCOPE {
            return Err(invalid("unsupported schema_version/scope"));
        }
        if self.modules.len() != 12 || self.engines.len() != ENGINE_COUNT {
            return Err(invalid(
                "Appendix G requires exactly 12 modules and 28 engines",
            ));
        }
        for (i, m) in self.modules.iter().enumerate() {
            if m.id != format!("LANG-{:02}", i + 1) || m.feature != m.id.to_lowercase() {
                return Err(invalid(
                    "module IDs/features must be LANG-01..12 / lang-01..12",
                ));
            }
            let mut deps = BTreeSet::new();
            for dep in &m.depends_on {
                if dep == &m.id || self.module(dep).is_none() || !deps.insert(dep) {
                    return Err(invalid("unknown, duplicate or self-referencing dependency"));
                }
            }
        }
        for m in &self.modules {
            self.visit_dependencies(&m.id, &mut BTreeSet::new(), &mut BTreeSet::new())?;
        }
        for (i, e) in self.engines.iter().enumerate() {
            if e.number != i + 1 || e.id != CANONICAL_IDS[i] {
                return Err(invalid(
                    "engine numbers/IDs must match the 28 stable Appendix G IDs",
                ));
            }
            if e.modules.is_empty() || e.rule.is_empty() || e.rule.len() > 8 {
                return Err(invalid("engine requires modules and 1..8 detection rules"));
            }
            let mut seen = BTreeSet::new();
            for module in &e.modules {
                if self.module(module).is_none() || !seen.insert(module) {
                    return Err(invalid("unknown or duplicate engine module"));
                }
            }
            for field in [
                &e.name,
                &e.artifact,
                &e.strategy,
                &e.adapter_crate,
                &e.notes,
            ] {
                if field.trim().is_empty() {
                    return Err(invalid("empty engine metadata"));
                }
            }
            // Runtime status describes the existing VM only, never a new engine.
            let partial = matches!(
                e.id.as_str(),
                "java" | "kotlin" | "jvm-dynamic" | "generated-java"
            );
            if (e.runtime_status == RuntimeStatus::PartialDex) != partial {
                return Err(invalid("runtime status may not claim new implementations"));
            }
            for rule in &e.rule {
                if rule.state == DetectionState::NotDetected
                    || rule.all.len() + rule.any.len() == 0
                    || rule.all.len() + rule.any.len() > 16
                    || rule.explanation.trim().is_empty()
                {
                    return Err(invalid("invalid/empty detection rule"));
                }
                for pattern in rule.all.iter().chain(&rule.any) {
                    detect::validate_pattern(pattern)?;
                }
            }
        }
        Ok(())
    }

    fn visit_dependencies(
        &self,
        id: &str,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> RdResult<()> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(registry_error(
                "INVALID_ENGINE_CATALOG",
                "cyclic language dependencies",
            ));
        }
        let module = self.module(id).ok_or_else(|| {
            registry_error("INVALID_ENGINE_CATALOG", format!("unknown dependency {id}"))
        })?;
        for dep in &module.depends_on {
            self.visit_dependencies(dep, visiting, visited)?;
        }
        visiting.remove(id);
        visited.insert(id.to_string());
        Ok(())
    }

    pub fn modules(&self) -> &[LanguageModule] {
        &self.modules
    }
    pub fn engines(&self) -> &[EngineDescriptor] {
        &self.engines
    }
    pub fn module(&self, id: &str) -> Option<&LanguageModule> {
        self.modules.iter().find(|m| m.id == id)
    }
    pub fn get(&self, id: &str) -> RdResult<&EngineDescriptor> {
        self.engines.iter().find(|e| e.id == id).ok_or_else(|| {
            registry_error("UNKNOWN_ENGINE", format!("engine {id:?} is not registered"))
                .with_suggestion("run `rd engine list`")
        })
    }

    pub fn required_modules(&self, e: &EngineDescriptor) -> RdResult<Vec<String>> {
        let mut all = BTreeSet::new();
        for id in &e.modules {
            self.visit_dependencies(id, &mut BTreeSet::new(), &mut all)?;
        }
        Ok(all.into_iter().collect())
    }

    pub fn info<'a>(&'a self, e: &'a EngineDescriptor) -> RdResult<EngineInfo<'a>> {
        let required_modules = self.required_modules(e)?;
        let disabled_modules: Vec<String> = required_modules
            .iter()
            .filter(|id| !feature_enabled(id))
            .cloned()
            .collect();
        Ok(EngineInfo {
            engine: e,
            feature_enabled: disabled_modules.is_empty(),
            required_modules,
            disabled_modules,
            registry_adapter_status: "NOT_IMPLEMENTED",
        })
    }

    pub fn module_states(&self) -> Vec<ModuleState<'_>> {
        self.modules
            .iter()
            .map(|m| ModuleState {
                module: m,
                feature_enabled: feature_enabled(&m.id),
            })
            .collect()
    }

    pub fn engine_infos(&self) -> RdResult<Vec<EngineInfo<'_>>> {
        self.engines.iter().map(|e| self.info(e)).collect()
    }
}

/// Cargo feature selection is intentionally separate from runtime capability.
pub fn feature_enabled(module: &str) -> bool {
    const FEATURES: [(&str, bool); 12] = [
        ("LANG-01", cfg!(feature = "lang-01")),
        ("LANG-02", cfg!(feature = "lang-02")),
        ("LANG-03", cfg!(feature = "lang-03")),
        ("LANG-04", cfg!(feature = "lang-04")),
        ("LANG-05", cfg!(feature = "lang-05")),
        ("LANG-06", cfg!(feature = "lang-06")),
        ("LANG-07", cfg!(feature = "lang-07")),
        ("LANG-08", cfg!(feature = "lang-08")),
        ("LANG-09", cfg!(feature = "lang-09")),
        ("LANG-10", cfg!(feature = "lang-10")),
        ("LANG-11", cfg!(feature = "lang-11")),
        ("LANG-12", cfg!(feature = "lang-12")),
    ];
    FEATURES
        .iter()
        .any(|(id, enabled)| *id == module && *enabled)
}

pub(crate) fn registry_error(code: &str, cause: impl Into<String>) -> RdError {
    RdError::new(code, cause, MODULE_ID)
        .with_suggestion("see docs/engine-registry.md and ADR 003; detection is not execution")
}
