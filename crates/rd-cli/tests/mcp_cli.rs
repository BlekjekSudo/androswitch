//! E2E do servidor MCP (`rd mcp`) — JSON-RPC 2.0 stdio de ponta a ponta com
//! o binário real. DoD M5: "LLM instala, navega e assera estado só com as
//! tools" — aqui o "LLM" é o driver de teste falando o protocolo na fio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// APK fixture do app trivial — builder compartilhado em `rd-testsupport`.
fn build_apk_path(tmp: &std::path::Path) -> String {
    let path = tmp.join("app.apk");
    std::fs::write(&path, rd_testsupport::trivial_app::trivial_app_apk()).expect("apk fixture");
    path.to_string_lossy().to_string()
}

struct McpProc {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl McpProc {
    fn spawn() -> McpProc {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rd"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rd mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        McpProc {
            child,
            stdin,
            reader: BufReader::new(stdout),
        }
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").expect("escreve no stdin do rd mcp");
    }

    /// Lê UMA resposta (notifications não respondem).
    fn read(&mut self) -> serde_json::Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("lê resposta");
        serde_json::from_str(line.trim()).expect("JSON-RPC válido")
    }

    fn call_tool(&mut self, id: u32, name: &str, args: serde_json::Value) -> serde_json::Value {
        self.send(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\"arguments\":{args}}}}}"
        ));
        self.read()
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Diretório temporário por-teste.
fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rd-mcp-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).expect("tmpdir");
    d
}

