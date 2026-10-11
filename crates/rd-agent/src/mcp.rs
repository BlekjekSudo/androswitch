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
use std::path::{Component, PathBuf};
use std::sync::{Arc, Mutex};

use rd_apk::manifest::ComponentKind;
use rd_apk::{Apk, RdError};
use rd_framework::fd_behavior::SwitchTable;
use rd_framework::fd_devices::DeviceTable;
use rd_framework::fd_permissions::{GrantState, PermissionEngine, PermissionTable};
use rd_vm::framework::UiNode;
use rd_vm::{Engine, HostOut, Value, VmConfig, VmExit};

pub const MODULE_ID: &str = "rd-agent";

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
        Ok(canon_parent.join(p.file_name().unwrap_or_default()))
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
    pub apk: Apk,
    pub perms: PermissionEngine,
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
    activity: Option<String>,
    /// System.out do app (issue #55: buffer da sessão, tool `logs` lê).
    logs: Arc<Mutex<Vec<u8>>>,
    devices: DeviceTable,
    behavior: SwitchTable,
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
            activity: None,
            logs,
            devices: DeviceTable::embedded().expect("devices.toml embutido parseia"),
            behavior: SwitchTable::embedded().expect("behavior-switches.toml embutido parseia"),
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
        self.activity.as_deref()
    }

    /// Mata o engine se o app em execução é `package`. Devolve se matou.
    /// (remove/clear/install do MESMO package: Android mata o app — nunca
    /// deixa estado inconsistente.)
    fn kill_engine_if_running(&mut self, package: &str) -> bool {
        if self.engine_package.as_deref() == Some(package) {
            self.engine = None;
            self.engine_package = None;
            self.activity = None;
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
        let d =
            rd_dex::Dex::parse(bytes).map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
        out.push(d);
    }
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

/// Registro das 30 tools (M5 completo: pacote list/rem/clear, input
/// tap/swipe/type/key, observação, wait/wait_for, asserts, permissões,
/// estado/saúde, lacunas de compat., batch — spec PARTE 4).
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
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
                "Executa o app instalado: cria a activity (LAUNCHER ou `activity`) e roda o lifecycle até o onCreate completar. Com `package` lança outro app do registro (multi-app); sem, o app ativo (último install/launch).",
            schema: obj(
                serde_json::json!({
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
                "Digita texto na primeira EditText visível+habilitada da activity (modelo v1 sem foco real — issue #54). `replace=true` substitui; default anexa. Devolve o texto final.",
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
                "Evento de tecla: BACK (finaliza a activity corrente), DEL (apaga último caractere da EditText), ENTER (entregue, sem IME action no v1). Outras teclas: erro tipado NOT_IMPLEMENTED com a lista suportada.",
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
                "Espera CONDICIONAL: avança o clock virtual em passos de `step_ms` (default 100) até a view (resource_id/text/class) existir ou `timeout_ms` vencer (cap RD_MCP_MAX_WAIT_MS). found=false é resposta honesta (não é erro) — o agente decide.",
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
                "Screenshot PNG determinístico 720×1440. Com `path` escreve o arquivo (DENTRO do sandbox — RD_MCP_SANDBOX); sem, devolve o PNG em base64 (conteúdo image).",
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
                "Asserção de existência: procura view por resource-id (ex.: com.app:id/btn), texto ou classe. Retorna matches.",
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
            description: "Identidade do app ATIVO: package, versão, SDKs, floor e componentes do manifest.",
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
            description: "Perfil de device corrente (piso contratual moto-e5) com os budgets de hardware.",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "behavior_switches",
            description: "Comutadores de comportamento targetSdk 26→36 ativos para o app instalado (PRF-07).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "runtime_health",
            description:
                "Saúde do runtime: sessão, app em execução, activity, finished, clock virtual, heap, fuel, fila do Looper, bytes de log e tamanho do registro — sem exigir app (reporta o que existe).",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "compat_gaps",
            description:
                "Catálogo HONESTO das lacunas de compatibilidade do runtime v1 (notificações M9, args String/array na sonda, foco, teclas, gestures, intent implícito, transporte HTTP) — o agente consulta antes de planejar.",
            schema: obj(serde_json::json!({}), &[]),
        },
        ToolDef {
            name: "batch",
            description:
                "Executa uma lista de chamadas [{name, arguments}] EM ORDEM, uma vez cada, com resultados por passo. stop_on_error (default true) para no 1º erro; `resume_from` pula os N primeiros passos já executados — retomada idempotente. Cap RD_MCP_MAX_BATCH (default 64). batch não aninha.",
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
            name: "notifications",
            description:
                "Notificações do app. FORA DO ESCOPO do M5 (entram no M9 com widgets/IME) — responde erro tipado NOT_IMPLEMENTED (honesto).",
            schema: obj(serde_json::json!({}), &[]),
        },
    ]
}

impl AgentSession {
    /// Dispatch de uma tool (o envelope MCP chama isto). Erros de tool voltam
    /// como ToolResult::is_error com o contrato {code, cause, suggestion}.
    pub fn call_tool(&mut self, name: &str, args: &serde_json::Value) -> ToolResult {
        let args = args.as_object().cloned().unwrap_or_default();
        let res: Result<ToolResult, RdError> = match name {
            "install" => self.tool_install(args.get("apk").and_then(|v| v.as_str()).unwrap_or("")),
            "launch" => self.tool_launch(
                args.get("activity").and_then(|v| v.as_str()),
                args.get("package").and_then(|v| v.as_str()),
            ),
            "tap" => {
                let (Some(x), Some(y)) = (
                    args.get("x").and_then(|v| v.as_i64()),
                    args.get("y").and_then(|v| v.as_i64()),
                ) else {
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "tap exige x,y inteiros",
                        Some("use {\"x\": <int>, \"y\": <int>} (px do viewport 720×1440)"),
                    ));
                };
                self.tool_tap(x as i32, y as i32)
            }
            "swipe" => {
                let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    args.get("x1").and_then(|v| v.as_i64()),
                    args.get("y1").and_then(|v| v.as_i64()),
                    args.get("x2").and_then(|v| v.as_i64()),
                    args.get("y2").and_then(|v| v.as_i64()),
                ) else {
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "swipe exige x1,y1,x2,y2 inteiros",
                        Some("use {\"x1\":…,\"y1\":…,\"x2\":…,\"y2\":…,\"ms\":…} (px do viewport)"),
                    ));
                };
                let ms = args.get("ms").and_then(|v| v.as_u64()).unwrap_or(300);
                self.tool_swipe(x1 as i32, y1 as i32, x2 as i32, y2 as i32, ms)
            }
            "type_text" => {
                let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "type_text exige text (string)",
                        Some("use {\"text\": \"…\", \"replace\": false}"),
                    ));
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
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "key exige key (BACK|ENTER|DEL)",
                        None,
                    ));
                };
                self.tool_key(key)
            }
            "wait" => {
                let Some(ms) = args.get("ms").and_then(|v| v.as_u64()) else {
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "wait exige ms inteiro ≥ 0",
                        Some("use {\"ms\": 250}"),
                    ));
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
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "assert_text exige resource_id",
                        None,
                    ));
                };
                let Some(expected) = args.get("expected").and_then(|v| v.as_str()) else {
                    return ToolResult::error(&agent_error(
                        "INVALID_FORMAT",
                        "assert_text exige expected",
                        None,
                    ));
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
            "device_profile" => self.tool_device_profile(),
            "behavior_switches" => self.tool_behavior_switches(),
            "runtime_health" => self.tool_runtime_health(),
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
            "notifications" => self.tool_notifications(),
            other => {
                return ToolResult::error(&agent_error(
                    "UNKNOWN_TOOL",
                    format!("tool '{other}' não existe neste servidor"),
                    Some("veja tools/list para o catálogo"),
                ))
            }
        };
        match res {
            Ok(r) => r,
            Err(e) => ToolResult::error(&e),
        }
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
        let bytes = std::fs::read(&apk_file).map_err(|e| {
            RdError::new(
                "IO_ERROR",
                format!("lendo {}: {e}", apk_file.display()),
                MODULE_ID,
            )
            .with_suggestion("confira o caminho absoluto do APK")
        })?;
        let apk =
            Apk::from_bytes(bytes).map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?;
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
            apk,
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

    fn tool_launch(
        &mut self,
        activity: Option<&str>,
        package: Option<&str>,
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
        let (desc, package_out) = match activity {
            Some(a) => (normalize_activity_desc(a), app.apk.manifest.package.clone()),
            None => {
                let launcher = app
                    .apk
                    .manifest
                    .application
                    .components
                    .iter()
                    .find(|c| matches!(c.kind, ComponentKind::Activity) && c.is_launcher)
                    .ok_or_else(|| {
                        agent_error(
                            "MISSING_ENTRY",
                            "activity LAUNCHER não encontrada no manifest",
                            Some("passe {\"activity\": \"com.ex.Main\"} explicitamente"),
                        )
                    })?;
                let cls = launcher.class_name.clone();
                (
                    format!("L{};", cls.replace('.', "/")),
                    app.apk.manifest.package.clone(),
                )
            }
        };
        let dexes = dexes_of_apk(&app.apk)?;
        let resources = app.apk.clone();
        let mut eng = Engine::new(dexes, VmConfig::default());
        eng.set_resources(resources);
        eng.launch_app(&desc, &package_out, Vec::new())
            .map_err(vm_exit_to_rd)?;
        self.engine = Some(eng);
        self.engine_package = Some(package_out.clone());
        self.activity = Some(desc.clone());
        self.active = Some(package_out.clone());
        Ok(ToolResult::json(&serde_json::json!({
            "launched": true,
            "session_id": self.session_id,
            "activity": desc,
            "package": package_out,
        })))
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
        let ran = self
            .require_engine()?
            .advance_clock(ms)
            .map_err(vm_exit_to_rd)?;
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
        if resource_id.is_none() && text.is_none() && class.is_none() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "wait_for exige resource_id, text ou class",
                None,
            ));
        }
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
        let tree = self.require_engine()?.view_tree().ok_or_else(|| {
            agent_error(
                "INVALID_FORMAT",
                "sem activity/window para screenshot",
                None,
            )
        })?;
        let png = rd_render::render_snapshot(&tree).to_png();
        match path {
            Some(p) if !p.is_empty() => {
                // Controle M5: escrita de arquivo SÓ dentro do sandbox
                let out = self.policy.check_write_path(p, &self.roots)?;
                std::fs::write(&out, &png).map_err(|e| {
                    RdError::new(
                        "IO_ERROR",
                        format!("escrevendo {}: {e}", out.display()),
                        MODULE_ID,
                    )
                })?;
                Ok(ToolResult::json(&serde_json::json!({
                    "path": out.display().to_string(),
                    "bytes": png.len(),
                    "size": [720, 1440],
                })))
            }
            _ => Ok(ToolResult {
                content: vec![ToolContent::Image {
                    data_b64: base64_encode(&png),
                    mime: "image/png",
                }],
                is_error: false,
            }),
        }
    }

    fn tool_logs(&mut self, clear: bool) -> Result<ToolResult, RdError> {
        let mut buf = self.logs.lock().expect("logs");
        let text = String::from_utf8_lossy(&buf).to_string();
        if clear {
            buf.clear();
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
        if resource_id.is_none() && text.is_none() && class.is_none() {
            return Err(agent_error(
                "INVALID_FORMAT",
                "assert_view exige resource_id, text ou class",
                None,
            ));
        }
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
        let app = self.require_apk()?;
        let m = &app.apk.manifest;
        Ok(ToolResult::json(&serde_json::json!({
            "package": m.package,
            "version_name": m.version_name,
            "version_code": m.version_code,
            "min_sdk": m.min_sdk,
            "target_sdk": m.target_sdk,
            "floor": serde_json::json!(match app.apk.floor_status() {
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
            app.apk.manifest.clone()
        };
        // Android clear data: app em execução é morto e as permissões
        // runtime voltam ao estado de instalação.
        let killed = self.kill_engine_if_running(package);
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
        Ok(ToolResult::json(&serde_json::json!({
            "permission": permission,
            "state": grant_state_str(st),
        })))
    }

    fn tool_permissions_audit(&mut self) -> Result<ToolResult, RdError> {
        let app = self.require_apk()?;
        let findings: Vec<serde_json::Value> = app
            .perms
            .audit(&app.apk.manifest)
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

    fn tool_device_profile(&mut self) -> Result<ToolResult, RdError> {
        // v1: o device virtual é o piso contratual moto-e5
        let d = self.devices.get("moto-e5").ok_or_else(|| {
            agent_error(
                "INTERNAL",
                "perfil moto-e5 ausente dos dados embutidos",
                None,
            )
        })?;
        Ok(ToolResult::json(&serde_json::json!({
            "id": d.id,
            "label": d.label,
            "class": d.class,
            "screen": [d.width_px, d.height_px],
            "density_dpi": d.density_dpi,
            "refresh_hz": d.refresh_hz,
            "cpu_cores": d.cpu_cores,
            "hardware_floor": d.is_hardware_floor(),
            "budgets": d.budget_summary(),
        })))
    }

    fn tool_behavior_switches(&mut self) -> Result<ToolResult, RdError> {
        let target = self
            .require_apk()
            .map(|a| a.apk.manifest.target_sdk)
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
                v["activity"] = serde_json::json!(self.activity);
                v["finished"] = serde_json::json!(eng.activity_is_finished());
                v["clock_ms"] = serde_json::json!(eng.fw.clock());
                v["heap_used_bytes"] = serde_json::json!(eng.heap_used());
                v["fuel_used"] = serde_json::json!(eng.fuel_used());
                v["looper_pending"] = serde_json::json!(eng.looper_pending());
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
            (
                "notifications",
                "ui",
                "not-implemented",
                "notificações entram no M9 (widgets/IME/notificações)",
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
                "sem foco real (issue #54): type_text digita na primeira EditText visível+habilitada",
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
                "IME e widgets entram no M9",
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
        Ok(ToolResult::json(&serde_json::json!({
            "gaps": gaps,
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
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name == "batch" {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("batch não aninha (passo {i})"),
                    Some("chame as sub-chamadas direto no lote"),
                ));
            }
            if name.is_empty() {
                return Err(agent_error(
                    "INVALID_FORMAT",
                    format!("passo {i} sem \"name\""),
                    None,
                ));
            }
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
            "resume_hint": if stopped_early || executed < list.len() {
                serde_json::json!(executed + resume_from)
            } else {
                serde_json::json!(null)
            },
            "steps": steps,
        })))
    }

    fn tool_notifications(&mut self) -> Result<ToolResult, RdError> {
        Err(agent_error(
            "NOT_IMPLEMENTED",
            "notificações entram no M9 (widgets/IME/notificações) — fora do escopo do agente v1",
            Some("acompanhe docs/ROADMAP.md (M9)"),
        ))
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
        GrantState::RuntimePending => "runtime-pending",
        GrantState::SpecialPending => "special-pending",
        GrantState::Unknown => "unknown",
        GrantState::NotRequested => "not-requested",
    }
}

/// Busca DFS na árvore de UI por resource-id/text/class.
fn collect_matches(
    n: &UiNode,
    rid: Option<&str>,
    text: Option<&str>,
    class: Option<&str>,
    out: &mut Vec<serde_json::Value>,
) {
    let rid_hit = rid.map_or(true, |r| n.resource_id.as_deref() == Some(r));
    let text_hit = text.map_or(true, |t| n.text.contains(t));
    let class_hit = class.map_or(true, |c| n.class == c || n.class.ends_with(c));
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
            "I" => Value::Int(a.as_i64().ok_or_else(|| bad_arg(ty, a))? as i32),
            "J" => Value::Long(a.as_i64().ok_or_else(|| bad_arg(ty, a))?),
            "F" => Value::Float(a.as_f64().ok_or_else(|| bad_arg(ty, a))? as f32),
            "D" => Value::Double(a.as_f64().ok_or_else(|| bad_arg(ty, a))?),
            "Z" => Value::Int(a.as_bool().ok_or_else(|| bad_arg(ty, a))? as i32),
            "C" => {
                let s = a.as_str().ok_or_else(|| bad_arg(ty, a))?;
                let c = s.chars().next().ok_or_else(|| bad_arg(ty, a))?;
                Value::Int(c as i32)
            }
            "B" => Value::Int(a.as_i64().ok_or_else(|| bad_arg(ty, a))? as i8 as i32),
            "S" => Value::Int(a.as_i64().ok_or_else(|| bad_arg(ty, a))? as i16 as i32),
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
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();
    let is_notification = id.is_none();

    match method {
        "initialize" => Some(Ok(jsonrpc_result(
            id,
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
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
            let result = session.call_tool(&name, args);
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
        m if m.starts_with("notifications/") => None, // notificações conhecidas: silêncio
        other => Some(Err(jsonrpc_error(
            id,
            -32601,
            &format!("método '{other}' não existe (initialize/tools/list/tools/call/ping)"),
        ))),
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
    mut output: W,
) -> Result<(), RdError> {
    let reader = std::io::BufReader::new(input);
    let mut session = AgentSession::new();
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
    }
    Ok(())
}

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
    fn fixture_apk_path() -> String {
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
        let apk = rd_testsupport::apkfix::build_apk_sdk(
            "com.test.mcp",
            "com.test.mcp.Main",
            arsc.build(),
            vec![
                ("res/layout/activity_main.xml", build_axml(&layout_xml)),
                ("classes.dex", b.finish()),
            ],
            26,
            30,
        );
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
    fn notifications_is_typed_not_implemented() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        let (v, err) = call(&mut s, "notifications", serde_json::json!({}));
        assert!(err);
        assert_eq!(v["code"], "NOT_IMPLEMENTED");
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
        assert_eq!(tools.len(), 30, "catálogo M5 completo: 30 tools");
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

    /// DoD (schemas): toda tool do catálogo despacha (nada de UNKNOWN_TOOL)
    /// e tem schema com required ⊆ properties.
    #[test]
    fn catalog_matches_dispatch_and_schemas() {
        let _guard = SESSION_LOCK.lock().unwrap();
        let mut s = AgentSession::new();
        let defs = tool_defs();
        assert_eq!(defs.len(), 30);
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
}
