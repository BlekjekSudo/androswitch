//! Servidor MCP v1 do RustyDroid (M5 DoD: "LLM instala, navega e assera
//! estado só com as tools").
//!
//! Envelope: JSON-RPC 2.0 newline-delimited sobre stdio — o transporte
//! stdio do MCP É isso (ADR 001: rmcp exige MSRV 1.88 > workspace 1.76;
//! envelope próprio, protocolo idêntico na fio). Métodos:
//! `initialize` · `notifications/initialized` · `tools/list` · `tools/call`
//! · `ping`.
//!
//! Sessão: UM app por processo (single-thread determinístico — decisão M3;
//! o MCP server é um loop bloqueante de leitura). A saída do app
//! (System.out) vai para um buffer da SESSÃO (tool `logs`) — nunca para o
//! stdout do processo, que é do protocolo (mesmo contrato da issue #55).
//!
//! Erros: tools falham com o contrato {code, cause, suggestion, module_id}
//! serializado no conteúdo do resultado com isError=true (convenção MCP);
//! erros de PROTOCOLO (método desconhecido, JSON inválido) são erros
//! JSON-RPC (-32601/-32700). Lei 1: nunca pânico, nunca silêncio.

use std::io::BufRead;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use rd_apk::manifest::ComponentKind;
use rd_apk::{Apk, RdError};
use rd_framework::fd_behavior::SwitchTable;
use rd_framework::fd_devices::DeviceTable;
use rd_framework::fd_permissions::{GrantState, PermissionEngine, PermissionTable};
use rd_vm::framework::UiNode;
use rd_vm::{Engine, HostOut, Value, VmConfig, VmExit};

pub const MODULE_ID: &str = "rd-agent";
/// ADR 009 cold-start phase telemetry: `RD_PHASES=1` prints
/// `phase <name> <ms>` lines to stderr. Zero work when off beyond one
/// `Instant::now()` per phase boundary.
pub fn phases_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("RD_PHASES")
            .map(|v| v == "1" || v == "on")
            .unwrap_or(false)
    })
}

pub fn phase(name: &str, t0: std::time::Instant) {
    if phases_on() {
        eprintln!("phase {name} {:.3}", t0.elapsed().as_secs_f64() * 1000.0);
    }
}

/// Versão do protocolo MCP anunciada no initialize (shape 2024-11-05).
pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "rustydroid-agent";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ── resultados de tool (conteúdo MCP) ───────────────────────────────────────

/// Conteúdo de um resultado de tool (MCP: text | image).
#[derive(Debug, Clone)]
pub enum ToolContent {
    Text(String),
    Image {
        /// PNG codificado em base64 (encoder próprio — zero deps).
        data_b64: String,
        mime: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    pub is_error: bool,
}

impl ToolResult {
    fn text(s: impl Into<String>) -> Self {
        ToolResult {
            content: vec![ToolContent::Text(s.into())],
            is_error: false,
        }
    }
    fn json(v: &serde_json::Value) -> Self {
        ToolResult {
            content: vec![ToolContent::Text(
                serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
            )],
            is_error: false,
        }
    }
    fn error(e: &RdError) -> Self {
        let mut v = serde_json::json!({
            "code": e.code,
            "cause": e.cause,
            "module_id": e.module_id,
        });
        if let Some(s) = &e.suggestion {
            v["suggestion"] = serde_json::json!(s);
        }
        ToolResult {
            content: vec![ToolContent::Text(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| e.to_string()),
            )],
            is_error: true,
        }
    }
}

// ── política de segurança do agente (M5: controles do enunciado) ─────────

/// Controles contra: caminhos fora do sandbox, APKs excessivos, URLs não
/// autorizadas, tempo ilimitado e lotes gigantes. Tudo com override por
/// env (documentado nas tools) e SEMPRE erro tipado ao violar — nunca
/// pânico, nunca silêncio (Lei 1).
#[derive(Debug, Clone)]
pub struct Policy {
    /// Raízes permitidas para E/S de arquivos (install/screenshot).
    /// Env `RD_MCP_SANDBOX` (separadas por ":"); default: cwd + temp dir.
    pub roots: Vec<PathBuf>,
    /// Teto do APK em bytes (default 256 MB; env `RD_MCP_MAX_APK_MB`).
    pub max_apk_bytes: u64,
    /// Teto de wait/swipe/timeout em ms (default 60_000; env
    /// `RD_MCP_MAX_WAIT_MS`) — contra tempo ilimitado.
    pub max_wait_ms: u64,
    /// Teto de passos num `batch` (default 64; env `RD_MCP_MAX_BATCH`).
    pub max_batch_steps: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            roots: vec![
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                std::env::temp_dir(),
            ],
            max_apk_bytes: 256 * 1024 * 1024,
            max_wait_ms: 60_000,
            max_batch_steps: 64,
        }
    }
}

impl Policy {
    /// Política a partir do ambiente (o processo `rd mcp` é o limite de
    /// confiança — env vem do operador, nunca do cliente MCP).
    pub fn from_env() -> Self {
        let mut p = Policy::default();
        if let Ok(s) = std::env::var("RD_MCP_SANDBOX") {
            let roots: Vec<PathBuf> = s
                .split(':')
                .filter(|t| !t.trim().is_empty())
                .map(PathBuf::from)
                .collect();
            if !roots.is_empty() {
                p.roots = roots;
            }
        }
        if let Ok(s) = std::env::var("RD_MCP_MAX_APK_MB") {
            if let Ok(mb) = s.parse::<u64>() {
                if mb > 0 {
                    p.max_apk_bytes = mb.saturating_mul(1024 * 1024);
                }
            }
        }
        if let Ok(s) = std::env::var("RD_MCP_MAX_WAIT_MS") {
            if let Ok(ms) = s.parse::<u64>() {
                if ms > 0 {
                    p.max_wait_ms = ms;
                }
            }
        }
        if let Ok(s) = std::env::var("RD_MCP_MAX_BATCH") {
            if let Ok(n) = s.parse::<usize>() {
                if n > 0 {
                    p.max_batch_steps = n;
                }
            }
        }
        p
    }

    /// Raízes canonizadas (uma vez, na criação da sessão).
    fn canonical_roots(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|r| r.canonicalize().unwrap_or_else(|_| absolutize(r)))
            .collect()
    }

    /// Valida um caminho de ARQUIVO EXISTENTE (install): sem URL, sem `..`,
    /// dentro do sandbox. Devolve o caminho canonizado.
    fn check_read_path(&self, raw: &str, roots: &[PathBuf]) -> Result<PathBuf, RdError> {
        let p = self.check_path_common(raw)?;
        let canon = p.canonicalize().map_err(|e| {
            agent_error(
                "IO_ERROR",
                format!("{raw} não acessível: {e}"),
                Some("use um caminho absoluto dentro do sandbox e existente"),
            )
        })?;
        Self::ensure_inside(canon, raw, roots)
    }

    /// Valida um caminho de ESCRITA (screenshot): o PAI precisa existir
    /// (canonizável) e o destino ficar dentro do sandbox.
    fn check_write_path(&self, raw: &str, roots: &[PathBuf]) -> Result<PathBuf, RdError> {
        let p = self.check_path_common(raw)?;
        let parent = p.parent().ok_or_else(|| {
            agent_error("INVALID_FORMAT", format!("{raw} sem diretório pai"), None)
        })?;
        let canon_parent = parent.canonicalize().map_err(|e| {
            agent_error(
                "IO_ERROR",
                format!("diretório pai de {raw} não acessível: {e}"),
                Some("crie o diretório antes (o agente não cria diretórios)"),
            )
        })?;
        Self::ensure_inside(canon_parent.clone(), raw, roots)?;
        let out = canon_parent.join(p.file_name().unwrap_or_default());
        // issue #67: o último componente NÃO pode ser symlink. Canonizar só o
        // pai deixava o arquivo do outro lado do link ser sobrescrito — e a
        // resposta informava o caminho do link (dentro do sandbox), escondendo
        // a escrita real.
        if let Ok(md) = out.symlink_metadata() {
            if md.file_type().is_symlink() {
                return Err(agent_error(
                    "SYMLINK_NOT_ALLOWED",
                    format!(
                        "{} é um symlink: escrita recusada (o alvo do link pode estar fora do sandbox)",
                        out.display()
                    ),
                    Some("remova o link e use um caminho de arquivo real dentro do sandbox"),
                ));
            }
            // arquivo/diretório existente: canonizar o próprio destino e
            // reconferir o sandbox (defesa extra sobre o pai já canonizado)
            let canon = out.canonicalize().unwrap_or_else(|_| out.clone());
            Self::ensure_inside(canon, raw, roots)?;
        }
        Ok(out)
    }

    /// Checagens comuns: não-vazio, sem esquema de URL (URLs não
    /// autorizadas — o transporte v1 não baixa nada), sem traversal `..`.
    fn check_path_common(&self, raw: &str) -> Result<PathBuf, RdError> {
        if raw.trim().is_empty() {
            return Err(agent_error("INVALID_FORMAT", "caminho vazio", None));
        }
        if raw.contains("://") {
            return Err(agent_error(
                "URL_NOT_ALLOWED",
                format!("URL {raw:?} não autorizada — o transporte v1 (stdio) não baixa arquivos"),
                Some("baixe o arquivo manualmente e passe o caminho local dentro do sandbox"),
            ));
        }
        let p = PathBuf::from(raw);
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(agent_error(
                "PATH_OUTSIDE_SANDBOX",
                format!("{raw:?} contém `..` (traversal proibido)"),
                Some("use um caminho absoluto sem `..`"),
            ));
        }
        Ok(absolutize(&p))
    }

    fn ensure_inside(canon: PathBuf, raw: &str, roots: &[PathBuf]) -> Result<PathBuf, RdError> {
        if roots.iter().any(|r| canon.starts_with(r)) {
            return Ok(canon);
        }
        let list = roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(":");
        Err(agent_error(
            "PATH_OUTSIDE_SANDBOX",
            format!("{raw} fora do sandbox (raízes permitidas: {list})"),
            Some(
                "mova o arquivo para dentro do sandbox ou ajuste RD_MCP_SANDBOX no processo do servidor",
            ),
        ))
    }
}

/// issue #67: escreve SEM seguir symlink no último componente.
///
/// A checagem em `check_write_path` acontece ANTES da escrita; entre as duas,
/// o caminho validado pode ser trocado por um link (TOCTOU). `O_EXCL` na
/// criação e `O_NOFOLLOW` na reabertura fecham a janela: o arquivo escrito é
/// sempre o arquivo validado, dentro do sandbox.
fn write_no_follow(path: &Path, data: &[u8]) -> Result<(), RdError> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).truncate(true);
    // O_NOFOLLOW: 0x20000 (Linux/Android) · 0x100 (Darwin). Demais unixes
    // ficam só com O_EXCL (POSIX: `open(O_CREAT|O_EXCL)` nunca segue link).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(0x2_0000);
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(0x0_0100);
    }
    let io_err = |e: std::io::Error| {
        RdError::new(
            "IO_ERROR",
            format!("escrevendo {}: {e}", path.display()),
            MODULE_ID,
        )
    };
    match opts.create_new(true).open(path) {
        Ok(mut f) => f.write_all(data).map_err(io_err),
        // já existe (arquivo real validado em check_write_path): truncar
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => opts
            .create_new(false)
            .open(path)
            .and_then(|mut f| f.write_all(data))
            .map_err(io_err),
        Err(e) => Err(io_err(e)),
    }
}

/// Absolutiza sem exigir existência (canonicalize falha para arquivos novos).
fn absolutize(p: &PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p.clone();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(p)
}

// ── sessão do agente ────────────────────────────────────────────────────────

/// App instalado no REGISTRO da sessão (M5: a sessão guarda múltiplos
/// packages — package_list/remove/clear operam sobre este registro).
pub struct InstalledApp {
    pub package: String,
    pub version_name: Option<String>,
    pub version_code: Option<u64>,
    /// Manifest parsed at install (cached: the majority of tools only
    /// need this, never the whole container).
    pub manifest: rd_apk::Manifest,
    /// The fully opened APK. `None` only after an ADR 009 snapshot fast
    /// install; loaded on first real use via [`InstalledApp::apk`].
    pub apk: Option<std::sync::Arc<Apk>>,
    /// Where the APK was installed from (lazy load / re-fingerprint).
    pub source_path: Option<PathBuf>,
    /// ADR 009 content fingerprint, once known.
    pub snapshot_key: Option<String>,
    pub perms: PermissionEngine,
}

impl InstalledApp {
    /// The fully opened APK, loading it from `source_path` on first use
    /// (ADR 009 lazy materialization of a snapshot fast install).
    pub fn apk(&mut self) -> Result<std::sync::Arc<Apk>, RdError> {
        if let Some(a) = &self.apk {
            return Ok(a.clone());
        }
        let path = self.source_path.clone().ok_or_else(|| {
            agent_error(
                "NO_APP",
                format!(
                    "APK de {} sem caminho de origem para materializar",
                    self.package
                ),
                Some("reinstale o APK (tool `install`)"),
            )
        })?;
        let t0 = std::time::Instant::now();
        let bytes = std::fs::read(&path).map_err(|e| {
            RdError::new(
                "IO_ERROR",
                format!("lendo {}: {e}", path.display()),
                MODULE_ID,
            )
        })?;
        let apk =
            Apk::from_bytes(bytes).map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        phase("lazy_apk_load", t0);
        let arc = std::sync::Arc::new(apk);
        self.apk = Some(arc.clone());
        Ok(arc)
    }
}

/// Estado do "dispositivo" headless: registro de apps instalados, app
/// ativo, app EM EXECUÇÃO (engine), permissões, device profile. Uma sessão
/// por processo (ADR 001) — isolamento entre sessões é por processo, e o
/// `session_id` identifica esta.
pub struct AgentSession {
    /// Identificador estável da sessão — acompanha TODOS os resultados.
    pub session_id: String,
    policy: Policy,
    roots: Vec<PathBuf>,
    installed: Vec<InstalledApp>,
    /// package ativo (último install/launch bem-sucedido).
    active: Option<String>,
    engine: Option<Engine>,
    /// package do app EM EXECUÇÃO (o engine é de UM app por vez).
    engine_package: Option<String>,
    native_engine: Option<String>,
    activity: Option<String>,
    /// System.out do app (issue #55: buffer da sessão, tool `logs` lê).
    logs: Arc<Mutex<Vec<u8>>>,
    /// ADR 009: snapshot frame presented but engine not yet materialized.
    pending_snapshot: Option<PendingSnapshot>,
    devices: DeviceTable,
    behavior: SwitchTable,
    /// Round 6: cached screen model / PNG per frame version, resource
    /// subscriptions, pending notifications and the live viewer.
    screen: ScreenState,
    ui_epoch: u64,
    log_generation: u64,
    tool_error_count: u64,
    last_tool_error: Option<LastToolError>,
}

impl Default for AgentSession {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSession {
    pub fn new() -> Self {
        // Um processo = uma sessão: System.out do app vai para o buffer
        // (nunca ao stdout do protocolo — mesmo contrato da #55).
        let logs: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        rd_vm::set_app_stdout(HostOut::Buffer(logs.clone()));
        // Issue #230: sessão nova = dispositivo limpo. O VFS do hóspede é
        // thread_local no rd-vm; sem este reset, arquivos/SQLite/prefs de
        // uma sessão (ou cenário de suíte) anterior na mesma thread
        // vazavam para a próxima, quebrando o isolamento do M6.
        rd_vm::reset_vfs();
        let policy = Policy::from_env();
        let roots = policy.canonical_roots();
        let session_id = format!(
            "sess-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        AgentSession {
            session_id,
            policy,
            roots,
            installed: Vec::new(),
            active: None,
            engine: None,
            engine_package: None,
            native_engine: None,
            activity: None,
            logs,
            pending_snapshot: None,
            devices: DeviceTable::embedded().expect("devices.toml embutido parseia"),
            behavior: SwitchTable::embedded().expect("behavior-switches.toml embutido parseia"),
            screen: ScreenState::default(),
            ui_epoch: 0,
            log_generation: 0,
            tool_error_count: 0,
            last_tool_error: None,
        }
    }

    /// Ajusta a política pós-construção (testes e embedders — o server
    /// `rd mcp` usa from_env). Recalcula as raízes canonizadas.
    pub fn with_policy(mut self, p: Policy) -> Self {
        self.roots = p.canonical_roots();
        self.policy = p;
        self
    }

    /// App ativo do registro (erro tipado se nenhum).
    fn require_apk(&self) -> Result<&InstalledApp, RdError> {
        self.active
            .as_deref()
            .and_then(|p| self.find_installed(p))
            .ok_or_else(|| {
                agent_error(
                    "NO_APP",
                    "nenhum APK instalado nesta sessão",
                    Some("chame a tool `install` com o caminho do APK primeiro"),
                )
            })
    }

    fn find_installed(&self, package: &str) -> Option<&InstalledApp> {
        self.installed.iter().find(|a| a.package == package)
    }

    fn find_installed_mut(&mut self, package: &str) -> Option<&mut InstalledApp> {
        self.installed.iter_mut().find(|a| a.package == package)
    }

    fn require_engine(&mut self) -> Result<&mut Engine, RdError> {
        // ADR 009: first engine-requiring tool after a snapshot launch
        // runs the real cold launch now (disclosed via launch result
        // `materialized: false`).
        if self.engine.is_none() && self.pending_snapshot.is_some() {
            self.materialize_snapshot()?;
        }
        let running = self.engine.is_some();
        if running {
            Ok(self.engine.as_mut().expect("engine"))
        } else {
            Err(agent_error(
                "NOT_RUNNING",
                "app não está em execução",
                Some("chame a tool `launch` antes de interagir (tap/ui_tree/screenshot)"),
            ))
        }
    }

    fn current_activity(&self) -> Option<&str> {
        self.engine
            .as_ref()
            .and_then(|e| e.fw.current_activity.and_then(|r| e.heap.class_of(r).ok()))
            .or(self.activity.as_deref())
    }

    /// Mata o engine se o app em execução é `package`. Devolve se matou.
    /// (remove/clear/install do MESMO package: Android mata o app — nunca
    /// deixa estado inconsistente.)
    fn kill_engine_if_running(&mut self, package: &str) -> bool {
        if self.engine_package.as_deref() == Some(package) {
            self.engine = None;
            self.pending_snapshot = None;
            self.engine_package = None;
            self.native_engine = None;
            self.activity = None;
            // Issue #230: o heap morreu aqui — o estado host indexado por
            // ObjRef nas thread_locals do rd-vm morre junto, para o
            // próximo Engine (mesma thread) não herdar fantasmas.
            rd_vm::reset_host_thread_state();
            true
        } else {
            false
        }
    }
}

/// Erro do agente no contrato {code, cause, suggestion, module_id}.
fn agent_error(code: &str, cause: impl Into<String>, suggestion: Option<&str>) -> RdError {
    let mut e = RdError::new(code, cause, MODULE_ID);
    if let Some(s) = suggestion {
        e = e.with_suggestion(s);
    }
    e
}

/// Extrai os DEXs de um APK (mesma política do CLI `boot_app`).
fn dexes_of_apk(apk: &Apk) -> Result<Vec<rd_dex::Dex>, RdError> {
    // ADR 008 Stage 1b: mount DEX files in parallel (scoped threads, joins
    // in declaration order -> deterministic Vec<Dex>). Each file's inflate +
    // CRC + parse is independent; on multi-core hosts this turns the sum of
    // per-DEX costs into roughly the largest single DEX cost.
    let parse_one = |name: &String| -> Result<rd_dex::Dex, RdError> {
        let entry = apk
            .zip
            .find(name)
            .ok_or_else(|| RdError::missing_entry(format!("entrada zip {name:?}")))?;
        let t_entry = std::time::Instant::now();
        let bytes = apk
            .zip
            .read_entry(entry)
            .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        phase(&format!("dex_read_entry:{name}"), t_entry);
        let t_parse = std::time::Instant::now();
        // ADR 009: mount defere o adler32 do DEX (o CRC32 da entrada ZIP já
        // foi conferido em read_entry; `Dex::verify_checksum` fica sob demanda).
        let d = rd_dex::Dex::parse_unverified(bytes)
            .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        phase(&format!("dex_parse:{name}"), t_parse);
        Ok(d)
    };
    let out: Vec<rd_dex::Dex> = if apk.dex_files.len() > 1 {
        std::thread::scope(|scope| {
            let handles: Vec<_> = apk
                .dex_files
                .iter()
                .map(|name| scope.spawn(|| parse_one(name)))
                .collect();
            let mut out = Vec::with_capacity(handles.len());
            for h in handles {
                match h.join() {
                    Ok(r) => out.push(r?),
                    Err(_) => {
                        return Err(RdError::new(
                            "INTERNAL",
                            "dex mount worker panicked",
                            "rd-agent",
                        ));
                    }
                }
            }
            Ok(out)
        })?
    } else {
        let mut out = Vec::new();
        for name in &apk.dex_files {
            out.push(parse_one(name)?);
        }
        out
    };
    if out.is_empty() {
        return Err(RdError::missing_entry("classes*.dex no APK")
            .with_suggestion("confirme com `rd inspect` — o campo content lista os dex"));
    }
    Ok(out)
}

/// "com.ex.Main" | "Lcom/ex/Main;" → "Lcom/ex/Main;" (mesma normalização CLI).
fn normalize_activity_desc(a: &str) -> String {
    if a.starts_with('L') && a.ends_with(';') {
        return a.to_string();
    }
    format!("L{};", a.replace('.', "/"))
}

/// issue #109: resolve o componente de launch. Para `<activity>` a classe é a
/// própria; para `<activity-alias>` o Android lança a `targetActivity`. Retorna
/// o descritor Dalvik a passar para a VM e, quando via alias, o par
/// `(alias, alvo)` (FQCN) para o resultado/erros estruturados.
fn launch_desc_of_component(
    c: &rd_apk::manifest::Component,
) -> Result<(String, Option<(String, String)>), RdError> {
    if matches!(c.kind, ComponentKind::ActivityAlias) {
        let target = c.target_activity.clone().ok_or_else(|| {
            agent_error(
                "INVALID_FORMAT",
                format!(
                    "activity-alias {} declarada sem android:targetActivity no manifest",
                    c.class_name
                ),
                Some("todo <activity-alias> precisa de targetActivity apontando uma <activity> declarada"),
            )
        })?;
        Ok((
            normalize_activity_desc(&target),
            Some((c.class_name.clone(), target)),
        ))
    } else {
        Ok((normalize_activity_desc(&c.class_name), None))
    }
}

// ── as 30 tools do M5 ───────────────────────────────────────────────────────

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: serde_json::Value,
}