/// DoD M5 ponta a ponta: initialize → tools/list (32) → install → launch →
/// navegação (tap/swipe/wait/wait_for) → asserções → observação → pacote →
/// batch → runtime_health — SÓ via tools, com sessionId/package no envelope.
#[test]
fn mcp_stdio_full_agent_flow() {
    let tmp = tempfile_dir();
    let apk_path = build_apk_path(&tmp);
    let mut mcp = McpProc::spawn();

    // initialize
    mcp.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let r = mcp.read();
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(r["result"]["serverInfo"]["name"], "rustydroid-agent");
    mcp.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // catálogo
    mcp.send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    let r = mcp.read();
    assert_eq!(r["result"]["tools"].as_array().map(|a| a.len()), Some(39));

    // install → launch (envelope carrega sessionId/package — contrato M5)
    let r = mcp.call_tool(10, "install", serde_json::json!({"apk": apk_path}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("com.test.mcp"), "install: {txt}");
    assert_eq!(r["result"]["isError"], false);
    let session_id = r["result"]["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_string();
    assert!(session_id.starts_with("sess-"), "{session_id}");
    assert_eq!(r["result"]["package"], "com.test.mcp");
    let r = mcp.call_tool(11, "launch", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("Lcom/test/mcp/Main;"), "launch: {txt}");

    // navegação: tap + swipe (relatório de views atravessadas) + wait
    let r = mcp.call_tool(12, "tap", serde_json::json!({"x": 8, "y": 8}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"listener_acionado\": false"), "tap: {txt}");
    let r = mcp.call_tool(
        13,
        "swipe",
        serde_json::json!({"x1": 0, "y1": 0, "x2": 720, "y2": 100, "ms": 50}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"crossed\""), "swipe relata views: {txt}");
    let r = mcp.call_tool(14, "wait", serde_json::json!({"ms": 50}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"runnables_executados\": 0"), "wait: {txt}");

    // espera condicional: a view já existe → found imediato
    let r = mcp.call_tool(
        15,
        "wait_for",
        serde_json::json!({"resource_id": "com.test:id/tv", "timeout_ms": 500}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"found\": true"), "wait_for: {txt}");

    // asserções (o coração do DoD)
    let r = mcp.call_tool(
        16,
        "assert_view",
        serde_json::json!({"resource_id": "com.test:id/tv"}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"found\": true"), "assert_view: {txt}");
    let r = mcp.call_tool(
        17,
        "assert_text",
        serde_json::json!({"resource_id": "com.test:id/tv", "expected": "Olá via MCP"}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"ok\": true"), "assert_text: {txt}");

    // observação: ui_tree xml parseável + logs do app + screenshot image
    let r = mcp.call_tool(18, "ui_tree", serde_json::json!({"format": "xml"}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.starts_with("<?xml"), "ui_tree: {txt}");
    let r = mcp.call_tool(19, "logs", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("log do app"), "logs: {txt}");
    let r = mcp.call_tool(20, "screenshot", serde_json::json!({}));
    let img = &r["result"]["content"][0];
    assert_eq!(img["type"], "image", "screenshot vira conteúdo image");
    assert_eq!(img["mimeType"], "image/png");
    assert!(img["data"].as_str().unwrap().len() > 1000);

    // registro de packages + saúde + lacunas de compat. (catálogo honesto)
    let r = mcp.call_tool(21, "package_list", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("com.test.mcp"), "package_list: {txt}");
    let r = mcp.call_tool(22, "runtime_health", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"running\": true"), "health: {txt}");
    assert!(txt.contains("\"looper_pending\""), "health fila: {txt}");
    let r = mcp.call_tool(23, "compat_gaps", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("http-transport"), "gaps: {txt}");

    // batch: 3 passos em ordem com resultados por passo + sessionId
    let r = mcp.call_tool(
        24,
        "batch",
        serde_json::json!({"calls": [
            {"name": "wait", "arguments": {"ms": 10}},
            {"name": "assert_view", "arguments": {"resource_id": "com.test:id/tv"}},
            {"name": "logs", "arguments": {}}
        ]}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"executed\": 3"), "batch: {txt}");
    assert!(txt.contains(session_id.as_str()), "batch herda sessionId");

    // erro tipado dentro do resultado (convenção MCP)
    let r = mcp.call_tool(25, "assert_view", serde_json::json!({}));
    assert_eq!(r["result"]["isError"], true);
    // sessionId presente mesmo no erro (contrato do enunciado)
    assert_eq!(r["result"]["sessionId"], session_id);

    mcp.kill();
}

/// DoD M5: o conjunto publicado corresponde ao `--describe-tools` e à doc
/// gerada — ambos saem do MESMO tool_defs() e casam com tools/list.
#[test]
fn describe_tools_matches_server_catalog() {
    use std::process::Command;
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["mcp", "--describe-tools"])
        .output()
        .expect("rd mcp --describe-tools");
    assert!(out.status.success());
    let described: serde_json::Value = serde_json::from_slice(&out.stdout).expect("catálogo JSON");
    let tools = described["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 39, "mesmo catálogo do servidor");
    for t in tools {
        assert!(t["name"].as_str().is_some());
        assert!(t["inputSchema"].is_object(), "schema por tool");
    }
    // e o servidor responde tools/list com O MESMO conjunto (nomes)
    let mut mcp = McpProc::spawn();
    mcp.send(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    let r = mcp.read();
    mcp.kill();
    let served = r["result"]["tools"].as_array().expect("tools/list");
    let names_described: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    let names_served: Vec<&str> = served.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(
        names_described, names_served,
        "--describe-tools == tools/list"
    );
}

/// issue #74: o README anunciava "~40 structured tools" e "MCP (rmcp)". O
/// catálogo real tem 39 tools (M9) e o crate NÃO depende de rmcp — a stack é
/// JSON-RPC próprio, por decisão do ADR 001 (MSRV). Este teste trava o número
/// publicado no README contra `rd mcp --describe-tools`.
#[test]
fn readme_tool_count_matches_catalog() {
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["mcp", "--describe-tools"])
        .output()
        .expect("rd mcp --describe-tools");
    assert!(out.status.success());
    let described: serde_json::Value = serde_json::from_slice(&out.stdout).expect("catálogo JSON");
    let live = described["tools"].as_array().expect("tools").len();

    // README (raiz do repositório, relativo ao diretório do crate)
    let readme = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("README.md"),
    )
    .expect("README.md");
    let expected = format!("{live} structured tools");
    assert!(
        readme.contains(&expected),
        "README não anuncia {expected:?} — catálogo mudou ou a doc ficou defasada (issue #74)"
    );
    assert!(
        !readme.contains("~40") && !readme.contains("rmcp"),
        "README voltou a citar '~40' ferramentas ou 'rmcp' (issue #74)"
    );

    // o crate também não pode descrever a stack errada
    let cargo = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("rd-agent")
            .join("Cargo.toml"),
    )
    .expect("crates/rd-agent/Cargo.toml");
    assert!(
        !cargo.contains("rmcp"),
        "rd-agent não depende de rmcp (ADR 001) — corrigir a description (issue #74)"
    );
}
