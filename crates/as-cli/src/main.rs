use as_runtime::{InspectReport, Runtime};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "asctl",
    version,
    about = "AndroSwitch Android runtime tools (inspection milestone)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect an APK without executing its code or claiming compatibility.
    Inspect {
        apk: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Summarize a raw DEX for comparison with the portable Switch parser.
    DexSummary {
        dex: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show implemented AndroSwitch capabilities, separately from upstream.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = Runtime::default();
    match cli.command {
        Commands::DexSummary { dex, json } => {
            let result = dex
                .to_str()
                .ok_or_else(|| as_runtime::RuntimeError {
                    code: "INVALID_PATH".into(),
                    operation: "inspect_dex".into(),
                    module_id: "as-cli".into(),
                    cause: "the DEX path must be valid Unicode".into(),
                })
                .and_then(|path| runtime.inspect_dex(path));
            match result {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable report")
                        );
                    } else {
                        println!("DEX {}: {} classes, {} methods, {} strings; checksum={}; bytecode not executed",
                        report.version, report.classes, report.methods, report.strings, report.checksum_ok);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({"schema_version":1,"status":"error","error":error})
                        );
                    } else {
                        eprintln!("{error}");
                    }
                    ExitCode::from(1)
                }
            }
        }
        Commands::Inspect { apk, json } => {
            let result = apk
                .to_str()
                .ok_or_else(|| as_runtime::RuntimeError {
                    code: "INVALID_PATH".into(),
                    operation: "inspect".into(),
                    module_id: "as-cli".into(),
                    cause: "the APK path must be valid Unicode".into(),
                })
                .and_then(|path| runtime.inspect(path));
            match result {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).expect("serializable report")
                        );
                    } else {
                        print_inspect(&report);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({"schema_version": 1, "status": "error", "error": error})
                        );
                    } else {
                        eprintln!("{error}");
                    }
                    ExitCode::from(1)
                }
            }
        }
        Commands::Capabilities { json } => {
            let capabilities = runtime.capabilities();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&capabilities).expect("serializable capabilities")
                );
            } else {
                println!("AndroSwitch {}", capabilities.version);
                println!("APK inspection: implemented");
                println!(
                    "Device target: API {}, ARM64, offline",
                    capabilities.runtime_api
                );
                println!("Install / DEX / native execution: not implemented in AndroSwitch yet");
                println!("Horizon emulator / physical device: not verified");
            }
            ExitCode::SUCCESS
        }
    }
}

fn print_inspect(report: &InspectReport) {
    println!("AndroSwitch APK inspection");
    println!("package       {}", report.manifest.package);
    println!(
        "version       {} ({})",
        report.manifest.version_name.as_deref().unwrap_or("unknown"),
        report
            .manifest
            .version_code
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
    println!(
        "SDK           min {} / target {} / runtime {}",
        report.manifest.min_sdk, report.manifest.target_sdk, report.preflight.runtime_api
    );
    println!("DEX files     {}", report.inventory.dex_files.len());
    println!(
        "native ABIs   {}",
        if report.inventory.abis.is_empty() {
            "none (DEX-only)".into()
        } else {
            report.inventory.abis.join(", ")
        }
    );
    println!("launcher      {}", report.launcher_activities.join(", "));
    println!(
        "signing       {} (detected only; not cryptographically verified)",
        report.signing.detected_schemes.join(", ")
    );
    println!(
        "static checks {}",
        if report.preflight.static_requirements_met {
            "passed (SDK / ABI / package form only)"
        } else {
            "blocked"
        }
    );
    for gap in &report.preflight.blockers {
        println!("  {}: {} [{}]", gap.code, gap.cause, gap.milestone);
    }
    println!("execution     not implemented / not verified in AndroSwitch");
}
