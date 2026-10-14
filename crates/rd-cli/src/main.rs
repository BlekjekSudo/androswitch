//! `rd` — CLI do RustyDroid (M0/M1/M2).
//!
//! Subcomandos:
//! - `rd inspect <apk>`      — componentes/permissões/features/assinatura (DoD do M0)
//! - `rd perm list|info|audit` — motor de permissões consultável via CLI
//! - `rd device list|show`   — perfis de device (piso: moto-e5)
//! - `rd behavior`           — comutadores por targetSdk 26→36
//! - `rd dex summary|disasm` — parser DEX 100% + disassembler smali (DoD do M1)
//! - `rd vm exec`            — interpretador Dalvik mínimo (DoD do M2: métodos puros)
//! - `rd engine list|detect|check` — registro/detecção do Apêndice G (não execução)
//! - `rd perf <apk>`         — gate de performance (M7): startup cold/warm e RSS no piso moto-e5
//!
//! Códigos de saída (contrato API pública — issue #34):
//! 0 ok · 1 erro estruturado · 2 NOT_IMPLEMENTED · 4 erro estruturado da VM
//! (`rd vm exec`) · 64 usage inválida do CLI (não colide com NOT_IMPLEMENTED).

use clap::{Parser, Subcommand};
use rd_apk::{Apk, RdError};
use rd_dex::{disasm, Dex};
use rd_framework::fd_behavior::SwitchTable;
use rd_framework::fd_devices::DeviceTable;
use rd_framework::fd_permissions::{GrantState, PermissionEngine, PermissionTable};
use rd_framework::DeviceProfile;
mod engines;
mod perf;

