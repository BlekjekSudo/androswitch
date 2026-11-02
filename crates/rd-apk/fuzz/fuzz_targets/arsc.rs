#![no_main]
//! Fuzz: parser ARSC (ResTable, configs, sparse/offset16, referências) nunca
//! entra em pânico — e, desde o LazyArsc (ADR 014), TAMBÉM a materialização
//! sob demanda: resolve_string/resolve_name/resolve_value sobre TODOS os
//! resids do índice exercitam decodificação de string individual (None
//! memoizado p/ malformadas), bags complexos e cadeias de referência.
use libfuzzer_sys::fuzz_target;
use rd_apk::arsc::Arsc;

fuzz_target!(|data: &[u8]| {
    if let Ok(arsc) = Arsc::parse(data) {
        // ids por pacote (ordenados); a contagem é limitada pelo tamanho do
        // input (pares ≤ bytes/8), então o loop é bounded pelo próprio fuzzer.
        for pkg in &arsc.packages {
            for id in pkg.resource_ids() {
                let _ = arsc.resolve_string(id);
                let _ = arsc.resolve_name(id);
                let _ = arsc.resolve_value(id);
            }
        }
    }
});
