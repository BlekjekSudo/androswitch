//! Validação empírica da issue #16: api_level == sdkVersion (u16@24) em
//! resources.arsc REAL? Se os valores decodificados parecem API levels
//! (26..36) e nunca screenHeight (ex.: 1440), o layout está correto.

use std::collections::BTreeMap;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: verify_config_apilevel <apk>");
    let path = std::path::Path::new(&path);
    let apk = rd_apk::Apk::open(path).expect("apk");
    let arsc = apk.arsc.as_ref().expect("arsc");
    let mut hist: BTreeMap<u16, usize> = BTreeMap::new();
    let mut total = 0usize;
    for pkg in &arsc.packages {
        // ADR 014: snapshot materializado (diagnóstico) — mesmo resultado do
        // eager, custo equivalente; caminho quente usa resolve_*/índice lazy.
        for (_id, entries) in pkg.entries_snapshot() {
            for e in &entries {
                if let Some(api) = e.config.api_level {
                    *hist.entry(api).or_default() += 1;
                    total += 1;
                }
            }
        }
    }
    println!("configs com api_level: {total}");
    for (api, n) in &hist {
        println!("  api_level {api:5}  ×{n}");
    }
    let implausible: Vec<_> = hist.keys().filter(|k| **k > 40 && **k < 100).collect();
    // screenHeight do moto-e5 = 1440 (0x5A0); apareceria aos milhares se o
    // campo fosse lido no offset errado
    let sh1440 = hist.get(&1440).copied().unwrap_or(0);
    println!("entradas com valor 1440 (screenHeight vazaria aqui): {sh1440}");
    if !implausible.is_empty() || sh1440 > 0 {
        eprintln!(
            "SUSPEITO: valores incompatíveis com API levels: {implausible:?} / 1440×{sh1440}"
        );
        std::process::exit(1);
    }
    println!("OK: todos os valores são API levels plausíveis (≤40 ou flags especiais)");
}
