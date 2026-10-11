//! E2E do resumo de permissões do `rd inspect` (issue #76).
//!
//! O JSON já expunha `not_requested`, mas o resumo textual listava apenas
//! auto-granted/runtime-pending/special/unknown — as categorias não somavam o
//! total declarado e o usuário não conseguia explicar a diferença.

use std::process::Command;

use rd_testsupport::apkfix::{build_apk_sdk_perms, ArscFix};

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run_rd(args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(args)
        .output()
        .expect("spawn rd");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

/// Diretório temporário por-teste (sem deps externas).
fn tempfile_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rd-cli-inspect-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).expect("tmpdir");
    d
}

/// APK com UMA permissão em cada categoria do motor:
/// INTERNET (auto-granted), CAMERA (runtime-pending), SYSTEM_ALERT_WINDOW
/// (special), POST_NOTIFICATIONS (since_api 33 > target 26 → not requested) e
/// MYSTERY_PERM (unknown).
fn build_apk(tmp: &std::path::Path, target_sdk: i32) -> String {
    let arsc = ArscFix::new();
    let bytes = build_apk_sdk_perms(
        "com.example.perms",
        "com.example.perms.Main",
        arsc.build(),
        vec![],
        26,
        target_sdk,
        &[
            "android.permission.INTERNET",
            "android.permission.CAMERA",
            "android.permission.SYSTEM_ALERT_WINDOW",
            "android.permission.POST_NOTIFICATIONS",
            "com.example.perms.MYSTERY_PERM",
        ],
    );
    let path = tmp.join("perms.apk");
    std::fs::write(&path, bytes).expect("apk");
    path.to_string_lossy().to_string()
}

/// Lê uma categoria ("auto-granted: 1") da linha de resumo.
fn category(summary: &str, label: &str) -> usize {
    let part = summary
        .split('·')
        .find(|p| p.trim().starts_with(label))
        .unwrap_or_else(|| panic!("categoria {label:?} ausente no resumo: {summary:?}"));
    let digits: String = part.chars().filter(|c| c.is_ascii_digit()).collect();
    digits
        .parse()
        .unwrap_or_else(|e| panic!("{label:?} sem número em {part:?}: {e}"))
}

/// issue #76: as categorias do resumo somam o total declarado.
#[test]
fn inspect_summary_counts_sum_to_total() {
    let tmp = tempfile_dir("sum");
    let apk = build_apk(&tmp, 26);
    let out = run_rd(&["inspect", &apk]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let total: usize = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("permissions ("))
        .and_then(|rest| {
            rest.split(" declaradas")
                .next()
                .map(|n| n.parse().expect("total de declaradas"))
        })
        .expect("linha `permissions (N declaradas; …)`");

    let summary = out
        .stdout
        .lines()
        .find(|l| l.contains("auto-granted:"))
        .expect("linha de resumo das categorias");
    let auto = category(summary, "auto-granted");
    let runtime = category(summary, "runtime-pending");
    let special = category(summary, "special");
    let unknown = category(summary, "unknown");
    let not_requested = category(summary, "not requested");

    assert_eq!(total, 5, "as 5 permissões da fixture: {}", out.stdout);
    assert_eq!(
        auto + runtime + special + unknown + not_requested,
        total,
        "issue #76: a soma das categorias tem de fechar com o total (resumo: {summary:?})"
    );
    // e a distribuição é a esperada
    assert_eq!(
        (auto, runtime, special, unknown, not_requested),
        (1, 1, 1, 1, 1),
        "uma permissão por categoria: {summary:?}"
    );
    // o resumo explica o que é `not requested`
    assert!(
        out.stdout
            .contains("not requested = fora da janela do targetSdk"),
        "sem explicação da categoria: {}",
        out.stdout
    );
}

/// issue #76: com target 34 a permissão sai da janela de gating e volta a
/// `runtime-pending` — o `not requested` zera e a linha de explicação some.
#[test]
fn inspect_summary_omits_not_requested_when_zero() {
    let tmp = tempfile_dir("zero");
    let apk = build_apk(&tmp, 34);
    let out = run_rd(&["inspect", &apk]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);

    let summary = out
        .stdout
        .lines()
        .find(|l| l.contains("auto-granted:"))
        .expect("linha de resumo");
    assert_eq!(category(summary, "not requested"), 0, "{summary:?}");
    assert_eq!(category(summary, "runtime-pending"), 2, "{summary:?}");
    assert!(
        !out.stdout.contains("not requested = fora da janela"),
        "explicação só aparece quando há not requested: {}",
        out.stdout
    );
}