use std::io::Write as _;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "rd",
    version,
    about = "RustyDroid — runtime Android em Rust, sem VM, headless-first, nativo para agentes de IA",
    long_about = None,
    after_help = "Erros seguem o contrato {code, cause, suggestion, module_id} — consumível por agentes. Códigos de saída: 0 ok · 1 erro · 2 NOT_IMPLEMENTED · 4 erro VM (vm exec) · 64 usage."
)]
struct Cli {
    /// Diretório de dados versionados (default: ./data, env RD_DATA_DIR, fallback embutido)
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspeciona um APK real: identidade, SDKs, componentes, permissões, features
    Inspect {
        path: PathBuf,
        /// Saída em JSON (contrato do agente)
        #[arg(long)]
        json: bool,
        /// Perfil de device para orçamentos (ex.: moto-e5)
        #[arg(long)]
        device: Option<String>,
    },
    /// Registro e detecção de engines (Apêndice G); não executa novos runtimes
    Engine {
        #[command(subcommand)]
        sub: EngineSub,
    },
    /// Motor de permissões (Apêndice B — dado versionado)
    Perm {
        #[command(subcommand)]
        sub: PermSub,
    },
    /// Perfis de device e orçamentos de hardware
    Device {
        #[command(subcommand)]
        sub: DeviceSub,
    },
    /// Behavior changes por targetSdk 26→36 (PRF-07)
    Behavior {
        /// targetSdk do app (default: 36 — teto)
        #[arg(long)]
        target: Option<u32>,
        /// Filtrar por impacto (storage, notifications, ui, …)
        #[arg(long)]
        impact: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Parser DEX 100% + disassembler smali (M1) — validado vs baksmali 2.5.2
    Dex {
        #[command(subcommand)]
        sub: DexSub,
    },
    /// Gate de performance (M7, PRF-06): startup cold/warm, RSS e heap do
    /// app no perfil de device (estimativa por fator de CPU)
    Perf {
        /// APK a medir (dentro das raízes do sandbox: cwd ou temp)
        apk: PathBuf,
        /// Perfil de device (ex.: moto-e5)
        #[arg(long, default_value = "moto-e5")]
        device: String,
        /// Execuções por regime (cold e warm)
        #[arg(long, default_value_t = 3)]
        runs: u32,
        /// Saída em JSON (contrato do agente)
        #[arg(long)]
        json: bool,
    },
    /// VM Dalvik mínima (M2): executa métodos static puros de APK/DEX real
    Vm {
        #[command(subcommand)]
        sub: VmSub,
    },
    /// Executa o app headless: lifecycle, views e toques do agente (M3)
    //
    // issue #56: SEM path no nível externo — o path duplicado era obrigatório
    // e descartado (`Cmd::App { path: _, sub }`); a forma documentada no
    // ROADMAP (`rd app dump <apk>`) saía com exit 64. issue #77: esta nota já
    // foi texto de ajuda (doc comment) e poluía `rd --help` — virou comentário.
    App {
        #[command(subcommand)]
        sub: AppSub,
    },
    /// M5: servidor MCP v1 (JSON-RPC 2.0 stdio — ADR 001) para agentes LLM
    Mcp {
        /// Imprime o catálogo JSON das tools (mesmo shape de tools/list) e sai
        #[arg(long)]
        describe_tools: bool,
        /// Escreve a documentação GERADA do catálogo no arquivo e sai
        #[arg(long)]
        gen_docs: Option<PathBuf>,
    },
    /// M6: executa uma suíte JSON de cenários → resumo + JUnit XML (TST-01).
    /// Exit: 0 tudo passou · 1 houve falha · 64 suíte inválida/IO (ver docs/suite-format.md)
    Test {
        /// Arquivo da suíte (JSON)
        suite: PathBuf,
        /// Grava o relatório JUnit XML neste arquivo
        #[arg(long)]
        junit: Option<PathBuf>,
        /// Imprime o resumo em JSON (em vez do texto humano)
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum EngineSub {
    /// Lista os 28 itens, módulos/features e estado real de runtime
    List {
        #[arg(long)]
        json: bool,
    },
    /// Analisa nomes ZIP/manifest sem carregar ou executar artefatos
    Detect {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verifica o contrato de runtime: indisponível responde NOT_IMPLEMENTED (exit 2)
    Check {
        id: String,
        /// Erros são sempre JSON no stderr (contrato do CLI)
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum VmSub {
    /// Invoca um método static e imprime o resultado (ou a exceção escapada)
    Exec {
        path: PathBuf,
        /// Descritor da classe (ex.: Lcom/x/Calc;)
        #[arg(long)]
        class: String,
        /// Nome do método
        #[arg(long)]
        method: String,
        /// Descritor completo (ex.: (II)I) — desambigua sobrecargas
        #[arg(long)]
        sig: String,
        /// Literais Java separados por vírgula: 42, -7, 1L, 1.5f, 2.5, 'c', "txt", true
        #[arg(long)]
        args: Option<String>,
        #[arg(long)]
        json: bool,
        /// Limite de instruções (anti-loop)
        #[arg(long)]
        fuel: Option<u64>,
        /// Profundidade máxima de chamadas
        #[arg(long)]
        depth: Option<usize>,
        /// Heap do app em MB (default 256 = piso moto-e5)
        #[arg(long)]
        heap_mb: Option<usize>,
    },
}

#[derive(Subcommand)]
enum AppSub {
    /// Cria a activity, roda o lifecycle e aplica o script do agente
    Run {
        path: PathBuf,
        /// Classe da activity (Lpkg/Cls; ou com.ex.Main). Default: LAUNCHER
        #[arg(long)]
        activity: Option<String>,
        /// Ação por passo: "tap X,Y" | "wait MS" | "dump" | "xml" | "shot FILE" (repetível)
        #[arg(long = "script")]
        scripts: Vec<String>,
        /// Anexa linhas "at classe->metodo(proto)" a exceções Java não
        /// capturadas (stack trace estilo JVM — diagnóstico de call site)
        #[arg(long)]
        trace: bool,
        #[arg(long)]
        json: bool,
    },
    /// M4 DoD "get_ui_tree": dump UI — uiautomator XML (default) ou textual M3
    Dump {
        path: PathBuf,
        /// Classe da activity. Default: LAUNCHER
        #[arg(long)]
        activity: Option<String>,
        /// Dump textual M3 em vez do XML uiautomator
        #[arg(long)]
        text: bool,
        #[arg(long)]
        json: bool,
    },
    /// M4 DoD "screenshot": PNG headless da tela corrente (720×1440)
    Shot {
        path: PathBuf,
        /// Classe da activity. Default: LAUNCHER
        #[arg(long)]
        activity: Option<String>,
        /// Arquivo PNG de saída
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum DexSub {
    /// Resumo de cada dex do APK/arquivo: header, mapa e contagens
    Summary {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Desmonta em formato smali (fidelidade baksmali 2.5.2)
    Disasm {
        path: PathBuf,
        /// Descritor da classe (ex.: Lcom/x/Y;). Sem isso: lista classes.
        #[arg(long)]
        class: Option<String>,
        /// Filtra um método pelo nome (requer --class)
        #[arg(long)]
        method: Option<String>,
        /// Escreve os .smali nesse diretório em vez de imprimir
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum PermSub {
    /// Lista permissões declaradas + estado calculado pelo motor
    List {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Detalhe de uma permissão da tabela versionada
    Info {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Auditoria estática (exported sem guarda, debuggable, cleartext…)
    Audit {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum DeviceSub {
    /// Lista perfis disponíveis
    List {
        #[arg(long)]
        json: bool,
    },
    /// Mostra um perfil em detalhe (orçamentos)
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    // issue #55: stdout do processo host é do ARTEFATO do comando (dump XML,
    // JSON, PNG status) — System.out do app vai para o stderr (o Android real
    // manda saída do app para o logcat, nunca para o stream do dump).
    rd_vm::route_app_stdout_to_stderr();
    // issue #34: clap sai com 2 em usage error — colide com o 2 =
    // NOT_IMPLEMENTED do contrato. Remapeado: help/version = 0; usage = 64
    // (EX_USAGE, convenção BSD) — o contrato JSON do projeto fica intacto.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if e.use_stderr() => {
            let _ = e.print();
            std::process::exit(64);
        }
        Err(e) => {
            let _ = e.print(); // help/version
            std::process::exit(0);
        }
    };
    let code = run(cli);
    std::process::exit(code);
}

/// Diretório de dados + se foi pedido EXPLICITAMENTE (--data/RD_DATA_DIR).
/// issue #38: --data apontando para dir sem os TOMLs NÃO pode cair
/// silenciosamente no embedded — o usuário acreditaria ter auditado contra
/// a tabela dele.
struct DataDir {
    path: Option<PathBuf>,
    explicit: bool,
}

impl DataDir {
    /// Erro tipado se o arquivo esperado não existir num --data explícito
    /// (ou RD_DATA_DIR — issue #51: env é explícito também).
    fn require(&self, file: &str) -> Result<(), RdError> {
        if self.explicit {
            if let Some(d) = &self.path {
                if !d.join(file).exists() {
                    // issue #51: module_id correto — este erro nasce no rd-cli
                    // (não no rd-apk, que era o MODULE_ID do RdError::io herdado)
                    return Err(RdError::new(
                        "IO_ERROR",
                        format!(
                            "--data {file}: {} não encontrado: fallback para o embedded desativado",
                            d.join(file).display()
                        ),
                        "rd-cli",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn run(cli: Cli) -> i32 {
    // resolução do diretório de dados: --data > RD_DATA_DIR > ./data
    let data_path = data_dir(cli.data.as_deref());
    let dd = DataDir {
        path: data_path,
        // issue #51: RD_DATA_DIR (env) também é escolha explícita do usuário —
        // sem isso, um env apontando para diretório vazio caía silenciosamente
        // no embedded, sem o warning do require()
        explicit: cli.data.is_some() || std::env::var_os("RD_DATA_DIR").is_some(),
    };
    // M6: `rd test` tem contrato de saída próprio (0 passou · 1 falhou · 64 inválida)
    if let Cmd::Test { suite, junit, json } = &cli.cmd {
        return cmd_test(suite, junit.as_deref(), *json);
    }
    let result = match &cli.cmd {
        Cmd::Inspect { path, json, device } => cmd_inspect(path, *json, device.as_deref(), &dd),
        Cmd::Engine { sub } => engines::run(sub, &dd),
        Cmd::Perm { sub } => match sub {
            PermSub::List { path, json } => cmd_perm_list(path, *json, &dd),
            PermSub::Info { name, json } => cmd_perm_info(name, *json, &dd),
            PermSub::Audit { path, json } => cmd_perm_audit(path, *json, &dd),
        },
        Cmd::Device { sub } => match sub {
            DeviceSub::List { json } => cmd_device_list(*json, &dd),
            DeviceSub::Show { id, json } => cmd_device_show(id, *json, &dd),
        },
        Cmd::Behavior {
            target,
            impact,
            json,
        } => cmd_behavior(*target, impact.as_deref(), *json, &dd),
        Cmd::Dex { sub } => match sub {
            DexSub::Summary { path, json } => cmd_dex_summary(path, *json),
            DexSub::Disasm {
                path,
                class,
                method,
                out,
                json,
            } => cmd_dex_disasm(
                path,
                class.as_deref(),
                method.as_deref(),
                out.as_deref(),
                *json,
            ),
        },
        Cmd::App { sub } => match sub {
            AppSub::Run {
                path,
                activity,
                scripts,
                trace,
                json,
            } => cmd_app_run(path, activity.as_deref(), scripts, *trace, *json),
            AppSub::Dump {
                path,
                activity,
                text,
                json,
            } => cmd_app_dump(path, activity.as_deref(), *text, *json),
            AppSub::Shot {
                path,
                activity,
                out,
            } => cmd_app_shot(path, activity.as_deref(), out),
        },
        Cmd::Mcp {
            describe_tools,
            gen_docs,
        } => {
            // M5 (DoD): o conjunto publicado corresponde ao --describe-tools
            // e à documentação gerada — ambos saem do MESMO tool_defs().
            if *describe_tools {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rd_agent::mcp::tools_list_json())
                        .expect("catálogo serializa")
                );
                return 0;
            }
            if let Some(path) = gen_docs {
                return match std::fs::write(path, rd_agent::mcp::tools_markdown()) {
                    Ok(()) => {
                        println!("docs geradas em {}", path.display());
                        0
                    }
                    Err(e) => {
                        let err = RdError::new(
                            "IO_ERROR",
                            format!("escrevendo {}: {e}", path.display()),
                            "rd-cli",
                        );
                        let _ = writeln!(std::io::stderr(), "{}", to_json(&err));
                        1
                    }
                };
            }
            // M5: o stdout É o canal do protocolo — nada mais pode escrever
            // nele (o app roteia saída para o buffer da sessão, ADR 001).
            // O erro sai pelo caminho padrão do contrato (JSON no stderr).
            rd_agent::mcp::serve_stdio(std::io::stdin(), std::io::stdout())
        }
        Cmd::Perf {
            apk,
            device,
            runs,
            json,
        } => {
            // exit 0 = passou · 1 = orçamento falhou ou erro de execução
            return match dd
                .require("devices.toml")
                .and_then(|()| DeviceTable::load(dd.path.as_deref()))
                .and_then(|table| {
                    table.get(device).cloned().ok_or_else(|| {
                        RdError::missing_entry(format!("device profile {device:?}"))
                            .with_suggestion("run `rd device list`")
                    })
                })
                .and_then(|profile| perf::cmd_perf(apk, &profile, *runs, *json))
            {
                Ok(true) => 0,
                Ok(false) => 1,
                Err(e) => {
                    let mut stderr = std::io::stderr();
                    let _ = writeln!(stderr, "{}", to_json(&e));
                    1
                }
            };
        }
        // `rd test` devolve código de saída próprio (0/1/64): tratado acima
        Cmd::Test { .. } => unreachable!("rd test é despachado antes deste match"),
        Cmd::Vm { sub } => match sub {
            VmSub::Exec {
                path,
                class,
                method,
                sig,
                args,
                json,
                fuel,
                depth,
                heap_mb,
            } => {
                // exit code próprio: 0 ok/exceção · 4 erro estruturado da VM
                return match cmd_vm_exec(
                    path,
                    class,
                    method,
                    sig,
                    args.as_deref(),
                    *json,
                    *fuel,
                    *depth,
                    *heap_mb,
                ) {
                    Ok(code) => code,
                    Err(e) => {
                        let mut stderr = std::io::stderr();
                        let _ = writeln!(stderr, "{}", to_json(&e));
                        if e.code == "NOT_IMPLEMENTED" {
                            2
                        } else {
                            1
                        }
                    }
                };
            }
        },
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "{}", to_json(&e));
            if e.code == "NOT_IMPLEMENTED" {
                2
            } else {
                1
            }
        }
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|e| {
        format!("{{\"code\":\"INTERNAL\",\"cause\":\"{e}\",\"module_id\":\"rd-cli\"}}")
    })
}

/// Resolve o diretório de dados: --data > RD_DATA_DIR > ./data (fallback embutido).
fn data_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    explicit
        .map(|p| p.to_path_buf())
        .or_else(|| std::env::var_os("RD_DATA_DIR").map(PathBuf::from))
        .or_else(|| {
            let cwd = PathBuf::from("data");
            cwd.exists().then_some(cwd)
        })
}

fn load_permissions(dd: &DataDir) -> Result<PermissionTable, RdError> {
    dd.require("permissions.toml")?;
    match dd
        .path
        .as_deref()
        .and_then(|d| d.join("permissions.toml").exists().then_some(d))
    {
        Some(d) => {
            let s = std::fs::read_to_string(d.join("permissions.toml"))
                .map_err(|e| RdError::io(&e, "reading permissions.toml"))?;
            PermissionTable::from_toml(&s)
        }
        None => PermissionTable::embedded(),
    }
}

/// Converte o RdError do rd-render para o contrato do binário (mesma forma).
fn render_err(e: rd_render::RdError) -> RdError {
    let mut r = RdError::new(&e.code, &e.cause, &e.module_id);
    if let Some(s) = e.suggestion {
        r = r.with_suggestion(s);
    }
    r
}

fn open_apk(path: &Path) -> Result<Apk, RdError> {
    Apk::open(path)
}

// ─── dex (M1) ───────────────────────────────────────────────────────────────

/// Converte o RdError do rd-dex para o contrato do binário (mesma forma).
fn dex_err(e: rd_dex::RdError) -> RdError {
    let mut r = RdError::new(&e.code, &e.cause, &e.module_id);
    if let Some(s) = e.suggestion {
        r = r.with_suggestion(s);
    }
    r
}

/// Carrega todos os dex de um APK (classes*.dex) ou de um arquivo .dex solto.
fn load_dex_files(path: &Path) -> Result<Vec<(String, Dex)>, RdError> {
    // issue #38: cap prévio tipado — whole-file read sem teto + panic=abort
    // no release = abort sem erro estruturado em input gigante (budget piso E5)
    const MAX_INPUT: u64 = 512 * 1024 * 1024;
    let meta =
        std::fs::metadata(path).map_err(|e| RdError::io(&e, format!("stat {}", path.display())))?;
    if meta.len() > MAX_INPUT {
        return Err(RdError::invalid_format(format!(
            "{} tem {} bytes (> teto de {} bytes): leitura recusada antes de alocar",
            path.display(),
            meta.len(),
            MAX_INPUT
        )));
    }
    let raw =
        std::fs::read(path).map_err(|e| RdError::io(&e, format!("lendo {}", path.display())))?;
    if Dex::looks_like_dex(&raw) {
        let d = Dex::parse(raw).map_err(dex_err)?;
        return Ok(vec![("classes.dex".into(), d)]);
    }
    let apk = open_apk(path)?;
    dexes_of_apk(&apk)
}

/// M3.2: dex de um APK JÁ aberto — o mesmo `Apk` vira Resources da Engine
/// (leitura única do arquivo; antes `app run` relia o zip 3x).
fn dexes_of_apk(apk: &Apk) -> Result<Vec<(String, Dex)>, RdError> {
    let mut out = Vec::new();
    for name in &apk.dex_files {
        let entry = apk
            .zip
            .find(name)
            .ok_or_else(|| RdError::missing_entry(format!("entrada zip {name:?}")))?;
        let bytes = apk
            .zip
            .read_entry(entry)
            .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        let d = Dex::parse(bytes).map_err(dex_err)?;
        out.push((name.clone(), d));
    }
    if out.is_empty() {
        return Err(RdError::missing_entry("classes*.dex no APK")
            .with_suggestion("confirme com `rd inspect` — o campo content lista os dex"));
    }
    Ok(out)
}

/// Contagem de um tipo de mapa (0 se ausente).
fn map_size(dex: &Dex, want: rd_dex::MapType) -> u32 {
    dex.map_list
        .iter()
        .find(|m| m.map_type == want)
        .map(|m| m.size)
        .unwrap_or(0)
}

// ─── inspect ────────────────────────────────────────────────────────────────

fn cmd_inspect(path: &Path, json: bool, device: Option<&str>, dd: &DataDir) -> Result<(), RdError> {
    let apk = open_apk(path)?;
    let table = load_permissions(dd)?;
    let engine = PermissionEngine::install(&apk.manifest, table);

    let device_profile: Option<DeviceProfile> = match device {
        Some(id) => {
            dd.require("devices.toml")?;
            let dt = DeviceTable::load(dd.path.as_deref())?;
            Some(
                dt.get(id)
                    .ok_or_else(|| {
                        RdError::missing_entry(format!("device profile {id:?}"))
                            .with_suggestion("run `rd device list`")
                    })?
                    .clone(),
            )
        }
        None => None,
    };

    if json {
        let mut v = serde_json::to_value(&apk)
            .map_err(|e| RdError::new("INTERNAL", e.to_string(), "rd-cli"))?;
        if let serde_json::Value::Object(map) = &mut v {
            // issue #51: mesmo shape do `rd perm list --json` ({state, granted_by?})
            // — granted_by é o contrato primário de confiança para agents
            let mut perms = serde_json::Map::new();
            for (name, state) in engine.all_states() {
                let mut o = serde_json::Map::new();
                o.insert("state".into(), serde_json::json!(state));
                if let Some(by) = engine.granted_by(name) {
                    o.insert("granted_by".into(), serde_json::json!(by));
                }
                perms.insert(name.clone(), serde_json::Value::Object(o));
            }
            map.insert("permissions_state".into(), serde_json::Value::Object(perms));
            map.insert(
                "engines".into(),
                serde_json::json!(engines::load_registry(dd)?.detect(&apk)?),
            );
            map.insert(
                "floor_status".into(),
                serde_json::to_value(apk.floor_status())
                    .map_err(|e| RdError::new("INTERNAL", e.to_string(), "rd-cli"))?,
            );
            // issue #84: ambiguidade do container é DADO, não silêncio
            if !apk.zip_duplicates.is_empty() {
                map.insert(
                    "zip_duplicates".into(),
                    serde_json::json!(apk.zip_duplicates),
                );
            }
            if let Some(dp) = &device_profile {
                map.insert(
                    "device_budget".into(),
                    serde_json::to_value(dp)
                        .map_err(|e| RdError::new("INTERNAL", e.to_string(), "rd-cli"))?,
                );
            }
        }
        println!("{v}");
        return Ok(());
    }

    // ── saída humana ──
    let m = &apk.manifest;
    let file = path.display();
    let mut out = String::new();
    out.push_str(&format!("RustyDroid inspect — {file}\n"));
    out.push_str(&"=".repeat(64));
    out.push('\n');

    out.push_str(&format!(
        "package            {}  v{} ({})\n",
        m.package,
        m.version_name.as_deref().unwrap_or("?"),
        m.version_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into())
    ));
    if let Some(label) = &m.application.label {
        let src = m
            .application
            .label_res
            .map(|r| format!(" (resolved @0x{r:08x})"))
            .unwrap_or_default();
        out.push_str(&format!("label              {label}{src}\n"));
    }
    if let Some(app) = &m.application.name {
        out.push_str(&format!("application class  {app}\n"));
    }
    out.push_str(&format!(
        "SDK                min {} · target {} · compile {}\n",
        m.min_sdk,
        m.target_sdk,
        m.compile_sdk
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into())
    ));
    match apk.floor_status() {
        rd_apk::FloorStatus::Ok => {
            out.push_str(&format!(
                "api floor          OK (≥ {})\n",
                rd_apk::FLOOR_MIN_SDK
            ));
        }
        rd_apk::FloorStatus::BelowFloor { min_sdk, floor } => {
            out.push_str(&format!(
                "api floor          ⚠ BELOW_FLOOR(minSdk={min_sdk}, floor={floor}) — inspect ok; execução responde erro estruturado\n"
            ));
        }
    }
    out.push_str(&format!(
        "signing            {}\n",
        if apk.signing.is_signed() {
            apk.signing.schemes().join(", ")
        } else {
            "UNSIGNED (esquemas v1/v2/v3 não detectados)".into()
        }
    ));
    out.push_str(&format!(
        "content            {} entradas zip · {} dex · {} lib .so (abis: {}) · {} assets · {} res · arsc={}\n",
        apk.zip_entry_count,
        apk.dex_files.len(),
        apk.native_libs.len(),
        if apk.abis.is_empty() { "—".into() } else { apk.abis.join(",") },
        apk.assets.len(),
        apk.res_count,
        apk.resources_arsc,
    ));
    // issue #84: nomes repetidos no ZIP — o APK é ambíguo; avisar em vez de
    // escolher em silêncio (o inventário acima conta por nome único).
    if !apk.zip_duplicates.is_empty() {
        out.push_str(&format!(
            "zip duplicates     ⚠ {} nome(s) repetido(s) no ZIP: {} — rd usa a 1ª ocorrência\n",
            apk.zip_duplicates.len(),
            apk.zip_duplicates.join(" · ")
        ));
    }
    if let Some(dp) = &device_profile {
        out.push_str(&format!(
            "device budget      {} [{}]\n{}\n",
            dp.id,
            dp.label,
            dp.budget_summary()
        ));
    }

    // features
    if !m.uses_features.is_empty() {
        out.push_str("\nfeatures\n");
        for f in &m.uses_features {
            let what = f
                .name
                .as_deref()
                .or(f.gl_es_version.as_deref().map(|_| "opengles"))
                .unwrap_or("?");
            let version = f.gl_es_version.as_deref().unwrap_or("");
            out.push_str(&format!(
                "  {}{} (required={})\n",
                what,
                if version.is_empty() {
                    String::new()
                } else {
                    format!(" {version}")
                },
                f.required
            ));
        }
    }

    // permissões — estados do motor
    let states = engine.all_states();
    let count = |pred: fn(GrantState) -> bool| states.values().filter(|s| pred(**s)).count();
    // issue #38: contagem da tabela EM USO (load_permissions/--data), não do embedded
    out.push_str(&format!(
        "\npermissions ({} declaradas; tabela: {} permissões)\n",
        states.len(),
        engine.table.len()
    ));
    // issue #76: `not requested` existia no JSON mas não no resumo textual —
    // as quatro categorias antigas não somavam o total declarado e o usuário
    // não conseguia explicar a diferença.
    let not_requested = count(|s| s == GrantState::NotRequested);
    // issue #69: `app-defined` entra como categoria para que a soma continue
    // fechando com o total declarado (mesma regra da #76).
    let app_defined = count(|s| s == GrantState::AppDefined);
    // issue #110: `denied-system` (signature-class fora da whitelist B5) entra
    // como categoria própria — apps de terceiros não a recebem no Android real,
    // e a soma das categorias precisa continuar fechando com o total declarado.
    let denied_system = count(|s| s == GrantState::DeniedSystem);
    out.push_str(&format!(
        "  auto-granted: {} · runtime-pending: {} · special: {} · denied-system: {} · unknown: {} · not requested: {} · app-defined: {}\n",
        count(|s| s == GrantState::AutoGranted),
        count(|s| s == GrantState::RuntimePending),
        count(|s| s == GrantState::SpecialPending),
        denied_system,
        count(|s| s == GrantState::Unknown),
        not_requested,
        app_defined,
    ));
    if not_requested > 0 {
        out.push_str(
            "    (not requested = fora da janela do targetSdk: maxSdkVersion vencido ou since_api acima do alvo)\n",
        );
    }
    for group in ["dangerous (runtime-pending)", "special access", "unknown"] {
        let want = match group {
            "dangerous (runtime-pending)" => GrantState::RuntimePending,
            "special access" => GrantState::SpecialPending,
            _ => GrantState::Unknown,
        };
        let names: Vec<&String> = states
            .iter()
            .filter(|(_, s)| **s == want)
            .map(|(n, _)| n)
            .collect();
        if !names.is_empty() {
            out.push_str(&format!("  {group}:\n"));
            for n in &names {
                out.push_str(&format!("    {n}\n"));
            }
        }
    }

    // componentes
    let comps = &m.application.components;
    let mut by_kind = std::collections::BTreeMap::new();
    for c in comps {
        let entry = by_kind.entry(c.kind.to_string()).or_insert(0usize);
        *entry += 1;
    }
    let summary: Vec<String> = by_kind
        .into_iter()
        .map(|(k, n)| format!("{k}×{n}"))
        .collect();
    out.push_str(&format!(
        "\ncomponents ({}: {})\n",
        comps.len(),
        summary.join(" · ")
    ));
    for c in comps {
        let mut tags = Vec::new();
        if c.is_launcher {
            tags.push("LAUNCHER".into());
        }
        if c.exported_effective {
            tags.push("exported".into());
        }
        if let Some(p) = &c.permission {
            tags.push(format!("perm={p}"));
        }
        if c.exported_effective && c.permission.is_none() {
            tags.push("⚠ sem guarda".into());
        }
        out.push_str(&format!(
            "  {:<15} {}{}\n",
            c.kind.to_string(),
            if c.class_name.is_empty() {
                "(sem nome)"
            } else {
                &c.class_name
            },
            if tags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", tags.join(", "))
            }
        ));
        for f in &c.intent_filters {
            let actions = if f.actions.is_empty() {
                "—".to_string()
            } else {
                f.actions.join(", ")
            };
            let cats = f.categories.join(", ");
            out.push_str(&format!(
                "  {:<15}   filter: actions=[{}] categories=[{}]\n",
                "", actions, cats
            ));
            for d in &f.data {
                let mut parts = Vec::new();
                for (k, v) in [
                    ("scheme", &d.scheme),
                    ("host", &d.host),
                    ("mime", &d.mime_type),
                    ("pathPrefix", &d.path_prefix),
                ] {
                    if let Some(v) = v {
                        parts.push(format!("{k}={v}"));
                    }
                }
                if !parts.is_empty() {
                    out.push_str(&format!("  {:<15}     data: {}\n", "", parts.join(" ")));
                }
            }
        }
    }

    print!("{out}");
    Ok(())
}

// ─── perm ───────────────────────────────────────────────────────────────────

fn cmd_perm_list(path: &Path, json: bool, dd: &DataDir) -> Result<(), RdError> {
    let apk = open_apk(path)?;
    let table = load_permissions(dd)?;
    let engine = PermissionEngine::install(&apk.manifest, table.clone());
    if json {
        // issue #38: permissão → {state, granted_by?} — signature AutoGranted
        // carrega "runtime-is-system" para não enganar agents (issue #31)
        let mut entries = serde_json::Map::new();
        for (name, state) in engine.all_states() {
            let mut o = serde_json::Map::new();
            o.insert("state".into(), serde_json::json!(state));
            if let Some(by) = engine.granted_by(name) {
                o.insert("granted_by".into(), serde_json::json!(by));
            }
            entries.insert(name.clone(), serde_json::Value::Object(o));
        }
        let mut v = serde_json::Value::Object(entries);
        if let serde_json::Value::Object(map) = &mut v {
            map.insert(
                "target_sdk".into(),
                serde_json::json!(apk.manifest.target_sdk),
            );
            map.insert("table_size".into(), serde_json::json!(table.len()));
        }
        println!("{v}");
        return Ok(());
    }
    println!(
        "permissions de {} (target {}) — engine fd-permissions",
        apk.manifest.package, apk.manifest.target_sdk
    );
    for (name, state) in engine.all_states() {
        let def = table.get(name);
        let level = def
            .map(|d| format!("{:?}", d.level))
            .unwrap_or_else(|| "?".into());
        let marker = engine
            .granted_by(name)
            .map(|b| format!(" [{b}]"))
            .unwrap_or_default();
        println!(
            "  {:<28} {:<16} {level}{marker}",
            format!("{state:?}"),
            name
        );
    }
    Ok(())
}

fn cmd_perm_info(name: &str, json: bool, dd: &DataDir) -> Result<(), RdError> {
    let table = load_permissions(dd)?;
    let def = table.get(name).ok_or_else(|| {
        RdError::missing_entry(format!("permission {name:?}"))
            .with_suggestion("check data/permissions.toml — names follow android.permission.*")
    })?;
    if json {
        println!("{}", to_json(def));
        return Ok(());
    }
    println!("permission       {}", def.name);
    println!("level            {:?}", def.level);
    if let Some(g) = def.group_name() {
        println!("group            {g}");
    }
    if !def.flags.is_empty() {
        println!("flags            {}", def.flags.join(", "));
    }
    println!("since_api        {}", def.since_api);
    if !def.description.is_empty() {
        println!("description      {}", def.description);
    }
    Ok(())
}

fn cmd_perm_audit(path: &Path, json: bool, dd: &DataDir) -> Result<(), RdError> {
    let apk = open_apk(path)?;
    let table = load_permissions(dd)?;
    let engine = PermissionEngine::install(&apk.manifest, table);
    let findings = engine.audit(&apk.manifest);
    if json {
        println!("{}", to_json(&findings));
        return Ok(());
    }
    println!(
        "audit de {} — {} finding(s)\n",
        apk.manifest.package,
        findings.len()
    );
    for f in &findings {
        let sev = format!("{:?}", f.severity).to_uppercase();
        println!("  [{sev:<7}] {} — {}", f.code, f.message);
    }
    Ok(())
}

// ─── device ─────────────────────────────────────────────────────────────────

fn cmd_device_list(json: bool, dd: &DataDir) -> Result<(), RdError> {
    dd.require("devices.toml")?;
    let table = DeviceTable::load(dd.path.as_deref())?;
    if json {
        println!("{}", to_json(&table.device));
        return Ok(());
    }
    println!("perfis de device (piso marcado com ★):");
    for d in &table.device {
        let star = if d.is_hardware_floor() { " ★" } else { "" };
        println!(
            "  {:<18} {:<24} {}×{} @{}dpi · heap {} MB · api {}–{}{}",
            d.id,
            d.label,
            d.width_px,
            d.height_px,
            d.density_dpi,
            d.ram_app_heap_mb,
            d.min_api,
            d.max_api,
            star
        );
    }
    Ok(())
}

fn cmd_device_show(id: &str, json: bool, dd: &DataDir) -> Result<(), RdError> {
    dd.require("devices.toml")?;
    let table = DeviceTable::load(dd.path.as_deref())?;
    let d = table.get(id).ok_or_else(|| {
        RdError::missing_entry(format!("device profile {id:?}"))
            .with_suggestion("run `rd device list`")
    })?;
    if json {
        println!("{}", to_json(d));
        return Ok(());
    }
    println!("device            {} ({})", d.id, d.label);
    println!("class             {}", d.class);
    println!(
        "screen            {}×{} · {}dpi · {}\" · {} · {}Hz",
        d.width_px, d.height_px, d.density_dpi, d.diagonal_in, d.aspect_ratio, d.refresh_hz
    );
    println!(
        "cpu               {} cores · single-thread {}% de 1 core · quota total {:.2} cores",
        d.cpu_cores,
        (d.cpu_single_thread_factor * 100.0) as u32,
        d.cpu_quota_cores
    );
    println!(
        "gpu               {} (fill-rate: {})",
        d.gpu, d.fill_rate_budget
    );
    println!(
        "ram               device {} MB · processo ≤ {} MB · heap app ≤ {} MB",
        d.ram_device_mb, d.ram_process_cap_mb, d.ram_app_heap_mb
    );
    println!(
        "storage           {} GB · I/O: {}",
        d.storage_quota_gb, d.io_profile
    );
    println!(
        "os                fábrica API {} · suportado {}–{}",
        d.factory_api, d.min_api, d.max_api
    );
    if !d.flags.is_empty() {
        println!("flags             {}", d.flags.join(", "));
    }
    if !d.notes.is_empty() {
        println!("notes             {}", d.notes);
    }
    println!("\nbudgets           {}", d.budget_summary());
    Ok(())
}

// ─── behavior ───────────────────────────────────────────────────────────────

fn cmd_behavior(
    target: Option<u32>,
    impact: Option<&str>,
    json: bool,
    dd: &DataDir,
) -> Result<(), RdError> {
    dd.require("behavior-switches.toml")?;
    let table = SwitchTable::load(dd.path.as_deref())?;
    let target = target.unwrap_or(36);
    if !(26..=36).contains(&target) {
        return Err(RdError::new(
            "OUT_OF_RANGE",
            format!("target {target} fora do piso/teto (26–36)"),
            "fd-behavior",
        ));
    }
    let mut active = table.active_for(target);
    if let Some(imp) = impact {
        active.retain(|s| s.impact == imp);
    }
    if json {
        let v = serde_json::json!({
            "target_sdk": target,
            "active_switches": active,
            "inactive_count": table.inactive_for(target).len(),
        });
        println!("{v}");
        return Ok(());
    }
    println!(
        "behavior switches ativos com targetSdk {target} ({} de {})\n",
        active.len(),
        table.switch.len()
    );
    for s in &active {
        println!(
            "  @{}  {:<34} [{:<17}] {} ({})",
            s.api, s.title, s.impact, s.default_state, s.id
        );
        println!("       {}", s.summary);
    }
    Ok(())
}

fn cmd_dex_summary(path: &Path, json: bool) -> Result<(), RdError> {
    let files = load_dex_files(path)?;
    if json {
        let items: Vec<serde_json::Value> = files
            .iter()
            .map(|(name, d)| {
                serde_json::json!({
                    "entry": name,
                    "size_bytes": d.data.len(),
                    "version": format!("{:?}", d.header.version),
                    "checksum_ok": d.header.checksum_ok,
                    "strings": d.header.string_ids_size,
                    "types": d.header.type_ids_size,
                    "protos": d.header.proto_ids_size,
                    "fields": d.header.field_ids_size,
                    "methods": d.header.method_ids_size,
                    "classes": d.header.class_defs_size,
                    "code_items": map_size(d, rd_dex::MapType::Code),
                    "debug_info_items": map_size(d, rd_dex::MapType::DebugInfo),
                    "annotation_items": map_size(d, rd_dex::MapType::Annotation),
                    "map_types": d.map_list.len(),
                    "call_sites": d.call_site_offsets.len(),
                    "method_handles": d.method_handles.len(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source": path.display().to_string(),
                "dex_files": items,
            }))
            .map_err(|e| RdError::new("INTERNAL", e.to_string(), "rd-cli"))?
        );
        return Ok(());
    }
    println!("RustyDroid dex summary — {}", path.display());
    for (name, d) in &files {
        let mb = d.data.len() as f64 / (1024.0 * 1024.0);
        println!(
            "  {:<14} {:>8.2} MB · DEX {} · checksum {}",
            name,
            mb,
            d.header.version.as_str(),
            if d.header.checksum_ok {
                "ok"
            } else {
                "INVÁLIDO"
            }
        );
        println!(
            "    strings {:>6} · types {:>6} · protos {:>6} · fields {:>6} · methods {:>6} · classes {:>6}",
            d.header.string_ids_size,
            d.header.type_ids_size,
            d.header.proto_ids_size,
            d.header.field_ids_size,
            d.header.method_ids_size,
            d.header.class_defs_size
        );
        println!(
            "    código {:>6} code items · {:>6} debug · {:>6} annotations · map {}/18 tipos",
            map_size(d, rd_dex::MapType::Code),
            map_size(d, rd_dex::MapType::DebugInfo),
            map_size(d, rd_dex::MapType::Annotation),
            d.map_list.len()
        );
    }
    Ok(())
}

/// M2: executa um método static puro na VM Dalvik interpretada.
/// Exit codes: 0 = ok/exceção respondida; 4 = erro estruturado da VM.
#[allow(clippy::too_many_arguments)]
/// VmExit → RdError para o contrato {code, cause, module_id} do CLI.
fn vm_err_to_rd(e: rd_vm::VmExit) -> RdError {
    match e {
        rd_vm::VmExit::Error(rd) => RdError::new(rd.code, rd.cause, rd.module_id),
        rd_vm::VmExit::Exception(t) => RdError::new(
            "VM_EXCEPTION",
            format!(
                "exceção não capturada: {}: {}",
                t.name_without_l(),
                t.message.unwrap_or_default()
            ),
            "rd-vm",
        ),
    }
}

/// M3 (DoD): roda o app trivial headless — lifecycle + script do agente
/// ("tap X,Y", "wait MS", "dump"). O dump textual é a observabilidade do
/// agente até o UI dump rico do M4.
/// Boot comum dos `rd app *` (M4): o APK é aberto UMA vez — o mesmo `Apk`
/// fornece dex E Resources (layouts AXML + resources.arsc do LayoutInflater).
/// .dex solto segue válido (sem Resources: setContentView(I) responde
/// RESOURCES_MISSING). Com o APK aberto, o package é o do MANIFEST (fonte da
/// verdade — getPackageName correto mesmo com --activity explícito).
fn boot_app(
    path: &Path,
    activity: Option<&str>,
    exception_trace: bool,
) -> Result<(rd_vm::Engine, String, String), RdError> {
    let apk = open_apk(path).ok();
    let files = match &apk {
        Some(a) => dexes_of_apk(a)?,
        None => load_dex_files(path)?,
    };
    let dexes: Vec<Dex> = files.into_iter().map(|(_, d)| d).collect();
    let (activity_desc, package) = match activity {
        Some(a) => {
            let desc = normalize_activity_desc(a);
            let pkg = match &apk {
                Some(ap) => ap.manifest.package.clone(),
                None => package_of_desc(&desc),
            };
            (desc, pkg)
        }
        None => match &apk {
            Some(a) => launcher_of(a)?,
            None => launcher_of_apk(path)?,
        },
    };
    let mut eng = rd_vm::Engine::new(
        dexes,
        rd_vm::VmConfig {
            exception_trace,
            ..rd_vm::VmConfig::default()
        },
    );
    if let Some(a) = apk {
        // M8 (38º gap): classe de Application do manifest → o launch cria a
        // Application real do app antes da activity (getApplication).
        if let Some(name) = &a.manifest.application.name {
            let abs = if let Some(rel) = name.strip_prefix('.') {
                format!("{}{}", a.manifest.package, rel)
            } else {
                name.clone()
            };
            eng.set_application_class(format!("L{};", abs.replace('.', "/")));
        }
        eng.set_resources(a);
    }
    if let Err(e) = eng.launch_app(&activity_desc, &package, Vec::new()) {
        let mut rd = vm_err_to_rd(e);
        // Observabilidade (M8): com --trace, VM_EXCEPTION ganha postmortem —
        // rótulos dos corpos DEX executados (cobertura de entrada, AG-09).
        // Presença/ausência responde "esse método androidx rodou?" — o passo
        // seguinte ao call site revelado pelas linhas "at". Teto fixo para
        // manter o payload ordenável.
        if exception_trace && rd.code == "VM_EXCEPTION" {
            let labels = eng.executed_labels();
            if !labels.is_empty() {
                let mut post = format!(
                    "\n\npostmortem (--trace): {} métodos DEX executados neste Engine (cobertura de entrada, ordem alfabética):",
                    labels.len()
                );
                for l in labels.iter().take(256) {
                    post.push_str(&format!("\n  {l}"));
                }
                if labels.len() > 256 {
                    post.push_str(&format!("\n  ... (+{} mais)", labels.len() - 256));
                }
                rd.cause.push_str(&post);
            }
        }
        return Err(rd);
    }
    Ok((eng, activity_desc, package))
}

fn cmd_app_run(
    path: &Path,
    activity: Option<&str>,
    scripts: &[String],
    trace: bool,
    json: bool,
) -> Result<(), RdError> {
    let (mut eng, activity_desc, package) = boot_app(path, activity, trace)?;

    let mut dumps: Vec<String> = Vec::new();
    let mut xmls: Vec<String> = Vec::new();
    let mut shots: Vec<String> = Vec::new();
    for script in scripts {
        let parts: Vec<&str> = script.trim().splitn(2, ' ').collect();
        match (parts[0], parts.get(1)) {
            ("tap", rest) => {
                let (x, y) = rest.and_then(|r| r.split_once(',')).ok_or_else(|| {
                    RdError::new(
                        "INVALID_FORMAT",
                        format!("script tap inválido: {script:?}"),
                        "rd-cli",
                    )
                    .with_suggestion("use --script \"tap X,Y\" (px inteiros)")
                })?;
                let x: i32 = x.trim().parse().map_err(|_| {
                    RdError::new(
                        "INVALID_FORMAT",
                        format!("tap X inválido em {script:?}"),
                        "rd-cli",
                    )
                })?;
                let y: i32 = y.trim().parse().map_err(|_| {
                    RdError::new(
                        "INVALID_FORMAT",
                        format!("tap Y inválido em {script:?}"),
                        "rd-cli",
                    )
                })?;
                let hit = eng.touch_app(x, y).map_err(vm_err_to_rd)?;
                if !json {
                    println!("[tap {x},{y}] listener acionado: {hit}");
                }
            }
            ("wait", rest) => {
                let ms: u64 = rest.and_then(|r| r.trim().parse().ok()).ok_or_else(|| {
                    RdError::new(
                        "INVALID_FORMAT",
                        format!("script wait inválido: {script:?}"),
                        "rd-cli",
                    )
                    .with_suggestion("use --script \"wait 250\" (ms)")
                })?;
                let ran = eng.advance_clock(ms).map_err(vm_err_to_rd)?;
                if !json {
                    println!("[wait {ms}ms] runnables executados: {ran}");
                }
            }
            ("dump", _) => {
                let d = eng.dump_ui();
                dumps.push(d);
                if !json {
                    let d = dumps.last().unwrap();
                    print!("{d}");
                }
            }
            ("xml", _) => {
                // M4: dump uiautomator (get_ui_tree)
                let tree = eng.view_tree().ok_or_else(|| {
                    RdError::new("INVALID_FORMAT", "sem activity/window para dump", "rd-cli")
                })?;
                let xml = rd_render::uiautomator_xml(&tree, &package).map_err(render_err)?;
                if !json {
                    print!("{xml}");
                } else {
                    xmls.push(xml);
                }
            }
            ("shot", rest) => {
                // M4: screenshot PNG headless — "shot FILE.png"
                let file = rest
                    .map(|r| r.trim())
                    .filter(|r| !r.is_empty())
                    .ok_or_else(|| {
                        RdError::new(
                            "INVALID_FORMAT",
                            format!("script shot inválido: {script:?}"),
                            "rd-cli",
                        )
                        .with_suggestion("use --script \"shot tela.png\"")
                    })?;
                let file = file.strip_prefix("--out ").map(str::trim).unwrap_or(file);
                let tree = eng.view_tree().ok_or_else(|| {
                    RdError::new(
                        "INVALID_FORMAT",
                        "sem activity/window para screenshot",
                        "rd-cli",
                    )
                })?;
                let png = rd_render::render_snapshot(&tree).to_png();
                std::fs::write(file, &png).map_err(|e| {
                    RdError::new("IO_ERROR", format!("escrevendo {file}: {e}"), "rd-cli")
                })?;
                if !json {
                    println!("[shot {file}] {} bytes (720x1440)", png.len());
                }
                shots.push(file.to_string());
            }
            (other, _) => {
                return Err(RdError::new(
                    "INVALID_FORMAT",
                    format!("ação de script desconhecida: {other:?}"),
                    "rd-cli",
                )
                .with_suggestion("ações válidas: tap X,Y · wait MS · dump"));
            }
        }
    }
    if dumps.is_empty() {
        dumps.push(eng.dump_ui());
    }
    if json {
        println!(
            "{}",
            to_json(&serde_json::json!({
                "activity": activity_desc,
                "package": package,
                "finished": eng.activity_is_finished(),
                "clock_ms": eng.fw.clock(),
                "ui_tree": dumps.last().cloned().unwrap_or_default(),
                "ui_tree_xml": xmls.last().cloned().unwrap_or_default(),
                "shots": shots,
            }))
        );
    } else if let Some(d) = dumps.last() {
        print!("{d}");
    }
    Ok(())
}

/// "com.ex.Main" | "Lcom/ex/Main;" → "Lcom/ex/Main;"
fn normalize_activity_desc(a: &str) -> String {
    if a.starts_with('L') && a.ends_with(';') {
        return a.to_string();
    }
    format!("L{};", a.replace('.', "/"))
}

/// "Lcom/ex/Main;" → "com.ex"
fn package_of_desc(desc: &str) -> String {
    let inner = desc.trim_start_matches('L').trim_end_matches(';');
    inner
        .rfind('/')
        .map(|i| inner[..i].replace('/', "."))
        .unwrap_or_default()
}

/// Descobre a LAUNCHER activity + package pelo manifest do APK.
fn launcher_of_apk(path: &Path) -> Result<(String, String), RdError> {
    launcher_of(&open_apk(path)?)
}

/// M3.2: LAUNCHER a partir de um APK já aberto (mesma leitura única).
/// M4 DoD "get_ui_tree com ids certos": dump UI da activity corrente —
/// XML uiautomator (default) ou dump textual M3.
fn cmd_app_dump(
    path: &Path,
    activity: Option<&str>,
    text: bool,
    json: bool,
) -> Result<(), RdError> {
    let (eng, activity_desc, package) = boot_app(path, activity, false)?;
    if text {
        let d = eng.dump_ui();
        if json {
            println!(
                "{}",
                to_json(&serde_json::json!({
                    "activity": activity_desc,
                    "package": package,
                    "ui_tree": d,
                }))
            );
        } else {
            print!("{d}");
        }
        return Ok(());
    }
    let tree = eng
        .view_tree()
        .ok_or_else(|| RdError::new("INVALID_FORMAT", "sem activity/window para dump", "rd-cli"))?;
    let xml = rd_render::uiautomator_xml(&tree, &package).map_err(render_err)?;
    if json {
        println!(
            "{}",
            to_json(&serde_json::json!({
                "activity": activity_desc,
                "package": package,
                "ui_tree_xml": xml,
            }))
        );
    } else {
        print!("{xml}");
    }
    Ok(())
}

/// M4 DoD "screenshot correto": PNG headless 720×1440 da tela corrente —
/// software renderer determinístico do rd-render.
fn cmd_app_shot(path: &Path, activity: Option<&str>, out: &Path) -> Result<(), RdError> {
    let (eng, _activity, _package) = boot_app(path, activity, false)?;
    let tree = eng.view_tree().ok_or_else(|| {
        RdError::new(
            "INVALID_FORMAT",
            "sem activity/window para screenshot",
            "rd-cli",
        )
    })?;
    let png = rd_render::render_snapshot(&tree).to_png();
    std::fs::write(out, &png).map_err(|e| {
        RdError::new(
            "IO_ERROR",
            format!("escrevendo {}: {e}", out.display()),
            "rd-cli",
        )
    })?;
    println!(
        "screenshot: {} ({} bytes, 720x1440, determinístico)",
        out.display(),
        png.len()
    );
    Ok(())
}

fn launcher_of(apk: &Apk) -> Result<(String, String), RdError> {
    let pkg = apk.manifest.package.clone();
    let app = &apk.manifest.application;
    let launcher = app
        .components
        .iter()
        .find(|c| matches!(c.kind, rd_apk::manifest::ComponentKind::Activity) && c.is_launcher)
        .ok_or_else(|| {
            RdError::missing_entry("activity LAUNCHER no manifest")
                .with_suggestion("passe --activity Lpkg/Cls; explicitamente")
        })?;
    let cls = launcher.class_name.clone();
    Ok((format!("L{};", cls.replace('.', "/")), pkg))
}

#[allow(clippy::too_many_arguments)]
fn cmd_vm_exec(
    path: &Path,
    class: &str,
    method: &str,
    sig: &str,
    args: Option<&str>,
    json: bool,
    fuel: Option<u64>,
    depth: Option<usize>,
    heap_mb: Option<usize>,
) -> Result<i32, RdError> {
    let files = load_dex_files(path)?;
    let dexes: Vec<Dex> = files.into_iter().map(|(_, d)| d).collect();
    // issue #37: heap-mb N*1024*1024 podia overflow (debug panic) — checked
    let heap_budget = heap_mb
        .unwrap_or(256)
        .checked_mul(1024 * 1024)
        .ok_or_else(|| {
            RdError::invalid_format(format!(
                "--heap-mb {} excede o limite",
                heap_mb.unwrap_or(256)
            ))
        })?;
    let cfg = rd_vm::VmConfig {
        fuel: fuel.unwrap_or(rd_vm::VmConfig::default().fuel),
        max_depth: depth.unwrap_or(rd_vm::VmConfig::default().max_depth),
        heap_budget,
        // default OFF — `rd vm exec` é usado pelo harness golden (saídas
        // byte-idênticas); trace opt-in existe em `rd app run --trace`.
        ..rd_vm::VmConfig::default()
    };
    let mut eng = rd_vm::Engine::new(dexes, cfg);

    let param_types = rd_vm::engine::parse_param_types(sig)
        .ok_or_else(|| RdError::invalid_format(format!("assinatura malformada: {sig}")))?;
    let mut values: Vec<rd_vm::Value> = Vec::new();
    if let Some(raw) = args {
        let lits: Vec<&str> = split_args(raw);
        if lits.len() != param_types.len() {
            return Err(RdError::invalid_format(format!(
                "{sig} pede {} argumentos, recebi {}",
                param_types.len(),
                lits.len()
            ))
            .with_suggestion("passe --args 'a, b' com um literal por parâmetro"));
        }
        for (lit, t) in lits.iter().zip(&param_types) {
            values.push(rd_vm::parse_arg_value(lit, t).map_err(RdError::invalid_format)?);
        }
    } else if !param_types.is_empty() {
        return Err(RdError::invalid_format(format!(
            "{sig} pede {} argumentos — passe --args",
            param_types.len()
        )));
    }

    let ret_desc = sig.rsplit(')').next().unwrap_or("V").to_string();
    match eng.invoke_static(class, method, sig, &values) {
        Ok(v) => {
            // String do heap vira conteúdo; outros objetos continuam opacos
            let string_content = match &v {
                rd_vm::Value::Obj(r) => eng.heap.as_str(*r).ok().map(str::to_string),
                _ => None,
            };
            if json {
                println!("{}", result_json(&v, &ret_desc, string_content.as_deref()));
            } else if let Some(s) = string_content {
                println!("{s}");
            } else {
                match &v {
                    rd_vm::Value::Null => println!("null"),
                    other => println!("{}", rd_vm::interp::render_result(other, &ret_desc)),
                }
            }
            Ok(0)
        }
        Err(rd_vm::VmExit::Exception(t)) => {
            let name = t.name_without_l();
            let msg = t.message.clone().unwrap_or_default();
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "exception",
                        "class": name,
                        "message": msg,
                    })
                );
            } else {
                eprintln!("exception: {name}: {msg}");
            }
            Ok(0)
        }
        Err(rd_vm::VmExit::Error(e)) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "error",
                        "code": e.code,
                        "cause": e.cause,
                        "suggestion": e.suggestion,
                        "module_id": e.module_id,
                    })
                );
            } else {
                eprintln!("{e}");
            }
            Ok(4)
        }
    }
}

/// Separa literais por vírgula respeitando aspas simples/duplas.
fn split_args(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = raw[i..].chars().next().unwrap();
        let clen = c.len_utf8();
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                ',' => {
                    if let Some(s) = start.take() {
                        out.push(raw[s..i].trim());
                    }
                }
                _ => {}
            },
        }
        if start.is_none() && !c.is_whitespace() && c != ',' {
            start = Some(i);
        }
        i += clen;
    }
    if let Some(s) = start {
        out.push(raw[s..].trim());
    }
    out
}

/// JSON do resultado no contrato do M2 (float/double no estilo Java;
/// String do heap vira conteúdo).
fn result_json(v: &rd_vm::Value, ret_desc: &str, string_content: Option<&str>) -> String {
    let (ty, value): (&str, serde_json::Value) = match v {
        rd_vm::Value::Int(i) => ("int", serde_json::json!(i.to_string())),
        rd_vm::Value::Long(l) => ("long", serde_json::json!(l.to_string())),
        rd_vm::Value::Float(f) => ("float", serde_json::json!(rd_vm::repr::java_float(*f))),
        rd_vm::Value::Double(d) => ("double", serde_json::json!(rd_vm::repr::java_double(*d))),
        rd_vm::Value::Obj(_) => match string_content {
            Some(s) => ("string", serde_json::json!(s)),
            None => ("object", serde_json::json!("<object>")),
        },
        rd_vm::Value::Null => ("null", serde_json::Value::Null),
        rd_vm::Value::WideHi => ("wide-hi", serde_json::json!("<wide-hi>")),
        rd_vm::Value::StrPlaceholder(_) => ("string", serde_json::json!("<string>")),
    };
    let _ = ret_desc;
    serde_json::json!({
        "status": "ok",
        "result": { "type": ty, "value": value },
    })
    .to_string()
}

fn cmd_dex_disasm(
    path: &Path,
    class: Option<&str>,
    method: Option<&str>,
    out: Option<&Path>,
    json: bool,
) -> Result<(), RdError> {
    // issue #34: --method sem --class era silenciosamente ignorado — agora
    // erro tipado com sugestão (mesmo padrão de behavior --target 99)
    if method.is_some() && class.is_none() {
        return Err(RdError::invalid_format(
            "--method requer --class (use --class Lpkg/Cls; --method nome)",
        )
        .with_suggestion("passe --class para filtrar o método"));
    }
    let files = load_dex_files(path)?;
    if let Some(desc) = class {
        // disassembly de UMA classe (primeiro dex que a contém)
        for (name, d) in &files {
            let Some(def) = d.find_class(desc) else {
                continue;
            };
            let text = if let Some(m) = method {
                disasm::render_method(d, def.index, m).map_err(dex_err)?
            } else {
                disasm::render_class(d, def.index).map_err(dex_err)?
            };
            if let Some(dir) = out {
                let file = dir.join(disasm::smali_file_name(desc));
                // o path mapping cria subdiretórios (ex.: org/fdroid/fdroid/…)
                // — criar o pai do arquivo, não só a raiz de --out
                // issue #63: E/S nascida no rd-cli (path de SAÍDA escolhido pelo
                // usuário) — module_id rd-cli, não o rd-apk herdado de RdError::io
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        RdError::new(
                            "IO_ERROR",
                            format!("criando diretórios de --out: {e}"),
                            "rd-cli",
                        )
                    })?;
                }
                std::fs::write(&file, &text).map_err(|e| {
                    RdError::new("IO_ERROR", format!("escrevendo smali: {e}"), "rd-cli")
                })?;
                println!("{} -> {} ({} bytes)", desc, file.display(), text.len());
            } else if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "entry": name,
                        "class": desc,
                        "method": method,
                        "smali": text,
                    })
                );
            } else {
                print!("{text}");
            }
            return Ok(());
        }
        return Err(RdError::missing_entry(format!("classe {desc:?}"))
            .with_suggestion("liste as classes com `rd dex disasm <apk>` (sem --class)"));
    }

    // sem --class: lista classes (ou despeja tudo com --out)
    if let Some(dir) = out {
        let mut written = 0usize;
        let mut skipped = 0usize;
        for (_, d) in &files {
            for def in &d.class_defs {
                let desc = d.type_str(def.class_idx);
                // resiliente: classe com constructo problemático é pulada com
                // contagem — o despejo continua (mesma política do baksmali).
                let Ok(text) = disasm::render_class(d, def.index) else {
                    skipped += 1;
                    continue;
                };
                let file = dir.join(disasm::smali_file_name(desc));
                if let Some(parent) = file.parent() {
                    if std::fs::create_dir_all(parent).is_err() {
                        skipped += 1;
                        continue;
                    }
                }
                if std::fs::write(&file, &text).is_err() {
                    skipped += 1;
                    continue;
                }
                written += 1;
            }
        }
        println!(
            "{written} arquivos .smali em {} (pulos: {skipped})",
            dir.display()
        );
        return Ok(());
    }
    let total: usize = files.iter().map(|(_, d)| d.class_defs.len()).sum();
    if json {
        let items: Vec<serde_json::Value> = files
            .iter()
            .map(|(name, d)| {
                serde_json::json!({
                    "entry": name,
                    "classes": d.class_defs.iter().map(|c| d.type_str(c.class_idx)).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source": path.display().to_string(),
                "total": total,
                "dex_files": items,
            }))
            .map_err(|e| RdError::new("INTERNAL", e.to_string(), "rd-cli"))?
        );
        return Ok(());
    }
    println!(
        "classes em {} — {total} no total (use --class <L...;> para desmontar)\n",
        path.display()
    );
    for (name, d) in &files {
        println!("  {name} ({} classes):", d.class_defs.len());
        for def in d.class_defs.iter().take(20) {
            let desc = d.type_str(def.class_idx);
            let flags =
                rd_dex::fields::format_flags(def.access_flags, rd_dex::fields::CLASS_FLAG_ORDER);
            println!("    {:<64} {}", desc, flags);
        }
        if d.class_defs.len() > 20 {
            println!("    … +{} classes", d.class_defs.len() - 20);
        }
    }
    Ok(())
}

