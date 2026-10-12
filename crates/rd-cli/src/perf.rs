//! `rd perf` — gate de performance do M7 (PRF-06, Apêndice A2 · piso moto-e5).
//!
//! Mede, no HOST, o caminho `launch → primeiro frame` (frame = render do view
//! tree, via a tool `screenshot`) em dois regimes:
//! - **cold**: `AgentSession` nova por execução; o APK é registrado antes (fora
//!   do cronômetro) e o cronômetro cobre só o launch + primeiro frame;
//! - **warm**: relançamento na MESMA sessão, que já tem o APK registrado.
//!
//! Tempo emulado = tempo do host / `cpu_single_thread_factor` do perfil
//! (moto-e5: 0.25 ⇒ ×4). É uma ESTIMATIVA: o host não é um Moto E5, e o fator
//! não modela cache, IO, GPU nem o quota de 1 core (`cpu_quota_cores`).
//!
//! Orçamentos avaliados:
//! - startup cold emulado (pior das execuções) `< 2000 ms` (ROADMAP M7 DoD);
//! - RSS de pico do processo `≤ ram_process_cap_mb` (512 MB, moto-e5);
//! - heap do app `ram_app_heap_mb` (256 MB): NÃO medido — reportado como tal.
//!
//! Medição de RSS e CPU vem de `/proc` (Linux). Fora do Linux o orçamento de
//! RSS falha fechado (`actual: null`), porque não foi medido.

use std::fs;
use std::path::Path;
use std::time::Instant;

use rd_agent::mcp::{AgentSession, ToolContent, ToolResult};
use rd_apk::RdError;
use rd_framework::DeviceProfile;
use serde::Serialize;

/// Orçamento de startup (ROADMAP M7 DoD: "startup < 2 s no trivial app").
pub const STARTUP_BUDGET_EMULATED_MS: f64 = 2000.0;

/// Mínimo de execuções aceito por `--runs`.
pub const MIN_RUNS: u32 = 1;
/// Máximo de execuções aceito por `--runs` (teto contra gate infinito).
pub const MAX_RUNS: u32 = 50;

