# Golden de detecção — Apêndice G

[`signatures.json`](signatures.json) tem exatamente **28 casos**, na ordem dos IDs
estáveis de [`data/engines.toml`](../../data/engines.toml). Cada caso gera um APK
sintético (ZIP + manifest AXML) em memória, usando `rd-testsupport`; nenhum binário
é commitado. Os payloads são deliberadamente inválidos como executáveis: a análise
só pode ler nomes ZIP e declarações do manifest.

**Estes são testes de registro/detecção, NÃO golden flows de execução.**
`candidate` não identifica uma linguagem de origem; `signature` é heurística de
marcadores de framework, não validação do payload nem prova de funcionamento.

```sh
cargo test -p rd-engine-registry --no-default-features --locked
cargo test -p rd-engine-registry --all-features --locked
cargo test -p rd-cli --test engine_cli --locked
```

A suíte cobre também formatos ambíguos, híbridos, pistas incompletas, classes e
metadata de TWA sem assets locais, paths hostis, duplicatas, ABI desconhecida,
ordem determinística, amostras limitadas, teto de 100.000 entradas e o contrato
`NOT_IMPLEMENTED` de parse/event/syscall para os 28 adaptadores.

## Integração com APK OSS real

```sh
bash golden-apks/fetch.sh
RD_TEST_APK="$PWD/golden-apks/org.fdroid.fdroid.apk" \
  cargo test -p rd-engine-registry --test registry real_oss_apk_inventory \
  --locked -- --ignored --nocapture
```

O teste é explicitamente `ignored` no teste normal. Quando solicitado, **falha**
se `RD_TEST_APK`/arquivo estiver ausente; não transforma fixture faltante em
sucesso. Ele valida o inventário do APK real, não executa seu código.

Para uma engine futura mudar o estado de runtime, é necessário outro golden:
parse/event/syscall reais, navegação/resultado em um APK OSS correspondente e
budgets Moto E5. Ver [registro gerado](../../docs/engine-registry.md) e
[ADR 003](../../docs/adr/003-appendix-g-engine-registry.md).
