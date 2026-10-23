use as_runtime::{InspectReport, Runtime};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "asctl",
    version,
    about = "AndroSwitch APK inspection and persistent package management"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Copy and atomically publish an APK installation (preserves user data).
    Install {
        apk: PathBuf,
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        update: bool,
        #[arg(long)]
        activity: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List installed package revisions.
    List {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Remove a package; retain private/OBB data by default.
    Uninstall {
        package: String,
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Explicitly clear package-private data (separate from uninstall).
    ClearData {
        package: String,
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
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
        Commands::Install {
            apk,
            root,
            update,
            activity,
            json,
        } => package_command(&runtime, root, json, |pm| {
            use as_platform::Storage;
            let path = apk.to_str().ok_or_else(|| {
                as_platform::fs::error("INVALID_PATH", "install", "APK path is not Unicode")
            })?;
            let bytes = as_platform::host::HostPlatform::default().read_bounded(path, 512 << 20)?;
            pm.install(bytes, update, activity.as_deref())
                .map(|r| serde_json::json!({"schema_version":1,"status":"ok","package":r}))
        }),
        Commands::List { root, json } => package_command(&runtime, root, json, |pm| {
            pm.list().map(|packages|serde_json::json!({"schema_version":1,"status":"ok","packages":packages}))
        }),
        Commands::Uninstall {
            package,
            root,
            json,
        } => package_command(&runtime, root, json, |pm| {
            pm.uninstall(&package)
                .map(|r| serde_json::json!({"schema_version":1,"status":"ok","uninstall":r}))
        }),
        Commands::ClearData {
            package,
            root,
            json,
        } => package_command(&runtime, root, json, |pm| {
            pm.clear_data(&package).map(|()|serde_json::json!({"schema_version":1,"status":"ok","package":package,"private_data_cleared":true}))
        }),
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
                println!("Package install / update / list / uninstall: implemented");
                println!("Application lifecycle launch / stop: not implemented yet");
                println!("DEX/native method-probe emulator evidence: docs/M2_M4_E2E.md");
                println!("Physical device: not verified");
            }
            ExitCode::SUCCESS
        }
    }
}

fn package_command(
    runtime: &Runtime,
    root: PathBuf,
    json: bool,
    call: impl FnOnce(
        &as_runtime::PackageManager<as_platform::host_fs::HostFileSystem>,
    ) -> Result<serde_json::Value, as_platform::PlatformError>,
) -> ExitCode {
    let result = runtime.packages(root).and_then(|pm| {
        call(&pm).map_err(|e| as_runtime::RuntimeError {
            code: e.code.into(),
            operation: e.operation.into(),
            module_id: "as-package".into(),
            cause: e.cause,
        })
    });
    match result {
        Ok(report) => {
            if json {
                println!("{}", serde_json::to_string(&report).expect("report"));
            } else {
                println!("{}", serde_json::to_string_pretty(&report).expect("report"));
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
