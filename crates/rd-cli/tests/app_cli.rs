//! E2E do CLI `rd app dump/shot` sobre APK sintético (fixtures do
//! rd-testsupport — mesmos do rd-vm). Cobre:
//! - issue #56: `rd app dump <apk>` na forma DOCUMENTADA (path único — antes
//!   exigia path duplicado e saía exit 64);
//! - issue #55: System.out do app NÃO contamina o stdout do comando (XML e
//!   --json parseáveis; o println aparece no stderr);
//! - issue #63: IO_ERROR de path de SAÍDA nasce no rd-cli (module_id rd-cli).

use std::process::{Command, Stdio};

use rd_testsupport::apkfix::{a, build_apk_sdk, build_axml, ArscFix, AxNode, AxVal};
use rd_testsupport::vm::{op10x, op21c, op35c, DexBuilder, ACC_PUBLIC};

/// const/32 vAA, lit32 (helper local — resid de 32 bits não cabe em op21c).
fn op31i_local(op: u8, reg: u8, lit: i32) -> Vec<u16> {
    let mut u = vec![op as u16 | ((reg as u16) << 8)];
    let lit = lit as u32;
    u.push((lit & 0xFFFF) as u16);
    u.push((lit >> 16) as u16);
    u
}

/// APK com Main.onCreate: System.out.println("hi do app") + setContentView.
/// Layout: LinearLayout(vertical) { TextView @string/hello }.
/// minSdk=26 (valor realista, acima do piso 9 do ADR 016): a fixture
/// anterior (build_apk, min_sdk=1) só rodava porque o `rd app`
/// ignorava o piso que o MCP já aplicava — e 1 segue abaixo do piso.
fn build_app_apk() -> Vec<u8> {
    build_app_apk_sdk(26)
}

/// Mesmo app, minSdk parametrizável (regressão BELOW_FLOOR do CLI).
fn build_app_apk_sdk(min_sdk: i32) -> Vec<u8> {
    let mut arsc = ArscFix::new();
    let layout = arsc.add_layout("activity_main");
    let s_hello = arsc.add_string("hello", "Olá do e2e");
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
    // classes de plataforma (mesma receita do app_xml do rd-vm)
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
    let s_hi = b.intern("hi do app") as u16;
    // FQCN pontuado no manifest (fiel ao aapt) ↔ descritor cheio no DEX —
    // o launcher_of compõe "Lcom/test/e2e/Main;" a partir do nome pontuado.
    let main = b.class("Lcom/test/e2e/Main;", "Landroid/app/Activity;");
    // onCreate(this, bundle): regs=5, ins=2 → this=v3, bundle=v4
    b.direct(
        main,
        "onCreate",
        "V",
        vec!["Landroid/os/Bundle;"],
        ACC_PUBLIC,
        Some(b.code(5, 2, 2, {
            let mut u = op21c(0x1A, 0, s_hi); // const-string v0, "hi do app"
            u.extend(op21c(0x60, 1, f_out)); // sget v1, System.out
            u.extend(op35c(0x6E, 2, ps_println, [1, 0, 0, 0, 0])); // v1.println(v0)
            u.extend(op31i_local(0x14, 2, layout as i32)); // const v2, layout
            u.extend(op35c(0x6E, 2, set_content, [3, 2, 0, 0, 0])); // v3.setContentView(v2)
            u.extend(op10x(0x0E)); // return-void
            u
        })),
    );

    build_apk_sdk(
        "com.test.e2e",
        "com.test.e2e.Main",
        arsc.build(),
        vec![
            ("res/layout/activity_main.xml", build_axml(&layout_xml)),
            ("classes.dex", b.finish()),
        ],
        min_sdk,
        26,
    )
}

fn write_apk(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("app.apk");
    std::fs::write(&path, build_app_apk()).expect("apk fixture");
    path
}

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_rd(args: &[&str], tmp: &std::path::Path) -> Out {
    let apk = write_apk(tmp);
    let apk_str = apk.to_string_lossy().to_string();
    let mut full: Vec<&str> = vec!["app"];
    full.extend(args.iter().map(|a| match *a {
        "<APK>" => apk_str.as_str(),
        other => other,
    }));
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn rd");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

/// Diretório temporário por-teste (sem deps externas).
fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rd-cli-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).expect("tmpdir");
    d
}

/// issue #56: a forma DOCUMENTADA (path único) funciona — exit 0.
/// issue #55: stdout é XML PURO (parseável; sem "hi do app") e o println do
/// app sai no stderr — como o logcat do Android real.
#[test]
fn app_dump_documented_form_is_clean_xml() {
    let tmp = tempfile_dir();
    let out = run_rd(&["dump", "<APK>"], &tmp);

    assert_eq!(
        out.code, 0,
        "issue #56: exit 0 na forma documentada. stderr: {}",
        out.stderr
    );
    assert!(
        out.stdout.starts_with("<?xml"),
        "stdout começa com XML (sem contaminação): {:?}",
        out.stdout.chars().take(80).collect::<String>()
    );
    assert!(
        out.stdout.trim_end().ends_with("</hierarchy>"),
        "XML completo: {}",
        &out.stdout[..out.stdout.len().min(400)]
    );
    assert!(
        !out.stdout.contains("hi do app"),
        "issue #55: System.out do app NÃO sai no stdout: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains("hi do app"),
        "issue #55: println do app vai para o stderr: {:?}",
        out.stderr
    );
    assert!(
        out.stdout.contains("resource-id=\"com.test:id/tv\""),
        "ids do arsc no dump: {}",
        &out.stdout[..out.stdout.len().min(600)]
    );
}

