//! Registry CLI: JSON is an artifact, errors keep the shared stderr contract.

use crate::{DataDir, EngineSub};
use rd_apk::{Apk, RdResult};
use rd_engine_registry::{Artifact, DetectionState, EngineAdapter, EngineEvent, Registry};

pub(super) fn load_registry(dd: &DataDir) -> RdResult<Registry> {
    dd.require("engines.toml")?;
    match dd.path.as_deref().map(|d| d.join("engines.toml")) {
        Some(path) if path.exists() => Registry::load(&path),
        _ => Registry::embedded(),
    }
}

pub(super) fn run(sub: &EngineSub, dd: &DataDir) -> RdResult<()> {
    let registry = load_registry(dd)?;
    match sub {
        EngineSub::List { json } => {
            if *json {
                println!(
                    "{}",
                    crate::to_json(&serde_json::json!({
                        "schema_version": registry.schema_version,
                        "scope": registry.scope,
                        "registered_engines": registry.engines().len(),
                        "execution_claimed": false,
                        "modules": registry.module_states(),
                        "engines": registry.engine_infos()?,
                    }))
                );
            } else {
                println!("Apêndice G — 28 registros; detecção NÃO é execução\n");
                for e in registry.engines() {
                    let info = registry.info(e)?;
                    println!(
                        "{:2} {:23} {:16} {}",
                        e.number,
                        e.id,
                        e.modules.join("+"),
                        e.name
                    );
                    println!(
                        "   runtime: {}; feature_enabled={}; registry_adapter={}",
                        e.runtime_status.label(),
                        info.feature_enabled,
                        info.registry_adapter_status
                    );
                }
                println!("\nUse `rd engine detect <apk> --json` para evidências. Features não implementam runtimes.");
            }
            Ok(())
        }
        EngineSub::Detect { path, json } => {
            let apk = Apk::open(path)?;
            let report = registry.detect(&apk)?;
            if *json {
                println!("{}", crate::to_json(&report));
            } else {
                println!(
                    "Engines de {} — 28 registros; payload NÃO validado; execução NÃO prometida\n",
                    report.package
                );
                for e in report
                    .engines
                    .iter()
                    .filter(|e| e.state != DetectionState::NotDetected)
                {
                    println!(
                        "{:23} {:?} · {} · {}",
                        e.id,
                        e.state,
                        e.modules.join("+"),
                        e.runtime_status.label()
                    );
                    for rule in &e.evidence {
                        for ev in &rule.matches {
                            println!(
                                "   {}: {} matches; amostra: {}",
                                ev.predicate,
                                ev.total_matches,
                                ev.samples.join(", ")
                            );
                        }
                    }
                }
                for diag in &report.diagnostics {
                    println!("\n{}: {}", diag.code, diag.cause);
                }
                println!("\nAusência de assinatura não descarta engine. TWA/PWA podem não ter payload local.");
            }
            Ok(())
        }
        EngineSub::Run { id, path, json: _ } => {
            use std::io::Read;
            let adapter = registry.adapter(id)?;
            if !rd_engine_registry::native_execution_available(id) {
                adapter.parse(Artifact {
                    name: "unsupported",
                    bytes: &[],
                })?;
            }
            let file = std::fs::File::open(path)
                .map_err(|e| rd_apk::RdError::io(&e, format!("open {}", path.display())))?;
            let mut bytes = Vec::new();
            file.take(256 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| rd_apk::RdError::io(&e, "read bounded native APK"))?;
            if bytes.len() > 256 * 1024 * 1024 {
                return Err(rd_apk::RdError::new(
                    "APK_TOO_LARGE",
                    "native APK exceeds 256 MiB",
                    "rd-cli",
                )
                .with_suggestion("use an APK no larger than 256 MiB"));
            }
            let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            rd_vm::set_app_stdout(rd_vm::HostOut::Buffer(logs.clone()));
            let mut process = adapter.parse(Artifact {
                name: &path.to_string_lossy(),
                bytes: &bytes,
            })?;
            adapter.event(
                &mut process,
                EngineEvent {
                    name: "launch",
                    payload: &[],
                },
            )?;
            let report = process.report()?;
            #[cfg(feature = "native-engines")]
            let report = {
                let mut report = report;
                report["logs"] = serde_json::json!(String::from_utf8_lossy(
                    &logs.lock().expect("CLI app log buffer")
                )
                .to_string());
                report
            };
            println!("{}", crate::to_json(&report));
            Ok(())
        }
        EngineSub::Check { id, json: _ } => {
            if rd_engine_registry::native_execution_available(id) {
                println!(
                    "{}",
                    crate::to_json(
                        &serde_json::json!({"engine":id,"adapter":"AVAILABLE_BOUNDED","execution_claimed":false,"hint":"rd engine run <id> <apk> --json"})
                    )
                );
                registry.get(id)?;
                Ok(())
            } else {
                registry
                    .adapter(id)?
                    .parse(Artifact {
                        name: id,
                        bytes: &[],
                    })
                    .map(|_| ())
            }
        }
    }
}