/// issue #76: `--json` não muda (sem regressão no contrato do agente).
#[test]
fn inspect_json_still_exposes_not_requested() {
    let tmp = tempfile_dir("json");
    let apk = build_apk(&tmp, 26);
    let out = run_rd(&["inspect", &apk, "--json"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: serde_json::Value =
        serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("JSON: {e}; {}", out.stdout));
    let states = v["permissions_state"]
        .as_object()
        .expect("permissions_state");
    assert_eq!(
        states["android.permission.POST_NOTIFICATIONS"]["state"],
        "not_requested"
    );
    assert_eq!(
        states["android.permission.INTERNET"]["state"],
        "auto_granted"
    );
}

/// issue #69: APK que declara a própria permissão (`<permission>`) — o
/// `protectionLevel` tem de sair DECODIFICADO no JSON (com o bruto em campo
/// separado) e o `rd perm audit` não pode acusar `UNKNOWN_PERMISSION`.
#[test]
fn declared_permissions_are_decoded_and_not_unknown() {
    use rd_testsupport::apkfix::build_apk_sdk_perms_declared;

    let tmp = tempfile_dir("declared");
    let arsc = ArscFix::new();
    let bytes = build_apk_sdk_perms_declared(
        "com.example.declared",
        "com.example.declared.Main",
        arsc.build(),
        26,
        34,
        &[
            "android.permission.INTERNET",
            "com.example.declared.permission.UPDATE_REPOS",
            "com.example.declared.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION",
        ],
        &[
            (
                "com.example.declared.permission.UPDATE_REPOS",
                "18", // signature|privileged
            ),
            (
                "com.example.declared.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION",
                "2", // signature
            ),
        ],
    );
    let apk = tmp.join("declared.apk");
    std::fs::write(&apk, bytes).expect("apk");
    let apk = apk.to_string_lossy().to_string();

    // ── inspect --json: protection_level decodificado + bruto preservado ──
    let out = run_rd(&["inspect", &apk, "--json"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let v: serde_json::Value =
        serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("JSON: {e}; {}", out.stdout));
    let declared = v["declared_permissions"].as_array().expect("array");
    assert_eq!(declared.len(), 2, "{declared:?}");
    let update = declared
        .iter()
        .find(|d| d["name"] == "com.example.declared.permission.UPDATE_REPOS")
        .expect("UPDATE_REPOS");
    assert_eq!(update["protection_level"], "signature|privileged");
    assert_eq!(update["protection_level_raw"], 18);
    let dyn_recv = declared
        .iter()
        .find(|d| d["name"] == "com.example.declared.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION")
        .expect("DYNAMIC_RECEIVER");
    assert_eq!(dyn_recv["protection_level"], "signature");
    assert_eq!(dyn_recv["protection_level_raw"], 2);

    // ── estado: permissão do próprio app é `app_defined`, não `unknown` ──
    let states = v["permissions_state"].as_object().expect("states");
    assert_eq!(
        states["com.example.declared.permission.UPDATE_REPOS"]["state"],
        "app_defined"
    );
    assert_eq!(
        states["com.example.declared.permission.UPDATE_REPOS"]["granted_by"],
        "self-declared"
    );

    // ── resumo textual: categorias somam o total (inclui app-defined) ──
    let text = run_rd(&["inspect", &apk]);
    let summary = text
        .stdout
        .lines()
        .find(|l| l.contains("auto-granted:"))
        .expect("resumo");
    let total: usize = text
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("permissions ("))
        .and_then(|r| r.split(" declaradas").next())
        .map(|n| n.parse().expect("total"))
        .expect("linha do total");
    let sum = category(summary, "auto-granted")
        + category(summary, "runtime-pending")
        + category(summary, "special")
        + category(summary, "unknown")
        + category(summary, "not requested")
        + category(summary, "app-defined");
    assert_eq!(sum, total, "soma fecha: {summary:?}");
    assert_eq!(category(summary, "app-defined"), 2, "{summary:?}");
    assert_eq!(category(summary, "unknown"), 0, "{summary:?}");

    // ── perm audit: nenhum UNKNOWN_PERMISSION para as duas do app ──
    let audit = run_rd(&["perm", "audit", &apk, "--json"]);
    assert_eq!(audit.code, 0, "stderr: {}", audit.stderr);
    let findings: serde_json::Value = serde_json::from_str(&audit.stdout)
        .unwrap_or_else(|e| panic!("JSON do audit: {e}; {}", audit.stdout));
    let arr = findings
        .as_array()
        .or_else(|| findings.get("findings").and_then(|f| f.as_array()))
        .expect("findings");
    let unknown: Vec<&str> = arr
        .iter()
        .filter(|f| f["code"] == "UNKNOWN_PERMISSION")
        .filter_map(|f| f["permission"].as_str())
        .collect();
    assert!(
        unknown.is_empty(),
        "issue #69: UNKNOWN_PERMISSION para permissão declarada pelo APK: {unknown:?}"
    );
    assert!(
        arr.iter().any(|f| f["code"] == "APP_DEFINED_PERMISSION"),
        "esperava INFO APP_DEFINED_PERMISSION: {arr:?}"
    );
}

/// issue #84: APK com `classes.dex` DUPLICADO no ZIP. Antes o `inspect`
/// contava as duas entradas como DEX distintos e não avisava nada — o resumo
/// descrevia um APK que não corresponde ao que é instalado.
#[test]
fn inspect_flags_duplicate_zip_entries_and_counts_dex_once() {
    let tmp = tempfile_dir("dupdex");
    let arsc = ArscFix::new();
    let bytes = build_apk_sdk_perms(
        "com.example.dupdex",
        "com.example.dupdex.Main",
        arsc.build(),
        vec![
            ("classes.dex", b"dex-primeira-ocorrencia".to_vec()),
            ("classes2.dex", b"dex-2".to_vec()),
            ("classes.dex", b"DEX-SEGUNDA-OCORRENCIA".to_vec()),
        ],
        26,
        26,
        &["android.permission.INTERNET"],
    );
    let path = tmp.join("dupdex.apk");
    std::fs::write(&path, &bytes).expect("apk");
    let apk = path.to_string_lossy().to_string();

    // ── saída humana: avisa e conta por nome único ──
    let text = run_rd(&["inspect", &apk]);
    assert_eq!(text.code, 0, "stderr: {}", text.stderr);
    let content_line = text
        .stdout
        .lines()
        .find(|l| l.starts_with("content"))
        .unwrap_or_else(|| panic!("linha de conteúdo ausente: {}", text.stdout));
    assert!(
        content_line.contains("5 entradas zip"),
        "5 entradas (o container é real): {content_line:?}"
    );
    assert!(
        content_line.contains("· 2 dex ·"),
        "issue #84: DEX conta por nome ÚNICO (antes vinha 3): {content_line:?}"
    );
    let warn = text
        .stdout
        .lines()
        .find(|l| l.starts_with("zip duplicates"))
        .unwrap_or_else(|| panic!("aviso de duplicata ausente: {}", text.stdout));
    assert!(
        warn.contains("classes.dex"),
        "nome repetido citado: {warn:?}"
    );
    assert!(
        warn.contains("1ª ocorrência"),
        "política explícita: {warn:?}"
    );

    // ── JSON: a ambiguidade é dado, não silêncio ──
    let json = run_rd(&["inspect", &apk, "--json"]);
    assert_eq!(json.code, 0, "stderr: {}", json.stderr);
    let v: serde_json::Value = serde_json::from_str(&json.stdout).expect("json do inspect");
    assert_eq!(
        v["zip_duplicates"][0], "classes.dex",
        "json expõe a duplicata: {v}"
    );
    assert_eq!(
        v["dex_files"].as_array().map(|a| a.len()),
        Some(2),
        "dex_files sem repetição: {v}"
    );

    // ── APK normal: sem aviso, sem campo novo (sem regressão) ──
    let tmp2 = tempfile_dir("cleandex");
    let clean = build_apk(&tmp2, 26);
    let text = run_rd(&["inspect", &clean]);
    assert_eq!(text.code, 0, "stderr: {}", text.stderr);
    assert!(
        !text.stdout.contains("zip duplicates"),
        "APK sem duplicata não deve avisar: {}",
        text.stdout
    );
    let json = run_rd(&["inspect", &clean, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&json.stdout).expect("json do inspect");
    assert!(
        v.get("zip_duplicates").is_none(),
        "sem duplicata ⇒ campo ausente: {v}"
    );
}
