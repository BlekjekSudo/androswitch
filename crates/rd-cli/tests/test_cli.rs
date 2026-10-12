//! E2E do executor de testes M6 (`rd test`): suíte JSON → JUnit XML → exit code,
//! com o binário real. O APK é o mesmo fixture do golden flow do MCP.

use std::path::Path;
use std::process::Command;

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

fn write_suite(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).expect("escreve suíte");
    p
}

fn suite_with_apk(expected_text: &str) -> String {
    format!(
        r#"{{
  "name": "e2e-m6",
  "scenarios": [
    {{"name": "abre e confere texto", "steps": [
        {{"name": "install", "arguments": {{"apk": "app.apk"}}}},
        {{"name": "launch", "arguments": {{}}}},
        {{"name": "wait", "arguments": {{"ms": 10}}}},
        {{"name": "assert_text", "arguments": {{"resource_id": "com.test:id/tv", "expected": "{expected_text}"}}}}
    ]}},
    {{"name": "lançar sem APK dá NO_APP", "steps": [
        {{"name": "launch", "arguments": {{}}, "expect": "error:NO_APP"}}
    ]}}
  ]
}}"#
    )
}

#[test]
fn rd_test_passes_and_writes_junit() {
    let tmp = tempfile_dir();
    // o APK fica na pasta da suíte: install.apk relativo deve resolver para lá
    // build_apk_path grava tmp/app.apk (a suíte o referencia por caminho relativo)
    build_apk_path(&tmp);
    let suite = write_suite(&tmp, "suite.json", &suite_with_apk("Olá via MCP"));
    let junit = tmp.join("out.xml");
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["test"])
        .arg(&suite)
        .arg("--junit")
        .arg(&junit)
        .output()
        .expect("roda rd test");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={stdout} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("PASS  abre e confere texto"), "{stdout}");
    assert!(stdout.contains("2 passou, 0 falhou"), "{stdout}");
    let xml = std::fs::read_to_string(&junit).expect("junit gravado");
    assert!(xml.contains("tests=\"2\" failures=\"0\""), "{xml}");
    assert!(!xml.contains("<failure"), "{xml}");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn rd_test_wrong_assertion_exits_1_with_failure_in_junit() {
    let tmp = tempfile_dir();
    // build_apk_path grava tmp/app.apk (a suíte o referencia por caminho relativo)
    build_apk_path(&tmp);
    let suite = write_suite(&tmp, "suite.json", &suite_with_apk("TEXTO QUE NÃO EXISTE"));
    let junit = tmp.join("out.xml");
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["test", "--json"])
        .arg(&suite)
        .arg("--junit")
        .arg(&junit)
        .output()
        .expect("roda rd test");
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("--json imprime JSON");
    assert_eq!(summary["failed"], 1, "{summary}");
    assert_eq!(summary["results"][0]["passed"], false, "{summary}");
    let msg = summary["results"][0]["failure"].as_str().unwrap_or("");
    assert!(msg.contains("passo 3 (assert_text)"), "{msg}");
    let xml = std::fs::read_to_string(&junit).expect("junit gravado");
    assert!(xml.contains("failures=\"1\""), "{xml}");
    assert!(
        xml.contains("<failure message=\"passo 3 (assert_text)"),
        "{xml}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn rd_test_invalid_suite_exits_64_before_running_anything() {
    let tmp = tempfile_dir();
    let suite = write_suite(
        &tmp,
        "bad.json",
        r#"{"name": "x", "scenarios": [{"name": "c", "steps": [
            {"name": "install", "arguments": {"apk": "app.apk"}},
            {"name": "tap", "arguments": 5}
        ]}]}"#,
    );
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["test"])
        .arg(&suite)
        .output()
        .expect("roda rd test");
    assert_eq!(out.status.code(), Some(64));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("INVALID_SUITE"), "{err}");
    // nada foi instalado: o passo 0 (install) não executou
    assert!(
        out.stdout.is_empty(),
        "suíte inválida não deve imprimir resumo"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn rd_test_missing_file_exits_64() {
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["test", "/nao/existe/suite.json"])
        .output()
        .expect("roda rd test");
    assert_eq!(out.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&out.stderr).contains("IO_ERROR"));
}
