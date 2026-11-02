//! Benchmark de regressão (RustyDroid.md §3.2, `bench.yml`).
//!
//! Sem criterion: harness próprio que imprime linhas no formato libtest
//! (`test <nome> ... bench: <ns> ns/iter (+/- <desvio>)`), que é o formato
//! lido pelo `github-action-benchmark` (ferramenta `cargo`).
//!
//! Entrada sintética e determinística: tabelas `method_ids` / `field_ids`
//! de 20 000 entradas (8 bytes cada), com índices válidos.

use std::hint::black_box;
use std::time::Instant;

const ENTRIES: u32 = 20_000;
const SAMPLES: usize = 30;

fn table(entries: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(entries as usize * 8);
    for i in 0..entries {
        buf.extend_from_slice(&((i % 512) as u16).to_le_bytes()); // class_idx
        buf.extend_from_slice(&((i % 64) as u16).to_le_bytes()); // proto/type_idx
        buf.extend_from_slice(&i.to_le_bytes()); // name_idx
    }
    buf
}

fn measure<F: FnMut() -> usize>(mut f: F) -> (f64, f64) {
    // aquecimento
    for _ in 0..3 {
        black_box(f());
    }
    // calibra iterações para ~20 ms por amostra
    let t0 = Instant::now();
    black_box(f());
    let one = t0.elapsed().as_nanos().max(1) as f64;
    let iters = ((20_000_000.0 / one).ceil() as usize).clamp(1, 1_000_000);
    let mut per_iter = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let t = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        per_iter.push(t.elapsed().as_nanos() as f64 / iters as f64);
    }
    let mean = per_iter.iter().sum::<f64>() / SAMPLES as f64;
    let var = per_iter.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / SAMPLES as f64;
    (mean, var.sqrt())
}

fn report(name: &str, (mean, dev): (f64, f64)) {
    println!(
        "test {name} ... bench: {:>12} ns/iter (+/- {:.0})",
        mean.round() as u64,
        dev.round()
    );
}

fn main() {
    let data = table(ENTRIES);
    report(
        "parse_method_ids_20k",
        measure(|| {
            let r = rd_dex::methods::parse_method_ids(&data, ENTRIES, 0);
            let n = r.as_ref().map(|v| v.len()).unwrap_or(0);
            // mantém o resultado materializado: sem isto o otimizador apaga o decode
            black_box(&r);
            n
        }),
    );
    report(
        "parse_field_ids_20k",
        measure(|| {
            let r = rd_dex::fields::parse_field_ids(&data, ENTRIES, 0);
            let n = r.as_ref().map(|v| v.len()).unwrap_or(0);
            // mantém o resultado materializado: sem isto o otimizador apaga o decode
            black_box(&r);
            n
        }),
    );
}
