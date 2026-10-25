//! AG-09 (metade estática) — `get_api_usage`: quais APIs `android.*` o app
//! REFERENCIA nos DEX (análise estática).
//!
//! Fontes da análise (tabelas do DEX já parseado pelo rd-dex, Lei 2):
//! - `method_ids` — métodos invocados/resolvidos;
//! - `field_ids` — campos acessados (implicam o tipo, ex.: `Build.VERSION`);
//! - hierarquia dos `class_defs` — superclass + interfaces das classes do APK.
//!
//! Escopo honesto (Lei 1 — sem falha silenciosa): isto é análise ESTÁTICA do
//! DEX ("o que o app referencia"), não coverage de execução — a metade
//! dinâmica do AG-09 vive em `get_coverage` (instrumentação do interpretador).
//! Além das referências brutas, cada tipo é classificado contra o registro
//! executável do Apêndice C (`data/api-coverage.toml`, via
//! `rd-framework::fd_api_coverage`): domínio funcional, status
//! supported/partial/stub/not_implemented/unknown e gap explícito. O resultado
//! registra esse escopo; referência estática nunca é vendida como execução.

use std::collections::{BTreeMap, BTreeSet};

use rd_framework::fd_api_coverage::{ApiCoverageSummary, ApiCoverageTable};
use serde::Serialize;

/// Resultado do scan de uso de APIs android.*.
#[derive(Debug, Clone, Serialize)]
pub struct ApiUsage {
    /// Package do APK analisado.
    pub package: String,
    /// Quantos DEX foram escaneados.
    pub dex_files: usize,
    /// Classes que o próprio APK define (soma dos class_defs).
    pub defined_classes: usize,
    /// Tipos android.* referenciados (FQCN ordenado).
    pub android_types: Vec<String>,
    /// Métodos android.* referenciados: FQCN da classe → nomes de método.
    pub android_methods: BTreeMap<String, Vec<String>>,
    /// Qtde de tipos android.* por prefixo de pacote de topo
    /// (android.app, android.view, …). Campo histórico: não é o domínio do
    /// Apêndice C — para isso veja `api_coverage.by_domain`.
    pub types_by_domain: BTreeMap<String, usize>,
    /// Classificação Apêndice C dos tipos referenciados: contagens por
    /// status/domínio funcional, tipos sem linha no registro e gaps.
    pub api_coverage: ApiCoverageSummary,
    /// Fonte da classificação de cobertura.
    pub coverage_source: &'static str,
    /// Escopo da análise (contrato de honestidade — ver docs do módulo).
    pub analysis: &'static str,
}

/// `Landroid/app/Activity;` → `android.app.Activity`.
fn fqcn(desc: &str) -> String {
    desc.trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".")
}

fn is_android(desc: &str) -> bool {
    desc.starts_with("Landroid/")
}

/// Domínio de topo: `android.app.Activity` → `android.app`.
fn domain(fqcn: &str) -> String {
    let mut parts = fqcn.split('.');
    let a = parts.next().unwrap_or("");
    let b = parts.next().unwrap_or("");
    if b.is_empty() {
        a.to_string()
    } else {
        format!("{a}.{b}")
    }
}