fn obj(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

/// Registro das tools (30 do M5 + AG-09: get_api_usage estático e
/// get_coverage dinâmico; pacote list/rem/clear, input tap/swipe/type/key,
/// observação, wait/wait_for, asserts, permissões, estado/saúde, lacunas de
/// compat., batch — spec PARTE 4).
pub fn tool_defs() -> Vec<ToolDef> {
    let mut defs = vec![
        ToolDef {
            name: "install",
            description:
                "Instala um APK na sessão (abre, valida o piso API 26 e prepara o estado de permissões). Caminho PRECISA estar dentro do sandbox (RD_MCP_SANDBOX); URL é recusada; APK acima de RD_MCP_MAX_APK_MB (default 256) é recusado. Reinstalar o package EM EXECUÇÃO o encerra (sem estado inconsistente). Não executa o app.",
            schema: obj(
                serde_json::json!({ "apk": { "type": "string", "description": "caminho do arquivo .apk dentro do sandbox" } }),
                &["apk"],
            ),
        },
        ToolDef {
            name: "launch",
            description:
                "Executa o app instalado: cria a activity (LAUNCHER ou `activity`) e roda o lifecycle até o onCreate completar. `activity` precisa estar declarada como <activity>/<activity-alias> no AndroidManifest.xml E existir no DEX (issues #82/#83) — o Android recusa o que não está declarado; um <activity-alias> lança a sua targetActivity (issue #109). Se o MESMO package já está em execução, o relançamento reutiliza o processo (warm start: sem re-parse do DEX; o resultado traz warm=true) (M7). Com `package` lança outro app do registro (multi-app); sem, o app ativo (último install/launch).",
            schema: obj(
                serde_json::json!({
                    "engine": {"type":"string","enum":["c-ndk","cpp-ndk","rust-ndk"],"description":"Explicit bounded native adapter; requires native-engines"},
                    "activity": { "type": "string", "description": "classe da activity (com.ex.Main ou Lcom/ex/Main;). Default: LAUNCHER" },
                    "package": { "type": "string", "description": "package do registro a lançar. Default: app ativo" },
                }),
                &[],
            ),
        },
        ToolDef {
            name: "tap",
            description: "Toque simulado nas coordenadas (px do viewport 720×1440). Retorna se algum listener foi acionado.",
            schema: obj(
                serde_json::json!({
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                }),
                &["x", "y"],
            ),
        },
        ToolDef {
            name: "swipe",
            description:
                "Gesto de arraste determinístico (x1,y1)→(x2,y2) em `ms` (cap RD_MCP_MAX_WAIT_MS). Reporta as views atravessadas em ordem; down+up na MESMA view vira clique (listener disparado) — semântica Android tap vs scroll.",
            schema: obj(
                serde_json::json!({
                    "x1": { "type": "integer" }, "y1": { "type": "integer" },
                    "x2": { "type": "integer" }, "y2": { "type": "integer" },
                    "ms": { "type": "integer", "description": "duração do gesto (default 300)" },
                }),
                &["x1", "y1", "x2", "y2"],
            ),
        },
        ToolDef {
            name: "type_text",
            description:
                "Digita texto: na EditText focada/primeira visível; sem EditText (Jetpack Compose, editores custom) usa o caminho do IME da plataforma — a view focada cria o InputConnection (onCreateInputConnection) e recebe commitText; toque no campo antes. `replace=true` substitui; default anexa. Devolve o texto final lido do editor.",
            schema: obj(
                serde_json::json!({
                    "text": { "type": "string" },
                    "replace": { "type": "boolean" },
                }),
                &["text"],
            ),
        },
        ToolDef {
            name: "key",
            description:
                "Evento de tecla: BACK (finaliza a activity corrente), DEL (apaga último caractere da EditText), ENTER (no editor do IME — Compose — executa a IME action do EditorInfo via performEditorAction; senão entregue sem efeito). Outras teclas: erro tipado NOT_IMPLEMENTED com a lista suportada.",
            schema: obj(
                serde_json::json!({ "key": { "type": "string", "enum": ["BACK", "ENTER", "DEL"] } }),
                &["key"],
            ),
        },
        ToolDef {
            name: "wait",
            description:
                "Avança o clock virtual (ms) e executa os runnables do Looper com deadline vencida. Retorna quantos rodaram. Cap RD_MCP_MAX_WAIT_MS (default 60_000) — contra tempo ilimitado.",
            schema: obj(
                serde_json::json!({ "ms": { "type": "integer", "minimum": 0 } }),
                &["ms"],
            ),
        },
        ToolDef {
            name: "wait_for",
            description:
                "Espera CONDICIONAL: avança o clock virtual em passos de `step_ms` (default 100) até a view (resource_id/text/class) existir ou `timeout_ms` vencer (cap RD_MCP_MAX_WAIT_MS). `class` casa o nome completo ou o nome simples exato (issue #78). found=false é resposta honesta (não é erro) — o agente decide.",
            schema: obj(
                serde_json::json!({
                    "resource_id": { "type": "string" },
                    "text": { "type": "string" },
                    "class": { "type": "string" },
                    "timeout_ms": { "type": "integer" },
                    "step_ms": { "type": "integer", "description": "granularidade do poll (default 100)" },
                }),
                &["timeout_ms"],
            ),
        },
        ToolDef {
            name: "ui_tree",
            description:
                "Dump da UI corrente: `xml` (uiautomator, formato dos seletores) ou `text` (árvore textual). Views GONE não aparecem.",
            schema: obj(
                serde_json::json!({ "format": { "type": "string", "enum": ["xml", "text"] } }),
                &[],
            ),
        },
        ToolDef {
            name: "screenshot",
            description:
                "Screenshot PNG determinístico 720×1440. Com `path` escreve o arquivo (DENTRO do sandbox — RD_MCP_SANDBOX; symlink no destino é recusado: SYMLINK_NOT_ALLOWED, issue #67); sem, devolve o PNG em base64 (conteúdo image).",
            schema: obj(
                serde_json::json!({ "path": { "type": "string", "description": "arquivo de saída (.png) dentro do sandbox" } }),
                &[],
            ),
        },
        ToolDef {
            name: "logs",
            description:
                "Saída do app (System.out/System.err) acumulada na sessão. `clear=true` limpa após ler.",
            schema: obj(
                serde_json::json!({ "clear": { "type": "boolean" } }),
                &[],
            ),
        },
        ToolDef {
            name: "invoke",
            description:
                "Invoca um método static no app em execução (sonda de estado geral — o 'eval' do agente). Exceções Java voltam tipadas.",
            schema: obj(
                serde_json::json!({
                    "class": { "type": "string", "description": "descritor (Lcom/x/Calc;) ou FQCN" },
                    "method": { "type": "string" },
                    "sig": { "type": "string", "description": "descritor completo (II)I" },
                    "args": { "type": "array", "description": "argumentos JSON (mapeados pelo tipo da assinatura)" },
                }),
                &["class", "method", "sig"],
            ),
        },
        ToolDef {
            name: "assert_view",
            description:
                "Asserção de existência: procura view por resource-id (ex.: com.app:id/btn), trecho de texto ou classe. `class` casa o nome COMPLETO (android.widget.TextView) ou o nome SIMPLES exato (TextView) — `View` NÃO casa TextView. Retorna matches.",
            schema: obj(
                serde_json::json!({
                    "resource_id": { "type": "string" },
                    "text": { "type": "string" },
                    "class": { "type": "string" },
                }),
                &[],
            ),
        },
        ToolDef {
            name: "assert_text",
            description: "Asserção de conteúdo: o texto da view com `resource_id` é igual a `expected` (ou contém, com contains=true).",
            schema: obj(
                serde_json::json!({
                    "resource_id": { "type": "string" },
                    "expected": { "type": "string" },
                    "contains": { "type": "boolean" },
                }),
                &["resource_id", "expected"],
            ),
        },
        ToolDef {
            name: "assert_activity",
            description: "Asserção: a activity corrente é `activity` (com.ex.Main ou Lcom/ex/Main;).",
            schema: obj(
                serde_json::json!({ "activity": { "type": "string" } }),
                &["activity"],
            ),
        },
        ToolDef {
            name: "assert_finished",
            description: "Asserção: a activity corrente terminou (finish() chamado).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "app_info",
            description: "Identidade do app ATIVO: package, versão, SDKs, floor e componentes do manifest. `engines` registra os 28 itens do Apêndice G e detecta assinaturas/candidatos por nomes ZIP/manifest; não valida payload nem promete execução. Features selecionam contratos, não runtimes.",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "package_list",
            description:
                "Lista os packages instalados NO REGISTRO da sessão (multi-app), marcando o ativo.",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "package_remove",
            description:
                "Desinstala um package do registro. Se estiver EM EXECUÇÃO, o app é encerrado antes (nunca estado inconsistente). Se era o ativo, o ativo vira o último remanescente.",
            schema: obj(
                serde_json::json!({ "package": { "type": "string" } }),
                &["package"],
            ),
        },
        ToolDef {
            name: "package_clear",
            description:
                "Limpa os DADOS de um package (Android clear data): permissões runtime voltam ao estado de instalação e o app em execução é encerrado. Permanece instalado.",
            schema: obj(
                serde_json::json!({ "package": { "type": "string" } }),
                &["package"],
            ),
        },
        ToolDef {
            name: "permissions_list",
            description: "Permissões declaradas pelo app com o estado atual (granted/denied/runtime-pending/special).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "permissions_grant",
            description: "Concede uma permissão (runtime/special) na sessão.",
            schema: obj(
                serde_json::json!({ "permission": { "type": "string", "description": "ex.: android.permission.CAMERA" } }),
                &["permission"],
            ),
        },
        ToolDef {
            name: "permissions_revoke",
            description: "Revoga uma permissão na sessão.",
            schema: obj(
                serde_json::json!({ "permission": { "type": "string" } }),
                &["permission"],
            ),
        },
        ToolDef {
            name: "permissions_audit",
            description: "Auditoria estática do manifest (exported sem guarda, debuggable, cleartext, permissões desconhecidas).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "device_profile",
            description: "Perfil de device corrente (piso contratual moto-e5, budgets e snapshot de periféricos virtuais M10) e controle determinístico AG-10 via `action` opcional (`get`, `location`, `sensor`, `battery`, `network`, `telephony`, `sms_receive`, `ble_advertisement`, `nfc_tag`, `camera_frame`, `biometric`, `audio_input`, `media_action`, `dark_mode`, `locale`, `font_scale`, `orientation`, `clipboard`).",
            schema: obj(device_profile_schema_props(), &[]),
        },
        ToolDef {
            name: "behavior_switches",
            description: "Comutadores de comportamento targetSdk 26→36 ativos para o app instalado (PRF-07).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "runtime_health",
            description:
                "Saúde do runtime: sessão, app em execução, activity, finished, clock virtual, heap, fuel, fila do Looper, bytes de log e tamanho do registro — sem exigir app (reporta o que existe). Inclui `gc` (gerações minor/major, chaves fracas, pausas), heap peak e `jit` quando compilado com a feature jit. `gc=true` força coleta major. Fuel é cumulativo; cada tool inicia uma janela de execução limitada nova.",
            schema: obj(serde_json::json!({ "gc": { "type": "boolean" } }), &[]),
        },
        ToolDef {
            name: "compat_gaps",
            description:
                "Catálogo HONESTO das lacunas de compatibilidade do runtime v1 (notificações M9, args String/array na sonda, foco, teclas, gestures, intent implícito, transporte HTTP) + sumário executável do Apêndice C (data/api-coverage.toml: domínios/status das superfícies android.*) — o agente consulta antes de planejar.",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "batch",
            description:
                "Executa uma lista de chamadas [{name, arguments}] EM ORDEM, uma vez cada, com resultados por passo. stop_on_error (default true) para no 1º erro; `resume_from` pula os N primeiros passos já executados — retomada idempotente. Cap RD_MCP_MAX_BATCH (default 64). batch não aninha. Validação de formato (name obrigatório/não vazio, sem batch aninhado, arguments objeto) rola sobre o lote INTEIRO antes de executar: passo inválido ⇒ NADA é executado (issue #80).",
            schema: obj(
                serde_json::json!({
                    "calls": { "type": "array", "items": { "type": "object" }, "description": "[{\"name\": \"tap\", \"arguments\": {…}}]" },
                    "stop_on_error": { "type": "boolean" },
                    "resume_from": { "type": "integer", "description": "passos já executados a pular (retomada)" },
                }),
                &["calls"],
            ),
        },
        ToolDef {
            name: "get_api_usage",
            description:
                "AG-09 (metade estática): quais APIs android.* o app instalado REFERENCIA nos DEX — tipos (method_ids/field_ids/superclass/interfaces), métodos por classe, contagem por pacote e classificação Apêndice C (`api_coverage`: domínio funcional/status/gaps via data/api-coverage.toml). É análise ESTÁTICA do DEX; para coverage de EXECUÇÃO use get_coverage. Com `package` consulta outro app instalado; sem, o app ativo.",
            schema: obj(
                serde_json::json!({
                    "package": { "type": "string", "description": "package instalado a analisar. Default: o app ativo" },
                }),
                &[],
            ),
        },
        ToolDef {
            name: "get_coverage",
            description:
                "AG-09 (metade dinâmica): cobertura de EXECUÇÃO do app em uso, nível de MÉTODO — instrumentação no interpretador registra cada corpo DEX executado. Requer app lançado (senão NOT_RUNNING); a contagem é do processo atual (reiniciar o app zera). Denominador: métodos com corpo nos DEX do app (abstract/native e classes host ficam fora).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef { name: "component_service", description: "M9: enable an APK-declared IME, accessibility, notification listener or widget provider. Runs its DEX lifecycle; returns the component object handle.", schema: obj(serde_json::json!({"class":{"type":"string"},"role":{"type":"string","enum":["ime","accessibility","notification_listener","widget"]}}), &["class","role"]) },
        ToolDef { name: "widgets", description: "M9: list/allocate/bind/delete widgets or click a RemoteViews PendingIntent. Provider is an object handle returned by component_service.", schema: obj(serde_json::json!({"action":{"type":"string","enum":["list","allocate","bind","delete","click"]},"id":{"type":"integer"},"host":{"type":"integer"},"provider":{"type":"integer","minimum":1},"view_id":{"type":"integer"}}), &[]) },
        ToolDef { name: "widget_screenshot", description: "M9: render the current RemoteViews widget through the normal deterministic PNG renderer.", schema: obj(serde_json::json!({"id":{"type":"integer"}}), &["id"]) },
        ToolDef { name: "ime", description: "M9: inspect/focus the editor, select UTF-16 range, commit/compose text, delete surrounding code units or finish composition.", schema: obj(serde_json::json!({"action":{"type":"string","enum":["state","focus","select","commit","compose","delete","finish"]},"view":{"type":"integer","minimum":1},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"text":{"type":"string"},"cursor":{"type":"integer"},"before":{"type":"integer","minimum":0},"after":{"type":"integer","minimum":0},"codepoints":{"type":"boolean"}}), &[]) },
        ToolDef { name: "accessibility", description: "M9: inspect visible accessibility nodes/events or perform a node action by object id. Action 16 click, 1 focus, 64 accessibility focus, 2097152 set text.", schema: obj(serde_json::json!({"action":{"type":"string","enum":["tree","events","perform"]},"node":{"type":"integer","minimum":1},"action_id":{"type":"integer"},"text":{"type":"string"}}), &[]) },
        ToolDef { name: "shortcuts", description: "M9: list current dynamic/pinned shortcuts or launch an enabled shortcut by id.", schema: obj(serde_json::json!({"action":{"type":"string","enum":["list","launch"]},"id":{"type":"string"}}), &[]) },
        ToolDef { name: "native_state", description: "M9: native feature availability, ABI, loaded ELF libraries and guest instruction/memory accounting. Never claims a library ran merely because it was loaded.", schema: obj(serde_json::json!({}), &[]) },
        ToolDef {
            name: "notifications",
            description:
                "M9: list active notifications/channels or click/cancel a notification (tag/id and optional action index). Uses the app NotificationManager state.",
            schema: obj(serde_json::json!({"action":{"type":"string","enum":["list","click","cancel"]},"tag":{"type":"string"},"id":{"type":"integer"},"action_index":{"type":"integer","minimum":0}}), &[]),
        },
        ToolDef {
            name: "observe",
            description:
                "See the screen WITHOUT a screenshot: semantic model of the current frame derived from the recorded display list (works for Views, Compose and custom canvases) merged with the host view tree. Returns visible texts with bounds, probable tap targets (label + center x,y; source frame|view), images and a frame `version` that only changes when visible content changes, plus a diff (+added/-removed texts) since the previous observe. format=text (default, compact, LLM-friendly) or json (structured; boxes=true adds drawn boxes). ~10-100x cheaper than screenshot (no rasterize/PNG). Also available as subscribable MCP resources rustydroid://screen and rustydroid://screen.txt.",
            schema: obj(
                serde_json::json!({
                    "format": { "type": "string", "enum": ["text", "json"], "description": "text (default) ou json" },
                    "diff": { "type": "boolean", "description": "incluir mudanças desde o observe anterior (default true)" },
                    "boxes": { "type": "boolean", "description": "json: incluir caixas desenhadas (default false)" },
                }),
                &[],
            ),
        },
        ToolDef {
            name: "tap_text",
            description:
                "Tap by visible label instead of coordinates: finds the tap target (or text) whose label matches `text` on the current frame (case/space-insensitive substring; exact=true for equality; nth picks among matches in reading order) and taps its center. Error VIEW_NOT_FOUND lists the visible texts. Works on Compose UIs (labels come from the drawn frame).",
            schema: obj(
                serde_json::json!({
                    "text": { "type": "string", "description": "rótulo visível (ex.: \"Continuar\")" },
                    "exact": { "type": "boolean", "description": "igualdade em vez de substring (default false)" },
                    "nth": { "type": "integer", "minimum": 0, "description": "índice entre os candidatos (default 0)" },
                }),
                &["text"],
            ),
        },
    ];
    defs.extend(ui_tool_defs());
    defs.extend(runtime_tool_defs());
    for d in defs.iter_mut() {
        if OBSERVE_AFTER_TOOLS.contains(&d.name) {
            if let Some(p) = d
                .schema
                .get_mut("properties")
                .and_then(|p| p.as_object_mut())
            {
                p.insert(
                    "observe".into(),
                    serde_json::json!({ "type": "boolean", "description": "anexar o resumo `observe` (texto) da tela após a ação (default false)" }),
                );
            }
        }
    }
    defs
}

impl AgentSession {
    /// Dispatch de uma tool (o envelope MCP chama isto). Erros de tool voltam
    /// como ToolResult::is_error com o contrato {code, cause, suggestion}.
    pub fn call_tool(&mut self, name: &str, args: &serde_json::Value) -> ToolResult {
        let diagnostic = matches!(name, "session_info" | "logs_tail" | "last_error");
        if diagnostic && !args.is_object() {
            return self.tool_error_result(
                name,
                &agent_error(
                    "INVALID_FORMAT",
                    "diagnostic arguments must be an object",
                    None,
                ),
            );
        }
        // Fuel limits one operation, not the lifetime of a healthy MCP session.
        // Total fuel remains observable; loops still exhaust a per-tool budget.
        if let Some(eng) = self.engine.as_mut().filter(|_| !diagnostic) {
            eng.reset_fuel_window();
        }
        let args = args.as_object().cloned().unwrap_or_default();
        if let Err(e) = validate_legacy_tool_arguments(name, &args) {
            return self.tool_error_result(name, &e);
        }
        let res: Result<ToolResult, RdError> = match name {
            "install" => self.tool_install(args.get("apk").and_then(|v| v.as_str()).unwrap_or("")),
            "launch" => {
                let engine = match args.get("engine") {
                    None => None,
                    Some(serde_json::Value::String(id)) => Some(id.as_str()),
                    _ => {
                        return self.tool_error_result(
                            name,
                            &agent_error("INVALID_FORMAT", "launch engine must be a string", None),
                        )
                    }
                };
                self.tool_launch(
                    args.get("activity").and_then(|v| v.as_str()),
                    args.get("package").and_then(|v| v.as_str()),
                    engine,
                )
            }
            "tap" => {
                let (Some(x), Some(y)) = (
                    args.get("x").and_then(|v| v.as_i64()),
                    args.get("y").and_then(|v| v.as_i64()),
                ) else {
                    return self.tool_error_result(
                        name,
                        &agent_error(
                            "INVALID_FORMAT",
                            "tap exige x,y inteiros",
                            Some("use {\"x\": <int>, \"y\": <int>} (px do viewport 720×1440)"),
                        ),
                    );
                };
                // issue #85: `as i32` truncava em silêncio (4294967396 virava
                // 100) e a resposta ecoava o valor truncado — o agente não
                // percebia que o toque não era o pedido.
                let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                    return self.tool_error_result(name, &agent_error(
                        "INVALID_FORMAT",
                        format!("tap: x/y fora da faixa de i32 (x={x}, y={y})"),
                        Some(
                            "use {\"x\": <int>, \"y\": <int>} dentro de -2147483648..=2147483647 (px do viewport 720×1440)",
                        ),
                    ));
                };
                self.tool_tap(x, y)
            }
            "swipe" => {
                let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    args.get("x1").and_then(|v| v.as_i64()),
                    args.get("y1").and_then(|v| v.as_i64()),
                    args.get("x2").and_then(|v| v.as_i64()),
                    args.get("y2").and_then(|v| v.as_i64()),
                ) else {
                    return self.tool_error_result(name, &agent_error(
                        "INVALID_FORMAT",
                        "swipe exige x1,y1,x2,y2 inteiros",
                        Some("use {\"x1\":…,\"y1\":…,\"x2\":…,\"y2\":…,\"ms\":…} (px do viewport)"),
                    ));
                };
                let ms = args.get("ms").and_then(|v| v.as_u64()).unwrap_or(300);
                // issue #85: sem truncamento silencioso (ver `tap`)
                let (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) = (
                    i32::try_from(x1),
                    i32::try_from(y1),
                    i32::try_from(x2),
                    i32::try_from(y2),
                ) else {
                    return self.tool_error_result(name, &agent_error(
                        "INVALID_FORMAT",
                        format!("swipe: coordenada fora da faixa de i32 ({x1},{y1} → {x2},{y2})"),
                        Some(
                            "use coordenadas dentro de -2147483648..=2147483647 (px do viewport 720×1440)",
                        ),
                    ));
                };
                self.tool_swipe(x1, y1, x2, y2, ms)
            }
            "type_text" => {
                let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
                    return self.tool_error_result(
                        name,
                        &agent_error(
                            "INVALID_FORMAT",
                            "type_text exige text (string)",
                            Some("use {\"text\": \"…\", \"replace\": false}"),
                        ),
                    );
                };
                self.tool_type_text(
                    text,
                    args.get("replace")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            }
            "key" => {
                let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
                    return self.tool_error_result(
                        name,
                        &agent_error("INVALID_FORMAT", "key exige key (BACK|ENTER|DEL)", None),
                    );
                };
                self.tool_key(key)
            }
            "wait" => {
                let Some(ms) = args.get("ms").and_then(|v| v.as_u64()) else {
                    return self.tool_error_result(
                        name,
                        &agent_error(
                            "INVALID_FORMAT",
                            "wait exige ms inteiro ≥ 0",
                            Some("use {\"ms\": 250}"),
                        ),
                    );
                };
                self.tool_wait(ms)
            }
            "wait_for" => self.tool_wait_for(
                args.get("resource_id").and_then(|v| v.as_str()),
                args.get("text").and_then(|v| v.as_str()),
                args.get("class").and_then(|v| v.as_str()),
                args.get("timeout_ms").and_then(|v| v.as_u64()),
                args.get("step_ms").and_then(|v| v.as_u64()),
            ),
            "ui_tree" => {
                self.tool_ui_tree(args.get("format").and_then(|v| v.as_str()).unwrap_or("xml"))
            }
            "screenshot" => self.tool_screenshot(args.get("path").and_then(|v| v.as_str())),
            "logs" => self.tool_logs(args.get("clear").and_then(|v| v.as_bool()).unwrap_or(false)),
            "invoke" => self.tool_invoke(
                args.get("class").and_then(|v| v.as_str()).unwrap_or(""),
                args.get("method").and_then(|v| v.as_str()).unwrap_or(""),
                args.get("sig").and_then(|v| v.as_str()).unwrap_or(""),
                args.get("args").cloned().unwrap_or(serde_json::json!([])),
            ),
            "assert_view" => self.tool_assert_view(
                args.get("resource_id").and_then(|v| v.as_str()),
                args.get("text").and_then(|v| v.as_str()),
                args.get("class").and_then(|v| v.as_str()),
            ),
            "assert_text" => {
                let Some(rid) = args.get("resource_id").and_then(|v| v.as_str()) else {
                    return self.tool_error_result(
                        name,
                        &agent_error("INVALID_FORMAT", "assert_text exige resource_id", None),
                    );
                };
                let Some(expected) = args.get("expected").and_then(|v| v.as_str()) else {
                    return self.tool_error_result(
                        name,
                        &agent_error("INVALID_FORMAT", "assert_text exige expected", None),
                    );
                };
                self.tool_assert_text(
                    rid,
                    expected,
                    args.get("contains")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            }
            "assert_activity" => self
                .tool_assert_activity(args.get("activity").and_then(|v| v.as_str()).unwrap_or("")),
            "assert_finished" => self.tool_assert_finished(),
            "app_info" => self.tool_app_info(),
            "package_list" => self.tool_package_list(),
            "package_remove" => {
                self.tool_package_remove(args.get("package").and_then(|v| v.as_str()).unwrap_or(""))
            }
            "package_clear" => {
                self.tool_package_clear(args.get("package").and_then(|v| v.as_str()).unwrap_or(""))
            }
            "permissions_list" => self.tool_permissions_list(),
            "permissions_grant" => self.tool_permissions_grant(
                args.get("permission")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                true,
            ),
            "permissions_revoke" => self.tool_permissions_grant(
                args.get("permission")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                false,
            ),
            "permissions_audit" => self.tool_permissions_audit(),
            "device_profile" => self.tool_device_profile_m10(&args),
            "behavior_switches" => self.tool_behavior_switches(),
            "runtime_health" => {
                if args.get("gc").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(eng) = self.engine.as_mut() {
                        eng.gc();
                    }
                }
                self.tool_runtime_health()
            }
            "compat_gaps" => self.tool_compat_gaps(),
            "batch" => self.tool_batch(
                args.get("calls"),
                args.get("stop_on_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                args.get("resume_from")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize,
            ),
            "notifications" => self.tool_notifications(&args),
            "component_service" | "widgets" | "widget_screenshot" | "ime" | "accessibility"
            | "shortcuts" | "native_state" => self.tool_m9(name, &args),
            "get_api_usage" => self.tool_api_usage(args.get("package").and_then(|v| v.as_str())),
            "get_coverage" => self.tool_coverage(),
            "observe" => self.tool_observe(&args),
            "tap_text" => self.tool_tap_text(&args),
            "find_views" => self.tool_find_views(&args),
            "tap_view" => self.tool_tap_view(&args),
            "wait_for_view" => self.tool_wait_for_view(&args),
            "session_info" => self.tool_session_info(&args),
            "logs_tail" => self.tool_logs_tail(&args),
            "last_error" => self.tool_last_error(&args),
            other => {
                return self.tool_error_result(
                    name,
                    &agent_error(
                        "UNKNOWN_TOOL",
                        format!("tool '{other}' não existe neste servidor"),
                        Some("veja tools/list para o catálogo"),
                    ),
                )
            }
        };
        let mut out = match res {
            Ok(r) => r,
            Err(e) => self.tool_error_result(name, &e),
        };
        self.screen_post_tool(name, &args, &mut out);
        // Pure diagnostics must not reset VM budgets or trigger automatic GC.
        if diagnostic {
            return out;
        }
        // safe point for the collector: no interpreter frame is live between
        // tool calls (see rd-vm gc.rs); adaptive threshold, RD_GC=off disables
        if let Some(eng) = self.engine.as_mut() {
            if let Some(st) = eng.maybe_gc() {
                let line = format!(
                    "I/RdGc: freed {} objects / {} KB in {:.1} ms (live {}, heap {} -> {} KB)\n",
                    st.last_freed_objects,
                    st.last_freed_bytes / 1024,
                    st.last_pause_ms,
                    st.last_live_objects,
                    st.last_used_before / 1024,
                    st.last_used_after / 1024
                );
                self.logs
                    .lock()
                    .expect("logs")
                    .extend_from_slice(line.as_bytes());
            }
        }
        out
    }

    // ── install / launch / input ────────────────────────────────────────────

    fn tool_install(&mut self, apk_path: &str) -> Result<ToolResult, RdError> {
        if apk_path.is_empty() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "install exige {\"apk\": caminho}",
                None,
            ));
        }
        // Controles M5: URL recusada, `..` recusado, arquivo dentro do sandbox
        let apk_file = self.policy.check_read_path(apk_path, &self.roots)?;
        // Controle M5: APK excessivo (tamanho ANTES de ler para a memória)
        let meta = std::fs::metadata(&apk_file).map_err(|e| {
            RdError::new(
                "IO_ERROR",
                format!("metadados de {}: {e}", apk_file.display()),
                MODULE_ID,
            )
            .with_suggestion("confira o caminho absoluto do APK dentro do sandbox")
        })?;
        if meta.len() > self.policy.max_apk_bytes {
            return Err(agent_error(
                "APK_TOO_LARGE",
                format!(
                    "APK tem {} bytes; o teto é {} (RD_MCP_MAX_APK_MB)",
                    meta.len(),
                    self.policy.max_apk_bytes
                ),
                Some("use um APK menor ou eleve RD_MCP_MAX_APK_MB no processo do servidor"),
            ));
        }
        // ADR 009 (opt-in RD_SNAPSHOT=on): paged fast install when a
        // content-keyed snapshot exists — no whole-file read at all.
        if snapshot_enabled() {
            if let Some(res) = self.try_install_snapshot(&apk_file)? {
                return Ok(res);
            }
        }
        let t_read = std::time::Instant::now();
        let bytes = std::fs::read(&apk_file).map_err(|e| {
            RdError::new(
                "IO_ERROR",
                format!("lendo {}: {e}", apk_file.display()),
                MODULE_ID,
            )
            .with_suggestion("confira o caminho absoluto do APK")
        })?;
        phase("install_fs_read", t_read);
        let t_open = std::time::Instant::now();
        let apk =
            Apk::from_bytes(bytes).map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        phase("install_apk_from_bytes", t_open);
        // piso API 26 é contrato: instalação recusa com erro tipado
        if let rd_apk::FloorStatus::BelowFloor { min_sdk, floor } = apk.floor_status() {
            return Err(agent_error(
                "BELOW_FLOOR",
                format!(
                    "minSdkVersion={min_sdk} abaixo do piso {floor} — o app não roda neste runtime"
                ),
                Some("use um APK com minSdkVersion ≥ 26"),
            ));
        }
        // Tudo validado — SÓ AGORA muta o estado (instalação transacional:
        // falha de parse/floor/sandbox não deixa registro pela metade).
        let manifest = apk.manifest.clone();
        let package = manifest.package.clone();
        let components = manifest.application.components.len();
        let perms = PermissionEngine::install(
            &manifest,
            PermissionTable::embedded()
                .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?,
        );
        let version = manifest.version_name.clone().unwrap_or_else(|| "?".into());
        // Reinstalar o package EM EXECUÇÃO o encerra (Android mata o app);
        // reinstalar outro package NÃO derruba o engine (multi-app honesto).
        let killed = self.kill_engine_if_running(&package);
        let entry = InstalledApp {
            package: package.clone(),
            version_name: manifest.version_name.clone(),
            version_code: manifest.version_code,
            manifest: manifest.clone(),
            apk: Some(std::sync::Arc::new(apk)),
            source_path: Some(apk_file.clone()),
            snapshot_key: None,
            perms,
        };
        match self.installed.iter().position(|a| a.package == package) {
            Some(i) => self.installed[i] = entry, // update in-place (mesma posição)
            None => self.installed.push(entry),
        }
        self.active = Some(package.clone());
        Ok(ToolResult::json(&serde_json::json!({
            "installed": true,
            "session_id": self.session_id,
            "package": package,
            "version_name": version,
            "components": components,
            "killed_running": killed,
            "installed_count": self.installed.len(),
            "runtime_pending": self.find_installed(&package).map(|a| a.perms.pending_runtime().len()).unwrap_or(0),
            "runtime_special": self.find_installed(&package).map(|a| a.perms.pending_special().len()).unwrap_or(0),
        })))
    }

    /// Real cold launch (new Engine + DEX mount + `launch_app`).
    /// Outer `Err` = host/setup failures; inner `Err` = guest `VmExit`,
    /// preserved so callers keep the existing exit mapping (issue #109).
    fn cold_launch(
        &mut self,
        package_out: &str,
        desc: &str,
        selected_engine: Option<&str>,
    ) -> Result<Result<u32, VmExit>, RdError> {
        #[cfg(feature = "advanced-components")]
        let (resources, granted, notification_permission) = {
            let app = self.find_installed_mut(package_out).ok_or_else(|| {
                agent_error(
                    "PACKAGE_NOT_INSTALLED",
                    format!("package {package_out:?} não está no registro desta sessão"),
                    Some("veja package_list — instale com `install` antes de lançar"),
                )
            })?;
            let notification_permission = app
                .perms
                .state("android.permission.POST_NOTIFICATIONS")
                .map(|s| s.is_granted())
                .unwrap_or(app.manifest.target_sdk < 33);
            let granted: Vec<String> = app
                .perms
                .all_states()
                .iter()
                .filter(|(_, s)| s.is_granted())
                .map(|(n, _)| n.clone())
                .collect();
            (app.apk()?, granted, notification_permission)
        };
        #[cfg(not(feature = "advanced-components"))]
        let (resources, granted) = {
            let app = self.find_installed_mut(package_out).ok_or_else(|| {
                agent_error(
                    "PACKAGE_NOT_INSTALLED",
                    format!("package {package_out:?} não está no registro desta sessão"),
                    Some("veja package_list — instale com `install` antes de lançar"),
                )
            })?;
            let granted: Vec<String> = app
                .perms
                .all_states()
                .iter()
                .filter(|(_, s)| s.is_granted())
                .map(|(n, _)| n.clone())
                .collect();
            (app.apk()?, granted)
        };
        // M8 (38º gap): Application do manifest como no boot_app do CLI.
        let app_desc = resources.manifest.application.name.as_ref().map(|name| {
            let abs = if let Some(rel) = name.strip_prefix('.') {
                format!("{}{}", resources.manifest.package, rel)
            } else {
                name.clone()
            };
            format!("L{};", abs.replace('.', "/"))
        });
        let mut eng = if let Some(id) = selected_engine {
            #[cfg(feature = "native-engines")]
            {
                // issue #260: o desc já foi resolvido e validado aqui em cima
                // (activity explícita não-launcher, alias via #109 ou launcher
                // default) — repassa o descritor em vez de exigir launcher de
                // novo, e compartilha o Arc<Apk> em vez de clonar o APK.
                rd_engine_registry::NativeProcess::from_apk_with_activity(
                    id,
                    resources.clone(),
                    desc,
                )?
                .into_engine()
            }
            #[cfg(not(feature = "native-engines"))]
            {
                let _ = id;
                return Err(agent_error(
                    "NOT_IMPLEMENTED",
                    "MCP native selection requires native-engines",
                    None,
                ));
            }
        } else {
            let t_mount = std::time::Instant::now();
            let dexes = dexes_of_apk(&resources)?;
            phase("dex_mount_total", t_mount);
            let t_new = std::time::Instant::now();
            let mut e = Engine::new(dexes, VmConfig::default());
            phase("engine_new", t_new);
            e.set_resources_arc(resources);
            e
        };
        #[cfg(feature = "advanced-components")]
        eng.m9_set_notification_permission(notification_permission);
        #[cfg(any(feature = "advanced-components", feature = "virtual-devices"))]
        eng.m10_enable_virtual_devices();
        if let Some(ad) = app_desc {
            eng.set_application_class(ad);
        }
        // perms.rs: checkSelfPermission/requestPermissions see the
        // session's real grant state
        eng.set_granted_permissions(granted);
        let t_launch = std::time::Instant::now();
        let r = eng.launch_app(desc, package_out, Vec::new());
        phase("launch_app_cold", t_launch);
        if r.is_ok() {
            self.pending_snapshot = None;
            self.engine = Some(eng);
        }
        Ok(r)
    }

    fn tool_launch(
        &mut self,
        activity: Option<&str>,
        package: Option<&str>,
        engine: Option<&str>,
    ) -> Result<ToolResult, RdError> {
        // package explícito (multi-app) ou o ativo
        let pkg = package
            .map(str::to_string)
            .or_else(|| self.active.clone())
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro (tool install)", None))?;
        let app = self.find_installed(&pkg).ok_or_else(|| {
            agent_error(
                "PACKAGE_NOT_INSTALLED",
                format!("package {pkg:?} não está no registro desta sessão"),
                Some("veja package_list — instale com `install` antes de lançar"),
            )
        })?;
        let selected_engine = engine.map(str::to_string).or_else(|| {
            if self.engine_package.as_deref() == Some(pkg.as_str()) {
                self.native_engine.clone()
            } else {
                None
            }
        });
        if let Some(id) = selected_engine.as_deref() {
            let registry = rd_engine_registry::Registry::embedded()?;
            let descriptor = registry.get(id)?;
            if !rd_engine_registry::native_execution_available(id) {
                return Err(rd_apk::RdError::new("NOT_IMPLEMENTED",format!("engine {id} requires a supported executing native adapter and native-engines feature"),&descriptor.modules[0]));
            }
        }
        // issue #109: `<activity-alias>` declarada lança a `targetActivity`
        // (comportamento do Android) — a resolução acontece AQUI; a VM só vê
        // a classe real que precisa existir no DEX (checagem da #82 intacta).
        let (desc, package_out, alias_of) = match activity {
            Some(a) => {
                let desc = normalize_activity_desc(a);
                // issue #83: o Android só inicia activities DECLARADAS no
                // AndroidManifest.xml — qualquer outra classe falha com
                // ActivityNotFoundException. O caminho sem `activity` já
                // consultava o manifest (LAUNCHER): os dois agora concordam.
                let comp = app
                    .manifest
                    .application
                    .components
                    .iter()
                    .find(|c| {
                        matches!(
                            c.kind,
                            ComponentKind::Activity | ComponentKind::ActivityAlias
                        ) && normalize_activity_desc(&c.class_name) == desc
                    })
                    .ok_or_else(|| {
                        agent_error(
                            "MISSING_ENTRY",
                            format!(
                                "activity {a} não está declarada como <activity>/<activity-alias> no AndroidManifest.xml (pacote {})",
                                app.manifest.package
                            ),
                            Some(
                                "o Android recusa iniciar classe não declarada; use `inspect`/`package_list` para listar as activities do manifest, ou lance sem `activity` para usar a LAUNCHER",
                            ),
                        )
                    })?;
                let (desc, alias_of) = launch_desc_of_component(comp)?;
                (desc, app.manifest.package.clone(), alias_of)
            }
            None => {
                // issue #109: a entrada MAIN/LAUNCHER pode estar declarada num
                // `<activity-alias>`, não só numa `<activity>`.
                let launcher = app
                    .manifest
                    .application
                    .components
                    .iter()
                    .find(|c| {
                        matches!(
                            c.kind,
                            ComponentKind::Activity | ComponentKind::ActivityAlias
                        ) && c.is_launcher
                    })
                    .ok_or_else(|| {
                        agent_error(
                            "MISSING_ENTRY",
                            "activity LAUNCHER não encontrada no manifest (nem <activity> nem <activity-alias> com MAIN/LAUNCHER)",
                            Some("passe {\"activity\": \"com.ex.Main\"} explicitamente"),
                        )
                    })?;
                let (desc, alias_of) = launch_desc_of_component(launcher)?;
                (desc, app.manifest.package.clone(), alias_of)
            }
        };
        // ADR 009: owned hints so the `app` borrow ends before branches
        // that need `&mut self`.
        let snap_key = app.snapshot_key.clone();
        let snap_source = app.source_path.clone();
        // M7 (regime warm): se o package ainda está em execução na sessão, o
        // relançamento REUTILIZA o processo (o Engine existente) — o Android
        // não re-parseia o DEX num warm start; a activity é recriada no mesmo
        // heap (`ensure_initialized` é idempotente, `<clinit>` não repete).
        // Caminho frio (Engine novo, com parse dos DEX) só quando o package
        // não está rodando. Falha no warm não derruba o app (mesma regra da
        // issue #82).
        let warm = self.engine.is_some()
            && self.engine_package.as_deref() == Some(package_out.as_str())
            && self.native_engine == selected_engine;
        // ADR 009 (opt-in): present a stored first frame immediately; the
        // engine materializes lazily on the first engine-requiring tool.
        // Only when no other engine is live (a cold launch would replace
        // it) and no native engine was selected.
        let snapshot_present: Option<Result<u32, VmExit>> =
            if !warm && self.engine.is_none() && selected_engine.is_none() && snapshot_enabled() {
                match self.snapshot_for_launch(
                    &package_out,
                    &desc,
                    snap_key.as_deref(),
                    snap_source.as_deref(),
                ) {
                    Some(pending) => {
                        self.present_snapshot(pending);
                        Some(Ok(0))
                    }
                    None => None,
                }
            } else {
                None
            };
        let used_snapshot = snapshot_present.is_some();
        let launch_result = if warm {
            let eng = self
                .engine
                .as_mut()
                .expect("engine presente: warm verificado acima");
            eng.launch_app(&desc, &package_out, Vec::new())
        } else if let Some(snap_result) = snapshot_present {
            snap_result
        } else {
            self.cold_launch(&package_out, &desc, selected_engine.as_deref())?
        };
        launch_result.map_err(|e| {
            let rd = vm_exit_to_rd(e);
            match (&alias_of, rd.code.as_str()) {
                // issue #109: o alvo do alias não existe no DEX — erro
                // estruturado nomeando alias E alvo (não o MISSING_ENTRY
                // genérico da VM, que citaria só a classe resolvida).
                (Some((alias, target)), "MISSING_ENTRY") => agent_error(
                    "MISSING_ENTRY",
                    format!(
                        "activity-alias {alias} aponta para {target}, que não existe no DEX — não há o que lançar"
                    ),
                    Some(
                        "confira o android:targetActivity do alias no manifest (`inspect` lista os componentes); o alvo precisa existir no DEX",
                    ),
                ),
                _ => rd,
            }
        })?;
        self.engine_package = Some(package_out.clone());
        self.native_engine = selected_engine.clone();
        self.activity = Some(desc.clone());
        self.active = Some(package_out.clone());
        // ADR 009: a REAL cold launch captures the snapshot for future
        // processes (install-time precomputation). Failure is disclosed
        // in the result, never fatal to the launch.
        let mut snapshot_error: Option<String> = None;
        if !warm && !used_snapshot && selected_engine.is_none() && snapshot_enabled() {
            if let Err(e) = self.capture_snapshot(&package_out, &desc) {
                snapshot_error = Some(e.cause);
            }
        }
        let mut out = serde_json::json!({
            "launched": true,
            "native_engine": self.native_engine,
            "session_id": self.session_id,
            "activity": desc,
            "package": package_out,
            // M7: true = warm start (processo reutilizado, sem re-parse do DEX)
            "warm": warm,
        });
        if used_snapshot {
            out["snapshot"] = serde_json::json!(true);
            out["materialized"] = serde_json::json!(false);
        }
        if let Some(e) = snapshot_error {
            out["snapshot_error"] = serde_json::json!(e);
        }
        if let Some((alias, _)) = &alias_of {
            // issue #109: o que foi pedido/declarado é o alias — o resultado
            // registra qual alias resolveu o launch (a `activity` é o alvo).
            out["alias"] = serde_json::json!(alias);
        }
        Ok(ToolResult::json(&out))
    }

    fn tool_tap(&mut self, x: i32, y: i32) -> Result<ToolResult, RdError> {
        let hit = self
            .require_engine()?
            .touch_app(x, y)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "tapped": [x, y],
            "listener_acionado": hit,
        })))
    }

    fn tool_wait(&mut self, ms: u64) -> Result<ToolResult, RdError> {
        // Controle M5: tempo ilimitado não existe — cap com erro tipado
        if ms > self.policy.max_wait_ms {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                format!("wait de {ms} ms excede o teto de {} ms (RD_MCP_MAX_WAIT_MS)", self.policy.max_wait_ms),
                Some("divida em chamadas menores ou eleve RD_MCP_MAX_WAIT_MS no processo do servidor"),
            ));
        }
        // Long waits advance in 512 ms slices (a multiple of the 16 ms vsync)
        // with a collector safe point between slices: no interpreter frame
        // and no host-held reference is live there, and runnables still run
        // in deadline order. Keeps a long animation/coroutine burst from
        // filling the heap inside one call.
        const SLICE_MS: u64 = 512;
        let eng = self.require_engine()?;
        let mut ran = 0;
        let mut left = ms;
        loop {
            let step = left.min(SLICE_MS);
            ran += eng.advance_clock(step).map_err(vm_exit_to_rd)?;
            left -= step;
            if left == 0 {
                break;
            }
            let _ = eng.maybe_gc();
        }
        Ok(ToolResult::json(&serde_json::json!({
            "avancado_ms": ms,
            "runnables_executados": ran,
        })))
    }

    // ── input estendido (swipe/type/key) e espera condicional ─────────────

    fn tool_swipe(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        ms: u64,
    ) -> Result<ToolResult, RdError> {
        if ms > self.policy.max_wait_ms {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                format!(
                    "swipe de {ms} ms excede o teto de {} ms (RD_MCP_MAX_WAIT_MS)",
                    self.policy.max_wait_ms
                ),
                Some("gestos mais curtos ou RD_MCP_MAX_WAIT_MS maior no servidor"),
            ));
        }
        let rep = self
            .require_engine()?
            .swipe_app(x1, y1, x2, y2, ms)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "from": [x1, y1],
            "to": [x2, y2],
            "ms": ms,
            "clicked": rep.clicked,
            "crossed": rep.crossed.iter().map(|c| serde_json::json!({
                "class": c.class,
                "bounds": [c.bounds.0, c.bounds.1, c.bounds.2, c.bounds.3],
            })).collect::<Vec<_>>(),
        })))
    }

    fn tool_type_text(&mut self, text: &str, replace: bool) -> Result<ToolResult, RdError> {
        let final_text = self
            .require_engine()?
            .type_text(text, replace)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "typed": text.len(),
            "replace": replace,
            "final_text": final_text,
        })))
    }

    fn tool_key(&mut self, key: &str) -> Result<ToolResult, RdError> {
        let rep = self
            .require_engine()?
            .key_event(key)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "key": rep.key,
            "delivered": rep.delivered,
            "effect": rep.effect,
        })))
    }

    /// Espera condicional: poll da condição avançando o clock virtual.
    fn tool_wait_for(
        &mut self,
        resource_id: Option<&str>,
        text: Option<&str>,
        class: Option<&str>,
        timeout_ms: Option<u64>,
        step_ms: Option<u64>,
    ) -> Result<ToolResult, RdError> {
        validate_view_criteria("wait_for", resource_id, text, class)?;
        let timeout = timeout_ms.unwrap_or(5_000);
        if timeout > self.policy.max_wait_ms {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                format!(
                    "timeout_ms={timeout} excede o teto de {} ms (RD_MCP_MAX_WAIT_MS)",
                    self.policy.max_wait_ms
                ),
                Some("timeout menor ou RD_MCP_MAX_WAIT_MS maior no servidor"),
            ));
        }
        let step = step_ms.unwrap_or(100).max(1);
        let mut waited: u64 = 0;
        let mut polls: usize = 0;
        let mut found: Option<serde_json::Value> = None;
        // condição checada ANTES do primeiro wait (a view pode já existir)
        loop {
            let tree = self.require_engine()?.view_tree();
            if let Some(t) = tree {
                let mut matches = Vec::new();
                collect_matches(&t, resource_id, text, class, &mut matches);
                if let Some(m) = matches.into_iter().next() {
                    found = Some(m);
                    break;
                }
            }
            if waited >= timeout {
                break;
            }
            let slice = step.min(timeout - waited);
            self.require_engine()?
                .advance_clock(slice)
                .map_err(vm_exit_to_rd)?;
            waited += slice;
            polls += 1;
        }
        Ok(ToolResult::json(&serde_json::json!({
            "found": found.is_some(),
            "match": found,
            "waited_ms": waited,
            "polls": polls,
            "timeout_ms": timeout,
        })))
    }

    // ── observação ──────────────────────────────────────────────────────────

    fn tool_ui_tree(&mut self, format: &str) -> Result<ToolResult, RdError> {
        // inspect a laid-out window (bounds are 0×0 before the first frame)
        let _ = self.require_engine()?.ensure_laid_out();
        let tree: UiNode = self
            .require_engine()?
            .view_tree()
            .ok_or_else(|| agent_error("INVALID_FORMAT", "sem activity/window para dump", None))?;
        let package = self
            .engine_package
            .clone()
            .or_else(|| self.active.clone())
            .unwrap_or_default();
        match format {
            "text" => {
                let d = self.require_engine()?.dump_ui();
                Ok(ToolResult::text(d))
            }
            _ => {
                let xml = rd_render::uiautomator_xml(&tree, &package)
                    .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
                Ok(ToolResult::text(xml))
            }
        }
    }

    fn tool_screenshot(&mut self, path: Option<&str>) -> Result<ToolResult, RdError> {
        // ADR 009: a presented-but-unmaterialized snapshot serves its
        // stored frame directly (byte-identical to the live frame under
        // Law 4 determinism) instead of forcing materialization.
        if self.engine.is_none() {
            if let Some(pending) = &self.pending_snapshot {
                let png = pending.loaded.png.clone();
                return match path {
                    Some(p) if !p.is_empty() => {
                        let out = self.policy.check_write_path(p, &self.roots)?;
                        write_no_follow(&out, &png)?;
                        Ok(ToolResult::json(&serde_json::json!({
                            "path": out.display().to_string(),
                            "bytes": png.len(),
                            "size": [720, 1440],
                            "snapshot": true,
                        })))
                    }
                    _ => Ok(ToolResult {
                        content: vec![ToolContent::Image {
                            data_b64: base64_encode(&png),
                            mime: "image/png",
                        }],
                        is_error: false,
                    }),
                };
            }
        }
        if self.require_engine()?.view_tree().is_none() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "sem activity/window para screenshot",
                None,
            ));
        }
        // App-drawn content (Compose, custom views): the host draw pass is
        // recorded once per frame version and the PNG cached until app code
        // runs again (round 6) — repeated screenshots are free.
        let png = self.screen_png()?;
        let draw_warning = self.screen.draw_error.clone();
        match path {
            Some(p) if !p.is_empty() => {
                // Controle M5: escrita de arquivo SÓ dentro do sandbox
                let out = self.policy.check_write_path(p, &self.roots)?;
                // issue #67: nem `fs::write` — ele segue o link se o caminho
                // for trocado entre a checagem e a escrita.
                write_no_follow(&out, &png)?;
                Ok(ToolResult::json(&serde_json::json!({
                    "path": out.display().to_string(),
                    "bytes": png.len(),
                    "size": [720, 1440],
                })))
            }
            _ => {
                let mut content = vec![ToolContent::Image {
                    data_b64: base64_encode(&png),
                    mime: "image/png",
                }];
                if let Some(w) = draw_warning {
                    content.push(ToolContent::Text(format!(
                        "warning: draw pass failed — app-drawn content may be missing: {w}"
                    )));
                }
                Ok(ToolResult {
                    content,
                    is_error: false,
                })
            }
        }
    }

    fn tool_logs(&mut self, clear: bool) -> Result<ToolResult, RdError> {
        let mut buf = self.logs.lock().expect("logs");
        let text = String::from_utf8_lossy(&buf).to_string();
        if clear {
            buf.clear();
            self.log_generation = self.log_generation.saturating_add(1);
        }
        Ok(ToolResult::json(&serde_json::json!({
            "logs": text,
            "bytes": text.len(),
        })))
    }

    // ── sonda de estado geral ───────────────────────────────────────────────

    fn tool_invoke(
        &mut self,
        class: &str,
        method: &str,
        sig: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, RdError> {
        let class_desc = normalize_activity_desc(class); // mesma normalização L…;
        let jargs = args.as_array().cloned().unwrap_or_default();
        let values = json_args_to_values(sig, &jargs)?;
        let eng = self.require_engine()?;
        match eng.invoke_static(&class_desc, method, sig, &values) {
            Ok(v) => Ok(ToolResult::json(&serde_json::json!({
                "ok": true,
                "value": value_to_json(&v),
            }))),
            Err(VmExit::Exception(t)) => Ok(ToolResult::json(&serde_json::json!({
                "ok": false,
                "exception": {
                    "class": t.name_without_l(),
                    "message": t.message,
                },
            }))),
            Err(VmExit::Error(e)) => {
                Err(RdError::new(&e.code, &e.cause, &e.module_id).maybe_suggestion(&e.suggestion))
            }
        }
    }

    // ── asserções ───────────────────────────────────────────────────────────

    fn tool_assert_view(
        &mut self,
        resource_id: Option<&str>,
        text: Option<&str>,
        class: Option<&str>,
    ) -> Result<ToolResult, RdError> {
        validate_view_criteria("assert_view", resource_id, text, class)?;
        let tree = self
            .require_engine()?
            .view_tree()
            .ok_or_else(|| agent_error("INVALID_FORMAT", "sem activity/window", None))?;
        let mut matches = Vec::new();
        collect_matches(&tree, resource_id, text, class, &mut matches);
        Ok(ToolResult::json(&serde_json::json!({
            "found": !matches.is_empty(),
            "matches": matches,
        })))
    }

    fn tool_assert_text(
        &mut self,
        resource_id: &str,
        expected: &str,
        contains: bool,
    ) -> Result<ToolResult, RdError> {
        let tree = self
            .require_engine()?
            .view_tree()
            .ok_or_else(|| agent_error("INVALID_FORMAT", "sem activity/window", None))?;
        let mut found: Option<String> = None;
        find_text_by_rid(&tree, resource_id, &mut found);
        let ok = match &found {
            Some(t) => {
                if contains {
                    t.contains(expected)
                } else {
                    t == expected
                }
            }
            None => false,
        };
        Ok(ToolResult::json(&serde_json::json!({
            "ok": ok,
            "resource_id": resource_id,
            "expected": expected,
            "actual": found,
            "mode": if contains { "contains" } else { "equals" },
        })))
    }

    fn tool_assert_activity(&mut self, activity: &str) -> Result<ToolResult, RdError> {
        let want = normalize_activity_desc(activity);
        let cur = self.current_activity().unwrap_or("").to_string();
        Ok(ToolResult::json(&serde_json::json!({
            "ok": cur == want,
            "expected": want,
            "actual": cur,
        })))
    }

    fn tool_assert_finished(&mut self) -> Result<ToolResult, RdError> {
        let finished = self.require_engine()?.activity_is_finished();
        Ok(ToolResult::json(
            &serde_json::json!({ "finished": finished }),
        ))
    }

    // ── permissões / device / behavior ──────────────────────────────────────

    fn tool_app_info(&mut self) -> Result<ToolResult, RdError> {
        let pkg = self.require_apk()?.package.clone();
        let apk = self
            .find_installed_mut(&pkg)
            .ok_or_else(|| agent_error("NO_APP", "app ativo sumiu do registro", None))?
            .apk()?;
        let engines = rd_engine_registry::Registry::embedded()?.detect_for_inspection(&apk)?;
        let m = &apk.manifest;
        Ok(ToolResult::json(&serde_json::json!({
            "package": m.package,
            "version_name": m.version_name,
            "version_code": m.version_code,
            "engines": engines,
            "min_sdk": m.min_sdk,
            "target_sdk": m.target_sdk,
            "floor": serde_json::json!(match apk.floor_status() {
                rd_apk::FloorStatus::Ok => serde_json::json!({"ok": true, "floor": 26}),
                rd_apk::FloorStatus::BelowFloor { min_sdk, floor } =>
                    serde_json::json!({"ok": false, "min_sdk": min_sdk, "floor": floor}),
            }),
            "components": m.application.components.iter().map(|c| serde_json::json!({
                "kind": format!("{:?}", c.kind).to_lowercase(),
                "class": c.class_name,
                "exported": c.exported,
                "launcher": c.is_launcher,
            })).collect::<Vec<_>>(),
        })))
    }

    // ── registro de packages (listar/remover/limpar — M5) ─────────────────

    fn tool_package_list(&mut self) -> Result<ToolResult, RdError> {
        let apps: Vec<serde_json::Value> = self
            .installed
            .iter()
            .map(|a| {
                serde_json::json!({
                    "package": a.package,
                    "version_name": a.version_name,
                    "version_code": a.version_code,
                    "active": self.active.as_deref() == Some(a.package.as_str()),
                    "running": self.engine_package.as_deref() == Some(a.package.as_str()),
                })
            })
            .collect();
        Ok(ToolResult::json(&serde_json::json!({
            "session_id": self.session_id,
            "packages": apps,
        })))
    }

    fn tool_package_remove(&mut self, package: &str) -> Result<ToolResult, RdError> {
        if package.is_empty() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "package_remove exige {\"package\": nome}",
                None,
            ));
        }
        if self.find_installed(package).is_none() {
            return Err(agent_error(
                "PACKAGE_NOT_INSTALLED",
                format!("package {package:?} não está no registro desta sessão"),
                Some("veja package_list"),
            ));
        }
        // Android: desinstalar app em execução o mata primeiro — nunca
        // estado inconsistente (engine de outro app segue rodando).
        let killed = self.kill_engine_if_running(package);
        // Issue #230: desinstalar apaga também os dados privados do app
        // no VFS (/data/user/0/<pkg>/), não só o registro da sessão.
        rd_vm::clear_package_data(package);
        self.installed.retain(|a| a.package != package);
        if self.active.as_deref() == Some(package) {
            self.active = self.installed.last().map(|a| a.package.clone());
        }
        Ok(ToolResult::json(&serde_json::json!({
            "removed": true,
            "session_id": self.session_id,
            "package": package,
            "killed_running": killed,
            "installed_count": self.installed.len(),
        })))
    }

    fn tool_package_clear(&mut self, package: &str) -> Result<ToolResult, RdError> {
        if package.is_empty() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "package_clear exige {\"package\": nome}",
                None,
            ));
        }
        let manifest = {
            let app = self.find_installed(package).ok_or_else(|| {
                agent_error(
                    "PACKAGE_NOT_INSTALLED",
                    format!("package {package:?} não está no registro desta sessão"),
                    Some("veja package_list"),
                )
            })?;
            app.manifest.clone()
        };
        // Android clear data: app em execução é morto e as permissões
        // runtime voltam ao estado de instalação.
        let killed = self.kill_engine_if_running(package);
        // Issue #230: clear data apaga de fato o armazenamento privado
        // (/data/user/0/<pkg>/: arquivos, SQLite, shared_prefs) — antes
        // só as permissões resetavam e os dados continuavam legíveis.
        rd_vm::clear_package_data(package);
        let table = PermissionTable::embedded()
            .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        let fresh = PermissionEngine::install(&manifest, table);
        let (pending, special) = {
            let app = self.find_installed_mut(package).expect("checado acima");
            app.perms = fresh;
            (
                app.perms.pending_runtime().len(),
                app.perms.pending_special().len(),
            )
        };
        Ok(ToolResult::json(&serde_json::json!({
            "cleared": true,
            "session_id": self.session_id,
            "package": package,
            "killed_running": killed,
            "runtime_pending": pending,
            "runtime_special": special,
        })))
    }

    fn tool_permissions_list(&mut self) -> Result<ToolResult, RdError> {
        let perms = &self.require_apk()?.perms;
        let states: Vec<serde_json::Value> = perms
            .all_states()
            .iter()
            .map(|(name, st)| {
                serde_json::json!({
                    "permission": name,
                    "state": grant_state_str(*st),
                })
            })
            .collect();
        Ok(ToolResult::json(
            &serde_json::json!({ "permissions": states }),
        ))
    }

    fn tool_permissions_grant(
        &mut self,
        permission: &str,
        grant: bool,
    ) -> Result<ToolResult, RdError> {
        let st = {
            let perms = &mut self
                .find_installed_mut(self.active.clone().unwrap_or_default().as_str())
                .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro", None))?
                .perms;
            if grant {
                perms.grant(permission).map_err(rd_to_rd)?
            } else {
                perms.revoke(permission).map_err(rd_to_rd)?
            }
        };
        // keep the running engine's grant set in sync (perms.rs)
        if self.engine_package == self.active {
            let granted: Option<Vec<String>> = self.active.as_ref().and_then(|p| {
                self.installed.iter().find(|a| &a.package == p).map(|a| {
                    a.perms
                        .all_states()
                        .iter()
                        .filter(|(_, s)| s.is_granted())
                        .map(|(n, _)| n.clone())
                        .collect()
                })
            });
            if let (Some(e), Some(g)) = (self.engine.as_mut(), granted) {
                e.set_granted_permissions(g);
            }
        }
        #[cfg(feature = "advanced-components")]
        if permission == "android.permission.POST_NOTIFICATIONS"
            && self.engine_package == self.active
        {
            if let Some(e) = self.engine.as_mut() {
                e.m9_set_notification_permission(st.is_granted());
            }
        }
        Ok(ToolResult::json(&serde_json::json!({
            "permission": permission,
            "state": grant_state_str(st),
        })))
    }

    fn tool_permissions_audit(&mut self) -> Result<ToolResult, RdError> {
        let app = self.require_apk()?;
        let findings: Vec<serde_json::Value> = app
            .perms
            .audit(&app.manifest)
            .iter()
            .map(|f| {
                serde_json::json!({
                    "severity": format!("{:?}", f.severity).to_lowercase(),
                    "code": f.code,
                    "message": f.message,
                    "permission": f.permission,
                    "component": f.component,
                })
            })
            .collect();
        Ok(ToolResult::json(
            &serde_json::json!({ "findings": findings }),
        ))
    }

    fn tool_behavior_switches(&mut self) -> Result<ToolResult, RdError> {
        let target = self
            .require_apk()
            .map(|a| a.manifest.target_sdk)
            .unwrap_or(26);
        let active: Vec<serde_json::Value> = self
            .behavior
            .active_for(target)
            .iter()
            .map(|s| serde_json::json!({"id": s.id, "since": s.api}))
            .collect();
        Ok(ToolResult::json(&serde_json::json!({
            "target_sdk": target,
            "active": active,
        })))
    }

    /// Saúde do runtime (M5): reporta o estado SEM exigir app — nunca
    /// falha por falta de app (é a tool de diagnóstico).
    fn tool_runtime_health(&mut self) -> Result<ToolResult, RdError> {
        let mut v = serde_json::json!({
            "session_id": self.session_id,
            "installed_count": self.installed.len(),
            "active_package": self.active,
            "running": self.engine.is_some(),
            "running_package": self.engine_package,
            "logs_bytes": self.logs.lock().expect("logs").len(),
        });
        match self.engine.as_ref() {
            Some(eng) => {
                v["activity"] = serde_json::json!(self.current_activity());
                v["native_engine"] = serde_json::json!(self.native_engine);
                v["finished"] = serde_json::json!(eng.activity_is_finished());
                v["clock_ms"] = serde_json::json!(eng.fw.clock());
                v["heap_used_bytes"] = serde_json::json!(eng.heap_used());
                // M7: marca d'água do heap do app (orçamento ram_app_heap_mb)
                v["heap_peak_bytes"] = serde_json::json!(eng.heap_peak());
                v["fuel_used"] = serde_json::json!(eng.fuel_used());
                v["looper_pending"] = serde_json::json!(eng.looper_pending());
                v["gc"] = serde_json::json!(eng.gc_stats());
                #[cfg(feature = "jit")]
                {
                    v["jit"] = serde_json::json!(eng.jit_stats());
                }
            }
            None => {
                v["activity"] = serde_json::json!(Option::<String>::None);
            }
        }
        Ok(ToolResult::json(&v))
    }

    /// Catálogo honesto das lacunas de compatibilidade (M5): o agente
    /// consulta antes de planejar — nunca descobre na unha.
    fn tool_compat_gaps(&mut self) -> Result<ToolResult, RdError> {
        const GAPS: &[(&str, &str, &str, &str)] = &[
            ("m8-recyclerview", "ui", "supported", "RecyclerView host (M8 full): setAdapter pergunta getItemCount ao adapter DEX, cria ViewHolders via onCreateViewHolder DEX e vincula via onBindViewHolder (cap 20 = viewport headless), anexa itemViews como filhos (ui_tree/touch veem a lista); notify* repopula e conta notificações; LayoutManager/Linear/Grid registrados; Adapter/ViewHolder <init> host. Ver crates/rd-vm/src/m8.rs + tests/m8.rs"),
            ("m8-fragments-androidx", "vm", "supported", "Fragments legacy (FragmentManager/transações/registro) + androidx FragmentManager/FragmentTransaction host: beginTransaction/add/replace/remove/commit/commitNow, findFragmentByTag/Id, popBackStack/getBackStackEntryCount, getSupportFragmentManager canônico. Sem animações nem lifecycle de view da fragment (documentado)"),
            ("m8-dialogs", "ui", "supported", "Dialog/AlertDialog/AlertDialog.Builder host: setTitle/setMessage/setView/setCancelable/setPositive|Negative|NeutralButton, create/show/dismiss/isShowing; clique de botão via Engine::m8_click_dialog_button dispara OnClickListener DEX"),
            ("m8-okhttp", "net", "supported", "OkHttp shim OFFLINE determinístico: OkHttpClient.newCall, Request.Builder(url/header/method/build), Call.execute/enqueue (síncrono, chama Callback DEX), Response code/message/isSuccessful/body, ResponseBody.string/bytes/contentLength. Resposta registrada via Engine::m8_set_http_response (default 200 vazio); requests ficam em m8_http_requests. Rede real NÃO é feita (sem socket) — desvio documentado M8"),
            ("m8-sqlite-room", "data", "partial", "SQLite real isolado (jsqlite; subset só como fallback RD_SQLITE=subset/backend indisponível): SQLiteOpenHelper (getWritable/ReadableDatabase + onCreate/onUpgrade/onDowngrade DEX), SQLiteDatabase execSQL/insert/replace/update/delete/query/rawQuery, transações, ContentValues e Cursor amplo. Bancos nomeados persistem como imagem SQLite no caminho app-private do VFS entre launches do Engine; :memory:/createInMemory e o fallback subset seguem só em memória. Room: databaseBuilder/inMemoryDatabaseBuilder + Builder fluente; DAOs DEX rodam sobre o SQLite. Literal AOSP ainda falta: result codes exatos, WAL/locale/custom functions/locking e todas as nuances de overloads — por isso é partial no Apêndice C, não pacote completo"),
            ("m8-services-broadcasts-deeplinks", "vm", "supported", "Context.startService/startForegroundService/stopService/bindService registrados (Engine::m8_started_services); sendBroadcast/sendOrderedBroadcast registram e entregam a receivers registrados (onReceive DEX); registerReceiver/unregisterReceiver; Intent setAction/getAction/setData/getData + Uri.parse/getScheme/getHost/getPath; startActivity VIEW/deep link registra (m8_deep_links) e navega quando há handler registrado (m8_register_deep_link) com classe no DEX; JobScheduler host (schedule/getAllPendingJobs/cancel)"),
            ("m8-workmanager", "vm", "supported", "WorkManager host: getInstance canônico, enqueue/enqueueUnique* registram WorkRecord (worker class do WorkRequest, estado ENQUEUED; m8_complete_work -> SUCCEEDED; cancel* -> CANCELLED), Operation/ListenableFuture completados. Sem execução em background (single-thread determinístico; Worker.doWork não roda em thread — documentado)"),
            ("m8-shared-preferences", "data", "supported", "SharedPreferences STATEFUL + PERSISTENTE: getSharedPreferences(name)/Activity.getPreferences/getDefaultSharedPreferences usam objetos canônicos por nome/pacote; Editor putString/putStringSet/putInt/putLong/putFloat/putBoolean/remove/clear + commit/apply aplicam e gravam shared_prefs/<nome>.xml no VFS app-private; getters tipados leem o XML entre launches da Engine (tipo errado -> ClassCastException); listeners/contains/getAll reais; key/nome null -> NPE. Sem reload multi-processo nem enforcement completo de modes"),
            (
                "java-util-collections",
                "vm",
                "partial",
                "HashMap/WeakHashMap/ArrayList/LinkedList/CopyOnWriteArrayList host (WeakHashMap: chaves fracas, valores fortes, null permanente; limpeza no GC) com métodos essenciais (put/get/add/remove/size/…); CopyOnWriteArraySet host com add que deduplica; subtipagem completa (check-cast p/ List/Set/Map/Collection/Iterable); LinkedHashMap host (≡ HashMap; estado Vec = ordem de inserção); entrySet/keySet/values (views snapshot HashSet) + Iterator host (hasNext/next; remove escreve na coleção/mapa de origem com ISE antes de next e após remoção dupla; iteradores COW/imutáveis continuam UOE) + Map.Entry host (getKey/getValue/setValue); equals de usuário não consultado em chaves; Collections.synchronized* devolve a própria coleção (identidade — single-threaded); ArrayDeque host (Deque completo: addFirst/Last, push/pop, peek/poll, occurrences; null rejeitado, NSE em vazio); Collections.unmodifiableMap/unmodifiableSet/singletonList host: views de SNAPSHOT em classes-marcadoras JDK (Collections$UnmodifiableMap/$UnmodifiableSet/$SingletonList) — leituras delegam, mutações → UOE tipada, null → NPE, sem estado host → VM_TYPE_ERROR tipado; singletonList(null) aceito (JDK); empty*/outros unmodifiable* seguem fora. Set.<init>(Collection) host: snapshot dedup na ordem do iterador (NPE em null; VM_TYPE_ERROR sem estado host). Enum é host (name/ordinal/toString/compareTo — inclusive herdado por enums do app via subtipagem)",
            ),
            (
                "java-lang-ref",
                "vm",
                "partial",
                "WeakReference/SoftReference host (get/clear); o GC mark-sweep limpa o referent de WeakReference/PhantomReference quando só ela o alcança (SoftReference fica forte) e, com fila registrada, a enfileira na ReferenceQueue (poll/remove não bloqueantes; vazia → null). AtomicReferenceFieldUpdater é host (newUpdater/get/set/CAS). AtomicInteger é host (get/set/CAS/increment/add com overflow wrapping). Executors/ExecutorService são host com execução INLINE (single-threaded): submit devolve Future já completado; exceções do Runnable propagam na hora. Object.getClass/Class host (getName/getSimpleName/toString; identidade canônica). ConcurrentHashMap host (≡ HashMap; hierarquia JDK: AbstractMap direto + ConcurrentMap). LinkedBlockingQueue host (FIFO unbounded; take/poll bloqueante com fila vazia → IllegalStateException — fail-fast honesto anti-deadlock). ThreadLocal/InheritableThreadLocal host (get com initialValue DEX memoizado; set/remove; override vence). System.getProperty host (tabela da plataforma; desconhecida → null; (key,def) → def). Class.getClassLoader host (ClassLoader canônico por VM). System.getSecurityManager → null (Android não tem SecurityManager). ServiceLoader host com descoberta REAL: load(Class)/(Class,ClassLoader) lê META-INF/services/<binário> do APK anexado (# comenta; linhas vazias puladas); iterator() INSTANCIA os providers encontrados via DEX (clinit+<init>()V) e devolve iterador do snapshot (eager, não lazy como JDK — desvio documentado); provider ausente → ServiceConfigurationError tipado; sem APK/arquivo → loader vazio (hasNext false; next → NoSuchElementException). LinkedHashSet host preserva inserção e suporta init/add/remove/contains/size/clear/iterator. ClassLoader.getResources(String) host → Enumeration vazia (contrato JDK: nunca null; name null → NPE; cursor ≡ Iterator com hasMoreElements/nextElement e NoSuchElementException em vazio) — entries do APK não são enumerados como classpath e META-INF/services agora lido (ServiceLoader). ArrayList.<init>(Collection) host: snapshot na ordem do iterador da fonte (duplicatas preservadas; sem aliasing); fonte null → NPE; coleção do app sem estado host → VM_TYPE_ERROR tipado. Idem LinkedList/CopyOnWriteArrayList via helper compartilhado. LinkedBlockingQueue.drainTo(Collection)/(Collection,int) host: drena até max na ORDEM FIFO respeitando o add do destino (Set deduplica); retorno conta os retirados (JDK); null → NPE, drainTo(this) → IAE, max<=0 → 0, destino do app → VM_TYPE_ERROR tipado. Sequence java.util do launch real COMPLETA. Activity.getFragmentManager() host: FragmentManager legacy canônico por VM (identidade estável, lazy); estado de fragments (back stack/transações) ainda não existe. FragmentManager.findFragmentByTag(String)/findFragmentById(int) host → null enquanto não há transações committadas (contrato Android; tag null também devolve null). FragmentTransaction host: beginTransaction → tx nova; add(Fragment,String)/add(int,Fragment)/(int,Fragment,String)/replace/remove fluentes com ops pendentes; commit/commitAllowingStateLoss aplicam no registro (validação ANTES — sem aplicação parcial; Add de fragment já registrada → ISE; 2º commit/uso pós-commit → ISE; fragment null → NPE; commit devolve id opaco 0 — sem back stack/animations/lifecycle DEX). findFragmentByTag/Id consultam o registro (última registrada vence; containerId 0 nunca casa). Fragment.<init>()V host: no-op (a fragment do app chama o super via invokespecial — sem estado de ctor, como Activity/Application/Context). FragmentManager.executePendingTransactions() → false sempre (commit aplica sincronamente — nunca há fila pendente; AOSP \"nothing was executed\"). array.clone() host ([LX;/[I/...): shallow copy com mesmo descritor/ElemKind (JLS 10.7 — cobre values()/$VALUES.clone() de enums como androidx Lifecycle$Event). Enum.clone() → CloneNotSupportedException capturável (final na JDK/libcore — nunca copia). Regime pós-bootstrap: sondagem seguinte já mostra exceção Java materializada (NPE invoke receiver null) — próximas investigações pedem tracing de call site — ENTREGUE: VmConfig.exception_trace + rd app run --trace (linhas at estilo JVM no unwind, innermost-first; off por default — golden vectors intactos) e postmortem Engine::executed_labels (cobertura de entrada, AG-09) no VM_EXCEPTION com --trace — responde rodou/não-rodou após o engine morrer. Sondagem real: NPE pós-bootstrap localizado em androidx FragmentManager.dispatchCreate e o postmortem provou attachController/attachHost/FragmentActivity.init NUNCA executados (init registra OnContextAvailableListener→attachHost; o DEX de dispatchCreate chegou ao frame do NPE pelas linhas de trace) — androidx Fragments (attachController e cia.) era o suspeito — REVISADO na iteração seguinte: o problema NÃO era estado androidx ausente, e sim DOIS bugs de semântica DEX de invoke na VM: (1) invoke-super iniciava a busca em super(mref.class) em vez da super da classe definidora do frame (FragmentActivity.onCreate pulava androidx/activity/ComponentActivity.onCreate inteiro e attachHost nunca rodava), (2) as variantes /range de invoke-virtual (0x74) e invoke-direct (0x76) estavam TROCADAS (conforme spec Dalvik: 0x70/0x76=direct — ctor Kotlin de lambda FunctionReferenceImpl re-executava o próprio <init> em loop até VM_STACK_OVERFLOW/abort); + <init>()V no-op das coleções abstratas host (AbstractList e cia.) para subclasses DEX diretas. O launch real sai do NPE e do overflow e avança até Activity.getApplication() (próximo gap NOT_FOUND). empty* de Collections, Views live (não snapshot) e ctors de slices de coleções (ArrayList(Collection) etc. já cobertos; guarde Set/Map cobertos como snapshots) seguem fora. Rodada seguinte da cadeia real (todas ENTREGUES): getApplication/getApplicationContext → Application canônica do manifest; cadeia java.util do LogController (unmodifiableMap/singletonList/HashSet(Collection)/unmodifiableSet); PreferenceManager.getDefaultSharedPreferences → SharedPreferencesImpl vazio host (sem persistência; edit segue fora); ServiceLoader discovery real. Resultado: o launch executa PftpdApp.onCreate + LogController.init/readPrefs completos e morre em ClassCastException tipada (NOPLoggerFactory → LoggerContext) — o APK golden não embarca META-INF/services/strip do gradle; slf4j cai no fallback NOP e o cast incondicional do setActiveConfig é o comportamento-fiel que um dispositivo real também entregaria com este APK.",
            ),
            (
                "android-framework-clock",
                "vm",
                "partial",
                "SystemClock host (uptimeMillis/elapsedRealtime(+Nanos)/currentThreadTimeMillis/sleep) sobre o relógio VIRTUAL determinístico da VM (mesmo clock do postDelayed/advance_clock; sleep avança e drena o Looper — sem tempo de parede). Looper getMainLooper/loop/getThread host. Thread host single-threaded: currentThread/getName (fixo main)/getId/isDaemon/isAlive + sleep avança o relógio virtual (yield no-op); start/interrupt fora. System.currentTimeMillis/nanoTime seguem fora",
            ),
            (
                "notifications",
                "ui",
                "not-implemented",
                "M9: channels, notification builders, PendingIntents, actions, listener callbacks, timeout/cancel and agent controls; platform visual notification shade not modeled",
            ),
            (
                "invoke-string-args",
                "probe",
                "not-implemented",
                "sonda invoke: arg String exige alocação de objeto no caller — use método auxiliar sem args de objeto",
            ),
            (
                "invoke-array-args",
                "probe",
                "not-implemented",
                "sonda invoke: args de array fora do escopo v1 — empacote num método auxiliar",
            ),
            (
                "focus-model",
                "input",
                "partial",
                "M9 focused EditText with UTF-16 selection/composition; initial typing auto-focuses first visible enabled editor",
            ),
            (
                "key-events",
                "input",
                "partial",
                "teclas BACK/ENTER/DEL apenas; demais caem em NOT_IMPLEMENTED tipado",
            ),
            (
                "swipe-gestures",
                "input",
                "partial",
                "swipe vira clique quando down/up caem na mesma view; scroll/fling só relatório (sem efeito)",
            ),
            (
                "start-activity-implicit",
                "intent",
                "not-implemented",
                "startActivity com intent implícito/action — navegação v1 é por componente",
            ),
            (
                "http-transport",
                "mcp",
                "not-implemented",
                "transporte stdio apenas (ADR 001); HTTP é opcional e exige política de segurança própria",
            ),
            (
                "ime-widgets",
                "ui",
                "not-implemented",
                "M9 IME InputConnection and RemoteViews widget rendering; unsupported methods fail typed (docs/m9.md)",
            ),
        ];
        let gaps: Vec<serde_json::Value> = GAPS
            .iter()
            .map(|(id, area, status, note)| {
                serde_json::json!({
                    "id": id,
                    "area": area,
                    "status": status,
                    "note": note,
                })
            })
            .collect();
        // Apêndice C executável: mesmo registro que `rd api-coverage` e o
        // relatório gerado consomem. Isto não substitui os gaps narrativos
        // acima; dá ao agente a contagem auditável por domínio/status.
        let api_table = rd_framework::fd_api_coverage::ApiCoverageTable::embedded()?;
        Ok(ToolResult::json(&serde_json::json!({
            "gaps": gaps,
            "api_coverage": {
                "source": api_table.source,
                "data": "data/api-coverage.toml",
                "api_floor": api_table.api_floor,
                "api_ceiling": api_table.api_ceiling,
                "domains": api_table.domain.len(),
                "surfaces": api_table.surface.len(),
                "by_status": api_table.status_counts(),
                "by_domain": api_table.domain_summaries(),
                "hint": "get_api_usage classifica as referências DEX de um app contra este registro; get_coverage continua sendo a cobertura de execução",
            },
            "hint": "behavior_switches lista os comutadores targetSdk ativos (PRF-07)",
        })))
    }

    /// Batch (M5): executa chamadas EM ORDEM, uma vez cada; retomada
    /// idempotente via `resume_from` (pula os N primeiros passos já
    /// aplicados). Sem batch aninhado; cap de passos pela política.
    fn tool_batch(
        &mut self,
        calls: Option<&serde_json::Value>,
        stop_on_error: bool,
        resume_from: usize,
    ) -> Result<ToolResult, RdError> {
        let Some(list) = calls.and_then(|c| c.as_array()) else {
            return Err(agent_error(
                "INVALID_FORMAT",
                "batch exige calls: [{\"name\": …, \"arguments\": {…}}]",
                Some("veja o schema da tool (tools/list)"),
            ));
        };
        if list.len() > self.policy.max_batch_steps {
            return Err(agent_error(
                "LIMIT_EXCEEDED",
                format!(
                    "batch com {} passos excede o teto de {} (RD_MCP_MAX_BATCH)",
                    list.len(),
                    self.policy.max_batch_steps
                ),
                Some("divida em lotes menores ou eleve RD_MCP_MAX_BATCH no servidor"),
            ));
        }
        if resume_from > list.len() {
            return Err(agent_error(
                "INVALID_FORMAT",
                format!(
                    "resume_from={resume_from} excede o total de passos ({})",
                    list.len()
                ),
                Some("resume_from é o NÚMERO de passos já executados a pular"),
            ));
        }
        // issue #80: validar o lote INTEIRO antes de executar qualquer passo.
        // As checagens viviam dentro do laço: um passo inválido no meio
        // abortava DEPOIS de executar os anteriores, e o erro devolvido não
        // trazia `steps`, `executed` nem `resume_hint` — o cliente ficava sem
        // saber o que mudou no app nem de onde retomar (retomada nada
        // idempotente). Política escolhida: formato inválido → NADA executa.
        for (i, c) in list.iter().enumerate() {
            let step = c.as_object().ok_or_else(|| {
                agent_error(
                    "INVALID_FORMAT",
                    format!("passo {i} não é um objeto"),
                    Some("cada passo é {\"name\": …, \"arguments\": {…}}"),
                )
            })?;
            let name = step.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("passo {i} sem \"name\""),
                    Some("informe o nome da tool (ex.: {\"name\": \"launch\", \"arguments\": {}})"),
                ));
            }
            if name == "batch" {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("batch não aninha (passo {i})"),
                    Some("chame as sub-chamadas direto no lote"),
                ));
            }
            if let Some(args) = step.get("arguments") {
                if !args.is_object() {
                    return Err(agent_error(
                        "INVALID_FORMAT",
                        format!("passo {i} ({name:?}): \"arguments\" precisa ser um objeto"),
                        Some("use {\"arguments\": {…}} ou omita o campo"),
                    ));
                }
            }
        }
        let mut steps: Vec<serde_json::Value> = Vec::new();
        let mut executed = 0usize;
        let mut stopped_early = false;
        for (i, c) in list.iter().enumerate() {
            if i < resume_from {
                steps.push(serde_json::json!({
                    "step": i,
                    "name": c.get("name").cloned().unwrap_or(serde_json::json!(null)),
                    "skipped": true,
                }));
                continue;
            }
            // issue #80: o formato já foi validado para o lote inteiro acima
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = c.get("arguments").cloned().unwrap_or(serde_json::json!({}));
            let r = self.call_tool(name, &args);
            let content_json: serde_json::Value = match &r.content[0] {
                ToolContent::Text(t) => serde_json::from_str(t).unwrap_or(serde_json::json!(t)),
                ToolContent::Image { mime, .. } => serde_json::json!({"image": mime}),
            };
            let errored = r.is_error;
            steps.push(serde_json::json!({
                "step": i,
                "name": name,
                "ok": !errored,
                "result": content_json,
            }));
            executed += 1;
            if errored && stop_on_error {
                stopped_early = true;
                break;
            }
        }
        Ok(ToolResult::json(&serde_json::json!({
            "session_id": self.session_id,
            "total_steps": list.len(),
            "executed": executed,
            "stopped_early": stopped_early,
            // issue #81: `executed` só conta o que rodou NESTA chamada; num
            // lote retomado (resume_from > 0) a comparação tem de incluir os
            // passos já pulados — senão um lote COMPLETO devolvia um
            // resume_hint não nulo, prometendo trabalho pendente que não há.
            "resume_hint": if stopped_early || executed + resume_from < list.len() {
                serde_json::json!(executed + resume_from)
            } else {
                serde_json::json!(null)
            },
            "steps": steps,
        })))
    }

    /// AG-09 (metade estática): uso de APIs android.* referenciadas nos DEX.
    fn tool_api_usage(&mut self, package: Option<&str>) -> Result<ToolResult, RdError> {
        let pkg = package
            .map(str::to_string)
            .or_else(|| self.active.clone())
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro (tool install)", None))?;
        let app = self.find_installed(&pkg).ok_or_else(|| {
            agent_error(
                "PACKAGE_NOT_INSTALLED",
                format!("package {pkg:?} não está no registro desta sessão"),
                Some("veja package_list — instale com `install` antes de analisar"),
            )
        })?;
        let _ = app;
        let apk = self
            .find_installed_mut(&pkg)
            .ok_or_else(|| agent_error("NO_APP", "app sumiu do registro", None))?
            .apk()?;
        let dexes = dexes_of_apk(&apk)?;
        let usage = crate::api_usage::scan(&apk.manifest.package, &dexes);
        let v = serde_json::to_value(usage).map_err(|e| {
            agent_error(
                "INTERNAL",
                format!("serializando o relatório de uso de API: {e}"),
                None,
            )
        })?;
        Ok(ToolResult::json(&v))
    }

    /// AG-09 (metade dinâmica): get_coverage reporta a cobertura de execução
    /// em nível de método do app em uso — instrumentação no interpretador
    /// (rd-vm), não análise estática. NOT_RUNNING sem app lançado.
    fn tool_coverage(&mut self) -> Result<ToolResult, RdError> {
        let pkg = self.engine_package.clone();
        let eng = self.require_engine()?;
        let snap = eng.coverage_snapshot();
        // pct com 1 casa decimal; 0 métodos (app sem corpos DEX) → 0.0
        let coverage_pct = if snap.methods_total > 0 {
            (f64::from(snap.methods_executed) * 1000.0 / f64::from(snap.methods_total)).round()
                / 10.0
        } else {
            0.0
        };
        let v = serde_json::json!({
            "package": pkg,
            "analysis": "method-entry-instrumentation",
            "classes_total": snap.classes_total,
            "methods_total": snap.methods_total,
            "methods_executed": snap.methods_executed,
            "coverage_pct": coverage_pct,
            "classes": snap.classes.iter().map(|c| serde_json::json!({
                "class": c.class,
                "methods_total": c.methods_total,
                "methods_executed": c.methods_executed,
            })).collect::<Vec<_>>(),
        });
        Ok(ToolResult::json(&v))
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn rd_to_rd(e: RdError) -> RdError {
    RdError::new(&e.code, &e.cause, &e.module_id).maybe_suggestion(&e.suggestion)
}

fn vm_exit_to_rd(e: VmExit) -> RdError {
    match e {
        VmExit::Error(rd) => {
            RdError::new(&rd.code, &rd.cause, &rd.module_id).maybe_suggestion(&rd.suggestion)
        }
        VmExit::Exception(t) => RdError::new(
            "JAVA_EXCEPTION",
            format!(
                "{}: {}",
                t.name_without_l(),
                t.message.clone().unwrap_or_default()
            ),
            MODULE_ID,
        ),
    }
}

trait MaybeSuggestion {
    fn maybe_suggestion(self, s: &Option<String>) -> Self;
}
impl MaybeSuggestion for RdError {
    fn maybe_suggestion(mut self, s: &Option<String>) -> Self {
        if let Some(s) = s {
            self = self.with_suggestion(s.clone());
        }
        self
    }
}

fn grant_state_str(s: GrantState) -> &'static str {
    match s {
        GrantState::AutoGranted => "auto-granted",
        GrantState::Granted => "granted",
        GrantState::Denied => "denied",
        // issue #110: signature-class fora da whitelist B5 — negada a terceiros
        GrantState::DeniedSystem => "denied-system",
        GrantState::RuntimePending => "runtime-pending",
        GrantState::SpecialPending => "special-pending",
        GrantState::Unknown => "unknown",
        GrantState::NotRequested => "not-requested",
        // issue #69: permissão declarada pelo próprio APK
        GrantState::AppDefined => "app-defined",
    }
}

