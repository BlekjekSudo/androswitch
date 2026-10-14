//! Conservative, bounded name/manifest analysis. No artifact bytes are read.

use crate::{registry_error, DetectionState, Registry, RuntimeStatus, ENGINE_COUNT};
use rd_apk::{Apk, RdResult};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ZIP_ENTRIES: usize = 100_000;
pub const MAX_SAMPLES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub predicate: String,
    pub total_matches: usize,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleEvidence {
    pub state: DetectionState,
    pub explanation: String,
    pub matches: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineFinding {
    pub number: usize,
    pub id: String,
    pub name: String,
    pub state: DetectionState,
    pub modules: Vec<String>,
    pub required_modules: Vec<String>,
    pub disabled_modules: Vec<String>,
    pub feature_enabled: bool,
    pub runtime_status: RuntimeStatus,
    pub registry_adapter_status: &'static str,
    pub evidence: Vec<RuleEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub cause: String,
    pub total: usize,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectionReport {
    pub schema_version: u32,
    pub analysis: &'static str,
    pub scope: &'static str,
    pub artifact_validation: &'static str,
    pub execution_claimed: bool,
    pub package: String,
    pub registered_engines: usize,
    pub entries_scanned: usize,
    pub max_entries: usize,
    pub max_evidence_samples: usize,
    pub engines: Vec<EngineFinding>,
    pub diagnostics: Vec<Diagnostic>,
    pub limitations: Vec<&'static str>,
}

impl Registry {
    pub fn detect(&self, apk: &Apk) -> RdResult<DetectionReport> {
        if apk.zip.entry_count() > MAX_ZIP_ENTRIES {
            return Err(registry_error(
                "DETECTION_LIMIT",
                format!(
                    "APK has {} ZIP entries; detection limit is {MAX_ZIP_ENTRIES}",
                    apk.zip.entry_count()
                ),
            ));
        }
        let mut paths = BTreeSet::new();
        let mut invalid_count = 0;
        let mut invalid_samples = BTreeSet::new();
        for name in apk.zip.names() {
            if !canonical_path(name) {
                invalid_count += 1;
                // O(1) bounded sample, sorted deterministically even on hostile ZIPs.
                invalid_samples.insert(name);
                if invalid_samples.len() > MAX_SAMPLES {
                    invalid_samples.pop_last();
                }
            } else if !name.ends_with('/') {
                paths.insert(name);
            }
        }
        let classes: BTreeSet<&str> = apk
            .manifest
            .application
            .name
            .as_deref()
            .into_iter()
            .chain(
                apk.manifest
                    .application
                    .components
                    .iter()
                    .map(|c| c.class_name.as_str()),
            )
            .collect();
        let metadata: BTreeSet<&str> = apk
            .manifest
            .application
            .components
            .iter()
            .flat_map(|c| &c.meta_data)
            .filter_map(|m| m.name.as_deref())
            .collect();
        let inventory = Inventory {
            paths: &paths,
            classes,
            metadata,
        };
        let mut cache = BTreeMap::new();
        for e in self.engines() {
            for pattern in e.rule.iter().flat_map(|r| r.all.iter().chain(&r.any)) {
                cache
                    .entry(pattern.as_str())
                    .or_insert_with(|| inventory.evidence(pattern));
            }
        }
        let mut engines = Vec::with_capacity(ENGINE_COUNT);
        for e in self.engines() {
            let info = self.info(e)?;
            let mut state = DetectionState::NotDetected;
            let mut evidence = Vec::new();
            for rule in &e.rule {
                let get = |p: &String| cache.get(p.as_str());
                let matched = |p: &String| get(p).is_some_and(|ev| ev.total_matches > 0);
                if rule.all.iter().all(matched)
                    && (rule.any.is_empty() || rule.any.iter().any(matched))
                {
                    state = state.max(rule.state);
                    let patterns: BTreeSet<&String> = rule.all.iter().chain(&rule.any).collect();
                    let matches = patterns
                        .into_iter()
                        .filter_map(get)
                        .filter(|ev| ev.total_matches > 0)
                        .cloned()
                        .collect();
                    evidence.push(RuleEvidence {
                        state: rule.state,
                        explanation: rule.explanation.clone(),
                        matches,
                    });
                }
            }
            engines.push(EngineFinding {
                number: e.number,
                id: e.id.clone(),
                name: e.name.clone(),
                state,
                modules: e.modules.clone(),
                required_modules: info.required_modules,
                disabled_modules: info.disabled_modules,
                feature_enabled: info.feature_enabled,
                runtime_status: e.runtime_status,
                registry_adapter_status: "NOT_IMPLEMENTED",
                evidence,
            });
        }
        let mut diagnostics = Vec::new();
        if !apk.zip_duplicates.is_empty() {
            let sorted: BTreeSet<&str> = apk.zip_duplicates.iter().map(String::as_str).collect();
            diagnostics.push(Diagnostic {
                code: "DUPLICATE_ZIP_ENTRY",
                cause:
                    "duplicate ZIP names are ambiguous; names counted once, payload not inspected"
                        .into(),
                total: sorted.len(),
                samples: sorted
                    .into_iter()
                    .take(MAX_SAMPLES)
                    .map(String::from)
                    .collect(),
            });
        }
        if invalid_count > 0 {
            diagnostics.push(Diagnostic {
                code: "NON_CANONICAL_ARTIFACT_PATH",
                cause: "absolute/traversal/backslash/control-character paths ignored; nothing is extracted".into(),
                total: invalid_count,
                samples: invalid_samples.into_iter().map(String::from).collect(),
            });
        }
        let mut unknown_abis = BTreeSet::new();
        for name in &paths {
            if let Some((abi, _)) = native_library(name) {
                if !matches!(
                    abi,
                    "x86_64"
                        | "x86"
                        | "arm64-v8a"
                        | "armeabi-v7a"
                        | "armeabi"
                        | "mips"
                        | "mips64"
                        | "riscv64"
                ) {
                    unknown_abis.insert(abi);
                }
            }
        }
        if !unknown_abis.is_empty() {
            diagnostics.push(Diagnostic {
                code: "UNKNOWN_ABI",
                cause: "native path has an unrecognized ABI; no loader/ABI support is claimed"
                    .into(),
                total: unknown_abis.len(),
                samples: unknown_abis
                    .into_iter()
                    .take(MAX_SAMPLES)
                    .map(String::from)
                    .collect(),
            });
        }
        if paths
            .iter()
            .any(|p| root_dex(p) || native_library(p).is_some())
        {
            diagnostics.push(Diagnostic {
                code: "AMBIGUOUS_SOURCE_LANGUAGE",
                cause: "DEX/ELF identify formats, not Java/Kotlin/Scala/C/C++/Rust/Zig source languages".into(),
                total: engines.iter().filter(|e| e.state == DetectionState::Candidate).count(),
                samples: Vec::new(),
            });
        }
        Ok(DetectionReport {
            schema_version: 1,
            analysis: "apk-artifact-signatures",
            scope: crate::SCOPE,
            artifact_validation: "not-performed",
            execution_claimed: false,
            package: apk.manifest.package.clone(),
            registered_engines: ENGINE_COUNT,
            entries_scanned: paths.len(),
            max_entries: MAX_ZIP_ENTRIES,
            max_evidence_samples: MAX_SAMPLES,
            engines,
            diagnostics,
            limitations: vec![
                "Signatures are path/manifest heuristics and can be forged; artifact bytes are not validated.",
                "candidate means compatible format, not identified source language or working runtime.",
                "not_detected is not exclusion: stripped/renamed/encrypted/remote payloads and split APKs may hide markers.",
                "TWA/PWA may contain no local web payload; detection never fetches URLs.",
                "Feature selection is not implementation; all registry adapters return NOT_IMPLEMENTED.",
            ],
        })
    }
}

struct Inventory<'a> {
    paths: &'a BTreeSet<&'a str>,
    classes: BTreeSet<&'a str>,
    metadata: BTreeSet<&'a str>,
}

impl Inventory<'_> {
    fn evidence(&self, pattern: &str) -> Evidence {
        let mut total_matches = 0;
        let mut samples = Vec::new();
        let mut record = |name: &str| {
            total_matches += 1;
            if samples.len() < MAX_SAMPLES {
                samples.push(name.to_string());
            }
        };
        match pattern.split_once(':') {
            Some(("class", prefix)) => {
                for name in &self.classes {
                    if name.starts_with(prefix) {
                        record(name);
                    }
                }
            }
            Some(("metadata", key)) => {
                if self.metadata.contains(key) {
                    record(key);
                }
            }
            _ => {
                for name in self.paths {
                    if path_matches(pattern, name) {
                        record(name);
                    }
                }
            }
        }
        Evidence {
            predicate: pattern.to_string(),
            total_matches,
            samples,
        }
    }
}

