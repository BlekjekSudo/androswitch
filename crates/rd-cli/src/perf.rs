//! `rd perf` — gate de performance do M7 (PRF-06, Apêndice A2 · piso moto-e5).
//!
//! Mede, no HOST, o caminho `launch → primeiro frame` (frame = render do view
//! tree, via a tool `screenshot`) em dois regimes:
//! - **cold**: `AgentSession` nova por execução; o APK é registrado antes (fora
//!   do cronômetro) e o cronômetro cobre só o launch + primeiro frame;
//! - **warm**: relançamento na MESMA sessão, que já tem o APK registrado —
//!   reutiliza o processo (Engine) em execução, sem re-parse do DEX
//!   (warm start do Android; resultado da tool traz `warm: true`).
//!
//! Tempo emulado = tempo do host / `cpu_single_thread_factor` do perfil
//! (moto-e5: 0.25 ⇒ ×4). É uma ESTIMATIVA: o host não é um Moto E5, e o fator
//! não modela cache, IO, GPU nem o quota de 1 core (`cpu_quota_cores`).
//!
//! Orçamentos avaliados:
//! - startup cold emulado (pior das execuções) `< 2000 ms` (ROADMAP M7 DoD);
//! - RSS de pico do processo `≤ ram_process_cap_mb` (1024 MB, moto-e5 — ADR 015);
//! - heap do app `≤ ram_app_heap_mb` (256 MB): marca d'água da arena da VM
//!   (`runtime_health.heap_peak_bytes`), lida fora do cronômetro.
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
/// ADR 008 Stage 5: lowered from 2000 ms to 1500 ms after the Stage 1b
/// hot-path work (parallel DEX mount, slicing-by-8 CRC32, classpath
/// resolution caches, measure short-circuit, declared-methods-only
/// annotation lookup) brought the M7 gate APK to ~1330 ms emulated p50.
pub const STARTUP_BUDGET_EMULATED_MS: f64 = 1500.0;

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
    /// Marca d'água do heap do app (bytes) após o primeiro frame, lida do
    /// `runtime_health` FORA do cronômetro. `None` se não reportado.
    pub heap_peak_bytes: Option<u64>,
    /// ADR 008 phase telemetry: `install` time (cold runs only; install is
    /// outside the launch timer by design, but reported for diagnosis).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_ms: Option<f64>,
    /// ADR 008 phase telemetry: host ms inside the `launch` tool call.
    pub launch_ms: f64,
    /// ADR 008 phase telemetry: host ms inside the first `screenshot` call
    /// (raster + PNG encode as seen by the session).
    pub screenshot_ms: f64,
    /// ADR 008: VmHWM (MB) sampled right after this run, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rss_after_mb: Option<f64>,
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
    /// Pico do heap do app (MB) — maior marca d'água entre as execuções.
    pub app_heap_peak_mb: Option<f64>,
    pub budgets: Vec<Budget>,
    /// Teto do heap do app (`ram_app_heap_mb`) — medido desde o M7 passo 2.
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

/// Heap do app dentro do teto (M7, `ram_app_heap_mb`): `None` (sem medição)
/// falha fechado, como o RSS — orçamento sem número não passa.
pub fn heap_within_budget(actual_mb: Option<f64>, limit_mb: u32) -> bool {
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
    call_json(s, name, args)?;
    Ok(())
}