/// issue #79: critério de busca vazio (`""`) casava TUDO — `contains("")` e
/// `ends_with("")` são sempre verdadeiros — então `assert_view {}` passava sem
/// verificar nada (falso verde). Critérios vazios são entrada inválida.
fn validate_view_criteria(
    tool: &str,
    resource_id: Option<&str>,
    text: Option<&str>,
    class: Option<&str>,
) -> Result<(), RdError> {
    if resource_id.is_none() && text.is_none() && class.is_none() {
        return Err(agent_error(
            "INVALID_FORMAT",
            format!("{tool} exige resource_id, text ou class"),
            None,
        ));
    }
    for (label, value) in [
        ("resource_id", resource_id),
        ("text", text),
        ("class", class),
    ] {
        if let Some(v) = value {
            if v.trim().is_empty() {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("{tool}: {label} vazio não é um critério de busca"),
                    Some(
                        "informe um valor não vazio ou omita o campo (ex.: {\"text\": \"Ok\"}, {\"class\": \"android.widget.TextView\"})",
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Busca DFS na árvore de UI por resource-id/text/class.
/// issue #78: casamento de classe por nome COMPLETO ou pelo nome SIMPLES
/// exato (segmento após o último `.`). O `ends_with` solto fazia `class="View"`
/// casar `android.widget.TextView` — a asserção deixava de ser falsificável.
fn class_matches(node_class: &str, want: &str) -> bool {
    node_class == want || node_class.rsplit('.').next().unwrap_or(node_class).eq(want)
}

fn collect_matches(
    n: &UiNode,
    rid: Option<&str>,
    text: Option<&str>,
    class: Option<&str>,
    out: &mut Vec<serde_json::Value>,
) {
    // issue #79: critério vazio é ignorado aqui também (a validação da tool já
    // o recusa, mas `collect_matches` não pode casar tudo por `contains("")`).
    let rid = rid.filter(|r| !r.trim().is_empty());
    let text = text.filter(|t| !t.trim().is_empty());
    let class = class.filter(|c| !c.trim().is_empty());
    let rid_hit = rid.map_or(true, |r| n.resource_id.as_deref() == Some(r));
    let text_hit = text.map_or(true, |t| n.text.contains(t));
    let class_hit = class.map_or(true, |c| class_matches(&n.class, c));
    if rid_hit && text_hit && class_hit {
        out.push(serde_json::json!({
            "class": n.class,
            "resource_id": n.resource_id,
            "text": n.text,
            "bounds": [n.bounds.0, n.bounds.1, n.bounds.2, n.bounds.3],
            "clickable": n.clickable,
            "enabled": n.enabled,
        }));
    }
    for c in &n.children {
        collect_matches(c, rid, text, class, out);
    }
}

fn find_text_by_rid(n: &UiNode, rid: &str, out: &mut Option<String>) {
    if out.is_some() {
        return;
    }
    if n.resource_id.as_deref() == Some(rid) {
        *out = Some(n.text.clone());
        return;
    }
    for c in &n.children {
        find_text_by_rid(c, rid, out);
    }
}

/// Args JSON → Value pela assinatura (sonda de estado tipada).
fn json_args_to_values(sig: &str, args: &[serde_json::Value]) -> Result<Vec<Value>, RdError> {
    let inner = sig
        .strip_prefix('(')
        .and_then(|s| s.split(')').next())
        .ok_or_else(|| agent_error("INVALID_FORMAT", format!("sig malformada: {sig}"), None))?;
    let mut types = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'I' | 'J' | 'F' | 'D' | 'Z' | 'C' | 'B' | 'S' => types.push(c.to_string()),
            'L' => {
                let mut t = String::from("L");
                for c2 in chars.by_ref() {
                    t.push(c2);
                    if c2 == ';' {
                        break;
                    }
                }
                types.push(t);
            }
            '[' => {
                return Err(agent_error(
                    "NOT_IMPLEMENTED",
                    "argumentos de tipo array fora do escopo da sonda v1",
                    Some("empacote o array num método auxiliar do app"),
                ))
            }
            _ => {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("tipo de parâmetro desconhecido '{c}' em {sig}"),
                    None,
                ))
            }
        }
    }
    if types.len() != args.len() {
        return Err(agent_error(
            "INVALID_FORMAT",
            format!(
                "{sig} pede {} argumentos, recebi {}",
                types.len(),
                args.len()
            ),
            None,
        ));
    }
    let mut out = Vec::new();
    for (ty, a) in types.iter().zip(args) {
        let v = match ty.as_str() {
            "I" => Value::Int(
                i32::try_from(a.as_i64().ok_or_else(|| bad_arg(ty, a))?)
                    .map_err(|_| bad_range(ty, a, "i32 (-2147483648..=2147483647)"))?,
            ),
            "J" => Value::Long(a.as_i64().ok_or_else(|| bad_arg(ty, a))?),
            "F" => Value::Float(a.as_f64().ok_or_else(|| bad_arg(ty, a))? as f32),
            "D" => Value::Double(a.as_f64().ok_or_else(|| bad_arg(ty, a))?),
            "Z" => Value::Int(a.as_bool().ok_or_else(|| bad_arg(ty, a))? as i32),
            "C" => {
                let s = a.as_str().ok_or_else(|| bad_arg(ty, a))?;
                let c = s.chars().next().ok_or_else(|| bad_arg(ty, a))?;
                Value::Int(c as i32)
            }
            // issue #85: `as i8`/`as i16` truncavam em silêncio — 128 virava
            // -128 e 40000 virava -25536, sem nenhum erro.
            "B" => Value::Int(
                i8::try_from(a.as_i64().ok_or_else(|| bad_arg(ty, a))?)
                    .map_err(|_| bad_range(ty, a, "i8 (-128..=127)"))? as i32,
            ),
            "S" => Value::Int(
                i16::try_from(a.as_i64().ok_or_else(|| bad_arg(ty, a))?)
                    .map_err(|_| bad_range(ty, a, "i16 (-32768..=32767)"))? as i32,
            ),
            "Ljava/lang/String;" => {
                // alocação de objeto exige engine no site do caller — a sonda
                // v1 aceita só primitivos (blocos de estado puro do app)
                let _ = a;
                return Err(agent_error(
                    "NOT_IMPLEMENTED",
                    "argumento String na sonda v1: alocação de objeto exige engine — use um método auxiliar sem args de objeto",
                    None,
                ));
            }
            t => {
                return Err(agent_error(
                    "NOT_IMPLEMENTED",
                    format!("tipo de argumento {t} fora do escopo da sonda v1"),
                    None,
                ))
            }
        };
        out.push(v);
    }
    Ok(out)
}

fn bad_arg(ty: &str, a: &serde_json::Value) -> RdError {
    agent_error(
        "INVALID_FORMAT",
        format!("argumento {a} não é compatível com o tipo {ty}"),
        None,
    )
}

/// issue #85: argumento fora da faixa do tipo do DEX — erro estruturado em vez
/// da conversão com `as`, que truncava em silêncio.
fn bad_range(ty: &str, a: &serde_json::Value, range: &str) -> RdError {
    agent_error(
        "INVALID_FORMAT",
        format!("argumento {ty} = {a} fora da faixa {range}"),
        Some("a sonda v1 não trunca — use um valor dentro da faixa do tipo"),
    )
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Int(i) => serde_json::json!(i),
        Value::Long(l) => serde_json::json!(l),
        Value::Float(f) => serde_json::json!(f),
        Value::Double(d) => serde_json::json!(d),
        Value::Obj(_) => serde_json::json!("(objeto)"),
        Value::Null => serde_json::json!(null),
        Value::WideHi => serde_json::json!(null),
        // placeholder interno da VM nunca escapa para o agente (Lei 1: honesto)
        Value::StrPlaceholder(i) => serde_json::json!(format!("(string#{i})")),
    }
}