pub(crate) fn canonical_path(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('/') && !name.contains('\\')
        && !name.contains(':') && !name.chars().any(char::is_control)
        && name.trim_end_matches('/').split('/').all(|c| !matches!(c, "" | "." | ".."))
        // A single trailing slash is a valid directory; repeated slashes are not.
        && !name.contains("//")
}

pub(crate) fn validate_pattern(pattern: &str) -> RdResult<()> {
    let valid = match pattern {
        "root-dex" | "native-so" => true,
        _ => match pattern.split_once(':') {
            Some((kind, value)) if !value.is_empty() && !value.chars().any(char::is_control) => {
                match kind {
                    "path" => canonical_path(value) && !value.ends_with('/'),
                    "prefix" => canonical_path(value) && value.ends_with('/'),
                    "suffix" => {
                        value.starts_with('.')
                            && value.len() > 1
                            && !value.contains(['/', '\\', ':'])
                    }
                    "native" => {
                        canonical_path(value) && !value.contains('/') && value.ends_with(".so")
                    }
                    "native-prefix" => canonical_path(value) && !value.contains('/'),
                    "class" | "metadata" => value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._$".contains(&b)),
                    _ => false,
                }
            }
            _ => false,
        },
    };
    if valid {
        Ok(())
    } else {
        Err(registry_error(
            "INVALID_ENGINE_CATALOG",
            format!("invalid detector predicate {pattern:?}"),
        ))
    }
}