/// issue #55: `--json` sai parseável mesmo com app que imprime.
#[test]
fn app_dump_json_is_parseable() {
    let tmp = tempfile_dir();
    let out = run_rd(&["dump", "<APK>", "--json"], &tmp);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "issue #55: stdout JSON parseável: {e}; stdout: {}",
            out.stdout
        )
    });
    assert!(v["ui_tree_xml"].as_str().unwrap_or("").starts_with("<?xml"));
    assert!(out.stderr.contains("hi do app"));
}

/// issue #63: IO_ERROR de path de saída é do rd-cli (JSON {module_id: rd-cli}).
#[test]
fn shot_io_error_reports_rd_cli_module() {
    let tmp = tempfile_dir();
    let bad = tmp.join("sem-esse-dir/tela.png");
    let out = run_rd(
        &["shot", "<APK>", "--out", bad.to_string_lossy().as_ref()],
        &tmp,
    );
    assert_eq!(out.code, 1);
    // o contrato de erro do CLI sai no STDERR (stdout reservado ao artefato);
    // o stderr pode conter output do app (System.out → stderr, issue #55)
    // antes do JSON — extrair do primeiro '{'.
    let json_start = out
        .stderr
        .find('{')
        .unwrap_or_else(|| panic!("sem JSON de erro no stderr: {:?}", out.stderr));
    let v: serde_json::Value = serde_json::from_str(&out.stderr[json_start..])
        .unwrap_or_else(|e| panic!("erro em JSON: {e}; stderr: {}", out.stderr));
    assert_eq!(v["code"], "IO_ERROR");
    assert_eq!(
        v["module_id"], "rd-cli",
        "issue #63: module_id do módulo onde nasce (não rd-apk herdado)"
    );
}

/// issue #55: `rd app run --json` também sai parseável com app que imprime.
#[test]
fn app_run_json_is_parseable() {
    let tmp = tempfile_dir();
    let out = run_rd(&["run", "<APK>", "--json"], &tmp);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    serde_json::from_str::<serde_json::Value>(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "issue #55: run --json parseável: {e}; stdout: {}",
            out.stdout
        )
    });
    assert!(out.stderr.contains("hi do app"));
}

/// issue #77: `--help` é contrato público da CLI (consumido por agentes) —
/// notas internas de desenvolvimento (histórico de issue, nomes de variantes
/// do clap) não podem vazar para o texto de ajuda.
#[test]
fn help_text_has_no_internal_dev_notes() {
    let run = |args: &[&str]| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_rd"))
            .args(args)
            .output()
            .expect("spawn rd");
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    for (label, text) in [
        ("rd --help", run(&["--help"])),
        ("rd app --help", run(&["app", "--help"])),
    ] {
        assert!(
            !text.contains("issue #"),
            "{label} vaza nota interna de desenvolvimento:\n{text}"
        );
        assert!(
            !text.contains("Cmd::App"),
            "{label} vaza nome interno de variante:\n{text}"
        );
    }
    // a descrição curta e útil continua presente (não é só remoção)
    let root = run(&["--help"]);
    assert!(
        root.contains("Executa o app headless"),
        "descrição do subcomando `app` sumiu do --help:\n{root}"
    );
}

/// The app launcher exposes the VM's deterministic instruction budget, so
/// malformed or runaway app code can be bounded in integration probes.
#[test]
fn app_run_fuel_exhaustion_is_a_structured_vm_error() {
    let tmp = tempfile_dir();
    let out = run_rd(&["run", "<APK>", "--json", "--fuel", "1"], &tmp);
    assert_eq!(out.code, 1);
    let json_start = out
        .stderr
        .find('{')
        .unwrap_or_else(|| panic!("missing structured VM error: {:?}", out.stderr));
    let error: serde_json::Value = serde_json::from_str(&out.stderr[json_start..])
        .unwrap_or_else(|e| panic!("invalid JSON error: {e}; stderr: {}", out.stderr));
    assert_eq!(error["code"], "VM_FUEL");
    assert_eq!(error["module_id"], "rd-vm");
}

/// APK-Tester (PR-Agent, item 5b): `rd app run` EXECUTAVA abaixo do piso
/// silenciosamente, enquanto o MCP `install` recusava com BELOW_FLOOR. O
/// piso de API é contrato de execução em todas as bordas: o CLI recusa com
/// o mesmo código ANTES de qualquer código do app rodar. (O piso é 9
/// desde o ADR 016 — a fixture usa 8, o degrau imediatamente abaixo.)
#[test]
fn app_run_refuses_below_floor() {
    let tmp = tempfile_dir();
    let path = tmp.join("below-floor.apk");
    std::fs::write(&path, build_app_apk_sdk(8)).expect("apk fixture minSdk=8");
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(["app", "run", path.to_string_lossy().as_ref(), "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn rd");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert_eq!(
        out.status.code(),
        Some(1),
        "minSdk=8 recusa com exit 1. stdout: {stdout} stderr: {stderr}"
    );
    assert!(
        !stderr.contains("hi do app"),
        "nenhum código do app roda abaixo do piso: {stderr}"
    );
    let json_start = stderr
        .find('{')
        .unwrap_or_else(|| panic!("sem JSON de erro no stderr: {stderr:?}"));
    let error: serde_json::Value = serde_json::from_str(&stderr[json_start..])
        .unwrap_or_else(|e| panic!("erro em JSON: {e}; stderr: {stderr}"));
    assert_eq!(error["code"], "BELOW_FLOOR", "erro: {error}");
    assert_eq!(error["module_id"], "rd-cli", "erro: {error}");
}
