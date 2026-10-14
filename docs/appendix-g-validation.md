# Validação do Apêndice G — 2026-10-14

## Escopo

Registro/detecção dos **28 itens** e contratos dos **12 módulos LANG**, conforme
escopo aprovado. Não é implementação de novos runtimes, golden de execução ou
certificação de performance Moto E5. M9/M11 permanecem pendentes.

## Verificações locais

| Gate | Resultado |
|---|---|
| Baseline antes da mudança | 385 testes passaram |
| `cargo test --workspace --locked` | 416 passaram, 0 falhas; 1 integração OSS explicitamente ignored |
| `cargo test --workspace --all-features --locked` | 416 passaram, 0 falhas; mesma integração ignored |
| Registro sem features default | 3 testes unitários + 17 de integração passaram; 28 casos golden de detecção dentro da suíte |
| Cada feature `lang-01`…`lang-12` separadamente | 12 configurações passaram; dependências transitivas conferidas |
| `cargo check -p rd-cli --no-default-features` / `--all-features` | ambos passaram |
| `cargo clippy --workspace --all-targets` (default e all-features) | limpo com `-D warnings` |
| `cargo fmt --all -- --check` | passou |
| Rust MSRV 1.76.0, workspace + all-features | passou |
| `rd test tests/suites/contract-no-app.json --junit ...` | 8 cenários passaram; JUnit gerado |
| `engine-report`, `engine-spec`, `compat-report` | regenerados idênticos aos arquivos versionados |
| Links Markdown, YAML do workflow, whitespace e segredos | sem erros detectados |

A integração OSS que aparece como `ignored` no comando padrão foi **executada
separadamente com sucesso** (com `RD_TEST_APK`), não contabilizada como um teste
executado quando pulada. Testes de catálogo/spec/features/contratos e CLI/MCP
acrescentaram **31 testes automatizados** ao baseline.

## APK OSS real

- Download: `bash golden-apks/fetch.sh` (política GH-05: binário não commitado).
- Package: `org.fdroid.fdroid` — versão **2.0.1**.
- Tamanho: **12.537.183 bytes**, **973 entradas ZIP**, minSdk **24**.
- SHA-256: `83d3fe522281c3cb89fce3bc05038f81b3c3b32c108536941f184c5f9bb53778`.
- `RD_TEST_APK=... cargo test -p rd-engine-registry --test registry real_oss_apk_inventory -- --ignored --nocapture`: **1 passou**.
- `rd engine detect ... --json` retorna 28 registros; `execution_claimed: false`,
  `artifact_validation: not-performed`.
- O relatório foi comparado byte-logicamente ao campo `engines` de
  `rd inspect ... --json`: **idêntico**.
- DEX/ELF comuns produziram **candidatos**, não atribuição de linguagens.
- minSdk 24 é abaixo do piso de execução 26: **inspeção/detecção permitidas**, mas
  este APK **não foi executado**. Não houve alteração no gate `BELOW_FLOOR`.

## Limites e trabalho futuro

- Assinaturas são heurísticas de nomes/manifest; podem ser falsificadas ou
  ocultadas por strip/rename/encriptação/split/containers/remoto.
- Nenhum asset/lib foi carregado, extraído ou executado pela detecção.
- Features selecionam contratos, não disponibilidade de runtime; parse/event/
  syscall dos 28 adaptadores sempre retornam `NOT_IMPLEMENTED(LANG-xx)`.
- Limites: catálogo ≤ 1 MiB, detecção ≤ 100.000 entradas, ≤ 4 amostras por
  predicado/diagnóstico, erros tipados ao exceder o teto.
- Novos runtimes precisam de crate/adaptador real, golden flow OSS e gate Moto E5.
- Este relatório registra testes **locais**. Execução dos jobs GitHub Actions e
  revisão humana do PR são verificações separadas; nenhum merge automático.

Referências: [registro gerado](engine-registry.md),
[ADR 003](adr/003-appendix-g-engine-registry.md), [ROADMAP](ROADMAP.md).
