//! Apêndice C CLI: the Android API surface registry is data, and this is
//! the operator/agent door to it (`data/api-coverage.toml`).

use crate::{ApiCoverageSub, DataDir};
use rd_apk::{RdError, RdResult};
use rd_framework::fd_api_coverage::{ApiCoverageTable, ApiStatus};

pub(super) fn load_table(dd: &DataDir) -> RdResult<ApiCoverageTable> {
    dd.require("api-coverage.toml")?;
    ApiCoverageTable::load(dd.path.as_deref())
}

fn parse_status(raw: &str) -> RdResult<ApiStatus> {
    match raw {
        "supported" => Ok(ApiStatus::Supported),
        "partial" => Ok(ApiStatus::Partial),
        "stub" => Ok(ApiStatus::Stub),
        "not_implemented" => Ok(ApiStatus::NotImplemented),
        other => Err(RdError::new(
            "INVALID_FORMAT",
            format!("status de api-coverage inválido: {other}"),
            "rd-cli",
        )
        .with_suggestion("use supported|partial|stub|not_implemented")),
    }
}

pub(super) fn run(sub: &ApiCoverageSub, dd: &DataDir) -> RdResult<()> {
    let table = load_table(dd)?;
    match sub {
        ApiCoverageSub::List {
            domain,
            status,
            json,
        } => {
            if let Some(d) = domain {
                if table.domain(d).is_none() {
                    return Err(RdError::missing_entry(format!("domínio Apêndice C {d}"))
                        .with_suggestion("veja os ids em `rd api-coverage check --json`"));
                }
            }
            let wanted_status = status.as_deref().map(parse_status).transpose()?;
            let surfaces: Vec<_> = table
                .surface
                .iter()
                .filter(|s| domain.as_deref().map_or(true, |d| s.domain == d))
                .filter(|s| wanted_status.map_or(true, |st| s.status == st))
                .collect();
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "source": table.source,
                        "data": "data/api-coverage.toml",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "domains": table.domain,
                        "surfaces": surfaces,
                        "by_status": table.status_counts(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "Apêndice C — {} superfícies em {} domínios (API {}–{}); registro ≠ execução\n",
                    table.surface.len(),
                    table.domain.len(),
                    table.api_floor,
                    table.api_ceiling
                );
                for s in surfaces {
                    println!(
                        "{:38} {:15} {:16} {}",
                        s.prefix,
                        s.status.as_str(),
                        s.domain,
                        s.module
                    );
                    if let Some(gap) = &s.gap_id {
                        println!("   gap: {gap} · marco: {}", s.milestone);
                    }
                    println!("   {}", s.notes);
                }
                println!("\nUse `rd api-coverage show <pacote.Classe|descritor|id> --json` para o lookup de maior prefixo.");
            }
            Ok(())
        }
        ApiCoverageSub::Show { query, json } => {
            let exact = table.surface_by_id_or_prefix(query);
            let classification = table.classify(query);
            let surface = exact.or_else(|| table.lookup(query));
            let Some(surface) = surface else {
                return Err(RdError::missing_entry(format!(
                    "superfície android.* para {query} (classificação {})",
                    classification.status
                ))
                .with_suggestion(
                    "adicione uma linha a data/api-coverage.toml no mesmo PR da API; não trate unknown como supported",
                ));
            };
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "query": query,
                        "match": if exact.is_some() { "exact" } else { "longest-prefix" },
                        "classification": classification,
                        "surface": surface,
                        "domain": table.domain(&surface.domain),
                        "execution_claimed": false,
                    }))
                );
            } else {
                let domain_name = table
                    .domain(&surface.domain)
                    .map_or(surface.domain.as_str(), |d| d.name.as_str());
                println!("{} → {} ({})", query, surface.prefix, surface.id);
                println!("domínio: {} [{}]", domain_name, surface.domain);
                println!(
                    "status: {} · módulo: {} ({}) · marco: {} · API {}–{}",
                    surface.status.as_str(),
                    surface.module,
                    surface.domain_module,
                    surface.milestone,
                    surface.since_api,
                    surface.until_api
                );
                if let Some(gap) = &surface.gap_id {
                    println!("gap: {gap}");
                }
                if !surface.code.is_empty() {
                    println!("código: {}", surface.code.join(", "));
                }
                if !surface.key_apis.is_empty() {
                    println!("APIs-chave: {}", surface.key_apis.join(", "));
                }
                println!("{}", surface.notes);
            }
            Ok(())
        }
        ApiCoverageSub::Check { json } => {
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "ok": true,
                        "source": table.source,
                        "data": "data/api-coverage.toml",
                        "api_floor": table.api_floor,
                        "api_ceiling": table.api_ceiling,
                        "domains": table.domain.len(),
                        "surfaces": table.surface.len(),
                        "by_status": table.status_counts(),
                        "by_domain": table.domain_summaries(),
                        "execution_claimed": false,
                    }))
                );
            } else {
                println!(
                    "api-coverage OK — {} domínios, {} superfícies, API {}–{}",
                    table.domain.len(),
                    table.surface.len(),
                    table.api_floor,
                    table.api_ceiling
                );
                for (status, n) in table.status_counts() {
                    println!("- {status}: {n}");
                }
                for d in table.domain_summaries() {
                    println!("- {}: {} superfícies", d.id, d.surfaces);
                }
            }
            Ok(())
        }
        ApiCoverageSub::Report { out } => {
            let report = table.markdown_report();
            if let Some(path) = out {
                std::fs::write(path, report)
                    .map_err(|e| RdError::io(&e, format!("writing {}", path.display())))?;
                println!("relatório Apêndice C gerado em {}", path.display());
            } else {
                print!("{report}");
            }
            Ok(())
        }
    }
}
