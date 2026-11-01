# Fuzzing — RustyDroid

Os parsers de formato não-confiável (ZIP, AXML, ARSC, DEX) são fuzz-hardened:
toda leitura é bounds-checked, todo loop tem guarda de avanço e a Lei nº 1
proíbe pânico — falhas são sempre `RdError { code, cause, suggestion, module_id }`.

## Layout

- `crates/rd-apk/fuzz/` — targets `zip`, `axml`, `arsc`, `apk_full`
- `crates/rd-dex/fuzz/` — target `dex` (parse + render das primeiras classes)
- `crates/rd-engine-wasm/fuzz/` — target `wasm_module` (parser + validador + interpretador WASM, M11 S1). Entra na campanha noturna `fuzz.yml` e no job `fuzz-build`. `scripts/fuzz-campaign.sh` ainda não o inclui; essa ampliação fica para uma campanha própria.

Os crates de fuzz ficam FORA do workspace principal (padrão cargo-fuzz) para
não contaminar o MSRV nem o lockfile de produção.

## Rodando

```bash
cargo install cargo-fuzz
cargo +nightly fuzz build -p rd-apk-fuzz          # valida que os targets compilam
cargo +nightly fuzz run zip -- -max_total_time=60 # smoke de 60 s
cargo +nightly fuzz run dex -- -max_total_time=60
```

## Política

- O DoD do M1 pede fuzz 24 h sem crash antes de declarar o parser "estável".
  O script recomendado: rodar os 5 targets por 24 h paralelos em uma noite e
  arquivar os crashes (se houver) como issues com o input minimizado.
- O CI roda `cargo fuzz build` (compilação em nightly, sem execução) para
  garantir que os targets não apodrecem entre milestones.
- Qualquer crash de fuzz é bug P0 — Lei nº 1 violada.