/// Como `call_ok`, mas devolve o conteúdo texto do resultado parseado como
/// JSON (`Value::Null` se não for JSON — o chamador decide o que fazer).
fn call_json(
    s: &mut AgentSession,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, RdError> {
    // Issue #317: `rd perf` também isola — panic de runtime vira
    // PERF_FAILED tipado neste passo, não crash do processo.
    let r = s.call_tool_isolated(name, args);
    if r.is_error {
        return Err(RdError::new(
            "PERF_FAILED",
            format!("tool {name:?} falhou: {}", tool_text(&r)),
            "rd-cli",
        ));
    }
    Ok(serde_json::from_str(&tool_text(&r)).unwrap_or(serde_json::Value::Null))
}

/// Um ciclo cronometrado: launch + primeiro frame (screenshot do view tree).
/// A leitura do heap (runtime_health) é feita DEPOIS de parar o cronômetro.
fn timed_launch_frame(s: &mut AgentSession, device_factor: f64) -> Result<Run, RdError> {
    let cpu0 = process_cpu_ns();
    let t0 = Instant::now();
    let t_launch = Instant::now();
    call_ok(s, "launch", &serde_json::json!({}))?;
    let launch_ms = t_launch.elapsed().as_secs_f64() * 1000.0;
    let t_shot = Instant::now();
    call_ok(s, "screenshot", &serde_json::json!({}))?;
    let screenshot_ms = t_shot.elapsed().as_secs_f64() * 1000.0;
    let host_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let cpu1 = process_cpu_ns();
    let cpu_ms = match (cpu0, cpu1) {
        (Some(a), Some(b)) => Some(b.saturating_sub(a) as f64 / 1_000_000.0),
        _ => None,
    };
    // M7: marca d'água do heap do app — fora da janela cronometrada.
    let health = call_json(s, "runtime_health", &serde_json::json!({}))?;
    let heap_peak_bytes = health.get("heap_peak_bytes").and_then(|v| v.as_u64());
    Ok(Run {
        host_ms,
        emulated_ms: emulated_ms(host_ms, device_factor),
        cpu_ms,
        heap_peak_bytes,
        install_ms: None,
        launch_ms: round1(launch_ms),
        screenshot_ms: round1(screenshot_ms),
        rss_after_mb: peak_rss_mb().map(round1),
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
        let t_install = Instant::now();
        call_ok(&mut s, "install", &install_args)?;
        let install_ms = t_install.elapsed().as_secs_f64() * 1000.0;
        let mut run = timed_launch_frame(&mut s, factor)?;
        run.install_ms = Some(round1(install_ms));
        cold.push(run);
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
    // M7: marca d'água do heap do app = maior pico entre todas as execuções.
    let app_heap_peak_mb = heap_peak_mb_max(&cold, &warm);

    let startup_ok = startup_within_budget(cold_max);
    let rss_ok = rss_within_budget(peak, device.ram_process_cap_mb);
    let heap_ok = heap_within_budget(app_heap_peak_mb, device.ram_app_heap_mb);
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
        Budget {
            name: "app_heap_peak",
            limit: f64::from(device.ram_app_heap_mb),
            unit: "MB",
            actual: app_heap_peak_mb.map(round1),
            pass: heap_ok,
            note: "marca d'água do heap do app (arena da VM, runtime_health)",
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
        app_heap_peak_mb: app_heap_peak_mb.map(round1),
        budgets,
        heap_budget_mb: device.ram_app_heap_mb,
        heap_budget_status: "measured",
        pass,
    })
}

/// Maior marca d'água do heap do app (MB) entre as execuções; `None` se
/// nenhuma execução reportou o valor (falha fechada no orçamento).
fn heap_peak_mb_max(cold: &[Run], warm: &[Run]) -> Option<f64> {
    cold.iter()
        .chain(warm.iter())
        .filter_map(|r| r.heap_peak_bytes)
        .max()
        .map(|b| b as f64 / (1024.0 * 1024.0))
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
    match r.app_heap_peak_mb {
        Some(v) => println!(
            "heap do app (pico): {v} MB (limite {} MB)",
            r.heap_budget_mb
        ),
        None => println!("heap do app (pico): não medido"),
    }
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
        // ADR 008 Stage 5: budget lowered 2000 -> 1500 ms.
        assert!(startup_within_budget(1499.9));
        assert!(!startup_within_budget(1500.0));
        assert!(!startup_within_budget(2500.0));
    }

    #[test]
    fn rss_budget_inclusive_and_fail_closed() {
        assert!(rss_within_budget(Some(512.0), 512));
        assert!(!rss_within_budget(Some(512.1), 512));
        assert!(!rss_within_budget(None, 512));
    }

    /// ADR 015 (owner decision 2026-10-25): the moto-e5 process cap is
    /// 1024 MB, so a run peaking between 512 and 1024 MB now passes and one
    /// above 1024 MB still fails closed. The cap comes from the embedded
    /// `data/devices.toml`, the same table `rd perf` uses.
    #[test]
    fn moto_e5_rss_cap_is_1024_mb_adr_015() {
        let table = rd_framework::fd_devices::DeviceTable::embedded().expect("devices.toml");
        let e5 = table.hardware_floor().expect("moto-e5 floor profile");
        assert_eq!(e5.ram_process_cap_mb, 1024);
        assert_eq!(e5.ram_app_heap_mb, 256, "guest heap unchanged by ADR 015");
        assert!(rss_within_budget(Some(900.0), e5.ram_process_cap_mb));
        assert!(rss_within_budget(Some(1024.0), e5.ram_process_cap_mb));
        assert!(!rss_within_budget(Some(1024.1), e5.ram_process_cap_mb));
    }

    #[test]
    fn p50_handles_odd_even_and_empty() {
        assert_eq!(p50(&[]), 0.0);
        assert_eq!(p50(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(p50(&[4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn heap_budget_inclusive_and_fail_closed() {
        assert!(heap_within_budget(Some(100.0), 256));
        assert!(heap_within_budget(Some(256.0), 256)); // limite é inclusivo
        assert!(!heap_within_budget(Some(256.1), 256));
        assert!(!heap_within_budget(None, 256)); // sem medição → falha fechada
    }

    #[test]
    fn heap_peak_mb_max_takes_worst_run_and_fail_closed() {
        let run = |b: Option<u64>| Run {
            host_ms: 1.0,
            emulated_ms: 1.0,
            cpu_ms: None,
            heap_peak_bytes: b,
            install_ms: None,
            launch_ms: 0.0,
            screenshot_ms: 0.0,
            rss_after_mb: None,
        };
        // nada medido → None (orçamento falha fechado)
        assert_eq!(heap_peak_mb_max(&[], &[]), None);
        assert_eq!(heap_peak_mb_max(&[run(None)], &[]), None);
        // maior marca d'água entre cold+warm, em MB
        assert_eq!(
            heap_peak_mb_max(
                &[run(Some(1024 * 1024))],
                &[run(Some(3 * 1024 * 1024)), run(Some(2 * 1024 * 1024))]
            ),
            Some(3.0)
        );
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