/// base64 padrão (encoder próprio — zero deps, mesmo padrão do PNG do render).
fn base64_encode(data: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TBL[(n >> 18) as usize & 63] as char);
        out.push(TBL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TBL[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TBL[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ── envelope JSON-RPC 2.0 (MCP stdio) ───────────────────────────────────────

pub fn tools_list_json() -> serde_json::Value {
    serde_json::json!({
        "tools": tool_defs().iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.schema,
        })).collect::<Vec<_>>(),
    })
}

/// Documentação GERADA do catálogo (DoD M5: o conjunto publicado corresponde
/// ao `--describe-tools` e à doc). O mesmo gerador alimenta
/// `rd mcp --gen-docs` e o teste de consistência contra docs/mcp-tools.md.
pub fn tools_markdown() -> String {
    let mut out = String::new();
    out.push_str("<!-- GERADO: `rd mcp --gen-docs docs/mcp-tools.md` — NÃO editar à mão. -->\n\n");
    out.push_str("# Ferramentas do Agente MCP (M5)\n\n");
    out.push_str(&format!(
        "Servidor `{SERVER_NAME}` v{SERVER_VERSION} · protocolo `{PROTOCOL_VERSION}` · \
         transporte stdio (JSON-RPC 2.0, uma mensagem por linha).\n\n"
    ));
    out.push_str(&format!(
        "Total: **{} tools**. Este catálogo é gerado de `rd_agent::mcp::tool_defs()` — \
         o MESMO registro respondido em `tools/list` e impresso por `rd mcp --describe-tools`.\n\n",
        tool_defs().len()
    ));
    out.push_str("## Controles de segurança\n\n");
    out.push_str("- **Sandbox de caminhos**: `install`/`screenshot` só acessam arquivos dentro das raízes `RD_MCP_SANDBOX` (default: cwd + temp do sistema); `..` e URLs são recusados (`PATH_OUTSIDE_SANDBOX`/`URL_NOT_ALLOWED`).\n");
    out.push_str("- **APK excessivo**: teto de tamanho `RD_MCP_MAX_APK_MB` (default 256 MB) — checado antes de ler (`APK_TOO_LARGE`).\n");
    out.push_str("- **Tempo ilimitado**: `wait`/`swipe`/`wait_for` têm teto `RD_MCP_MAX_WAIT_MS` (default 60 000 ms) (`LIMIT_EXCEEDED`).\n");
    out.push_str("- **Batch**: teto de passos `RD_MCP_MAX_BATCH` (default 64); sem aninhamento; retomada idempotente via `resume_from`.\n");
    out.push_str("- **Estado consistente**: install/remove/clear são transacionais (validam tudo antes de mutar); app em execução é encerrado antes de remove/clear/reinstall do mesmo package.\n\n");
    out.push_str("## Erros\n\n");
    out.push_str("Falhas de tool voltam com `isError: true` e o contrato estável `{code, cause, suggestion?, module_id}`. Erros de PROTOCOLO são JSON-RPC (-32601 método, -32700 parse).\n\n");
    out.push_str("## Catálogo\n\n");
    for t in tool_defs() {
        let schema = &t.schema;
        let props = schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|p| p.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push_str(&format!("### `{}`\n\n{}\n\n", t.name, t.description));
        out.push_str(&format!(
            "- Parâmetros: {}{}\n\n",
            if props.is_empty() {
                "nenhum".to_string()
            } else {
                format!("`{props}`")
            },
            if required.is_empty() {
                String::new()
            } else {
                format!(" · obrigatórios: `{required}`")
            },
        ));
    }
    // Keep generated Markdown canonical: one final newline, no blank EOF line.
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// Processa UMA linha JSON-RPC. `None` = notification (não responder).
/// `Err(json)` = erro de protocolo (responder com "error").
pub fn handle_message(
    session: &mut AgentSession,
    line: &str,
) -> Option<Result<serde_json::Value, serde_json::Value>> {
    let msg: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(Err(jsonrpc_error(
                Option::<serde_json::Value>::None,
                -32700,
                &format!("parse error: {e}"),
            )))
        }
    };
    // issue #70: JSON-RPC 2.0 §6 (Batch) — um array é um LOTE. Antes o array
    // caía em `msg.get("method")` → `""` → `-32601 "método '' não existe"` e
    // TODAS as requisições do lote eram descartadas em silêncio.
    if let Some(items) = msg.as_array() {
        return handle_batch(session, items);
    }
    handle_single(session, msg)
}

