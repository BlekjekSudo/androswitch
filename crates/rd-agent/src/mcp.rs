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

// ── sessão do agente ────────────────────────────────────────────────────────

/// Estado do "dispositivo" headless: app instalado, app rodando, permissões,
/// device profile. Uma sessão por processo (ADR 001).
pub struct AgentSession {
    apk: Option<Apk>,
    perms: Option<PermissionEngine>,
    engine: Option<Engine>,
    package: Option<String>,
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
        AgentSession {
            apk: None,
            perms: None,
            engine: None,
            package: None,
            activity: None,
            logs,
            devices: DeviceTable::embedded().expect("devices.toml embutido parseia"),
            behavior: SwitchTable::embedded().expect("behavior-switches.toml embutido parseia"),
        }
    }

    fn require_apk(&self) -> Result<&Apk, RdError> {
        self.apk.as_ref().ok_or_else(|| {
            agent_error(
                "NO_APP",
                "nenhum APK instalado nesta sessão",
                Some("chame a tool `install` com o caminho do APK primeiro"),
            )
        })
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

// ── as 20 tools do M5 ───────────────────────────────────────────────────────

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

/// Registro das ~20 tools (M5: install/launch/input/ui_tree/screenshot/logs/
/// assert/permissions/notifications — spec PARTE 4).
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "install",
            description:
                "Instala um APK na sessão (abre, valida o piso API 26 e prepara o estado de permissões). Não executa o app.",
            schema: obj(
                serde_json::json!({ "apk": { "type": "string", "description": "caminho do arquivo .apk" } }),
                &["apk"],
            ),
        },
        ToolDef {
            name: "launch",
            description:
                "Executa o app instalado: cria a activity (LAUNCHER ou `activity`) e roda o lifecycle até o onCreate completar.",
            schema: obj(
                serde_json::json!({ "activity": { "type": "string", "description": "classe da activity (com.ex.Main ou Lcom/ex/Main;). Default: LAUNCHER" } }),
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
            name: "wait",
            description:
                "Avança o clock virtual (ms) e executa os runnables do Looper com deadline vencida. Retorna quantos rodaram.",
            schema: obj(
                serde_json::json!({ "ms": { "type": "integer", "minimum": 0 } }),
                &["ms"],
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
                "Screenshot PNG determinístico 720×1440. Com `path` escreve o arquivo; sem, devolve o PNG em base64 (conteúdo image).",
            schema: obj(
                serde_json::json!({ "path": { "type": "string", "description": "arquivo de saída (.png)" } }),
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
            description: "Identidade do app instalado: package, versão, SDKs, floor e componentes do manifest.",
            schema: obj(serde_json::json!({}), &[]),
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
            "launch" => self.tool_launch(args.get("activity").and_then(|v| v.as_str())),
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
        let bytes = std::fs::read(apk_path).map_err(|e| {
            RdError::new("IO_ERROR", format!("lendo {}: {e}", apk_path), MODULE_ID)
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
        let manifest = apk.manifest.clone();
        let package = manifest.package.clone();
        let components = manifest.application.components.len();
        let perms = PermissionEngine::install(
            &manifest,
            PermissionTable::embedded()
                .map_err(|e| RdError::new(&e.code, &e.cause, &e.module_id))?,
        );
        let version = manifest.version_name.clone().unwrap_or_else(|| "?".into());
        self.apk = Some(apk);
        self.perms = Some(perms);
        self.engine = None; // instalar troca de app: sessão de execução antiga morre
        self.activity = None;
        self.package = Some(package.clone());
        Ok(ToolResult::json(&serde_json::json!({
            "installed": true,
            "package": package,
            "version_name": version,
            "components": components,
            "runtime_pending": self.perms.as_ref().map(|p| p.pending_runtime().len()).unwrap_or(0),
            "runtime_special": self.perms.as_ref().map(|p| p.pending_special().len()).unwrap_or(0),
        })))
    }

    fn tool_launch(&mut self, activity: Option<&str>) -> Result<ToolResult, RdError> {
        let apk = self
            .apk
            .as_ref()
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro (tool install)", None))?;
        let (desc, package) = match activity {
            Some(a) => (normalize_activity_desc(a), apk.manifest.package.clone()),
            None => {
                let pkg = apk.manifest.package.clone();
                let launcher = apk
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
                (format!("L{};", cls.replace('.', "/")), pkg)
            }
        };
        let dexes = dexes_of_apk(apk)?;
        let mut eng = Engine::new(dexes, VmConfig::default());
        eng.set_resources(self.apk.clone().expect("apk"));
        eng.launch_app(&desc, &package, Vec::new())
            .map_err(vm_exit_to_rd)?;
        self.engine = Some(eng);
        self.activity = Some(desc.clone());
        Ok(ToolResult::json(&serde_json::json!({
            "launched": true,
            "activity": desc,
            "package": package,
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
        let ran = self
            .require_engine()?
            .advance_clock(ms)
            .map_err(vm_exit_to_rd)?;
        Ok(ToolResult::json(&serde_json::json!({
            "avancado_ms": ms,
            "runnables_executados": ran,
        })))
    }

    // ── observação ──────────────────────────────────────────────────────────

    fn tool_ui_tree(&mut self, format: &str) -> Result<ToolResult, RdError> {
        let tree: UiNode = self
            .require_engine()?
            .view_tree()
            .ok_or_else(|| agent_error("INVALID_FORMAT", "sem activity/window para dump", None))?;
        let package = self.package.clone().unwrap_or_default();
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
                std::fs::write(p, &png).map_err(|e| {
                    RdError::new("IO_ERROR", format!("escrevendo {p}: {e}"), MODULE_ID)
                })?;
                Ok(ToolResult::json(&serde_json::json!({
                    "path": p,
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
        let apk = self.require_apk()?;
        let m = &apk.manifest;
        Ok(ToolResult::json(&serde_json::json!({
            "package": m.package,
            "version_name": m.version_name,
            "version_code": m.version_code,
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

    fn tool_permissions_list(&mut self) -> Result<ToolResult, RdError> {
        let perms = self
            .perms
            .as_ref()
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro", None))?;
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
        let perms = self
            .perms
            .as_mut()
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro", None))?;
        let st = if grant {
            perms.grant(permission).map_err(rd_to_rd)?
        } else {
            perms.revoke(permission).map_err(rd_to_rd)?
        };
        Ok(ToolResult::json(&serde_json::json!({
            "permission": permission,
            "state": grant_state_str(st),
        })))
    }

    fn tool_permissions_audit(&mut self) -> Result<ToolResult, RdError> {
        let apk = self.require_apk()?;
        let perms = self
            .perms
            .as_ref()
            .ok_or_else(|| agent_error("NO_APP", "instale um APK primeiro", None))?;
        let findings: Vec<serde_json::Value> = perms
            .audit(&apk.manifest)
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
            .apk
            .as_ref()
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

fn tools_list_response() -> serde_json::Value {
    serde_json::json!({
        "tools": tool_defs().iter().map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.schema,
        })).collect::<Vec<_>>(),
    })
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
        "tools/list" => Some(Ok(jsonrpc_result(id, tools_list_response()))),
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
            Some(Ok(jsonrpc_result(
                id,
                serde_json::json!({ "content": content, "isError": result.is_error }),
            )))
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
        assert_eq!(tools.len(), 20, "catálogo M5 ~20 tools");
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
}
