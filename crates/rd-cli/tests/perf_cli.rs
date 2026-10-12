//! Gate de performance do M7 (`rd perf`) no app trivial do fixture.
//!
//! Este é o gate de CI do M7 (PRF-06, Apêndice A2 · moto-e5). Ele roda o binário
//! real sobre o APK do `rd-testsupport`. O golden suite (APKs reais) NÃO roda
//! aqui: os APKs não estão no repositório. Os números são estimativas do host
//! escaladas pelo `cpu_single_thread_factor`, não medição num Moto E5 real.

use std::path::PathBuf;
use std::process::Command;

fn fixture_apk(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rd-perf-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("trivial.apk");
    std::fs::write(&path, rd_testsupport::trivial_app::trivial_app_apk()).expect("apk fixture");
    path
}

fn rd(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rd"))
        .args(args)
        .output()
        .expect("spawn rd")
}

#[test]
fn perf_trivial_app_meets_moto_e5_budgets() {
    let apk = fixture_apk("gate");
    let out = rd(&[
        "perf",
        apk.to_str().unwrap(),
        "--device",
        "moto-e5",
        "--runs",
        "3",
        "--json",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "rd perf falhou (exit {:?})\nstdout: {stdout}\nstderr: {stderr}",
        out.status.code()
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json do rd perf");
    assert_eq!(v["device"], "moto-e5");
    assert_eq!(v["pass"], true, "orçamento falhou: {v}");
    assert_eq!(v["cold"].as_array().unwrap().len(), 3);
    assert_eq!(v["warm"].as_array().unwrap().len(), 3);
    // heap do app não é medido e precisa estar declarado como tal
    assert_eq!(v["heap_budget_status"], "not_measured");
    assert_eq!(v["heap_budget_mb"], 256);
    let _ = std::fs::remove_dir_all(apk.parent().unwrap());
}

#[test]
fn perf_rejects_runs_out_of_range() {
    let apk = fixture_apk("runs");
    let out = rd(&["perf", apk.to_str().unwrap(), "--runs", "0"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("INVALID_FORMAT"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(apk.parent().unwrap());
}

#[test]
fn perf_unknown_device_is_an_error() {
    let apk = fixture_apk("device");
    let out = rd(&["perf", apk.to_str().unwrap(), "--device", "nao-existe"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("MISSING_ENTRY"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(apk.parent().unwrap());
}