/// Lote JSON-RPC 2.0: uma resposta por requisição, na mesma ordem.
/// Notificações não geram resposta; lote só de notificações → sem resposta.
/// Lote vazio ou elemento inválido → `-32600 Invalid Request`.
fn handle_batch(
    session: &mut AgentSession,
    items: &[serde_json::Value],
) -> Option<Result<serde_json::Value, serde_json::Value>> {
    if items.is_empty() {
        return Some(Err(jsonrpc_error(
            Option::<serde_json::Value>::None,
            -32600,
            "lote vazio: JSON-RPC 2.0 exige ao menos uma requisição",
        )));
    }
    let mut responses: Vec<serde_json::Value> = Vec::new();
    for item in items {
        match handle_single(session, item.clone()) {
            None => continue, // notificação: sem resposta
            Some(Ok(v)) | Some(Err(v)) => responses.push(v),
        }
    }
    if responses.is_empty() {
        None
    } else {
        Some(Ok(serde_json::Value::Array(responses)))
    }
}

/// Uma mensagem (objeto). Estrutura inválida → `-32600`, método desconhecido
/// → `-32601`. Notificações (sem `id`) não respondem (§4.1 — issue #71).
fn handle_single(
    session: &mut AgentSession,
    msg: serde_json::Value,
) -> Option<Result<serde_json::Value, serde_json::Value>> {
    if !msg.is_object() {
        return Some(Err(jsonrpc_error(
            Option::<serde_json::Value>::None,
            -32600,
            "requisição inválida: esperado um objeto JSON-RPC 2.0 (ou um lote)",
        )));
    }
    let id = msg.get("id").cloned();
    let is_notification = id.is_none();
    // issue #70: NUNCA mais `unwrap_or("")` — `method` ausente/não-string é
    // Invalid Request (-32600), não Method Not Found com nome vazio.
    let method = match msg.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            return Some(Err(jsonrpc_error(
                id,
                -32600,
                "requisição inválida: `method` ausente ou não é string",
            )))
        }
    };

    match method {
        "initialize" => Some(Ok(jsonrpc_result(
            id,
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {},
                    "resources": { "subscribe": true, "listChanged": false },
                },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            }),
        ))),
        "notifications/initialized" => {
            // notification: sem resposta (convenção MCP)
            if is_notification {
                None
            } else {
                Some(Ok(jsonrpc_result(id, serde_json::json!({}))))
            }
        }
        "tools/list" => Some(Ok(jsonrpc_result(id, tools_list_json()))),
        "tools/call" => {
            let name = msg
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let empty = serde_json::json!({});
            let args = msg.pointer("/params/arguments").unwrap_or(&empty);
            let t0 = std::time::Instant::now();
            let result = session.call_tool_isolated(&name, args);
            session.screen_after_call(&name, t0.elapsed().as_secs_f64() * 1000.0);
            let content: Vec<serde_json::Value> = result
                .content
                .iter()
                .map(|c| match c {
                    ToolContent::Text(t) => serde_json::json!({"type": "text", "text": t}),
                    ToolContent::Image { data_b64, mime } => serde_json::json!({
                        "type": "image", "data": data_b64, "mimeType": mime,
                    }),
                })
                .collect();
            // M5: TODO resultado carrega os identificadores de sessão e do
            // aplicativo (campos extras no envelope — clientes MCP toleram).
            let mut result_json = serde_json::json!({
                "content": content,
                "isError": result.is_error,
                "sessionId": session.session_id,
            });
            if let Some(pkg) = session
                .engine_package
                .as_deref()
                .or(session.active.as_deref())
            {
                result_json["package"] = serde_json::json!(pkg);
            }
            Some(Ok(jsonrpc_result(id, result_json)))
        }
        "ping" => Some(Ok(jsonrpc_result(id, serde_json::json!({})))),
        "resources/list" => Some(Ok(jsonrpc_result(id, AgentSession::resources_list()))),
        "resources/templates/list" => Some(Ok(jsonrpc_result(
            id,
            serde_json::json!({ "resourceTemplates": [] }),
        ))),
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => {
            let Some(uri) = msg.pointer("/params/uri").and_then(|v| v.as_str()) else {
                return Some(Err(jsonrpc_error(
                    id,
                    -32602,
                    "params.uri (string) é obrigatório",
                )));
            };
            let r = match method {
                "resources/read" => session.resource_read(uri),
                m => session
                    .resource_subscribe(uri, m == "resources/subscribe")
                    .map(|()| serde_json::json!({})),
            };
            Some(match r {
                Ok(v) => Ok(jsonrpc_result(id, v)),
                Err((code, m)) => Err(jsonrpc_error(id, code, &m)),
            })
        }
        m if m.starts_with("notifications/") => None, // notificações conhecidas: silêncio
        other => {
            // issue #71: JSON-RPC 2.0 §4.1 — o servidor NÃO responde a
            // notificações (mensagem sem `id`), nem quando o método é
            // desconhecido. Responder com `id: null` faz o cliente confundir o
            // erro com a resposta de uma requisição pendente.
            if is_notification {
                eprintln!("[rd mcp] notificação ignorada (método desconhecido): {other}");
                None
            } else {
                Some(Err(jsonrpc_error(
                    id,
                    -32601,
                    &format!("método '{other}' não existe (initialize/tools/list/tools/call/resources/list|read|subscribe|unsubscribe/ping)"),
                )))
            }
        }
    }
}

