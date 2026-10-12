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
//! `get_coverage` do AG-09 exige instrumentação do interpretador e segue
//! pendente. O resultado registra isso explicitamente.

use std::collections::{BTreeMap, BTreeSet};

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
    /// Qtde de tipos android.* por domínio (android.app, android.view, …).
    pub types_by_domain: BTreeMap<String, usize>,
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
                .insert(dex.strings.get_or_empty(m.name_idx).to_string());
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
    }

    #[test]
    fn scan_without_android_references_is_vazio_honesto() {
        let mut b = DexBuilder::new();
        b.class("Lcom/x/Solo;", "Ljava/lang/Object;");
        let dex = rd_dex::Dex::parse(b.finish()).expect("dex válido");
        let u = scan("com.x", std::slice::from_ref(&dex));
        assert!(u.android_types.is_empty(), "{:?}", u.android_types);
        assert!(u.android_methods.is_empty());
        assert_eq!(u.defined_classes, 1);
    }
}
