//! Gera o APK de demonstração do engine WASM (M11 S1b).
//!
//! `cargo run -p rd-testsupport --example wasm_demo_apk -- golden/engines/wasm/wasm-demo.apk`
//!
//! Os bytes são determinísticos; `wasm_demo_golden` exige que o arquivo commitado
//! seja idêntico ao gerado aqui.

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "golden/engines/wasm/wasm-demo.apk".to_string());
    if let Err(e) = std::fs::write(&out, rd_testsupport::wasm_demo::apk()) {
        eprintln!("escrevendo {out}: {e}");
        std::process::exit(1);
    }
    println!("{out}");
}