fn jsonrpc_result(id: Option<serde_json::Value>, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: Option<serde_json::Value>, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Loop stdio: uma linha = uma mensagem (LSP-style). Termina em EOF.
pub fn serve_stdio<R: std::io::Read, W: std::io::Write>(
    input: R,
    output: W,
) -> Result<(), RdError> {
    serve_stdio_with(input, output, std::env::var("RD_MCP_LIVE").ok().as_deref())
}

/// [`serve_stdio`] with an optional live viewer address (`rd mcp --live`):
/// a std-only HTTP server showing the current frame, visible texts and tap
/// targets, updated after every tool call (long-poll, no polling storm).
pub fn serve_stdio_with<R: std::io::Read, W: std::io::Write>(
    input: R,
    mut output: W,
    live: Option<&str>,
) -> Result<(), RdError> {
    let reader = std::io::BufReader::new(input);
    let mut session = AgentSession::new();
    if let Some(addr) = live.filter(|a| !a.is_empty()) {
        let local = session.enable_live(addr)?;
        eprintln!("[rd mcp] live viewer: http://{local}/");
    }
    for line in reader.lines() {
        let line = line.map_err(|e| RdError::new("IO_ERROR", format!("stdin: {e}"), MODULE_ID))?;
        if line.trim().is_empty() {
            continue;
        }
        match handle_message(&mut session, &line) {
            None => continue, // notification
            Some(Ok(resp)) | Some(Err(resp)) => {
                writeln!(output, "{resp}")
                    .map_err(|e| RdError::new("IO_ERROR", format!("stdout: {e}"), MODULE_ID))?;
                output
                    .flush()
                    .map_err(|e| RdError::new("IO_ERROR", format!("stdout: {e}"), MODULE_ID))?;
            }
        }
        // server → client notifications (resources/updated) after the reply
        for n in session.take_notifications() {
            writeln!(output, "{n}")
                .and_then(|()| output.flush())
                .map_err(|e| RdError::new("IO_ERROR", format!("stdout: {e}"), MODULE_ID))?;
        }
    }
    Ok(())
}

include!("m9_tools.rs");
include!("m10_tools.rs");
include!("observe.rs");
include!("snapshot.rs");
include!("ui_tools.rs");
include!("runtime_tools.rs");

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use rd_testsupport::apkfix::build_apk;
    use rd_testsupport::apkfix::{a, build_axml, ArscFix, AxNode, AxVal};
    use rd_testsupport::vm::{op10x, op21c, op21s, op35c, DexBuilder, ACC_PUBLIC, ACC_STATIC};

    /// HostOut é GLOBAL no processo (uma sessão por processo — ADR 001); os
    /// testes de sessão se serializam para não roubar o buffer uns dos outros.
    static SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// const/32 vAA, lit32 (resid de 32 bits).
    fn op31i_local(op: u8, reg: u8, lit: u32) -> Vec<u16> {
        let mut u = vec![op as u16 | ((reg as u16) << 8)];
        u.push((lit & 0xFFFF) as u16);
        u.push((lit >> 16) as u16);
        u
    }

    /// APK fixture: Main.onCreate faz println("hi do mcp") + setContentView;
    /// layout LinearLayout{TextView @string/hello id/tv}; LState.get()I → 42.
    /// Partes compartilhadas dos fixtures MCP (arsc, layout AXML, dex).
    fn fixture_parts() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut arsc = ArscFix::new();
        let layout = arsc.add_layout("activity_main");
        let s_hello = arsc.add_string("hello", "Olá do MCP");
        let id_tv = arsc.add_id("tv");
        let layout_xml = AxNode::new(
            "LinearLayout",
            vec![
                a(true, "orientation", AxVal::Int(1)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            vec![AxNode::new(
                "TextView",
                vec![
                    a(true, "id", AxVal::Ref(id_tv)),
                    a(true, "text", AxVal::Ref(s_hello)),
                    a(true, "layout_width", AxVal::Int(-1)),
                    a(true, "layout_height", AxVal::Int(96)),
                ],
                vec![],
            )],
        );
        let mut b = DexBuilder::new();
        b.class("Landroid/view/View;", "Ljava/lang/Object;");
        b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
        b.class("Landroid/widget/TextView;", "Landroid/view/View;");
        b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
        b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
        b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
        let p_iv = b.proto_idx("V", vec!["I".to_string()]);
        let p_v_str = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
        let f_out = b.field_idx("Ljava/lang/System;", "Ljava/io/PrintStream;", "out");
        let ps_println = b.method_idx("Ljava/io/PrintStream;", p_v_str, "println");
        let set_content = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
        let s_hi = b.intern("hi do mcp") as u16;
        let main = b.class("Lcom/test/mcp/Main;", "Landroid/app/Activity;");
        // onCreate(this, bundle): regs=5, ins=2 → this=v3
        b.direct(
            main,
            "onCreate",
            "V",
            vec!["Landroid/os/Bundle;"],
            ACC_PUBLIC,
            Some(b.code(5, 2, 2, {
                let mut u = op21c(0x1A, 0, s_hi); // const-string v0
                u.extend(op21c(0x60, 1, f_out)); // sget v1, System.out
                u.extend(op35c(0x6E, 2, ps_println, [1, 0, 0, 0, 0])); // println(v0)
                u.extend(op31i_local(0x14, 2, layout)); // const v2, resid
                u.extend(op35c(0x6E, 2, set_content, [3, 2, 0, 0, 0])); // setContentView
                u.extend(op10x(0x0E));
                u
            })),
        );
        // sonda de estado: LState.get()I → 42 (const/16 — const/4 não cabe)
        let state = b.class("LState;", "Ljava/lang/Object;");
        b.direct(
            state,
            "get",
            "I",
            vec![],
            ACC_PUBLIC | ACC_STATIC,
            Some(b.code(1, 0, 0, {
                let mut u = op21s(0x13, 0, 42);
                u.extend(op11x_ret());
                u
            })),
        );
        (arsc.build(), build_axml(&layout_xml), b.finish())
    }

    fn fixture_apk_path() -> String {
        let (arsc, layout, dex) = fixture_parts();
        fixture_apk_write(rd_testsupport::apkfix::build_apk_sdk(
            "com.test.mcp",
            "com.test.mcp.Main",
            arsc,
            vec![
                ("res/layout/activity_main.xml", layout),
                ("classes.dex", dex),
            ],
            26,
            30,
        ))
    }

    /// issue #109: MESMO DEX/recursos de `fixture_apk_path`, mas o manifest
    /// deixa `Main` sem filtro e põe o MAIN/LAUNCHER num `<activity-alias>`
    /// (`Alias` → `Main`) — padrão comum para trocar ícone/rótulo.
    fn fixture_apk_path_alias() -> String {
        let (arsc, layout, dex) = fixture_parts();
        let manifest = rd_testsupport::apkfix::build_manifest_axml_sdk_alias(
            "com.test.mcp",
            "com.test.mcp.Main",
            "com.test.mcp.Alias",
            "com.test.mcp.Main",
            26,
            30,
        );
        fixture_apk_write(fixture_apk_zip(manifest, arsc, layout, dex))
    }

    /// issue #109: variante com alias cujo `targetActivity` NÃO existe no DEX
    /// (caminho de erro estruturado).
    fn fixture_apk_path_alias_ghost_target() -> String {
        let (arsc, layout, dex) = fixture_parts();
        let manifest = rd_testsupport::apkfix::build_manifest_axml_sdk_alias(
            "com.test.mcp",
            "com.test.mcp.Main",
            "com.test.mcp.Alias",
            "com.test.mcp.Ghost",
            26,
            30,
        );
        fixture_apk_write(fixture_apk_zip(manifest, arsc, layout, dex))
    }

    /// Monta o zip do fixture com manifest arbitrário (mesma ordem de entradas
    /// do `build_apk_sdk`: manifest, resources.arsc, extras).
    fn fixture_apk_zip(manifest: Vec<u8>, arsc: Vec<u8>, layout: Vec<u8>, dex: Vec<u8>) -> Vec<u8> {
        let mut z = rd_testsupport::apkfix::ZipFix::new();
        z.add("AndroidManifest.xml", manifest);
        z.add("resources.arsc", arsc);
        z.add("res/layout/activity_main.xml", layout);
        z.add("classes.dex", dex);
        z.build()
    }

    fn fixture_apk_write(apk: Vec<u8>) -> String {
        let dir = std::env::temp_dir().join(format!(
            "rd-agent-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("app.apk");
        std::fs::write(&path, apk).expect("apk fixture");
        path.to_string_lossy().to_string()
    }

    fn op11x_ret() -> Vec<u16> {
        op10x(0x0F) // return v0
    }

    fn call(
        s: &mut AgentSession,
        name: &str,
        args: serde_json::Value,
    ) -> (serde_json::Value, bool) {
        let r = s.call_tool(name, &args);
        let content = match &r.content[0] {
            ToolContent::Text(t) => serde_json::from_str(t).unwrap_or(serde_json::json!(t)),
            ToolContent::Image { data_b64, mime } => {
                serde_json::json!({"image": mime, "b64_len": data_b64.len()})
            }
        };
        (content, r.is_error)
    }

    /// Round 6: the agent sees the screen without screenshots — `observe`
    /// (text/json + diff), `tap_text`, cached screenshots, MCP resources with
    /// subscriptions and `notifications/resources/updated`.
    #[test]
    fn observe_tap_text_and_screen_resources() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        let (v, err) = call(&mut s, "observe", serde_json::json!({}));
        assert!(err, "observe sem app é erro tipado: {v}");
        assert!(!call(&mut s, "install", serde_json::json!({"apk": apk_path})).1);
        let r = s.call_tool("launch", &serde_json::json!({"observe": true}));
        assert!(!r.is_error);
        assert_eq!(r.content.len(), 2, "observe:true anexa o resumo");
        match &r.content[1] {
            ToolContent::Text(t) => assert!(t.contains("Olá do MCP"), "{t}"),
            _ => panic!("resumo é texto"),
        }
        let (v, err) = call(&mut s, "observe", serde_json::json!({}));
        assert!(!err);
        let t = v.as_str().unwrap_or_default().to_string();
        assert!(t.starts_with("frame v1"), "{t}");
        assert!(t.contains("unchanged since last observe"), "{t}");
        let (v, err) = call(&mut s, "observe", serde_json::json!({"format": "json"}));
        assert!(!err);
        assert_eq!(v["version"], 1);
        assert_eq!(v["finished"], false);
        assert!(
            v["texts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["text"] == "Olá do MCP"),
            "{v}"
        );
        let (v, err) = call(&mut s, "observe", serde_json::json!({"format": "xml"}));
        assert!(err && v["code"] == "INVALID_FORMAT", "{v}");
        // tap by label: a visible text resolves; a missing one lists texts
        let (v, err) = call(
            &mut s,
            "tap_text",
            serde_json::json!({"text": "olá   do mcp"}),
        );
        assert!(!err, "tap_text: {v}");
        assert_eq!(v["label"], "Olá do MCP");
        let (v, err) = call(
            &mut s,
            "tap_text",
            serde_json::json!({"text": "nao existe"}),
        );
        assert!(err && v["code"] == "VIEW_NOT_FOUND", "{v}");
        assert!(
            v["suggestion"].as_str().unwrap().contains("Olá do MCP"),
            "{v}"
        );
        let (v, err) = call(&mut s, "tap_text", serde_json::json!({}));
        assert!(err && v["code"] == "INVALID_FORMAT", "{v}");
        // screenshot twice: same frame version → identical cached PNG
        let a = s.call_tool("screenshot", &serde_json::json!({}));
        let b = s.call_tool("screenshot", &serde_json::json!({}));
        match (&a.content[0], &b.content[0]) {
            (ToolContent::Image { data_b64: x, .. }, ToolContent::Image { data_b64: y, .. }) => {
                assert_eq!(x, y)
            }
            _ => panic!("screenshot devolve imagem"),
        }
        // resources over JSON-RPC
        let rpc = |s: &mut AgentSession, m: &str| -> serde_json::Value {
            match handle_message(s, m) {
                Some(Ok(v)) | Some(Err(v)) => v,
                None => serde_json::json!(null),
            }
        };
        let init = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        assert_eq!(
            init["result"]["capabilities"]["resources"]["subscribe"],
            true
        );
        let l = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#,
        );
        let uris: Vec<&str> = l["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["uri"].as_str())
            .collect();
        assert!(
            uris.contains(&"rustydroid://screen") && uris.contains(&"rustydroid://screen.png"),
            "{l}"
        );
        let rd = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"rustydroid://screen.txt"}}"#,
        );
        assert!(
            rd["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Olá do MCP"),
            "{rd}"
        );
        let png = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"rustydroid://screen.png"}}"#,
        );
        assert!(png["result"]["contents"][0]["blob"].as_str().unwrap().len() > 1000);
        let bad = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"rustydroid://nope"}}"#,
        );
        assert_eq!(bad["error"]["code"], -32002, "{bad}");
        let bad = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/subscribe","params":{"uri":"rustydroid://logs"}}"#,
        );
        assert_eq!(bad["error"]["code"], -32602, "{bad}");
        let ok = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":7,"method":"resources/subscribe","params":{"uri":"rustydroid://screen"}}"#,
        );
        assert!(ok["result"].is_object(), "{ok}");
        let _ = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"wait","arguments":{"ms":16}}}"#,
        );
        let n = s.take_notifications();
        assert_eq!(
            n.len(),
            1,
            "primeira versão observada após assinar → 1 aviso: {n:?}"
        );
        assert_eq!(n[0]["method"], "notifications/resources/updated");
        assert_eq!(n[0]["params"]["uri"], "rustydroid://screen");
        // same visible content → no further notification
        let _ = rpc(
            &mut s,
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"wait","arguments":{"ms":16}}}"#,
        );
        assert!(s.take_notifications().is_empty());
        // tools/list exposes the optional `observe` flag on action tools
        let defs = tool_defs();
        let tap = defs.iter().find(|d| d.name == "tap").unwrap();
        assert!(tap.schema["properties"]["observe"].is_object());
    }

    /// Round 6: the live viewer serves the page, the state (long-poll) and
    /// the frame PNG over plain HTTP.
    #[test]
    fn live_viewer_serves_page_state_and_frame() {
        use std::io::{Read, Write};
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        let addr = s.enable_live("127.0.0.1:0").expect("bind");
        let get = |path: &str| -> String {
            let mut c = std::net::TcpStream::connect(addr).expect("connect");
            write!(c, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = Vec::new();
            c.read_to_end(&mut out).unwrap();
            String::from_utf8_lossy(&out).to_string()
        };
        assert!(get("/").contains("RustyDroid"));
        assert!(
            get("/frame.png").starts_with("HTTP/1.1 204"),
            "sem frame ainda"
        );
        assert!(!call(&mut s, "install", serde_json::json!({"apk": apk_path})).1);
        let msg = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"launch","arguments":{}}}"#;
        assert!(handle_message(&mut s, msg).is_some());
        let st = get("/state.json?since=0");
        assert!(
            st.contains("Olá do MCP") && st.contains("\"last_tool\":\"launch\""),
            "{st}"
        );
        assert!(get("/frame.png").contains("image/png"));
        assert!(get("/nope").starts_with("HTTP/1.1 404"));
    }

    /// DoD M5 na unidade: instala → executa → navega → assera SÓ com as tools.
    #[test]
    fn golden_flow_install_launch_navigate_assert() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();

        let (v, err) = call(&mut s, "install", serde_json::json!({"apk": apk_path}));
        assert!(!err, "install: {v}");
        assert_eq!(v["package"], "com.test.mcp");

        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "launch: {v}");
        assert_eq!(v["activity"], "Lcom/test/mcp/Main;");

        // navegação
        let (v, err) = call(&mut s, "tap", serde_json::json!({"x": 10, "y": 10}));
        assert!(!err, "tap: {v}");
        assert_eq!(v["listener_acionado"], false);
        let (v, err) = call(&mut s, "wait", serde_json::json!({"ms": 100}));
        assert!(!err);
        assert_eq!(v["runnables_executados"], 0);

        // asserções
        let (v, err) = call(
            &mut s,
            "assert_view",
            serde_json::json!({"resource_id": "com.test:id/tv"}),
        );
        assert!(!err, "assert_view: {v}");
        assert_eq!(v["found"], true);
        let (v, err) = call(
            &mut s,
            "assert_text",
            serde_json::json!({"resource_id": "com.test:id/tv", "expected": "Olá do MCP"}),
        );
        assert!(!err);
        assert_eq!(v["ok"], true, "assert_text: {v}");
        let (v, _) = call(
            &mut s,
            "assert_text",
            serde_json::json!({"resource_id": "com.test:id/tv", "expected": "outro"}),
        );
        assert_eq!(v["ok"], false, "asserção negativa");
        let (v, err) = call(
            &mut s,
            "assert_activity",
            serde_json::json!({"activity": "com.test.mcp.Main"}),
        );
        assert!(!err);
        assert_eq!(v["ok"], true);

        // sonda de estado
        let (v, err) = call(
            &mut s,
            "invoke",
            serde_json::json!({"class": "LState;", "method": "get", "sig": "()I"}),
        );
        assert!(!err, "invoke: {v}");
        assert_eq!(v["value"], 42, "sonda: {v}");

        // observação
        let (v, err) = call(&mut s, "ui_tree", serde_json::json!({"format": "xml"}));
        assert!(!err);
        assert!(v.as_str().unwrap().starts_with("<?xml"), "xml: {v}");
        let (v, err) = call(&mut s, "screenshot", serde_json::json!({}));
        assert!(!err);
        assert!(v["b64_len"].as_u64().unwrap() > 1000, "png base64: {v}");
        let (v, _) = call(&mut s, "logs", serde_json::json!({}));
        assert_eq!(
            v["logs"], "hi do mcp\n",
            "System.out do app no buffer da sessão"
        );
    }

    #[test]
    fn explicit_native_selection_never_falls_back_to_generic_android() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &path);
        for id in ["c-ndk", "cpp-ndk", "rust-ndk"] {
            let (v, failed) = call(&mut s, "launch", serde_json::json!({"engine":id}));
            assert!(
                failed,
                "selected native route must reject an APK with no native libraries: {v}"
            );
            assert_eq!(
                v["code"],
                if cfg!(feature = "native-engines") {
                    "MISSING_ENTRY"
                } else {
                    "NOT_IMPLEMENTED"
                }
            );
            assert!(s.engine.is_none());
            assert!(s.native_engine.is_none());
        }
        let launched = launch_ok(&mut s, serde_json::json!({}));
        assert_eq!(launched["native_engine"], serde_json::Value::Null);
        let (v, failed) = call(&mut s, "launch", serde_json::json!({"engine":"flutter"}));
        assert!(failed);
        assert_eq!(v["code"], "NOT_IMPLEMENTED");
        assert!(s.engine.is_some());
        assert!(s.native_engine.is_none());
        let _ = std::fs::remove_file(path);
    }

    /// app_info exposes the same static registry report without launching or
    /// parsing any executable artifact, including malformed native payloads.
    #[test]
    fn app_info_includes_all_engine_findings_without_runtime_claims() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut zip = rd_testsupport::apkfix::ZipFix::new();
        zip.add(
            "AndroidManifest.xml",
            rd_testsupport::apkfix::build_manifest_axml_sdk(
                "com.test.registry",
                "com.test.registry.Main",
                26,
                36,
            ),
        );
        for path in [
            "classes.dex",
            "lib/arm64-v8a/libflutter.so",
            "assets/flutter_assets/AssetManifest.bin",
        ] {
            zip.add(path, b"not-executable".to_vec());
        }
        let bytes = zip.build();
        let expected = rd_engine_registry::Registry::embedded()
            .unwrap()
            .detect(&Apk::from_bytes(bytes.clone()).unwrap())
            .unwrap();
        let apk_path = fixture_apk_write(bytes);
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        let (v, err) = call(&mut s, "app_info", serde_json::json!({}));
        assert!(!err, "app_info: {v}");
        assert_eq!(v["engines"], serde_json::to_value(expected).unwrap());
        assert_eq!(v["engines"]["registered_engines"], 28);
        assert_eq!(v["engines"]["execution_claimed"], false);
        assert!(
            s.engine.is_none(),
            "static detection must never create a process"
        );
        assert_eq!(
            tool_defs().len(),
            47,
            "preserve the existing MCP tool contract"
        );
        let _ = std::fs::remove_file(apk_path);
    }

    #[test]
    fn app_info_preserves_identity_when_optional_engine_analysis_is_limited() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path =
            fixture_apk_write(rd_testsupport::apkfix::build_large_inventory_apk(100_000));
        let mut session = AgentSession::new();
        install_ok(&mut session, &apk_path);
        let (info, error) = call(&mut session, "app_info", serde_json::json!({}));
        assert!(!error, "basic app identity must remain available: {info}");
        assert_eq!(info["package"], "com.example.inventory");
        assert_eq!(info["engines"]["analysis_status"], "not_analyzed");
        assert_eq!(info["engines"]["error"]["code"], "DETECTION_LIMIT");
        assert_eq!(info["engines"]["execution_claimed"], false);
        assert!(session.engine.is_none());
        let _ = std::fs::remove_file(apk_path);
    }

    /// Sem app: tools falham tipadas (nunca pânico).
    #[test]
    fn tools_fail_typed_without_app() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        // launch: sem APK instalado → NO_APP
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(err);
        assert_eq!(v["code"], "NO_APP", "{v}");
        assert_eq!(v["module_id"], MODULE_ID);
        // com APK instalado mas sem launch: interação → NOT_RUNNING (issue #55:
        // erro diz o QUE falta, com sugestão acionável)
        let apk_path = fixture_apk_path();
        let (v, err) = call(&mut s, "install", serde_json::json!({"apk": apk_path}));
        assert!(!err, "install: {v}");
        for (name, args) in [
            ("tap", serde_json::json!({"x": 1, "y": 1})),
            ("ui_tree", serde_json::json!({})),
            ("screenshot", serde_json::json!({})),
        ] {
            let (v, err) = call(&mut s, name, args);
            assert!(err, "{name} sem execução deve falhar tipado");
            assert_eq!(v["code"], "NOT_RUNNING", "{name}: {v}");
        }
    }

    /// notifications tool: honesta NOT_IMPLEMENTED (M9).
    #[test]
    fn notifications_without_running_app_is_typed() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        let (v, err) = call(&mut s, "notifications", serde_json::json!({}));
        assert!(err);
        assert_eq!(v["code"], "NOT_RUNNING");
    }

    /// Apêndice C: compat_gaps expõe o sumário executável do registro
    /// android.* junto dos gaps narrativos históricos.
    #[test]
    fn compat_gaps_includes_appendix_c_summary() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        let (v, err) = call(&mut s, "compat_gaps", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["api_coverage"]["data"], "data/api-coverage.toml");
        assert_eq!(v["api_coverage"]["domains"], 17);
        assert!(v["api_coverage"]["surfaces"].as_u64() >= Some(100), "{v}");
        // Invariante de honestidade: `supported` é apenas o subconjunto
        // documentado de cada superfície (nunca o pacote AOSP inteiro), por
        // isso permanece uma minoria pequena frente ao miolo `partial` —
        // o registro não pode fingir Android completo mesmo com zero
        // superfícies `not_implemented`.
        let supported = v["api_coverage"]["by_status"]["supported"].as_u64();
        let partial = v["api_coverage"]["by_status"]["partial"].as_u64();
        assert!(
            partial > supported && supported <= Some(8),
            "registro não pode fingir Android completo: {v}"
        );
    }

    /// AG-09 (metade estática): get_api_usage reporta as APIs android.* que o
    /// app referencia nos DEX (análise estática) e exige app instalado.
    #[test]
    fn get_api_usage_reports_android_references() {
        let _guard = SESSION_LOCK.lock().unwrap();
        // sem app: erro tipado
        let mut s0 = AgentSession::new();
        let (v, err) = call(&mut s0, "get_api_usage", serde_json::json!({}));
        assert!(err, "{v}");
        assert_eq!(v["code"], "NO_APP", "{v}");

        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        let (v, err) = call(&mut s, "get_api_usage", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["package"], "com.test.mcp", "{v}");
        assert_eq!(v["analysis"], "static-dex-references", "{v}");
        let types = v["android_types"].as_array().expect("android_types");
        let names: Vec<&str> = types.iter().filter_map(|t| t.as_str()).collect();
        // o fixture estende android.app.Activity e chama setContentView
        assert!(names.contains(&"android.app.Activity"), "{names:?}");
        assert!(
            v["android_methods"]["android.app.Activity"]
                .as_array()
                .is_some_and(|ms| ms.iter().any(|m| m == "setContentView")),
            "métodos por classe: {v}"
        );
        assert!(
            v["types_by_domain"]["android.app"].as_u64() >= Some(1),
            "domínios: {v}"
        );
        // Apêndice C: a mesma referência estática é classificada por domínio
        // funcional/status, sem virar cobertura de execução.
        assert_eq!(v["coverage_source"], "data/api-coverage.toml (Appendix C)");
        assert!(
            v["api_coverage"]["by_domain"]["app-lifecycle"]["total"].as_u64() >= Some(1),
            "classificação Apêndice C: {v}"
        );
        assert!(
            v["api_coverage"]["by_status"]["partial"].as_u64() >= Some(1),
            "android.app é parcial, não pacote completo: {v}"
        );

        // package não instalado → tipado
        let (v, err) = call(
            &mut s,
            "get_api_usage",
            serde_json::json!({"package": "nao.instalado"}),
        );
        assert!(err);
        assert_eq!(v["code"], "PACKAGE_NOT_INSTALLED", "{v}");
    }

    /// AG-09 (metade dinâmica): get_coverage mede a cobertura de execução
    /// (nível de método) do app em uso; sem app lançado → NOT_RUNNING.
    #[test]
    fn get_coverage_reports_executed_methods() {
        let _guard = SESSION_LOCK.lock().unwrap();
        // nada instalado → NOT_RUNNING (é tool de runtime, como tap/screenshot)
        let mut s0 = AgentSession::new();
        let (v, err) = call(&mut s0, "get_coverage", serde_json::json!({}));
        assert!(err, "{v}");
        assert_eq!(v["code"], "NOT_RUNNING", "{v}");

        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        // instalado mas sem launch → NOT_RUNNING
        let (v, err) = call(&mut s, "get_coverage", serde_json::json!({}));
        assert!(err, "{v}");
        assert_eq!(v["code"], "NOT_RUNNING", "{v}");

        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "{v}");
        let (v, err) = call(&mut s, "get_coverage", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["package"], "com.test.mcp", "{v}");
        assert_eq!(v["analysis"], "method-entry-instrumentation", "{v}");
        let total = v["methods_total"].as_u64().expect("methods_total");
        let executed = v["methods_executed"].as_u64().expect("executed");
        assert!(total > 0, "{v}");
        assert!(executed >= 1, "launch executou onCreate: {v}");
        assert!(executed <= total, "{v}");
        let pct = v["coverage_pct"].as_f64().expect("coverage_pct");
        assert!((0.0..=100.0).contains(&pct), "{v}");
        let classes = v["classes"].as_array().expect("classes");
        let main = classes
            .iter()
            .find(|c| c["class"] == "Lcom/test/mcp/Main;")
            .unwrap_or_else(|| panic!("classe Main na cobertura: {v}"));
        assert!(
            main["methods_executed"].as_u64().unwrap_or(0) >= 1,
            "onCreate contado: {main}"
        );
    }

    /// Envelope JSON-RPC no shape MCP.
    #[test]
    fn envelope_jsonrpc_is_mcp_shape() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();

        let r = handle_message(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(r["result"]["serverInfo"]["name"], SERVER_NAME);

        // notification: NENHUMA resposta
        assert!(handle_message(
            &mut s,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        )
        .is_none());

        let r = handle_message(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap()
            .unwrap();
        let tools = r["result"]["tools"].as_array().expect("tools");
        assert_eq!(
            tools.len(),
            47,
            "catálogo completo: M5 + AG-09 + M9 + round 6 observe/tap_text (47 tools)"
        );
        for t in tools {
            assert!(t["name"].as_str().is_some());
            assert!(t["inputSchema"].is_object(), "schema por tool");
        }

        let r = handle_message(&mut s, r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(r["id"], 3);

        let r = handle_message(&mut s, r#"{"jsonrpc":"2.0","id":4,"method":"x/unknown"}"#)
            .unwrap()
            .expect_err("erro de protocolo");
        assert_eq!(r["error"]["code"], -32601);

        let r = handle_message(&mut s, "{nope").unwrap().expect_err("parse");
        assert_eq!(r["error"]["code"], -32700);

        let r = handle_message(
            &mut s,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("UNKNOWN_TOOL"));
    }

    /// issue #71: JSON-RPC 2.0 §4.1 (Notification) — uma mensagem SEM `id` é
    /// notificação e NÃO recebe resposta, mesmo quando o método é
    /// desconhecido. Só requisições (com `id`) recebem `-32601`.
    #[test]
    fn unknown_method_notification_is_silent() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();

        // notificações desconhecidas → silêncio (nenhuma resposta)
        assert!(
            handle_message(&mut s, r#"{"jsonrpc":"2.0","method":"foo/unknown"}"#).is_none(),
            "notificação com método desconhecido não deve responder"
        );
        assert!(
            handle_message(&mut s, r#"{"jsonrpc":"2.0","method":"foo"}"#).is_none(),
            "notificação sem `id` nunca responde"
        );

        // a MESMA mensagem com `id` continua recebendo -32601, com o id ecoado
        let r = handle_message(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"foo"}"#)
            .unwrap()
            .expect_err("requisição com método desconhecido é erro");
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 1);

        // no loop real de E/S: nada é escrito no stdout do protocolo
        let mut out: Vec<u8> = Vec::new();
        serve_stdio(
            "{\"jsonrpc\":\"2.0\",\"method\":\"foo/unknown\"}\n".as_bytes(),
            &mut out,
        )
        .expect("stdio");
        assert!(
            out.is_empty(),
            "notificação não produz saída: {}",
            String::from_utf8_lossy(&out)
        );
    }

    /// issue #70: JSON-RPC 2.0 §6 (Batch) — um array é um lote: UMA resposta
    /// por requisição, na mesma ordem. Antes o lote virava "método ''" e todas
    /// as requisições eram descartadas em silêncio.
    #[test]
    fn batch_returns_one_response_per_request() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();

        let r = handle_message(
            &mut s,
            r#"[{"jsonrpc":"2.0","id":2,"method":"ping"},{"jsonrpc":"2.0","id":3,"method":"ping"}]"#,
        )
        .unwrap()
        .expect("lote processado");
        let arr = r.as_array().expect("resposta de lote é array");
        assert_eq!(arr.len(), 2, "uma resposta por requisição: {arr:?}");
        assert_eq!(arr[0]["id"], 2);
        assert_eq!(arr[1]["id"], 3);
        assert!(arr[0].get("result").is_some(), "sem erro: {arr:?}");

        // ferramenta de verdade dentro do lote também funciona (não só ping)
        let r = handle_message(
            &mut s,
            r#"[{"jsonrpc":"2.0","id":7,"method":"tools/list"},{"jsonrpc":"2.0","id":8,"method":"initialize"}]"#,
        )
        .unwrap()
        .expect("lote processado");
        let arr = r.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(
            arr[0]["result"]["tools"].as_array().expect("tools").len(),
            47
        );
        assert_eq!(arr[1]["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    /// issue #70: notificações dentro do lote não geram resposta; lote SÓ de
    /// notificações não produz resposta alguma (§4.1 + §6).
    #[test]
    fn batch_notifications_are_not_answered() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();

        let r = handle_message(
            &mut s,
            r#"[{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":4,"method":"ping"}]"#,
        )
        .unwrap()
        .expect("há uma requisição no lote");
        let arr = r.as_array().expect("array");
        assert_eq!(arr.len(), 1, "só a requisição responde: {arr:?}");
        assert_eq!(arr[0]["id"], 4);

        assert!(
            handle_message(
                &mut s,
                r#"[{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","method":"foo/bar"}]"#,
            )
            .is_none(),
            "lote só de notificações não responde"
        );
    }

    /// issue #70: estrutura inválida é Invalid Request (-32600) — nunca
    /// "método ''". Quando o `id` é detectável ele é ecoado; quando a
    /// estrutura não permite detectá-lo, é `null` (JSON-RPC 2.0 §5.1).
    #[test]
    fn invalid_structure_is_invalid_request_32600() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();

        for (label, line, want_id) in [
            ("lote vazio", "[]", None),
            ("escalar", "42", None),
            ("string", r#""texto""#, None),
            ("nulo", "null", None),
            // objeto válido detecta o id → ecoa
            ("objeto sem method", r#"{"jsonrpc":"2.0","id":9}"#, Some(9)),
            (
                "method não-string",
                r#"{"jsonrpc":"2.0","id":9,"method":5}"#,
                Some(9),
            ),
        ] {
            let r = handle_message(&mut s, line)
                .unwrap_or_else(|| panic!("{label}: deveria responder com erro"))
                .expect_err("{label}: deveria ser erro de protocolo");
            assert_eq!(
                r["error"]["code"], -32600,
                "{label}: esperava -32600, veio {r}"
            );
            match want_id {
                Some(want) => assert_eq!(r["id"], want, "{label}: id ecoado, veio {r}"),
                None => assert!(r["id"].is_null(), "{label}: id precisa ser null, veio {r}"),
            }
            let msg = r["error"]["message"].as_str().unwrap_or_default();
            assert!(
                !msg.contains("método ''"),
                "{label}: nunca mensagem de método vazio: {msg}"
            );
        }

        // elemento inválido DENTRO do lote: o lote responde com um array
        // contendo o erro daquele elemento (§6), em vez de descartar tudo.
        let r = handle_message(
            &mut s,
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},5,{"jsonrpc":"2.0","id":2,"method":"ping"}]"#,
        )
        .unwrap()
        .expect("lote responde");
        let arr = r.as_array().expect("array");
        assert_eq!(arr.len(), 3, "uma entrada por elemento: {arr:?}");
        assert!(arr[0].get("result").is_some(), "{arr:?}");
        assert_eq!(arr[1]["error"]["code"], -32600, "{arr:?}");
        assert!(arr[1]["id"].is_null(), "{arr:?}");
        assert!(arr[2].get("result").is_some(), "{arr:?}");
    }

    /// issue #85: `json_args_to_values` recusa valores fora da faixa do tipo
    /// DEX em vez de truncar com `as` (128 → -128, 40000 → -25536…).
    #[test]
    fn invoke_args_out_of_range_are_invalid_format() {
        let out_of_range: &[(&str, serde_json::Value)] = &[
            ("I", serde_json::json!(2147483648i64)),  // i32::MAX + 1
            ("I", serde_json::json!(-2147483649i64)), // i32::MIN - 1
            ("B", serde_json::json!(128)),
            ("B", serde_json::json!(-129)),
            ("S", serde_json::json!(40000)),
            ("S", serde_json::json!(-32769)),
        ];
        for (ty, value) in out_of_range {
            let sig = format!("({ty})V");
            let err = json_args_to_values(&sig, std::slice::from_ref(value))
                .err()
                .unwrap_or_else(|| panic!("{ty} = {value} deveria ser recusado"));
            assert_eq!(
                err.code, "INVALID_FORMAT",
                "{ty} = {value}: código do erro (cause: {})",
                err.cause
            );
            assert!(
                err.cause.contains("fora da faixa"),
                "{ty} = {value}: cause deveria citar a faixa: {}",
                err.cause
            );
        }

        // limites exatos continuam aceitos (sem regressão)
        let ok: &[(&str, serde_json::Value, i32)] = &[
            ("I", serde_json::json!(2147483647i64), 2147483647),
            ("I", serde_json::json!(-2147483648i64), -2147483648),
            ("B", serde_json::json!(127), 127),
            ("B", serde_json::json!(-128), -128),
            ("S", serde_json::json!(32767), 32767),
            ("S", serde_json::json!(-32768), -32768),
        ];
        for (ty, value, want) in ok {
            let sig = format!("({ty})V");
            let values = json_args_to_values(&sig, std::slice::from_ref(value))
                .unwrap_or_else(|e| panic!("{ty} = {value} deveria ser aceito: {}", e.cause));
            let got = match &values[0] {
                Value::Int(v) => *v,
                other => panic!("{ty} = {value}: esperava Int, veio {other:?}"),
            };
            assert_eq!(got, *want, "{ty} = {value} truncado");
        }
    }

    /// issue #85: `tap` com coordenada fora de i32 é INVALID_FORMAT e NÃO toca
    /// na tela (antes virava x=100 e a resposta ecoava o valor truncado).
    #[test]
    fn tap_and_swipe_reject_coordinates_outside_i32() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        let too_big = 4_294_967_396i64; // 0x1_0000_0064 → `as i32` daria 100
        let (v, err) = call(&mut s, "tap", serde_json::json!({"x": too_big, "y": 5}));
        assert!(err, "tap fora de i32 precisa ser erro: {v}");
        assert_eq!(v["code"], "INVALID_FORMAT", "{v}");
        assert!(v["cause"].as_str().unwrap_or("").contains("fora da faixa"));

        let (v, err) = call(
            &mut s,
            "swipe",
            serde_json::json!({"x1": too_big, "y1": 5, "x2": 100, "y2": 5, "ms": 10}),
        );
        assert!(err, "swipe fora de i32 precisa ser erro: {v}");
        assert_eq!(v["code"], "INVALID_FORMAT", "{v}");

        // e o limite exato continua aceito (ecoando o valor recebido)
        let (v, err) = call(
            &mut s,
            "tap",
            serde_json::json!({"x": i32::MAX, "y": i32::MIN}),
        );
        assert!(!err, "i32::MAX/MIN devem ser aceitos: {v}");
        assert_eq!(v["tapped"], serde_json::json!([i32::MAX, i32::MIN]), "{v}");
    }

    /// issue #79: critério vazio (`""`) casava tudo — `contains("")` e
    /// `ends_with("")` são sempre verdadeiros — então `assert_view`/`wait_for`
    /// passavam sem verificar nada (falso verde em teste automatizado).
    #[test]
    fn empty_assert_criteria_are_invalid_format() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        // os três critérios, vazios, em assert_view
        for (label, args) in [
            ("text", serde_json::json!({"text": ""})),
            ("class", serde_json::json!({"class": ""})),
            ("resource_id", serde_json::json!({"resource_id": ""})),
            ("nenhum critério", serde_json::json!({})),
        ] {
            let (v, err) = call(&mut s, "assert_view", args);
            assert!(err, "{label}: critério vazio deve ser erro, veio {v}");
            assert_eq!(v["code"], "INVALID_FORMAT", "{label}: {v}");
            assert!(
                v.get("found").is_none(),
                "{label}: erro não pode trazer `found` (falso verde): {v}"
            );
        }

        // wait_for: o mesmo, e sem `found=true` imediato
        for (label, args) in [
            ("text", serde_json::json!({"text": ""})),
            ("class", serde_json::json!({"class": ""})),
        ] {
            let (v, err) = call(&mut s, "wait_for", args);
            assert!(err, "{label}: wait_for com critério vazio: {v}");
            assert_eq!(v["code"], "INVALID_FORMAT", "{label}: {v}");
        }

        // critérios REAIS continuam funcionando (sem regressão)
        let (v, err) = call(&mut s, "assert_view", serde_json::json!({"text": "Olá"}));
        assert!(!err, "assert_view com texto real: {v}");
        assert_eq!(v["found"], true, "{v}");

        let (v, err) = call(
            &mut s,
            "assert_view",
            serde_json::json!({"text": "texto-que-nao-existe"}),
        );
        assert!(!err, "assert_view sem match não é erro: {v}");
        assert_eq!(
            v["found"], false,
            "texto inexistente continua found=false: {v}"
        );
    }

    /// issue #78: `collect_matches` casava classe por sufixo solto
    /// (`ends_with`), então `class="View"` casava `android.widget.TextView` e
    /// `class="Layout"` casava `LinearLayout` — asserção não falsificável.
    #[test]
    fn class_matching_requires_exact_full_or_simple_name() {
        // regra pura (sem sessão)
        assert!(class_matches(
            "android.widget.TextView",
            "android.widget.TextView"
        ));
        assert!(class_matches("android.widget.TextView", "TextView"));
        assert!(
            !class_matches("android.widget.TextView", "View"),
            "View (parcial) não pode casar TextView"
        );
        assert!(!class_matches("android.widget.TextView", "extView"));
        assert!(
            !class_matches("android.widget.LinearLayout", "Layout"),
            "Layout (parcial) não pode casar LinearLayout"
        );
        assert!(class_matches("android.widget.LinearLayout", "LinearLayout"));
        // classe sem ponto também funciona
        assert!(class_matches("TextView", "TextView"));
        assert!(!class_matches("TextView", "View"));

        // e na árvore real do app (TextView dentro de LinearLayout)
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        let (v, err) = call(&mut s, "assert_view", serde_json::json!({"class": "View"}));
        assert!(!err, "assert_view com classe: {v}");
        assert_eq!(
            v["found"], false,
            "issue #78: `View` não deve casar a TextView: {v}"
        );

        let (v, err) = call(
            &mut s,
            "assert_view",
            serde_json::json!({"class": "Layout"}),
        );
        assert!(!err, "{v}");
        assert_eq!(
            v["found"], false,
            "issue #78: `Layout` não deve casar LinearLayout: {v}"
        );

        // os dois nomes legítimos continuam casando
        for (label, want) in [
            ("nome simples", "TextView"),
            ("nome completo", "android.widget.TextView"),
        ] {
            let (v, err) = call(&mut s, "assert_view", serde_json::json!({"class": want}));
            assert!(!err, "{label}: {v}");
            assert_eq!(v["found"], true, "{label} ({want}) deveria casar: {v}");
            assert_eq!(
                v["matches"][0]["class"], "android.widget.TextView",
                "{label}: {v}"
            );
        }

        // wait_for com classe parcial: found=false até o timeout (não casa)
        let (v, err) = call(
            &mut s,
            "wait_for",
            serde_json::json!({"class": "View", "timeout_ms": 200, "step_ms": 50}),
        );
        assert!(!err, "wait_for: {v}");
        assert_eq!(
            v["found"], false,
            "issue #78: wait_for com classe parcial não deve encontrar: {v}"
        );
    }

    /// issue #82: `launch` de classe inexistente no DEX devolvia
    /// `launched: true` e criava uma instância vazia — falso sucesso que
    /// invalida tudo que o agente concluir depois.
    #[test]
    fn launch_rejects_activity_missing_from_dex() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        // as duas formas de escrever o nome (FQCN e descritor Dalvik)
        for (label, activity) in [
            ("FQCN inexistente", "com.test.mcp.Nope"),
            ("descritor inexistente", "Lcom/test/mcp/Nope;"),
        ] {
            let (v, err) = call(&mut s, "launch", serde_json::json!({"activity": activity}));
            assert!(
                err,
                "{label}: launch de classe ausente do DEX deve ser erro, veio {v}"
            );
            assert_eq!(v["code"], "MISSING_ENTRY", "{label}: {v}");
            assert!(
                v.get("launched").is_none(),
                "{label}: erro não pode trazer `launched` (falso sucesso): {v}"
            );
            assert!(
                v["cause"].as_str().unwrap_or_default().contains("Nope"),
                "{label}: cause cita a classe pedida: {v}"
            );
            assert!(!v["suggestion"].is_null(), "{label}: traz suggestion: {v}");
        }

        // nada foi lançado: a sessão continua sem app em execução
        let (v, err) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert!(!err, "runtime_health: {v}");
        assert_eq!(
            v["running"], false,
            "issue #82: launch inválido não pode criar estado de sessão: {v}"
        );
        assert!(
            v["activity"].is_null(),
            "activity da sessão continua ausente: {v}"
        );

        // a activity declarada no manifest (e presente no DEX) continua ok
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Main"}),
        );
        assert!(!err, "launch de com.test.mcp.Main: {v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["activity"], "Lcom/test/mcp/Main;", "{v}");

        // e o caminho sem `activity` (LAUNCHER do manifest) também
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "launch sem activity (LAUNCHER): {v}");
        assert_eq!(v["launched"], true, "{v}");

        // decisão (compat > elegância): a falha de um launch NÃO derruba o app
        // que já rodava — o Android mantém a activity anterior em primeiro
        // plano quando o `am start` falha. Nenhum estado NOVO é criado.
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Nope"}),
        );
        assert!(err, "segundo launch inválido continua erro: {v}");
        let (v, err) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert!(!err, "runtime_health: {v}");
        assert_eq!(
            v["running"], true,
            "app anterior continua de pé (Android não derruba): {v}"
        );
        assert_eq!(
            v["activity"], "Lcom/test/mcp/Main;",
            "nenhum estado novo foi criado pelo launch inválido: {v}"
        );
    }

    /// issue #83: com `activity` explícito, `launch` não consultava o manifest
    /// e lançava qualquer classe do DEX (ex.: a sonda `LState;`). O Android só
    /// inicia activities declaradas — as não declaradas dão
    /// ActivityNotFoundException.
    #[test]
    fn launch_rejects_activity_not_declared_in_manifest() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        // o manifest do fixture só declara com.test.mcp.Main; `LState;` EXISTE
        // no DEX (é a sonda de estado) mas não é uma <activity> declarada.
        for (label, activity) in [
            ("descritor", "LState;"),
            ("FQCN", "com.test.mcp.State"),
            ("inexistente no DEX e no manifest", "com.test.mcp.Nope"),
        ] {
            let (v, err) = call(&mut s, "launch", serde_json::json!({"activity": activity}));
            assert!(
                err,
                "{label}: launch de activity não declarada deve ser erro, veio {v}"
            );
            assert_eq!(v["code"], "MISSING_ENTRY", "{label}: {v}");
            assert!(
                v.get("launched").is_none(),
                "{label}: erro não pode trazer `launched`: {v}"
            );
            assert!(
                v["cause"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("AndroidManifest.xml"),
                "{label}: cause aponta o manifest: {v}"
            );
            assert!(!v["suggestion"].is_null(), "{label}: traz suggestion: {v}");
        }

        // nenhum estado de sessão criado pelas tentativas inválidas
        let (v, err) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert!(!err, "runtime_health: {v}");
        assert_eq!(
            v["running"], false,
            "issue #83: launch inválido não cria estado: {v}"
        );

        // a activity DECLARADA continua lançando — pelos dois caminhos
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Main"}),
        );
        assert!(!err, "launch da activity declarada: {v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["activity"], "Lcom/test/mcp/Main;", "{v}");

        // e pelo descritor Dalvik da mesma classe declarada
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "Lcom/test/mcp/Main;"}),
        );
        assert!(!err, "launch pelo descritor da activity declarada: {v}");
        assert_eq!(v["launched"], true, "{v}");

        // caminho LAUNCHER (sem `activity`) segue resolvendo pelo manifest
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "launch sem activity: {v}");
        assert_eq!(v["launched"], true, "{v}");
    }

    /// issue #109: `<activity-alias>` declarada no manifest não era lançável:
    /// o caminho explícito parava na checagem de DEX da #82 (a classe do alias
    /// não existe no DEX) e o caminho LAUNCHER ignorava alias no filtro
    /// MAIN/LAUNCHER. No Android, iniciar o alias abre a `targetActivity`.
    #[test]
    fn launch_resolves_activity_alias_to_target() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path_alias();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        // 1) launch explícito do alias → roda a Main (alvo), nomeando o alias
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Alias"}),
        );
        assert!(!err, "launch do alias deve resolver o alvo: {v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(
            v["activity"], "Lcom/test/mcp/Main;",
            "a activity executada é o alvo do alias: {v}"
        );
        assert_eq!(
            v["alias"], "com.test.mcp.Alias",
            "o resultado registra qual alias resolveu: {v}"
        );

        // o alvo realmente executou: a ui_tree traz o texto do layout da Main
        let (v, err) = call(&mut s, "ui_tree", serde_json::json!({}));
        assert!(!err, "ui_tree após launch via alias: {v}");
        assert!(
            v.to_string().contains("Olá do MCP"),
            "a Main inflou o layout (execução real, não instância vazia): {v}"
        );

        // 2) LAUNCHER declarada no alias: launch sem `activity` resolve
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "launch LAUNCHER quando a LAUNCHER é um alias: {v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["activity"], "Lcom/test/mcp/Main;", "{v}");
        assert_eq!(v["alias"], "com.test.mcp.Alias", "{v}");

        // 3) a activity comum continua lançando — sem campo `alias`
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Main"}),
        );
        assert!(!err, "launch direto da activity declarada: {v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["activity"], "Lcom/test/mcp/Main;", "{v}");
        assert!(
            v.get("alias").is_none(),
            "launch direto não pode trazer `alias`: {v}"
        );
    }

    /// issue #109: alias cujo `targetActivity` não existe no DEX devolve erro
    /// estruturado nomeando alias E alvo — não o falso sucesso de antes nem um
    /// MISSING_ENTRY que cita só a classe resolvida.
    #[test]
    fn launch_alias_with_ghost_target_is_structured_error() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path_alias_ghost_target();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Alias"}),
        );
        assert!(err, "alias com alvo fora do DEX é erro: {v}");
        assert_eq!(v["code"], "MISSING_ENTRY", "{v}");
        assert!(
            v.get("launched").is_none(),
            "erro não pode trazer `launched` (falso sucesso): {v}"
        );
        let cause = v["cause"].as_str().unwrap_or_default();
        assert!(
            cause.contains("com.test.mcp.Alias") && cause.contains("com.test.mcp.Ghost"),
            "cause nomeia o alias e o alvo: {v}"
        );
        assert!(!v["suggestion"].is_null(), "traz suggestion: {v}");

        // nada foi lançado: a sessão continua sem app em execução
        let (v, err) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert!(!err, "runtime_health: {v}");
        assert_eq!(v["running"], false, "launch inválido não cria estado: {v}");

        // e o caminho LAUNCHER herda o mesmo erro (a LAUNCHER é o alias)
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(
            err,
            "LAUNCHER == alias com alvo fantasma também é erro: {v}"
        );
        assert_eq!(v["code"], "MISSING_ENTRY", "{v}");
    }

    /// M7 (regime warm): o relançamento do package em execução reutiliza o
    /// processo (Engine) — `warm: true`, sem re-parse do DEX. install/clear
    /// matam o processo; o launch seguinte volta a ser frio.
    #[test]
    fn launch_warm_reuses_running_process() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        // primeiro launch: frio (parse dos DEX + Engine novo)
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["warm"], false, "primeiro launch é cold: {v}");

        // relançamento com o processo vivo: warm
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["launched"], true, "{v}");
        assert_eq!(v["warm"], true, "relançamento é warm: {v}");

        // relançamento explícito da mesma activity também é warm
        let (v, err) = call(
            &mut s,
            "launch",
            serde_json::json!({"activity": "com.test.mcp.Main"}),
        );
        assert!(!err, "{v}");
        assert_eq!(v["warm"], true, "{v}");

        // o app continua funcional no processo reutilizado
        let (v, err) = call(&mut s, "ui_tree", serde_json::json!({}));
        assert!(!err, "{v}");
        assert!(v.to_string().contains("Olá do MCP"), "{v}");

        // clear mata o processo: próximo launch é frio de novo
        let (v, _) = call(
            &mut s,
            "package_clear",
            serde_json::json!({"package": "com.test.mcp"}),
        );
        assert_eq!(v["cleared"], true, "{v}");
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "{v}");
        assert_eq!(v["warm"], false, "clear mata o processo: launch frio: {v}");
    }

    /// issue #80: as checagens de formato viviam dentro do laço de execução,
    /// então um passo inválido no meio abortava DEPOIS de rodar os anteriores
    /// — e o erro não trazia `steps`/`executed`/`resume_hint` (o cliente não
    /// sabia o que mudou no app nem de onde retomar).
    #[test]
    fn batch_invalid_step_runs_nothing() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        // passo inválido na posição 1 (depois de um `launch` válido): o
        // `launch` do passo 0 NÃO pode ter sido executado
        for (label, bad) in [
            ("sem name", serde_json::json!({"arguments": {}})),
            (
                "name vazio",
                serde_json::json!({"name": "", "arguments": {}}),
            ),
            (
                "batch aninhado",
                serde_json::json!({"name": "batch", "arguments": {"calls": []}}),
            ),
            (
                "arguments não-objeto",
                serde_json::json!({"name": "tap", "arguments": 7}),
            ),
            ("passo não-objeto", serde_json::json!("tap")),
        ] {
            let (v, err) = call(
                &mut s,
                "batch",
                serde_json::json!({"calls": [{"name": "launch", "arguments": {}}, bad]}),
            );
            assert!(err, "{label}: lote com passo inválido é erro, veio {v}");
            assert_eq!(v["code"], "INVALID_FORMAT", "{label}: {v}");
            assert!(
                v["cause"].as_str().unwrap_or_default().contains('1'),
                "{label}: causa cita o índice do passo: {v}"
            );
            // efeito colateral zero: nada foi lançado
            let (h, herr) = call(&mut s, "runtime_health", serde_json::json!({}));
            assert!(!herr, "runtime_health: {h}");
            assert_eq!(
                h["running"], false,
                "{label}: issue #80 — lote inválido não pode executar o passo 0: {h}"
            );
        }

        // passo inválido na posição 0 também não executa nada (e cita o índice)
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [{"name": "", "arguments": {}}, {"name": "wait", "arguments": {"ms": 1}}]}),
        );
        assert!(err, "passo 0 inválido: {v}");
        assert_eq!(v["code"], "INVALID_FORMAT", "{v}");
        assert!(
            v["cause"].as_str().unwrap_or_default().contains('0'),
            "causa cita o passo 0: {v}"
        );

        // lote válido continua executando tudo (sem regressão)
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [
                {"name": "launch", "arguments": {}},
                {"name": "wait", "arguments": {"ms": 10}},
                {"name": "assert_text", "arguments": {"resource_id": "com.test:id/tv", "expected": "Olá do MCP"}},
            ]}),
        );
        assert!(!err, "lote válido: {v}");
        assert_eq!(v["executed"], 3, "{v}");
        assert!(v["resume_hint"].is_null(), "{v}");
        let (h, _) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert_eq!(h["running"], true, "lote válido lançou o app: {h}");
    }

    /// issue #81: num lote retomado (`resume_from > 0`) que executa todos os
    /// passos restantes, `resume_hint` vinha preenchido — a condição comparava
    /// `executed` com `list.len()` e ignorava os passos pulados. Um
    /// `resume_hint` não nulo promete trabalho pendente que não existe.
    #[test]
    fn batch_resume_hint_is_null_when_batch_completes() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        let calls = serde_json::json!([
            {"name": "wait", "arguments": {"ms": 5}},
            {"name": "tap", "arguments": {"x": 5, "y": 5}},
        ]);

        // lote completo sem retomada: resume_hint null (já era assim)
        let (v, err) = call(&mut s, "batch", serde_json::json!({"calls": calls}));
        assert!(!err, "lote completo: {v}");
        assert_eq!(v["executed"], 2, "{v}");
        assert!(v["resume_hint"].is_null(), "{v}");

        // lote COMPLETO retomado: resume_from=1 → só o passo 1 roda e o lote
        // termina ⇒ resume_hint precisa ser null (antes vinha 2)
        for (label, resume_from) in [("resume_from=1", 1), ("resume_from=total", 2)] {
            let (v, err) = call(
                &mut s,
                "batch",
                serde_json::json!({"calls": calls, "resume_from": resume_from}),
            );
            assert!(!err, "{label}: {v}");
            assert_eq!(
                v["executed"],
                2 - resume_from,
                "{label}: só os pendentes rodam: {v}"
            );
            assert!(
                v["resume_hint"].is_null(),
                "{label}: lote terminou — resume_hint deve ser null, veio {}",
                v["resume_hint"]
            );
        }

        // lote INTERROMPIDO retomado: ainda há trabalho ⇒ índice do próximo
        let calls3 = serde_json::json!([
            {"name": "wait", "arguments": {"ms": 5}},
            {"name": "assert_view", "arguments": {}}, // INVALID_FORMAT
            {"name": "tap", "arguments": {"x": 1, "y": 1}},
        ]);
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": calls3, "resume_from": 1}),
        );
        assert!(!err, "lote com passo falho: {v}");
        assert_eq!(v["stopped_early"], true, "{v}");
        assert_eq!(
            v["resume_hint"], 2,
            "próximo passo pendente é o índice 2 (1 pulado + 1 executado): {v}"
        );
    }

    /// issue #67: `check_write_path` canonizava só o diretório PAI e depois
    /// gravava em `pai.join(nome)`. Se o último componente fosse um symlink
    /// pré-existente dentro do sandbox, a escrita seguia o link e sobrescrevia
    /// o alvo FORA das raízes — e a resposta ainda informava o caminho do link
    /// (dentro do sandbox), escondendo a escrita real.
    #[cfg(unix)]
    #[test]
    fn screenshot_refuses_symlink_escape() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let base = std::env::temp_dir().join(format!(
            "rd-agent-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sandbox = base.join("sandbox");
        let outside = base.join("outside");
        std::fs::create_dir_all(&sandbox).expect("sandbox");
        std::fs::create_dir_all(&outside).expect("outside");
        let victim = outside.join("victim.png");
        let original = b"ORIGINAL-10".to_vec();
        std::fs::write(&victim, &original).expect("victim");
        // symlink DENTRO da raiz permitida apontando para FORA
        let link = sandbox.join("shot.png");
        std::os::unix::fs::symlink(&victim, &link).expect("symlink");

        // o APK do fixture precisa estar legível (install) — o symlink fica
        // na raiz `sandbox`, que é a que o screenshot usa como destino
        let fixture_dir = std::path::Path::new(&apk_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let mut s = AgentSession::new().with_policy(Policy {
            roots: vec![sandbox.clone(), fixture_dir],
            max_apk_bytes: 256 * 1024 * 1024,
            max_wait_ms: 1_000,
            max_batch_steps: 64,
        });
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        let (v, err) = call(
            &mut s,
            "screenshot",
            serde_json::json!({"path": link.to_string_lossy()}),
        );
        assert!(err, "screenshot via symlink deve ser erro, veio {v}");
        assert_eq!(v["code"], "SYMLINK_NOT_ALLOWED", "{v}");
        assert!(!v["suggestion"].is_null(), "erro traz suggestion: {v}");
        assert_eq!(
            std::fs::read(&victim).expect("victim legível"),
            original,
            "issue #67: o alvo fora do sandbox permanece byte a byte igual"
        );

        // escrita normal dentro do sandbox continua funcionando (e sobrescreve
        // um arquivo existente — o `create_new` do helper trata o AlreadyExists)
        let real = sandbox.join("real.png");
        std::fs::write(&real, b"lixo").expect("seed");
        let (v, err) = call(
            &mut s,
            "screenshot",
            serde_json::json!({"path": real.to_string_lossy()}),
        );
        assert!(!err, "screenshot normal dentro do sandbox: {v}");
        assert_eq!(v["path"], real.to_string_lossy().to_string(), "{v}");
        let bytes = std::fs::read(&real).expect("png");
        assert!(
            bytes.starts_with(b"\x89PNG"),
            "PNG gravado no caminho real: {v}"
        );

        // e um caminho novo (sem arquivo pré-existente) também grava
        let fresh = sandbox.join("fresh.png");
        let (v, err) = call(
            &mut s,
            "screenshot",
            serde_json::json!({"path": fresh.to_string_lossy()}),
        );
        assert!(!err, "screenshot em arquivo novo: {v}");
        assert!(
            std::fs::read(&fresh)
                .expect("fresh")
                .starts_with(b"\x89PNG"),
            "arquivo novo gravado: {v}"
        );
    }

    /// serve_stdio: sessão completa por linhas (loop real de E/S).
    #[test]
    fn serve_stdio_drives_full_session() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let input = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"install\",\"arguments\":{{\"apk\":\"{apk_path}\"}}}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"launch\",\"arguments\":{{}}}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{{\"name\":\"assert_text\",\"arguments\":{{\"resource_id\":\"com.test:id/tv\",\"expected\":\"Olá do MCP\"}}}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{{\"name\":\"logs\",\"arguments\":{{}}}}}}\n"
        );
        let mut out: Vec<u8> = Vec::new();
        serve_stdio(input.as_bytes(), &mut out).expect("sessão completa");
        let text = String::from_utf8(out).expect("utf8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5, "notifications não respondem: {lines:?}");
        let r2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert!(r2["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("com.test.mcp"));
        let r4: serde_json::Value = serde_json::from_str(lines[3]).unwrap();
        assert!(r4["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"ok\": true"));
        let r5: serde_json::Value = serde_json::from_str(lines[4]).unwrap();
        assert!(r5["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("hi do mcp"));
    }

    // ── M5 completo: catálogo/política/batch/registro/isolamento/wait_for/
    //    swipe+type+key e consistência de docs ─────────────────────────────

    /// APK fixture com FORMULÁRIO: EditText id/et + Button id/btn com
    /// android:onClick="onBtn" (println no toque) + TextView id/tv.
    fn fixture_form_apk_path(package: &str) -> String {
        let mut arsc = ArscFix::new();
        let layout = arsc.add_layout("activity_form");
        let id_et = arsc.add_id("et");
        let id_btn = arsc.add_id("btn");
        let id_tv = arsc.add_id("tv");
        let layout_xml = AxNode::new(
            "LinearLayout",
            vec![
                a(true, "orientation", AxVal::Int(1)),
                a(true, "layout_width", AxVal::Int(-1)),
                a(true, "layout_height", AxVal::Int(-1)),
            ],
            vec![
                AxNode::new(
                    "android.widget.EditText",
                    vec![
                        a(true, "id", AxVal::Ref(id_et)),
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Int(96)),
                    ],
                    vec![],
                ),
                AxNode::new(
                    "Button",
                    vec![
                        a(true, "id", AxVal::Ref(id_btn)),
                        a(true, "text", AxVal::Str("OK".into())),
                        a(true, "onClick", AxVal::Str("onBtn".into())),
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Int(96)),
                    ],
                    vec![],
                ),
                AxNode::new(
                    "TextView",
                    vec![
                        a(true, "id", AxVal::Ref(id_tv)),
                        a(true, "text", AxVal::Str("rodapé".into())),
                        a(true, "layout_width", AxVal::Int(-1)),
                        a(true, "layout_height", AxVal::Int(48)),
                    ],
                    vec![],
                ),
            ],
        );
        let mut b = DexBuilder::new();
        b.class("Landroid/view/View;", "Ljava/lang/Object;");
        b.class("Landroid/view/ViewGroup;", "Landroid/view/View;");
        b.class("Landroid/widget/TextView;", "Landroid/view/View;");
        b.class("Landroid/widget/EditText;", "Landroid/widget/TextView;");
        b.class("Landroid/widget/Button;", "Landroid/widget/TextView;");
        b.class("Landroid/widget/LinearLayout;", "Landroid/view/ViewGroup;");
        b.class("Landroid/os/Bundle;", "Ljava/lang/Object;");
        b.class("Landroid/content/Intent;", "Ljava/lang/Object;");
        let p_iv = b.proto_idx("V", vec!["I".to_string()]);
        // onBtn(View)V usa proto p_v_str para o println; o proto (View)V não é necessário aqui
        let p_v_str = b.proto_idx("V", vec!["Ljava/lang/String;".to_string()]);
        let f_out = b.field_idx("Ljava/lang/System;", "Ljava/io/PrintStream;", "out");
        let ps_println = b.method_idx("Ljava/io/PrintStream;", p_v_str, "println");
        let set_content = b.method_idx("Landroid/app/Activity;", p_iv, "setContentView");
        let main = b.class("Lcom/test/form/Main;", "Landroid/app/Activity;");
        b.direct(
            main,
            "onCreate",
            "V",
            vec!["Landroid/os/Bundle;"],
            ACC_PUBLIC,
            Some(b.code(5, 2, 2, {
                let mut u = op31i_local(0x14, 2, layout); // const v2, resid
                u.extend(op35c(0x6E, 2, set_content, [3, 2, 0, 0, 0]));
                u.extend(op10x(0x0E));
                u
            })),
        );
        // onBtn(View)V — println("btn via swipe")
        let s_btn = b.intern("btn via swipe") as u16;
        b.direct(
            main,
            "onBtn",
            "V",
            vec!["Landroid/view/View;"],
            ACC_PUBLIC,
            Some(b.code(5, 2, 2, {
                let mut u = op21c(0x1A, 0, s_btn);
                u.extend(op21c(0x60, 1, f_out));
                u.extend(op35c(0x6E, 2, ps_println, [1, 0, 0, 0, 0]));
                u.extend(op10x(0x0E));
                u
            })),
        );
        let dir = std::env::temp_dir().join(format!(
            "rd-agent-mcp-form-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join(format!("{package}.apk"));
        // declara CAMERA: o ciclo grant/clear do registro precisa de uma
        // permissão runtime real no manifest (apkfix com uses-permission)
        let apk = rd_testsupport::apkfix::build_apk_sdk_perms(
            package,
            "com.test.form.Main",
            arsc.build(),
            vec![
                ("res/layout/activity_form.xml", build_axml(&layout_xml)),
                ("classes.dex", b.finish()),
            ],
            26,
            30,
            &["android.permission.CAMERA"],
        );
        std::fs::write(&path, apk).expect("apk fixture");
        path.to_string_lossy().to_string()
    }

    fn install_ok(s: &mut AgentSession, apk: &str) -> serde_json::Value {
        let (v, err) = call(s, "install", serde_json::json!({"apk": apk}));
        assert!(!err, "install {apk}: {v}");
        v
    }

    fn launch_ok(s: &mut AgentSession, args: serde_json::Value) -> serde_json::Value {
        let (v, err) = call(s, "launch", args);
        assert!(!err, "launch: {v}");
        v
    }

    /// ADR 009: capture -> paged fast install -> present (no engine) ->
    /// screenshot from snapshot -> lazy materialization on require_engine.
    /// Calls the internal entry points directly: the RD_SNAPSHOT=on gate
    /// lives in the callers and its OnceLock is process-global.
    #[test]
    fn snapshot_capture_present_materialize() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let snap_root = std::env::temp_dir().join(format!(
            "rd-snap-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::env::set_var("RD_SNAPSHOT_DIR", &snap_root);

        // capture from a REAL launch in session 1
        let mut s1 = AgentSession::new();
        install_ok(&mut s1, &apk_path);
        let lv = launch_ok(&mut s1, serde_json::json!({}));
        let activity = lv["activity"].as_str().expect("activity desc").to_string();
        let live_png = s1.screen_png().expect("live png");
        s1.capture_snapshot("com.test.mcp", &activity)
            .expect("capture");
        let key = s1
            .find_installed("com.test.mcp")
            .and_then(|a| a.snapshot_key.clone())
            .expect("snapshot key recorded");
        let loaded = load_snapshot(&key).expect("snapshot loads");
        assert_eq!(loaded.package, "com.test.mcp");
        assert_eq!(loaded.activity, activity);
        assert_eq!(&*loaded.png, &*live_png, "stored frame == live frame bytes");

        // fingerprint is stable across opens (process-independent key)
        let pz = rd_apk::paged::PagedZip::open(std::path::Path::new(&apk_path)).expect("paged");
        let mb = pz.read("AndroidManifest.xml").expect("manifest");
        assert_eq!(apk_fingerprint(&pz, &mb), key);

        // session 2: fast install (no whole Apk), present, screenshot,
        // then materialize on first engine-requiring access
        let mut s2 = AgentSession::new();
        let res = s2
            .try_install_snapshot(std::path::Path::new(&apk_path))
            .expect("fast install runs")
            .expect("snapshot hit");
        match &res.content[0] {
            ToolContent::Text(txt) => {
                let v: serde_json::Value = serde_json::from_str(txt).expect("json");
                assert_eq!(v["snapshot_cached"], true);
                assert_eq!(v["package"], "com.test.mcp");
            }
            _ => panic!("install result is text"),
        }
        assert!(s2
            .find_installed("com.test.mcp")
            .map(|a| a.apk.is_none())
            .unwrap_or(false));
        let pending = s2
            .snapshot_for_launch("com.test.mcp", &activity, Some(&key), None)
            .expect("presentable snapshot");
        s2.present_snapshot(pending);
        assert!(s2.engine.is_none(), "present must not materialize");
        let shot = s2.call_tool("screenshot", &serde_json::json!({}));
        assert!(!shot.is_error);
        match &shot.content[0] {
            ToolContent::Image { mime, .. } => assert_eq!(*mime, "image/png"),
            _ => panic!("screenshot from snapshot is an image"),
        }
        assert!(s2.engine.is_none(), "screenshot must not materialize");
        {
            let _eng = s2.require_engine().expect("materialize on demand");
        }
        assert!(s2.pending_snapshot.is_none());
        assert!(s2.engine.is_some());
        let (v, err) = call(&mut s2, "tap", serde_json::json!({"x": 10, "y": 10}));
        assert!(!err, "tap after materialization: {v}");
        std::env::remove_var("RD_SNAPSHOT_DIR");
        let _ = std::fs::remove_dir_all(&snap_root);
    }

    /// DoD (schemas): toda tool do catálogo despacha (nada de UNKNOWN_TOOL)
    /// e tem schema com required ⊆ properties.
    #[test]
    fn catalog_matches_dispatch_and_schemas() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        let defs = tool_defs();
        assert_eq!(defs.len(), 47);
        for t in &defs {
            // schema: objeto com properties/required consistentes
            assert!(t.schema["type"] == "object", "{}: schema object", t.name);
            let props = t.schema["properties"].as_object().expect("properties");
            for req in t.schema["required"].as_array().unwrap_or(&vec![]) {
                let r = req.as_str().expect("required str");
                assert!(
                    props.contains_key(r),
                    "{}: required {r} fora das props",
                    t.name
                );
            }
            // dispatch: com args mínimos NUNCA é UNKNOWN_TOOL (pode falhar
            // tipado por falta de app — o contrato é o que importa)
            let (v, _err) = call(&mut s, t.name, serde_json::json!({}));
            assert_ne!(v["code"], "UNKNOWN_TOOL", "{}: não despachada", t.name);
        }
        // set de despacho == set do catálogo (nenhuma tool fantasma)
        let (v, err) = call(&mut s, "definitely_not_a_tool", serde_json::json!({}));
        assert!(err && v["code"] == "UNKNOWN_TOOL");
    }

    /// Controles M5: sandbox de caminhos, URLs, APK excessivo e caps de tempo.
    #[test]
    fn policy_sandbox_urls_and_caps() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        // sandbox restrita: SOMENTE o diretório do fixture (URLs e `..` e
        // caminhos externos recusados ANTES de tocar o estado)
        let root = std::path::Path::new(&apk_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let mut s = AgentSession::new().with_policy(Policy {
            roots: vec![root.clone()],
            max_apk_bytes: 256 * 1024 * 1024,
            max_wait_ms: 1_000,
            max_batch_steps: 64,
        });

        // URL → URL_NOT_ALLOWED (o transporte v1 não baixa nada)
        let (v, err) = call(
            &mut s,
            "install",
            serde_json::json!({"apk": "https://exemplo.com/app.apk"}),
        );
        assert!(err);
        assert_eq!(v["code"], "URL_NOT_ALLOWED", "{v}");

        // traversal → PATH_OUTSIDE_SANDBOX
        let (v, err) = call(
            &mut s,
            "install",
            serde_json::json!({"apk": format!("{}/../outro.apk", root.display())}),
        );
        assert!(err);
        assert_eq!(v["code"], "PATH_OUTSIDE_SANDBOX", "{v}");

        // caminho fora das raízes → PATH_OUTSIDE_SANDBOX
        let (v, err) = call(
            &mut s,
            "install",
            serde_json::json!({"apk": "/etc/hostname"}),
        );
        assert!(err);
        assert_eq!(v["code"], "PATH_OUTSIDE_SANDBOX", "{v}");
        // nada foi instalado (transacional)
        let (v, _) = call(&mut s, "package_list", serde_json::json!({}));
        assert_eq!(v["packages"].as_array().unwrap().len(), 0);

        // install válido dentro do sandbox
        install_ok(&mut s, &apk_path);

        // APK excessivo: teto de 1 byte → APK_TOO_LARGE (checado antes de ler)
        let mut s2 = AgentSession::new().with_policy(Policy {
            roots: vec![root.clone()],
            max_apk_bytes: 1,
            max_wait_ms: 60_000,
            max_batch_steps: 64,
        });
        let (v, err) = call(&mut s2, "install", serde_json::json!({"apk": apk_path}));
        assert!(err);
        assert_eq!(v["code"], "APK_TOO_LARGE", "{v}");

        // tempo ilimitado não existe: wait > cap → LIMIT_EXCEEDED
        let (v, err) = call(&mut s, "launch", serde_json::json!({}));
        assert!(!err, "{v}");
        let (v, err) = call(&mut s, "wait", serde_json::json!({"ms": 5_000}));
        assert!(err);
        assert_eq!(v["code"], "LIMIT_EXCEEDED", "{v}");
        // swipe também tem cap
        let (v, err) = call(
            &mut s,
            "swipe",
            serde_json::json!({"x1": 0, "y1": 0, "x2": 1, "y2": 1, "ms": 99_999}),
        );
        assert!(err);
        assert_eq!(v["code"], "LIMIT_EXCEEDED", "{v}");

        // screenshot fora do sandbox → PATH_OUTSIDE_SANDBOX (antes de escrever)
        let (v, err) = call(
            &mut s,
            "screenshot",
            serde_json::json!({"path": "/tmp/fora-e-o-sandbox-nao-permite.png"}),
        );
        assert!(
            err,
            "screenshot fora do sandbox deve falhar com esta policy"
        );
        assert_eq!(v["code"], "PATH_OUTSIDE_SANDBOX", "{v}");
        // dentro do sandbox funciona
        let inside = root.join("dentro.png");
        let (v, err) = call(
            &mut s,
            "screenshot",
            serde_json::json!({"path": inside.display().to_string()}),
        );
        assert!(!err, "screenshot dentro: {v}");
        assert!(inside.exists(), "png escrito");
    }

    /// DoD (batch): ordem, stop_on_error, retomada idempotente (resume_from),
    /// cap de passos e proibição de aninhamento.
    #[test]
    fn batch_idempotent_resume_and_caps() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        // lote ok de ponta a ponta: executed == total, resume_hint null
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [
                {"name": "tap", "arguments": {"x": 5, "y": 5}},
                {"name": "wait", "arguments": {"ms": 10}},
                {"name": "assert_text", "arguments": {"resource_id": "com.test:id/tv", "expected": "Olá do MCP"}},
            ]}),
        );
        assert!(!err, "batch ok: {v}");
        assert_eq!(v["executed"], 3);
        assert_eq!(v["stopped_early"], false);
        assert!(v["resume_hint"].is_null());
        assert_eq!(v["steps"].as_array().unwrap().len(), 3);
        assert_eq!(v["steps"][2]["result"]["ok"], true);

        // stop_on_error: passo com falha tipada para o lote e aponta retomada
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [
                {"name": "wait", "arguments": {"ms": 10}},
                {"name": "assert_view", "arguments": {}}, // INVALID_FORMAT
                {"name": "tap", "arguments": {"x": 1, "y": 1}},
            ]}),
        );
        assert!(!err, "batch em si não falha — o PASSO falha: {v}");
        assert_eq!(v["stopped_early"], true);
        assert_eq!(v["executed"], 2, "para no passo do erro");
        assert_eq!(v["steps"][1]["ok"], false);
        assert_eq!(v["resume_hint"], 2);
        // retomada idempotente: resume_from=2 pula os 2 primeiros
        let (v, _) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [
                {"name": "wait", "arguments": {"ms": 10}},
                {"name": "assert_view", "arguments": {}},
                {"name": "tap", "arguments": {"x": 1, "y": 1}},
            ], "resume_from": 2}),
        );
        assert_eq!(v["steps"][0]["skipped"], true);
        assert_eq!(v["steps"][1]["skipped"], true);
        assert_eq!(v["executed"], 1, "só o passo pendente roda");
        assert_eq!(v["steps"][2]["name"], "tap");

        // stop_on_error=false: continua e reporta cada passo
        let (v, _) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [
                {"name": "assert_view", "arguments": {}},
                {"name": "wait", "arguments": {"ms": 10}},
            ], "stop_on_error": false}),
        );
        assert_eq!(v["executed"], 2);
        assert_eq!(v["steps"][0]["ok"], false);
        assert_eq!(v["steps"][1]["ok"], true);

        // aninhamento proibido + cap de passos
        let (v, err) = call(
            &mut s,
            "batch",
            serde_json::json!({"calls": [{"name": "batch", "arguments": {"calls": []}}]}),
        );
        assert!(err);
        assert_eq!(v["code"], "INVALID_FORMAT", "{v}");
        let mut s3 = AgentSession::new().with_policy(Policy {
            roots: vec![],
            max_apk_bytes: 1,
            max_wait_ms: 60_000,
            max_batch_steps: 2,
        });
        let (v, err) = call(
            &mut s3,
            "batch",
            serde_json::json!({"calls": [
                {"name": "ping_tool_nao_existe"}, {"name": "x"}, {"name": "y"}
            ]}),
        );
        assert!(err);
        assert_eq!(v["code"], "LIMIT_EXCEEDED", "{v}");
    }

    /// DoD (pacote): listar/reinstalar/remover/limpar no registro multi-app,
    /// com o app em execução encerrado quando o Android encerraria.
    #[test]
    fn package_registry_lifecycle() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_a = fixture_apk_path(); // com.test.mcp
        let apk_b = fixture_form_apk_path("com.test.form");
        let mut s = AgentSession::new();

        install_ok(&mut s, &apk_a);
        // reinstalar o MESMO package: update in-place (não duplica)
        install_ok(&mut s, &apk_a);
        let (v, _) = call(&mut s, "package_list", serde_json::json!({}));
        assert_eq!(v["packages"].as_array().unwrap().len(), 1);

        // segundo package + launch explícito do B (multi-app)
        install_ok(&mut s, &apk_b);
        let (v, _) = call(&mut s, "package_list", serde_json::json!({}));
        let pkgs = v["packages"].as_array().unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[1]["package"], "com.test.form");
        assert_eq!(pkgs[1]["active"], true);
        launch_ok(&mut s, serde_json::json!({"package": "com.test.form"}));
        let (v, _) = call(&mut s, "runtime_health", serde_json::json!({}));
        assert_eq!(v["running_package"], "com.test.form");

        // remove do app EM EXECUÇÃO: mata o engine primeiro (nunca
        // estado inconsistente) — depois do remove, input falha NOT_RUNNING
        let (v, err) = call(
            &mut s,
            "package_remove",
            serde_json::json!({"package": "com.test.form"}),
        );
        assert!(!err, "{v}");
        assert_eq!(v["killed_running"], true);
        let (v, err) = call(&mut s, "tap", serde_json::json!({"x": 1, "y": 1}));
        assert!(err);
        assert_eq!(v["code"], "NOT_RUNNING", "{v}");
        // ativo cai para o remanescente
        let (v, _) = call(&mut s, "package_list", serde_json::json!({}));
        let pkgs = v["packages"].as_array().unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0]["package"], "com.test.mcp");

        // clear: app morto (se rodando) e permissões voltam ao estado de
        // instalação — o fixture do form declara CAMERA (runtime)
        let apk_form = fixture_form_apk_path("com.test.form");
        install_ok(&mut s, &apk_form);
        let (v, _) = call(
            &mut s,
            "permissions_grant",
            serde_json::json!({"permission": "android.permission.CAMERA"}),
        );
        assert_eq!(v["state"], "granted", "{v}");
        let (v, _) = call(
            &mut s,
            "package_clear",
            serde_json::json!({"package": "com.test.form"}),
        );
        assert_eq!(v["cleared"], true);
        let (v, _) = call(&mut s, "permissions_list", serde_json::json!({}));
        let cam = v["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["permission"] == "android.permission.CAMERA")
            .expect("CAMERA listada");
        assert_ne!(cam["state"], "granted", "clear reseta permissões: {cam}");

        // remove inexistente → tipado
        let (v, err) = call(
            &mut s,
            "package_remove",
            serde_json::json!({"package": "nao.instalado"}),
        );
        assert!(err);
        assert_eq!(v["code"], "PACKAGE_NOT_INSTALLED", "{v}");
    }

    /// issue #110: o MCP reflete que signature-class FORA da whitelist B5 não
    /// é concedida — `permissions_list` reporta `denied-system` e
    /// `permissions_grant` não muda o estado (BATTERY_STATS: o Android real
    /// não a entrega a apps de terceiros; a sessão não pode concedê-la).
    #[test]
    fn permissions_denied_system_for_signature_outside_b5() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let (arsc, layout, dex) = fixture_parts();
        let manifest = rd_testsupport::apkfix::build_manifest_axml_sdk_perms(
            "com.test.mcp",
            "com.test.mcp.Main",
            26,
            30,
            &["android.permission.BATTERY_STATS"],
        );
        let apk_path = fixture_apk_write(fixture_apk_zip(manifest, arsc, layout, dex));
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);

        let (v, err) = call(&mut s, "permissions_list", serde_json::json!({}));
        assert!(!err, "{v}");
        let bs = v["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["permission"] == "android.permission.BATTERY_STATS")
            .expect("BATTERY_STATS listada")
            .clone();
        assert_eq!(bs["state"], "denied-system", "lista reflete a regra: {v}");

        // grant não concede: reporta o mesmo estado (sem falso sucesso)
        let (v, err) = call(
            &mut s,
            "permissions_grant",
            serde_json::json!({"permission": "android.permission.BATTERY_STATS"}),
        );
        assert!(!err, "grant responde com o estado honesto: {v}");
        assert_eq!(v["state"], "denied-system", "{v}");

        // e a lista continua coerente depois da tentativa de grant
        let (v, err) = call(&mut s, "permissions_list", serde_json::json!({}));
        assert!(!err, "{v}");
        let bs = v["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["permission"] == "android.permission.BATTERY_STATS")
            .expect("BATTERY_STATS listada");
        assert_eq!(bs["state"], "denied-system", "grant não muda o estado: {v}");
    }

    /// DoD (isolamento + ids): sessões distintas têm ids distintos e
    /// registros independentes; TODO resultado carrega sessionId/package.
    #[test]
    fn session_ids_and_isolation() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_a = fixture_apk_path();
        let apk_b = fixture_form_apk_path("com.test.form");
        let mut s1 = AgentSession::new();
        let mut s2 = AgentSession::new();
        assert_ne!(s1.session_id, s2.session_id, "ids por sessão");

        install_ok(&mut s1, &apk_a);
        install_ok(&mut s2, &apk_b);

        // resultado carrega os identificadores (contrato do enunciado)
        let (v, _) = call(&mut s1, "package_list", serde_json::json!({}));
        assert_eq!(v["session_id"], s1.session_id);
        let (v, _) = call(&mut s2, "app_info", serde_json::json!({}));
        assert_eq!(v["package"], "com.test.form");

        // isolamento de registro: remove em s1 não afeta s2
        let (v, _) = call(
            &mut s1,
            "package_remove",
            serde_json::json!({"package": "com.test.mcp"}),
        );
        assert_eq!(v["removed"], true);
        let (v, _) = call(&mut s2, "package_list", serde_json::json!({}));
        assert_eq!(v["packages"].as_array().unwrap().len(), 1, "s2 intacta");
    }

    /// DoD (espera condicional): wait_for encontra já-existente sem esperar,
    /// vence por timeout honesto (found=false) e respeita o cap.
    #[test]
    fn wait_for_condition_and_caps() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_apk_path();
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        // já existe: polls=0, waited=0
        let (v, err) = call(
            &mut s,
            "wait_for",
            serde_json::json!({"resource_id": "com.test:id/tv", "timeout_ms": 1_000}),
        );
        assert!(!err, "{v}");
        assert_eq!(v["found"], true);
        assert_eq!(v["polls"], 0);
        assert_eq!(v["waited_ms"], 0);

        // inexistente: vence por timeout — resposta honesta, não erro
        let (v, err) = call(
            &mut s,
            "wait_for",
            serde_json::json!({"text": "nunca-existe", "timeout_ms": 200, "step_ms": 50}),
        );
        assert!(!err, "timeout de wait_for não é erro: {v}");
        assert_eq!(v["found"], false);
        assert_eq!(v["waited_ms"], 200);
        assert_eq!(v["polls"], 4);

        // sem seletor → INVALID_FORMAT
        let (v, err) = call(&mut s, "wait_for", serde_json::json!({"timeout_ms": 100}));
        assert!(err);
        assert_eq!(v["code"], "INVALID_FORMAT", "{v}");

        // cap: timeout acima do teto da policy → LIMIT_EXCEEDED
        let root = std::path::Path::new(&apk_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let mut s2 = AgentSession::new().with_policy(Policy {
            roots: vec![root],
            max_apk_bytes: 256 * 1024 * 1024,
            max_wait_ms: 500,
            max_batch_steps: 64,
        });
        install_ok(&mut s2, &apk_path);
        launch_ok(&mut s2, serde_json::json!({}));
        let (v, err) = call(
            &mut s2,
            "wait_for",
            serde_json::json!({"text": "x", "timeout_ms": 4_000}),
        );
        assert!(err);
        assert_eq!(v["code"], "LIMIT_EXCEEDED", "{v}");
    }

    /// DoD (input): swipe vira clique na mesma view (listener do app roda),
    /// swipe cruzando views só relata; type_text anexa/substitui na EditText;
    /// key DEL apaga e BACK finaliza a activity.
    #[test]
    fn swipe_type_key_form_flow() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let apk_path = fixture_form_apk_path("com.test.form");
        let mut s = AgentSession::new();
        install_ok(&mut s, &apk_path);
        launch_ok(&mut s, serde_json::json!({}));

        // layout vertical: et (0,0,720,96) · btn (0,96,720,96) · tv (0,192,720,48)
        // swipe DENTRO do botão: down/up na mesma view → clique
        let (v, err) = call(
            &mut s,
            "swipe",
            serde_json::json!({"x1": 10, "y1": 100, "x2": 700, "y2": 140, "ms": 120}),
        );
        assert!(!err, "swipe clique: {v}");
        assert_eq!(v["clicked"], true, "down+up na mesma view = clique: {v}");
        let (v, _) = call(&mut s, "logs", serde_json::json!({}));
        assert!(
            v["logs"].as_str().unwrap().contains("btn via swipe"),
            "listener do app rodou no swipe-clique: {v}"
        );

        // swipe cruzando et → tv: scroll/fling (relatório, sem clique)
        let (v, err) = call(
            &mut s,
            "swipe",
            serde_json::json!({"x1": 10, "y1": 10, "x2": 10, "y2": 210, "ms": 100}),
        );
        assert!(!err);
        assert_eq!(v["clicked"], false);
        let crossed = v["crossed"].as_array().unwrap();
        assert!(crossed.len() >= 2, "atravessa ≥2 views: {crossed:?}");
        assert!(crossed[0]["class"].as_str().unwrap().contains("EditText"));

        // type_text: anexa e substitui na EditText
        let (v, err) = call(&mut s, "type_text", serde_json::json!({"text": "abc"}));
        assert!(!err, "type_text: {v}");
        assert_eq!(v["final_text"], "abc");
        let (v, _) = call(&mut s, "type_text", serde_json::json!({"text": "def"}));
        assert_eq!(v["final_text"], "abcdef", "digitação anexa");
        let (v, err) = call(
            &mut s,
            "assert_text",
            serde_json::json!({"resource_id": "com.test:id/et", "expected": "abcdef"}),
        );
        assert!(!err);
        assert_eq!(v["ok"], true, "assert enxerga o texto digitado");
        let (v, _) = call(
            &mut s,
            "type_text",
            serde_json::json!({"text": "z", "replace": true}),
        );
        assert_eq!(v["final_text"], "z", "replace substitui");

        // key DEL apaga o último caractere
        let (v, err) = call(&mut s, "key", serde_json::json!({"key": "DEL"}));
        assert!(!err, "{v}");
        assert_eq!(v["delivered"], true);
        let (v, _) = call(
            &mut s,
            "assert_text",
            serde_json::json!({"resource_id": "com.test:id/et", "expected": ""}),
        );
        assert_eq!(v["actual"], "", "DEL apagou o 'z': {v}");

        // tecla fora do modelo v1: tipado com a lista suportada
        let (v, err) = call(&mut s, "key", serde_json::json!({"key": "MENU"}));
        assert!(err);
        assert_eq!(v["code"], "NOT_IMPLEMENTED", "{v}");
        assert!(v["suggestion"].as_str().unwrap().contains("BACK"));

        // key BACK finaliza a activity corrente
        let (v, err) = call(&mut s, "key", serde_json::json!({"key": "BACK"}));
        assert!(!err, "{v}");
        let (v, _) = call(&mut s, "assert_finished", serde_json::json!({}));
        assert_eq!(v["finished"], true, "BACK = finish");
    }

    /// DoD (docs geradas): docs/mcp-tools.md é exatamente tools_markdown()
    /// — o catálogo publicado nunca diverge do código.
    #[test]
    fn generated_docs_match_catalog() {
        let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let path = docs_dir.join("mcp-tools.md");
        let generated = tools_markdown();
        assert!(generated.ends_with('\n'));
        assert!(!generated.ends_with("\n\n"), "no generated blank EOF line");
        match std::fs::read_to_string(&path) {
            Ok(committed) => {
                assert_eq!(
                    committed, generated,
                    "docs/mcp-tools.md divergiu do catálogo — regenere com `rd mcp --gen-docs docs/mcp-tools.md`"
                );
            }
            Err(_) => {
                std::fs::write(&path, &generated).expect("gera docs na primeira execução");
            }
        }
    }
    /// M9: the agent drives the SAME component state the app APIs use —
    /// component activation from the manifest, IME, accessibility, widgets,
    /// notification permission sync and shortcuts.
    #[cfg(feature = "advanced-components")]
    #[test]
    fn m9_agent_tools_drive_components() {
        let _guard = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = AgentSession::new();
        let apk = fixture_apk_write(rd_testsupport::m9_app::apk());
        install_ok(&mut s, &apk);
        launch_ok(&mut s, serde_json::json!({}));
        // undeclared component is refused
        let (v, err) = call(
            &mut s,
            "component_service",
            serde_json::json!({"class":"test.m9.Nope","role":"ime"}),
        );
        assert!(err, "{v}");
        let (v, err) = call(
            &mut s,
            "component_service",
            serde_json::json!({"class":"test.m9.Accessibility","role":"accessibility"}),
        );
        assert!(!err, "{v}");
        let (tree, err) = call(
            &mut s,
            "accessibility",
            serde_json::json!({"action":"tree"}),
        );
        assert!(!err, "{tree}");
        let editor = tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["editable"] == true)
            .expect("editable")["id"]
            .as_u64()
            .unwrap();
        let (v, err) = call(
            &mut s,
            "ime",
            serde_json::json!({"action":"focus","view":editor}),
        );
        assert!(!err, "{v}");
        let (v, err) = call(
            &mut s,
            "ime",
            serde_json::json!({"action":"commit","text":"!"}),
        );
        assert!(!err, "{v}");
        assert_eq!(v["text"], "A😀B!");
        let (v, err) = call(
            &mut s,
            "accessibility",
            serde_json::json!({"action":"perform","node":editor,"action_id":2097152,"text":"set"}),
        );
        assert!(!err && v["performed"] == true, "{v}");
        let (v, _) = call(
            &mut s,
            "accessibility",
            serde_json::json!({"action":"events"}),
        );
        assert!(!v["events"].as_array().unwrap().is_empty());
        // widgets: allocate + bind a declared provider (DEX onUpdate runs)
        let (p, err) = call(
            &mut s,
            "component_service",
            serde_json::json!({"class":"test.m9.Widget","role":"widget"}),
        );
        assert!(!err, "{p}");
        let (w, err) = call(&mut s, "widgets", serde_json::json!({"action":"allocate"}));
        assert!(!err, "{w}");
        let (v, err) = call(
            &mut s,
            "widgets",
            serde_json::json!({"action":"bind","id":w["id"],"provider":p["object"]}),
        );
        assert!(!err, "{v}");
        // notification permission (targetSdk 34) syncs into the running engine
        let (v, _) = call(&mut s, "notifications", serde_json::json!({}));
        assert_eq!(v["permission_granted"], false, "{v}");
        let (_, err) = call(
            &mut s,
            "permissions_grant",
            serde_json::json!({"permission":"android.permission.POST_NOTIFICATIONS"}),
        );
        assert!(!err);
        let (v, _) = call(&mut s, "notifications", serde_json::json!({}));
        assert_eq!(v["permission_granted"], true, "{v}");
        let (v, err) = call(
            &mut s,
            "shortcuts",
            serde_json::json!({"action":"launch","id":"missing"}),
        );
        assert!(err, "{v}");
        let (v, err) = call(
            &mut s,
            "ime",
            serde_json::json!({"action":"select","start":-1,"end":0}),
        );
        assert!(err, "{v}");
    }
    include!("ui_tools_tests.rs");
    include!("runtime_tools_tests.rs");
}