#[derive(Debug, Clone, Serialize)]
pub struct Run {
    /// Tempo medido no host (ms, `Instant`).
    pub host_ms: f64,
    /// `host_ms / cpu_single_thread_factor` (ms, estimativa para o device).
    pub emulated_ms: f64,
    /// CPU consumida pelo processo durante a execução (ms). `None` sem /proc.
    pub cpu_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Budget {
    pub name: &'static str,
    pub limit: f64,
    pub unit: &'static str,
    /// `None` = não medido.
    pub actual: Option<f64>,
    pub pass: bool,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerfReport {
    pub device: String,
    pub cpu_single_thread_factor: f64,
    pub cpu_quota_cores: f64,
    pub apk: String,
    pub runs: u32,
    pub cold: Vec<Run>,
    pub warm: Vec<Run>,
    pub cold_p50_emulated_ms: f64,
    pub cold_max_emulated_ms: f64,
    pub warm_p50_emulated_ms: f64,
    /// VmHWM do processo (MB) — pico acumulado de todas as execuções.
    pub peak_rss_mb: Option<f64>,
    pub budgets: Vec<Budget>,
    /// Heap do app (`ram_app_heap_mb`) — ainda sem medição.
    pub heap_budget_mb: u32,
    pub heap_budget_status: &'static str,
    pub pass: bool,
}

/// Tempo emulado: o device é `factor` vezes mais lento que o host.
pub fn emulated_ms(host_ms: f64, factor: f64) -> f64 {
    host_ms / factor
}

/// Startup dentro do orçamento? Estrito (`<`), como o DoD escreve.
pub fn startup_within_budget(actual_max_emulated_ms: f64) -> bool {
    actual_max_emulated_ms < STARTUP_BUDGET_EMULATED_MS
}

/// RSS dentro do teto? Não medido = falha (fail-closed).
pub fn rss_within_budget(actual_mb: Option<f64>, limit_mb: u32) -> bool {
    matches!(actual_mb, Some(v) if v <= f64::from(limit_mb))
}

/// Mediana (p50) de uma lista não vazia de valores.
pub fn p50(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// VmHWM (pico de RSS) do processo, em MB. `None` fora do Linux.
pub fn peak_rss_mb() -> Option<f64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

/// CPU total do processo (todas as threads), em ns. Usa `schedstat` por
/// thread: já está em ns, sem assumir USER_HZ. `None` sem /proc ou sem
/// CONFIG_SCHEDSTATS.
pub fn process_cpu_ns() -> Option<u64> {
    let mut total: u64 = 0;
    for entry in fs::read_dir("/proc/self/task").ok()? {
        let path = entry.ok()?.path().join("schedstat");
        let text = fs::read_to_string(path).ok()?;
        let on_cpu: u64 = text.split_whitespace().next()?.parse().ok()?;
        total = total.checked_add(on_cpu)?;
    }
    Some(total)
}

fn tool_text(r: &ToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| match c {
            ToolContent::Text(t) => Some(t.as_str()),
            ToolContent::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Chama uma tool e converte `is_error` em `RdError` (o gate não ignora falha).
fn call_ok(s: &mut AgentSession, name: &str, args: &serde_json::Value) -> Result<(), RdError> {
    let r = s.call_tool(name, args);
    if r.is_error {
        return Err(RdError::new(
            "PERF_FAILED",
            format!("tool {name:?} falhou: {}", tool_text(&r)),
            "rd-cli",
        ));
    }
    Ok(())
}

/// Um ciclo cronometrado: launch + primeiro frame (screenshot do view tree).
fn timed_launch_frame(s: &mut AgentSession, device_factor: f64) -> Result<Run, RdError> {
    let cpu0 = process_cpu_ns();
    let t0 = Instant::now();
    call_ok(s, "launch", &serde_json::json!({}))?;
    call_ok(s, "screenshot", &serde_json::json!({}))?;
    let host_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let cpu1 = process_cpu_ns();
    let cpu_ms = match (cpu0, cpu1) {
        (Some(a), Some(b)) => Some(b.saturating_sub(a) as f64 / 1_000_000.0),
        _ => None,
    };
    Ok(Run {
        host_ms,
        emulated_ms: emulated_ms(host_ms, device_factor),
        cpu_ms,
    })
}

/// Executa o gate. Retorna o relatório (sem imprimir).
pub fn measure(apk: &Path, device: &DeviceProfile, runs: u32) -> Result<PerfReport, RdError> {
    let factor = f64::from(device.cpu_single_thread_factor);
    if !(factor.is_finite() && factor > 0.0) {
        return Err(RdError::new(
            "INVALID_FORMAT",
            format!(
                "cpu_single_thread_factor do perfil {:?} inválido: {factor}",
                device.id
            ),
            "rd-cli",
        ));
    }
    let apk_str = apk.to_string_lossy().to_string();
    let install_args = serde_json::json!({ "apk": apk_str });

    let mut cold = Vec::with_capacity(runs as usize);
    let mut warm = Vec::with_capacity(runs as usize);
    for i in 0..runs {
        // cold: sessão nova; o APK é registrado fora do cronômetro.
        let mut s = AgentSession::new();
        call_ok(&mut s, "install", &install_args)?;
        cold.push(timed_launch_frame(&mut s, factor)?);
        // warm: na primeira sessão, relança `runs` vezes com o APK já registrado.
        if i == 0 {
            for _ in 0..runs {
                warm.push(timed_launch_frame(&mut s, factor)?);
            }
        }
    }

    let cold_emu: Vec<f64> = cold.iter().map(|r| r.emulated_ms).collect();
    let warm_emu: Vec<f64> = warm.iter().map(|r| r.emulated_ms).collect();
    let cold_max = cold_emu.iter().copied().fold(f64::MIN, f64::max);
    let peak = peak_rss_mb();

    let startup_ok = startup_within_budget(cold_max);
    let rss_ok = rss_within_budget(peak, device.ram_process_cap_mb);
    let budgets = vec![
        Budget {
            name: "startup_cold_emulated_max",
            limit: STARTUP_BUDGET_EMULATED_MS,
            unit: "ms",
            actual: Some(round1(cold_max)),
            pass: startup_ok,
            note: "pior cold (launch + primeiro frame) × 1/fator; estimativa",
        },
        Budget {
            name: "rss_peak",
            limit: f64::from(device.ram_process_cap_mb),
            unit: "MB",
            actual: peak.map(round1),
            pass: rss_ok,
            note: if peak.is_some() {
                "VmHWM do processo (pico acumulado)"
            } else {
                "não medido: requer Linux /proc (falha fechada)"
            },
        },
    ];
    let pass = budgets.iter().all(|b| b.pass);

    Ok(PerfReport {
        device: device.id.clone(),
        cpu_single_thread_factor: factor,
        cpu_quota_cores: f64::from(device.cpu_quota_cores),
        apk: apk_str,
        runs,
        cold,
        warm,
        cold_p50_emulated_ms: round1(p50(&cold_emu)),
        cold_max_emulated_ms: round1(cold_max),
        warm_p50_emulated_ms: round1(p50(&warm_emu)),
        peak_rss_mb: peak.map(round1),
        budgets,
        heap_budget_mb: device.ram_app_heap_mb,
        heap_budget_status: "not_measured",
        pass,
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn print_human(r: &PerfReport) {
    println!(
        "rd perf — device {} (fator {}, quota {} core(s)) — {} execução(ões)",
        r.device, r.cpu_single_thread_factor, r.cpu_quota_cores, r.runs
    );
    println!("apk: {}", r.apk);
    println!(
        "cold (emulado): p50 {} ms · max {} ms",
        r.cold_p50_emulated_ms, r.cold_max_emulated_ms
    );
    println!("warm (emulado): p50 {} ms", r.warm_p50_emulated_ms);
    match r.peak_rss_mb {
        Some(v) => println!("RSS pico do processo: {v} MB"),
        None => println!("RSS pico do processo: não medido"),
    }
    for b in &r.budgets {
        let actual = match b.actual {
            Some(v) => format!("{v} {}", b.unit),
            None => "não medido".to_string(),
        };
        println!(
            "[{}] {}: {} (limite {} {}) — {}",
            if b.pass { "OK" } else { "FALHOU" },
            b.name,
            actual,
            b.limit,
            b.unit,
            b.note
        );
    }
    println!(
        "[NÃO MEDIDO] heap do app: limite {} MB (ram_app_heap_mb)",
        r.heap_budget_mb
    );
    println!("resultado: {}", if r.pass { "PASSOU" } else { "FALHOU" });
}

/// Ponto de entrada do subcomando. `Ok(true)` = passou; `Ok(false)` = orçamento
/// falhou (exit 1). Erros de execução voltam como `Err`.
pub fn cmd_perf(
    apk: &Path,
    device: &DeviceProfile,
    runs: u32,
    json: bool,
) -> Result<bool, RdError> {
    if !(MIN_RUNS..=MAX_RUNS).contains(&runs) {
        return Err(RdError::new(
            "INVALID_FORMAT",
            format!("--runs deve estar entre {MIN_RUNS} e {MAX_RUNS}, veio {runs}"),
            "rd-cli",
        ));
    }
    let report = measure(apk, device, runs)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        print_human(&report);
    }
    Ok(report.pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulated_scales_by_factor() {
        // factor 0.25 ⇒ device 4× mais lento
        assert!((emulated_ms(500.0, 0.25) - 2000.0).abs() < 1e-9);
        assert!((emulated_ms(100.0, 1.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn startup_budget_is_strict() {
        assert!(startup_within_budget(1999.9));
        assert!(!startup_within_budget(2000.0));
        assert!(!startup_within_budget(2500.0));
    }

    #[test]
    fn rss_budget_inclusive_and_fail_closed() {
        assert!(rss_within_budget(Some(512.0), 512));
        assert!(!rss_within_budget(Some(512.1), 512));
        assert!(!rss_within_budget(None, 512));
    }

    #[test]
    fn p50_handles_odd_even_and_empty() {
        assert_eq!(p50(&[]), 0.0);
        assert_eq!(p50(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(p50(&[4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn runs_bounds_are_checked_before_work() {
        let dt = rd_framework::fd_devices::DeviceTable::embedded().unwrap();
        let dev = dt.get("moto-e5").unwrap();
        let err = cmd_perf(Path::new("/nonexistent.apk"), dev, 0, false).unwrap_err();
        assert_eq!(err.code, "INVALID_FORMAT");
        let err = cmd_perf(Path::new("/nonexistent.apk"), dev, MAX_RUNS + 1, false).unwrap_err();
        assert_eq!(err.code, "INVALID_FORMAT");
    }
}
