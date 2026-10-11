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
