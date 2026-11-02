//! Medição reproduzível do interpretador S1 (ADR 013 §5-6).
//! Uso: cargo run --release -p rd-engine-wasm --example measure
//! Imprime instruções executadas (fuel) e tempo de parede para fib(n).
use rd_engine_wasm::{Instance, Module, Value};
use std::time::Instant;

/// Módulo `fib(i32) -> i32` montado à mão (mesmo de tests/wasm_mvp.rs).
const FIB: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x66, 0x69, 0x62, 0x00, 0x00, 0x0a, 0x1e, 0x01,
    0x1c, 0x00, 0x20, 0x00, 0x41, 0x02, 0x48, 0x04, 0x7f, 0x20, 0x00, 0x05, 0x20, 0x00, 0x41, 0x01,
    0x6b, 0x10, 0x00, 0x20, 0x00, 0x41, 0x02, 0x6b, 0x10, 0x00, 0x6a, 0x0b, 0x0b,
];

fn main() {
    let module = Module::parse(FIB).expect("módulo de referência válido");
    for n in [10, 20, 25] {
        let mut inst = Instance::new(module.clone(), 0).expect("instancia");
        let t = Instant::now();
        let out = inst
            .invoke("fib", &[Value::I32(n)], rd_engine_wasm::limits::MAX_FUEL)
            .expect("fib dentro do limite de fuel");
        let dt = t.elapsed();
        println!(
            "fib({n:>2}) = {:?}  instruções={}  tempo={:.3} ms",
            out.results[0],
            out.fuel_used,
            dt.as_secs_f64() * 1e3
        );
    }
}