fn path_matches(pattern: &str, name: &str) -> bool {
    match pattern {
        "root-dex" => root_dex(name),
        "native-so" => native_library(name).is_some(),
        _ => match pattern.split_once(':') {
            Some(("path", value)) => name == value,
            Some(("prefix", value)) => name.starts_with(value),
            Some(("suffix", value)) => name.ends_with(value),
            Some(("native", value)) => native_library(name).is_some_and(|(_, lib)| lib == value),
            Some(("native-prefix", value)) => {
                native_library(name).is_some_and(|(_, lib)| lib.starts_with(value))
            }
            _ => false,
        },
    }
}

fn root_dex(name: &str) -> bool {
    if name == "classes.dex" {
        return true;
    }
    name.strip_prefix("classes")
        .and_then(|s| s.strip_suffix(".dex"))
        .filter(|n| !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<u32>().ok())
        .is_some_and(|n| n >= 2)
}

fn native_library(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.split('/');
    let root = parts.next()?;
    let abi = parts.next()?;
    let lib = parts.next()?;
    (root == "lib"
        && !abi.is_empty()
        && lib.ends_with(".so")
        && lib.len() > 3
        && parts.next().is_none())
    .then_some((abi, lib))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_paths_accept_unicode_but_not_traversal_or_windows_paths() {
        assert!(canonical_path("assets/ação.wasm"));
        assert!(canonical_path("assets/"));
        for p in [
            "", "/a", "a/../b", "a/./b", "a//b", "a//", "C:/a", "a\\b", "a\0b",
        ] {
            assert!(!canonical_path(p), "{p:?}");
        }
    }

    #[test]
    fn multidex_name_grammar_is_exact() {
        for p in ["classes.dex", "classes2.dex", "classes200.dex"] {
            assert!(root_dex(p));
        }
        for p in [
            "classes0.dex",
            "classes1.dex",
            "classes02.dex",
            "classes.dex/",
            "assets/classes.dex",
            "classes9999999999999.dex",
        ] {
            assert!(!root_dex(p), "{p}");
        }
    }

    #[test]
    fn native_marker_cannot_match_an_asset_or_extra_subdirectory() {
        assert!(path_matches(
            "native:libflutter.so",
            "lib/arm64-v8a/libflutter.so"
        ));
        for p in [
            "assets/libflutter.so",
            "lib/arm64-v8a/sub/libflutter.so",
            "lib/arm64-v8a/libflutter.so.bak",
        ] {
            assert!(!path_matches("native:libflutter.so", p));
        }
        assert!(path_matches(
            "native-prefix:libQt6Core",
            "lib/arm64-v8a/libQt6Core_arm64-v8a.so"
        ));
    }
}
