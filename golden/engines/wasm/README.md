# Golden do engine WASM (M11 S1b)

APK de demonstração do ADR 013 §9–10: uma Activity chama
`io.rustydroid.wasm.WasmBridge.call("demo.wasm", "add", 2, 3)` dentro de
`onCreate` e mostra o resultado (`5`) no `TextView` de id `result`.

## Como é construído

Tudo sai do fonte, sem toolchain externa e sem binário commitado:

- `crates/rd-testsupport/src/wasm_demo.rs`: gera o APK (`apk()`), o
  `classes.dex` (`dex()`) e o módulo `assets/demo.wasm` (`wasm_module()`).
  Os bytes são determinísticos.
- `crates/rd-vm/tests/wasm_demo_golden.rs` (feature `wasm-engine`): lê
  `classes.dex` do APK, roda `launch_app` e exige que o texto visível seja
  `5`. Se o bridge não for chamado, o texto fica `aguardando` e o teste falha.

```sh
cargo test -p rd-vm --features wasm-engine --test wasm_demo_golden
```

- `wasm-demo.apk` é gerado por `cargo run -p rd-testsupport --example wasm_demo_apk`.
  O teste `committed_demo_apk_matches_the_generator` garante que o arquivo commitado
  é idêntico ao gerado.
- Suíte `rd test golden/engines/wasm/suite.json` (install → launch → `assert_text`
  com `5`), com `cargo build -p rd-cli --features wasm-engine`.

## O que ainda falta (S1b restante)

- Ligação no `rd-engine-registry`. Só depois disso `data/engines.toml` pode
  sair de `not_implemented`, com a frase **"WASM hospedado pelo RustyDroid via
  bridge documentada"**.

Veja [docs/m11-wasm.md](../../../docs/m11-wasm.md) e
[docs/ROADMAP.md](../../../docs/ROADMAP.md).
