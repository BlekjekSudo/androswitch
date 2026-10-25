//! Apêndice D CLI: the AndroidX/Jetpack per-module registry is data, and
//! this is the operator/agent door to it (`data/jetpack.toml`).

use crate::{DataDir, JetpackSub};
use rd_apk::{RdError, RdResult};
use rd_framework::fd_api_coverage::ApiStatus;
use rd_framework::fd_jetpack::JetpackTable;

pub(super) fn load_table(dd: &DataDir) -> RdResult<JetpackTable> {
    dd.require("jetpack.toml")?;
    JetpackTable::load(dd.path.as_deref())
}

fn parse_status(raw: &str) -> RdResult<ApiStatus> {
    match raw {
        "supported" => Ok(ApiStatus::Supported),
        "partial" => Ok(ApiStatus::Partial),
        "stub" => Ok(ApiStatus::Stub),
        "not_implemented" => Ok(ApiStatus::NotImplemented),
        other => Err(RdError::new(
            "INVALID_FORMAT",
            format!("status de jetpack inválido: {other}"),
            "rd-cli",
        )
        .with_suggestion("use supported|partial|stub|not_implemented")),
    }
}

pub(super) fn run(sub: &JetpackSub, dd: &DataDir) -> RdResult<()> {
    let table = load_table(dd)?;
    match sub {
        JetpackSub::List {
            category,
            status,
            json,
        } => {
            if let Some(c) = category {
                if table.category(c).is_none() {
                    return Err(RdError::missing_entry(format!("categoria Apêndice D {c}"))
                        .with_suggestion("veja os ids em `rd jetpack check --json`"));
                }
            }
            let wanted_status = status.as_deref().map(parse_status).transpose()?;
            let libraries: Vec<_> = table
                .library
                .iter()
                .filter(|l| category.as_deref().map_or(true, |c| l.category == c))
                .filter(|l| wanted_status.map_or(true, |st| l.status == st))
                .collect();
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "source": table.source,
                        "data": "data/jetpack.toml",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "categories": table.category,
                        "libraries": libraries,
                        "by_status": table.status_counts(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "Apêndice D — {} bibliotecas em {} categorias (API {}–{}); registro ≠ execução\n",
                    table.library.len(),
                    table.category.len(),
                    table.api_floor,
                    table.api_ceiling
                );
                for l in libraries {
                    println!(
                        "{:28} {:15} {:16} {:16} {}",
                        l.name,
                        l.status.as_str(),
                        l.strategy.as_str(),
                        l.category,
                        l.module
                    );
                    if let Some(gap) = &l.gap_id {
                        println!("   gap: {gap} · marco: {}", l.milestone);
                    }
                    println!("   {}", l.notes);
                }
                println!("\nUse `rd jetpack show <pacote.Classe|descritor|id> --json` para o lookup de maior prefixo.");
            }
            Ok(())
        }
        JetpackSub::Show { query, json } => {
            let exact = table.library_by_id_or_prefix(query);
            let classification = table.classify(query);
            let library = exact.or_else(|| table.lookup(query).map(|(l, _)| l));
            let Some(library) = library else {
                return Err(RdError::missing_entry(format!(
                    "biblioteca AndroidX/3rd-party para {query} (classificação {})",
                    classification.status
                ))
                .with_suggestion(
                    "adicione uma linha a data/jetpack.toml no mesmo PR da biblioteca; não trate unknown como supported",
                ));
            };
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "query": query,
                        "match": if exact.is_some() { "exact" } else { "longest-prefix" },
                        "classification": classification,
                        "library": library,
                        "category": table.category(&library.category),
                        "execution_claimed": false,
                    }))
                );
            } else {
                let category_name = table
                    .category(&library.category)
                    .map_or(library.category.as_str(), |c| c.name.as_str());
                println!("{} → {} ({})", query, library.name, library.id);
                println!("categoria: {} [{}]", category_name, library.category);
                println!(
                    "status: {} · estratégia: {} · módulo: {} · marco: {}",
                    library.status.as_str(),
                    library.strategy.as_str(),
                    library.module,
                    library.milestone
                );
                println!("coordenadas: {}", library.coordinates);
                println!("prefixos: {}", library.prefixes.join(", "));
                if let Some(gap) = &library.gap_id {
                    println!("gap: {gap}");
                }
                if !library.code.is_empty() {
                    println!("código: {}", library.code.join(", "));
                }
                if !library.key_apis.is_empty() {
                    println!("APIs-chave: {}", library.key_apis.join(", "));
                }
                println!("{}", library.notes);
            }
            Ok(())
        }
        JetpackSub::Check { json } => {
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "ok": true,
                        "source": table.source,
                        "data": "data/jetpack.toml",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "categories": table.category.len(),
                        "libraries": table.library.len(),
                        "by_status": table.status_counts(),
                        "by_category": table.category_summaries(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "jetpack OK — {} categorias, {} bibliotecas, API {}–{}",
                    table.category.len(),
                    table.library.len(),
                    table.api_floor,
                    table.api_ceiling
                );
                for (status, n) in table.status_counts() {
                    println!("- {status}: {n}");
                }
                for c in table.category_summaries() {
                    println!("- {}: {} bibliotecas", c.id, c.libraries);
                }
            }
            Ok(())
        }
        JetpackSub::Report { out } => {
            let report = table.markdown_report();
            if let Some(path) = out {
                std::fs::write(path, report)
                    .map_err(|e| RdError::io(&e, format!("writing {}", path.display())))?;
                println!("relatório Apêndice D gerado em {}", path.display());
            } else {
                print!("{report}");
            }
            Ok(())
        }
    }
}
