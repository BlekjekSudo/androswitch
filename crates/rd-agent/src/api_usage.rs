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
use rd_framework::fd_gms::{GmsSummary, GmsTable};
use rd_framework::fd_jetpack::{JetpackSummary, JetpackTable};
use serde::Serialize;

/// Uma superfície de biblioteca/serviço referenciada pelo APK cujo status
/// honesto é `stub` ou `not_implemented` — o detalhe por superfície que
/// `compat_gaps` expõe em `app_library_gaps` (issue #234, item 2).
#[derive(Debug, Clone, Serialize)]
pub struct LibraryGapSurface {
    /// `library_id` (Apêndice D) ou `service_id` (Apêndice E).
    pub id: String,
    /// `stub | not_implemented`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_id: Option<String>,
    /// Tipos referenciados pelo APK que caem nesta superfície.
    pub types: Vec<String>,
}

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
    /// Apêndice D: tipos `androidx.*` referenciados, classificados contra
    /// `data/jetpack.toml` (status/categoria/gaps). Referência estática.
    pub jetpack: JetpackSummary,
    /// Apêndice E: tipos `com.google.android.gms.*`/`com.google.firebase.*`
    /// referenciados, classificados contra `data/gms.toml` (modo
    /// no-gms-honest: registro ≠ backend; nada de rede).
    pub gms: GmsSummary,
    /// Superfícies D stub/not_implemented referenciadas (com tipos).
    pub jetpack_gap_surfaces: Vec<LibraryGapSurface>,
    /// Superfícies E stub/not_implemented referenciadas (com tipos).
    pub gms_gap_surfaces: Vec<LibraryGapSurface>,
    /// Fontes das classificações D/E.
    pub registry_sources: [&'static str; 2],
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

/// Apêndice D: as raízes vêm do próprio registro (fd_jetpack), nunca de uma
/// cópia local que pode divergir (androidx, Material, OkHttp, coroutines, …).
fn is_jetpack(desc: &str) -> bool {
    rd_framework::fd_jetpack::is_jetpack_ecosystem(desc)
}

/// Apêndice E: idem (fd_gms: gms, Firebase, Play Billing/Core, ML Kit).
fn is_gms(desc: &str) -> bool {
    rd_framework::fd_gms::is_gms_ecosystem(desc)
}

/// Tipos referenciados por família (Apêndices D/E), além do android.* (C).
#[derive(Default)]
struct ExtraRefs {
    jetpack: BTreeSet<String>,
    gms: BTreeSet<String>,
}

impl ExtraRefs {
    fn note(&mut self, desc: &str) {
        if is_jetpack(desc) {
            self.jetpack.insert(fqcn(desc));
        } else if is_gms(desc) {
            self.gms.insert(fqcn(desc));
        }
    }
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
/// Agrupa classificações stub/not_implemented por superfície (ordenado
/// por id; tipos na ordem de entrada — os conjuntos são BTreeSet).
fn gap_surfaces(
    rows: impl Iterator<Item = (String, Option<String>, String, Option<String>)>,
) -> Vec<LibraryGapSurface> {
    let mut by_id: BTreeMap<String, LibraryGapSurface> = BTreeMap::new();
    for (fqcn, id, status, gap_id) in rows {
        if status != "stub" && status != "not_implemented" {
            continue;
        }
        let Some(id) = id else { continue };
        by_id
            .entry(id.clone())
            .or_insert_with(|| LibraryGapSurface {
                id,
                status,
                gap_id,
                types: Vec::new(),
            })
            .types
            .push(fqcn);
    }
    by_id.into_values().collect()
}

pub fn scan(package: &str, dexes: &[rd_dex::Dex]) -> ApiUsage {
    let mut types: BTreeSet<String> = BTreeSet::new();
    let mut methods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut defined_classes = 0usize;
    let mut extra = ExtraRefs::default();
    for dex in dexes {
        defined_classes += dex.class_defs.len();
        // referências de método: o que o app invoca do android.*
        for m in &dex.method_ids {
            let desc = dex.type_str(m.class_idx as u32);
            if !is_android(desc) {
                extra.note(desc);
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
            } else {
                extra.note(desc);
            }
        }
        // hierarquia das classes definidas: superclass + interfaces
        for def in &dex.class_defs {
            let sup = dex.type_str(def.superclass_idx);
            if is_android(sup) {
                types.insert(fqcn(sup));
            } else {
                extra.note(sup);
            }
            if let Ok(ifaces) = dex.interfaces(def) {
                for t in ifaces {
                    let desc = dex.type_str(t);
                    if is_android(desc) {
                        types.insert(fqcn(desc));
                    } else {
                        extra.note(desc);
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
    // Apêndices D/E: mesma regra do C — registro embutido inválido vira
    // registry_error explícito, nunca panic nem classificação silenciosa.
    let mut jetpack_gap_surfaces: Vec<LibraryGapSurface> = Vec::new();
    let jetpack = match JetpackTable::embedded() {
        Ok(table) => {
            jetpack_gap_surfaces = gap_surfaces(extra.jetpack.iter().map(|t| {
                let c = table.classify(t);
                (c.fqcn, c.library_id, c.status, c.gap_id)
            }));
            table.summarize_types(extra.jetpack.iter().map(String::as_str))
        }
        Err(_) => JetpackSummary {
            total_types: extra.jetpack.len(),
            by_status: [(String::from("registry_error"), extra.jetpack.len())]
                .into_iter()
                .collect(),
            by_category: BTreeMap::new(),
            unknown_types: extra.jetpack.iter().cloned().collect(),
            gap_ids: vec!["gap-jetpack-registry".to_string()],
        },
    };
    let mut gms_gap_surfaces: Vec<LibraryGapSurface> = Vec::new();
    let gms = match GmsTable::embedded() {
        Ok(table) => {
            gms_gap_surfaces = gap_surfaces(extra.gms.iter().map(|t| {
                let c = table.classify(t);
                (c.fqcn, c.service_id, c.status, c.gap_id)
            }));
            table.summarize_types(extra.gms.iter().map(String::as_str))
        }
        Err(_) => GmsSummary {
            total_types: extra.gms.len(),
            by_status: [(String::from("registry_error"), extra.gms.len())]
                .into_iter()
                .collect(),
            by_category: BTreeMap::new(),
            unknown_types: extra.gms.iter().cloned().collect(),
            gap_ids: vec!["gap-gms-registry".to_string()],
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
        jetpack,
        gms,
        jetpack_gap_surfaces,
        gms_gap_surfaces,
        registry_sources: [
            "data/jetpack.toml (Appendix D)",
            "data/gms.toml (Appendix E, no-gms-honest)",
        ],
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
        assert_eq!(u.jetpack.total_types, 0);
        assert_eq!(u.gms.total_types, 0);
        assert_eq!(u.defined_classes, 1);
    }

    /// Apêndices D/E: referências androidx.* e GMS/Firebase entram nos
    /// registros próprios (não no android.* do C); desconhecido = gap explícito.
    #[test]
    fn scan_classifies_jetpack_and_gms_references() {
        let mut b = DexBuilder::new();
        b.class("Lcom/x/Main;", "Ljava/lang/Object;");
        let p_v = b.proto_idx("V", vec![]);
        b.method_idx("Landroidx/room/Room;", p_v, "databaseBuilder");
        b.method_idx(
            "Lcom/google/firebase/messaging/FirebaseMessaging;",
            p_v,
            "getInstance",
        );
        b.field_idx("Lcom/google/android/gms/future/NewApi;", "I", "X");
        // raízes além de androidx./gms./firebase. (#234): OkHttp e Material
        // são ecossistema D; Play Billing é ecossistema E
        b.method_idx("Lokhttp3/OkHttpClient;", p_v, "newCall");
        b.field_idx(
            "Lcom/google/android/material/button/MaterialButton;",
            "I",
            "Y",
        );
        b.field_idx("Lcom/android/billingclient/api/BillingClient;", "I", "Z");
        let dex = rd_dex::Dex::parse(b.finish()).expect("dex válido");
        let u = scan("com.x", std::slice::from_ref(&dex));

        assert!(
            u.android_types.is_empty(),
            "D/E não poluem o C: {:?}",
            u.android_types
        );
        assert_eq!(u.jetpack.total_types, 3, "{:?}", u.jetpack);
        assert_eq!(u.gms.total_types, 3, "{:?}", u.gms);
        assert!(
            u.gms
                .unknown_types
                .contains(&"com.google.android.gms.future.NewApi".to_string()),
            "{:?}",
            u.gms
        );
        assert!(u.gms.gap_ids.iter().any(|g| g == "gap-gms-unknown"));
        assert_eq!(u.registry_sources[0], "data/jetpack.toml (Appendix D)");
    }

    /// Nomes de método vêm do acessor sob demanda (`Dex::string`), não do
    /// cache já materializado: com o parse lazy (ADR 008 Stage 1b, usado pelo
    /// MCP via `parse_unverified`) o cache começa vazio.
    #[test]
    fn scan_reads_method_names_on_unverified_parse() {
        let mut b = DexBuilder::new();
        b.class("Lcom/x/Main;", "Ljava/lang/Object;");
        let p_v = b.proto_idx("V", vec![]);
        b.method_idx("Landroid/app/Activity;", p_v, "onCreate");
        let dex = rd_dex::Dex::parse_unverified(b.finish()).expect("dex válido");
        let u = scan("com.x", std::slice::from_ref(&dex));
        assert_eq!(
            u.android_methods.get("android.app.Activity"),
            Some(&vec!["onCreate".to_string()]),
            "{:?}",
            u.android_methods
        );
    }

    /// S8 (#234 item 2): superfícies gap referenciadas trazem id/status/tipos.
    #[test]
    fn scan_exposes_gap_surfaces_with_types() {
        let mut b = DexBuilder::new();
        b.class("Lcom/x/Main;", "Ljava/lang/Object;");
        let p_img = b.proto_idx("Lcom/google/mlkit/vision/common/InputImage;", vec![]);
        b.method_idx(
            "Lcom/google/mlkit/vision/common/InputImage;",
            p_img,
            "fromMediaImage",
        );
        let dex = rd_dex::Dex::parse(b.finish()).expect("dex válido");
        let u = scan("com.x", std::slice::from_ref(&dex));
        let mlkit = u
            .gms_gap_surfaces
            .iter()
            .find(|g| g.id == "mlkit")
            .expect("ML Kit como gap surface");
        assert_eq!(mlkit.status, "not_implemented");
        assert!(mlkit
            .types
            .contains(&"com.google.mlkit.vision.common.InputImage".to_string()));
        assert!(
            u.jetpack_gap_surfaces.is_empty(),
            "{:?}",
            u.jetpack_gap_surfaces
        );
    }
}