/// Scan estático dos DEX de um APK.
pub fn scan(package: &str, dexes: &[rd_dex::Dex]) -> ApiUsage {
    let mut types: BTreeSet<String> = BTreeSet::new();
    let mut methods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut defined_classes = 0usize;
    for dex in dexes {
        defined_classes += dex.class_defs.len();
        // referências de método: o que o app invoca do android.*
        for m in &dex.method_ids {
            let desc = dex.type_str(m.class_idx as u32);
            if !is_android(desc) {
                continue;
            }
            let cls = fqcn(desc);
            types.insert(cls.clone());
            methods
                .entry(cls)
                .or_default()
                .insert(dex.string(m.name_idx).to_string());
        }
        // referências de campo implicam o tipo (ex.: android.os.Build.VERSION)
        for f in &dex.field_ids {
            let desc = dex.type_str(f.class_idx as u32);
            if is_android(desc) {
                types.insert(fqcn(desc));
            }
        }
        // hierarquia das classes definidas: superclass + interfaces
        for def in &dex.class_defs {
            let sup = dex.type_str(def.superclass_idx);
            if is_android(sup) {
                types.insert(fqcn(sup));
            }
            if let Ok(ifaces) = dex.interfaces(def) {
                for t in ifaces {
                    let desc = dex.type_str(t);
                    if is_android(desc) {
                        types.insert(fqcn(desc));
                    }
                }
            }
        }
    }
    let mut types_by_domain: BTreeMap<String, usize> = BTreeMap::new();
    for t in &types {
        *types_by_domain.entry(domain(t)).or_default() += 1;
    }
    // Apêndice C: classificar é aditivo ao contrato histórico. Se o registro
    // embutido estiver inválido (bug de dados — os testes de rd-framework
    // barram), reportamos registry_error explícito em vez de panicar ou
    // esconder a falha dentro de um scan estático bem-sucedido.
    let api_coverage = match ApiCoverageTable::embedded() {
        Ok(table) => table.summarize_types(types.iter().map(String::as_str)),
        Err(_) => ApiCoverageSummary {
            total_types: types.len(),
            by_status: [(String::from("registry_error"), types.len())]
                .into_iter()
                .collect(),
            by_domain: BTreeMap::new(),
            unknown_types: types.iter().cloned().collect(),
            gap_ids: vec!["gap-api-coverage-registry".to_string()],
        },
    };
    ApiUsage {
        package: package.to_string(),
        dex_files: dexes.len(),
        defined_classes,
        android_types: types.iter().cloned().collect(),
        android_methods: methods
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
        types_by_domain,
        api_coverage,
        coverage_source: "data/api-coverage.toml (Appendix C)",
        analysis: "static-dex-references",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rd_testsupport::vm::{DexBuilder, ACC_PUBLIC};

    /// DexBuilder mínimo: classe do app estendendo android.app.Activity,
    /// invocando setContentView e lendo um campo de android.os.Build$VERSION.
    fn sample_dex() -> rd_dex::Dex {
        let mut b = DexBuilder::new();
        b.class("Landroid/app/Activity;", "Ljava/lang/Object;");
        b.class("Landroid/os/Build$VERSION;", "Ljava/lang/Object;");
        let main = b.class("Lcom/x/Main;", "Landroid/app/Activity;");
        let p_iv = b.proto_idx("V", vec!["I".to_string()]);
        let set_content = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
        b.field_idx("Landroid/os/Build$VERSION;", "I", "SDK_INT");
        b.direct(
            main,
            "onCreate",
            "V",
            vec!["Landroid/os/Bundle;"],
            ACC_PUBLIC,
            Some(b.code(2, 2, 1, {
                let mut u = Vec::new();
                // invoke-direct {v0, v1}, setContentView — só a referência basta
                u.extend(rd_testsupport::vm::op35c(
                    0x70,
                    2,
                    set_content,
                    [1, 0, 0, 0, 0],
                ));
                u.extend(rd_testsupport::vm::op10x(0x0E)); // return-void
                u
            })),
        );
        rd_dex::Dex::parse(b.finish()).expect("dex válido")
    }

    #[test]
    fn scan_reports_android_types_methods_and_domains() {
        let dex = sample_dex();
        let u = scan("com.x", std::slice::from_ref(&dex));

        assert_eq!(u.package, "com.x");
        assert_eq!(u.dex_files, 1);
        assert_eq!(u.analysis, "static-dex-references");
        assert!(
            u.defined_classes >= 3,
            "classes do dex: {}",
            u.defined_classes
        );

        assert!(
            u.android_types
                .contains(&"android.app.Activity".to_string()),
            "superclass + método: {:?}",
            u.android_types
        );
        assert!(
            u.android_types
                .contains(&"android.os.Build$VERSION".to_string()),
            "campo estático implica o tipo: {:?}",
            u.android_types
        );
        assert!(
            !u.android_types.iter().any(|t| t.starts_with("java.")),
            "somente android.*: {:?}",
            u.android_types
        );

        let acts = u.android_methods.get("android.app.Activity").unwrap();
        assert!(acts.contains(&"setContentView".to_string()), "{acts:?}");

        assert_eq!(u.types_by_domain.get("android.app").copied(), Some(1));
        assert_eq!(u.types_by_domain.get("android.os").copied(), Some(1));

        // Apêndice C: Activity é superfície parcial de app-lifecycle;
        // Build$VERSION cai no override estreito supported de android.os.Build.
        assert_eq!(u.coverage_source, "data/api-coverage.toml (Appendix C)");
        assert_eq!(u.api_coverage.total_types, 2);
        assert_eq!(u.api_coverage.by_status.get("partial").copied(), Some(1));
        assert_eq!(u.api_coverage.by_status.get("supported").copied(), Some(1));
        assert_eq!(
            u.api_coverage
                .by_domain
                .get("app-lifecycle")
                .map(|d| d.total),
            Some(1)
        );
        assert_eq!(
            u.api_coverage.by_domain.get("os-ipc").map(|d| d.total),
            Some(1)
        );
        assert!(u.api_coverage.unknown_types.is_empty());
        assert!(u
            .api_coverage
            .gap_ids
            .iter()
            .any(|g| g == "gap-android-app-partial"));
    }

    #[test]
    fn scan_without_android_references_is_vazio_honesto() {
        let mut b = DexBuilder::new();
        b.class("Lcom/x/Solo;", "Ljava/lang/Object;");
        let dex = rd_dex::Dex::parse(b.finish()).expect("dex válido");
        let u = scan("com.x", std::slice::from_ref(&dex));
        assert!(u.android_types.is_empty(), "{:?}", u.android_types);
        assert!(u.android_methods.is_empty());
        assert_eq!(u.api_coverage.total_types, 0);
        assert_eq!(u.defined_classes, 1);
    }
}
