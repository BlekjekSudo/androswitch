#![no_main]
//! Fuzz: parser, validador e interpretador do módulo WASM (ADR 013 §8).
//! O módulo vem de assets do APK, portanto é entrada não confiável.
//! Propriedade: nunca entra em pânico; só devolve `Ok` ou `WasmError` estruturado.
use libfuzzer_sys::fuzz_target;
use rd_engine_wasm::{Instance, Module, Value};

fuzz_target!(|data: &[u8]| {
    let Ok(module) = Module::parse(data) else {
        return;
    };
    let names: Vec<String> = module
        .exports()
        .into_iter()
        .filter(|(_, k)| *k == rd_engine_wasm::ExportKind::Func)
        .map(|(n, _)| n.to_string())
        .take(4)
        .collect();
    let Ok(mut inst) = Instance::new(module, 100_000) else {
        return;
    };
    for name in names {
        // Argumentos fixos: o objetivo é cobrir o interpretador, não a assinatura.
        let _ = inst.invoke(&name, &[Value::I32(3), Value::I32(5)], 100_000);
        let _ = inst.invoke(&name, &[Value::I64(-1)], 100_000);
    }
});
