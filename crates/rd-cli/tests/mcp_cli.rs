//! E2E do servidor MCP (`rd mcp`) — JSON-RPC 2.0 stdio de ponta a ponta com
//! o binário real. DoD M5: "LLM instala, navega e assera estado só com as
//! tools" — aqui o "LLM" é o driver de teste falando o protocolo na fio.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use rd_testsupport::apkfix::{a, build_axml, ArscFix, AxNode, AxVal};
use rd_testsupport::vm::{op10x, op21c, op35c, DexBuilder, ACC_PUBLIC};

fn op31i_local(op: u8, reg: u8, lit: u32) -> Vec<u16> {
    let mut u = vec![op as u16 | ((reg as u16) << 8)];
    u.push((lit & 0xFFFF) as u16);
    u.push((lit >> 16) as u16);
    u
}

/// APK fixture acima do piso (min 26): println + layout + TextView id/tv.
fn build_apk_path(tmp: &std::path::Path) -> String {
    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("activity_main");
    let s_hello = arsc.add_string("hello", "Olá via MCP");
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
    let s_hi = b.intern("log do app") as u16;
    let main = b.class("Lcom/test/mcp/Main;", "Landroid/app/Activity;");
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            let mut u = op21c(0x1A, 0, s_hi);
            u.extend(op21c(0x60, 1, f_out));
            u.extend(op35c(0x6E, 2, ps_println, [1, 0, 0, 0, 0]));
            u.extend(op31i_local(0x14, 2, layout));
            u.extend(op35c(0x6E, 2, set_content, [3, 2, 0, 0, 0]));
            u.extend(op10x(0x0E));
            u
        })),
    );
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
    let path = tmp.join("app.apk");
    std::fs::write(&path, apk).expect("apk fixture");
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

/// DoD M5 ponta a ponta: initialize → tools/list (20) → install → launch →
/// navegação (tap/wait) → asserções (assert_view/assert_text/invoke) →
/// observação (ui_tree/screenshot/logs) — SÓ via tools.
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
    assert_eq!(r["result"]["tools"].as_array().map(|a| a.len()), Some(20));

    // install → launch
    let r = mcp.call_tool(10, "install", serde_json::json!({"apk": apk_path}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("com.test.mcp"), "install: {txt}");
    assert_eq!(r["result"]["isError"], false);
    let r = mcp.call_tool(11, "launch", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("Lcom/test/mcp/Main;"), "launch: {txt}");

    // navegação
    let r = mcp.call_tool(12, "tap", serde_json::json!({"x": 8, "y": 8}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"listener_acionado\": false"), "tap: {txt}");
    let r = mcp.call_tool(13, "wait", serde_json::json!({"ms": 50}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"runnables_executados\": 0"), "wait: {txt}");

    // asserções (o coração do DoD)
    let r = mcp.call_tool(
        14,
        "assert_view",
        serde_json::json!({"resource_id": "com.test:id/tv"}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"found\": true"), "assert_view: {txt}");
    let r = mcp.call_tool(
        15,
        "assert_text",
        serde_json::json!({"resource_id": "com.test:id/tv", "expected": "Olá via MCP"}),
    );
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("\"ok\": true"), "assert_text: {txt}");

    // observação: ui_tree xml parseável + logs do app + screenshot image
    let r = mcp.call_tool(16, "ui_tree", serde_json::json!({"format": "xml"}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.starts_with("<?xml"), "ui_tree: {txt}");
    let r = mcp.call_tool(17, "logs", serde_json::json!({}));
    let txt = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("log do app"), "logs: {txt}");
    let r = mcp.call_tool(18, "screenshot", serde_json::json!({}));
    let img = &r["result"]["content"][0];
    assert_eq!(img["type"], "image", "screenshot vira conteúdo image");
    assert_eq!(img["mimeType"], "image/png");
    assert!(img["data"].as_str().unwrap().len() > 1000);

    // erro tipado dentro do resultado (convenção MCP)
    let r = mcp.call_tool(19, "assert_view", serde_json::json!({}));
    assert_eq!(r["result"]["isError"], true);

    mcp.kill();
}
