//! Apêndice E CLI: the Play Services / Firebase honest no-GMS registry
//! is data, and this is the operator/agent door to it (`data/gms.toml`).

use crate::{DataDir, GmsSub};
use rd_apk::{RdError, RdResult};
use rd_framework::fd_api_coverage::ApiStatus;
use rd_framework::fd_gms::GmsTable;

pub(super) fn load_table(dd: &DataDir) -> RdResult<GmsTable> {
    dd.require("gms.toml")?;
    GmsTable::load(dd.path.as_deref())
}

fn parse_status(raw: &str) -> RdResult<ApiStatus> {
    match raw {
        "supported" => Ok(ApiStatus::Supported),
        "partial" => Ok(ApiStatus::Partial),
        "stub" => Ok(ApiStatus::Stub),
        "not_implemented" => Ok(ApiStatus::NotImplemented),
        other => Err(RdError::new(
            "INVALID_FORMAT",
            format!("status de gms inválido: {other}"),
            "rd-cli",
        )
        .with_suggestion("use supported|partial|stub|not_implemented")),
    }
}

pub(super) fn run(sub: &GmsSub, dd: &DataDir) -> RdResult<()> {
    let table = load_table(dd)?;
    match sub {
        GmsSub::List {
            category,
            status,
            json,
        } => {
            if let Some(c) = category {
                if table.category(c).is_none() {
                    return Err(RdError::missing_entry(format!("categoria Apêndice E {c}"))
                        .with_suggestion("veja os ids em `rd gms check --json`"));
                }
            }
            let wanted_status = status.as_deref().map(parse_status).transpose()?;
            let services: Vec<_> = table
                .service
                .iter()
                .filter(|s| category.as_deref().map_or(true, |c| s.category == c))
                .filter(|s| wanted_status.map_or(true, |st| s.status == st))
                .collect();
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "source": table.source,
                        "data": "data/gms.toml",
                        "mode": table.mode,
                        "network": "none",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "categories": table.category,
                        "services": services,
                        "by_status": table.status_counts(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "Apêndice E — {} serviços em {} categorias (modo {}, rede: nenhuma); registro ≠ backend\n",
                    table.service.len(),
                    table.category.len(),
                    table.mode
                );
                for s in services {
                    println!(
                        "{:34} {:15} {:14} {}",
                        s.name,
                        s.status.as_str(),
                        s.category,
                        s.module
                    );
                    println!("   programável: {}", s.programmable);
                    if let Some(gap) = &s.gap_id {
                        println!("   gap: {gap} · marco: {}", s.milestone);
                    }
                    println!("   {}", s.notes);
                }
                println!("\nUse `rd gms show <pacote.Classe|descritor|id> --json` para o lookup de maior prefixo.");
            }
            Ok(())
        }
        GmsSub::Show { query, json } => {
            let exact = table.service_by_id_or_prefix(query);
            let classification = table.classify(query);
            let service = exact.or_else(|| table.lookup(query).map(|(s, _)| s));
            let Some(service) = service else {
                return Err(RdError::missing_entry(format!(
                    "serviço GMS/Firebase para {query} (classificação {})",
                    classification.status
                ))
                .with_suggestion(
                    "adicione uma linha a data/gms.toml no mesmo PR do serviço (mantendo network=none); não trate unknown como supported",
                ));
            };
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "query": query,
                        "match": if exact.is_some() { "exact" } else { "longest-prefix" },
                        "mode": table.mode,
                        "classification": classification,
                        "service": service,
                        "category": table.category(&service.category),
                        "execution_claimed": false,
                    }))
                );
            } else {
                let category_name = table
                    .category(&service.category)
                    .map_or(service.category.as_str(), |c| c.name.as_str());
                println!("{} → {} ({})", query, service.name, service.id);
                println!("categoria: {} [{}]", category_name, service.category);
                println!(
                    "status: {} · módulo: {} · marco: {} · rede: {} (modo {})",
                    service.status.as_str(),
                    service.module,
                    service.milestone,
                    service.network,
                    table.mode
                );
                println!("coordenadas: {}", service.coordinates);
                println!("prefixos: {}", service.prefixes.join(", "));
                println!("programável: {}", service.programmable);
                if let Some(gap) = &service.gap_id {
                    println!("gap: {gap}");
                }
                if !service.code.is_empty() {
                    println!("código: {}", service.code.join(", "));
                }
                if !service.key_apis.is_empty() {
                    println!("APIs-chave: {}", service.key_apis.join(", "));
                }
                println!("{}", service.notes);
            }
            Ok(())
        }
        GmsSub::Check { json } => {
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "ok": true,
                        "source": table.source,
                        "data": "data/gms.toml",
                        "mode": table.mode,
                        "network": "none",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "categories": table.category.len(),
                        "services": table.service.len(),
                        "by_status": table.status_counts(),
                        "by_category": table.category_summaries(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "gms OK — modo {} (rede: nenhuma), {} categorias, {} serviços, API {}–{}",
                    table.mode,
                    table.category.len(),
                    table.service.len(),
                    table.api_floor,
                    table.api_ceiling
                );
                for (status, n) in table.status_counts() {
                    println!("- {status}: {n}");
                }
                for c in table.category_summaries() {
                    println!("- {}: {} serviços", c.id, c.services);
                }
            }
            Ok(())
        }
        GmsSub::Report { out } => {
            let report = table.markdown_report();
            if let Some(path) = out {
                std::fs::write(path, report)
                    .map_err(|e| RdError::io(&e, format!("writing {}", path.display())))?;
                println!("relatório Apêndice E gerado em {}", path.display());
            } else {
                print!("{report}");
            }
            Ok(())
        }
    }
}