/// M6 — `rd test <suite.json>`: roda os cenários e emite relatório.
/// Suíte inválida ou arquivo ilegível → exit 64, antes de executar qualquer passo.
fn cmd_test(suite_path: &Path, junit: Option<&Path>, json: bool) -> i32 {
    let text = match std::fs::read_to_string(suite_path) {
        Ok(t) => t,
        Err(e) => {
            let err = RdError::new(
                "IO_ERROR",
                format!("lendo {}: {e}", suite_path.display()),
                "rd-cli",
            );
            let _ = writeln!(std::io::stderr(), "{}", to_json(&err));
            return 64;
        }
    };
    let suite = match rd_agent::suite::parse_suite(&text) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{}", to_json(&e));
            return 64;
        }
    };
    // caminhos relativos (install.apk) são resolvidos contra a pasta da suíte
    let base = suite_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let report = rd_agent::suite::run_suite(&suite, base);

    if json {
        let summary = serde_json::json!({
            "suite": report.name,
            "scenarios": report.scenarios.len(),
            "passed": report.passed_count(),
            "failed": report.failed_count(),
            "elapsed_ms": report.elapsed_ms,
            "results": report.scenarios.iter().map(|s| serde_json::json!({
                "name": s.name,
                "passed": s.passed,
                "elapsed_ms": s.elapsed_ms,
                "failure": s.failure,
            })).collect::<Vec<_>>(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        print!("{}", rd_agent::suite::to_human(&report));
    }

    if let Some(path) = junit {
        if let Err(e) = std::fs::write(path, rd_agent::suite::to_junit_xml(&report)) {
            let err = RdError::new(
                "IO_ERROR",
                format!("escrevendo JUnit {}: {e}", path.display()),
                "rd-cli",
            );
            let _ = writeln!(std::io::stderr(), "{}", to_json(&err));
            return 1;
        }
    }
    report.exit_code()
}
