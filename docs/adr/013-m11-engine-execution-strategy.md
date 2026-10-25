# ADR 013 — Estratégia de execução de engines do M11

- **Status:** aceito (design); implementação por PRs S1–S4, cada um com evidência própria
- **Data:** 2026-10-25 · **Escopo:** M11 (PARTE 4 do `RustyDroid.md`), Apêndice G (LANG-12, LANG-05, LANG-07, LANG-04), crates `rd-engine-*`, `data/engines.toml`, `golden/engines/`, docs
- **Relacionados:** ADR 003 (registro ≠ execução), ADR 004 (isolamento nativo + evidência golden), ADR 007 (feature opcional, off por padrão), ADR 008 (piso moto-e5, orçamentos e Leis 1/2/4)
- **Divergência da spec:** a PARTE 4 lista M11 como "Flutter → RN → Mono → wasm"; a PARTE 1 exige ADR para divergir. Este ADR inverte a ordem e fixa o critério de "rodar".

## Contexto

O DoD do M11 é "1 app de cada engine rodando golden flow" com as quatro engines do
roadmap: Flutter (#11), React Native (#12), Mono (#17/#18) e WASM in-app (#28).
Hoje as 28 entradas do Apêndice G são registro/detecção (ADR 003) e apenas três
adaptadores nativos (C/C++/Rust, M9/ADR 004) executam. O ADR 003 estabelece a regra
que este ADR operacionaliza: engine futura exige crate própria `rd-engine-<nome>`,
ADR, feature, adaptador real e golden flow com APK antes de mudar o estado de runtime
— goldens de detecção não contam.

Começar pelo Flutter (ordem literal da listagem) é o maior risco primeiro: `libapp.so`
é código AOT convidado + engine/render próprios, exigindo a rota isolada do ADR 004 e
superfície Dart enorme — meses sem prova ponta a ponta do pipeline de engines.

## Decisão

1. **Ordem de execução: WASM → React Native → Mono → Flutter.**
   (a) WASM tem o menor blast radius: formato especificado, interpretadores puros em
   Rust com `fuel` (candidata: `wasmi`), sem engine de render (a UI é cola Java/DEX já
   coberta pela VM). (b) Prova o pipeline M11 completo que os próximos copiam: crate
   engine + feature + adaptador `EngineAdapter` + APK golden construído do fonte +
   teste de integração + mudança honesta de estado no registro + leitura via MCP
   `observe` existente (sem tool nova, sem mudança de protocolo/contagem — ADR 001).
   (c) RN e Mono reutilizam o padrão com engines de maior superfície. (d) Flutter fica
   por último: Dart AOT é código convidado — reusa o worker isolado do ADR 004 e
   provavelmente exigirá ADR próprio antes do S4.

2. **Critério de "rodar" (sem overclaim).** Uma engine conta como executada somente
   quando: APK demo **próprio** construído do fonte em `golden/engines/<engine>/`
   (nunca APK de terceiro), artefato **real** da engine (ex.: módulo `.wasm` real)
   executado pelo runtime nosso com contagem positiva de instruções/fuel registrada,
   UI Java exibindo o resultado computado, e asserções via suíte `rd test`
   (JSON → JUnit XML, exit codes) com `observe`/`wait_for`. Detecção não satisfaz o
   DoD (ADR 003); compatibilidade com APKs de terceiros **não** é reivindicada; estado
   do registro muda para "runtime parcial" com wording limitado ao que executa
   (padrão `AVAILABLE_BOUNDED` + `execution_claimed` só com evidência positiva).

3. **Bridge LANG-12 (escolha deixada "para a implementação futura" pelo
   `engines.toml`): bridge JNI shim documentada.** O APK demo declara
   `System.loadLibrary("wasmbridge")` com métodos nativos mínimos fixos; o
   RustyDroid implementa essa bridge no framework (não no convidado), roteando para
   `rd-engine-wasm`, que carrega o `.wasm` real de `assets/` e o executa de verdade.
   A evidência golden divulga explicitamente que o `.so` convidado não roda sob o
   worker Unicorn nesta fase — quem executa é o runtime WASM próprio. Executar um
   `.so` real com runtime WASM embutido sob Unicorn fica como extensão pós-M11.

4. **Isolamento e limites duros (in-process, seguro por construção).** Intérprete
   WASM em Rust puro no processo host; `unsafe_code = "forbid"` do workspace se
   mantém (nenhuma exceção nova). Limites espelhando `native_execution.rs` e
   ajustáveis em S1: módulo ≤ 2 MiB; memória linear ≤ 16 MiB; fuel ≤ 10M por
   invocação; payload de chamada/resposta ≤ 16 KiB; ≤ 16 argumentos. Excesso/falta =
   `RdError` estruturado (Lei 1), nunca pânico; sem rede; sem relógio de parede no
   convidado (relógio virtual); saída determinística byte-a-byte (Lei 4) com versão
   da engine pinada.

5. **Orçamentos moto-e5 (ADR 008).** Instanciação/execução da engine contam no
   cold/warm do app — não há orçamento novo separado. Números medidos vão na
   evidência; se estourar, reduz-se fuel/limites antes de relaxar orçamento.

6. **Naming/features.** Crate nova `rd-engine-wasm` (Rust puro); habilitação em
   `rd-cli`/`rd-engine-registry` por feature `wasm-engine` (simétrica a
   `native-engines`, off por padrão — padrão ADR 007); seleção de contrato continua
   `lang-12` (ADR 003 §4). `rd engine check wasm` deixa de responder exit 2
   `NOT_IMPLEMENTED` **somente** para o que de fato executa no build com a feature.

7. **Dependência.** Candidata: `wasmi` (interpretador WASM puro em Rust,
   MIT OR Apache-2.0, `fuel`). O PR S1 declara versão pinada, MSRV 1.76,
   `cargo-deny` (licenças/advisories) e blast radius; se a versão corrente exceder o
   MSRV, pina-se a linha compatível mais recente. Nenhuma dependência C/unsafe.

8. **Fatiamento.** S0 = este ADR. S1 = `rd-engine-wasm` + golden
   `golden/engines/wasm/` (APK e `.wasm` construídos do fonte de forma reproduzível e
   hermética — emitter determinístico sem toolchain externa, ou ferramenta declarada
   no PR) + testes (unidade, integração, negativos: módulo grande, fuel esgotado,
   memória excedida) + `docs/m11-wasm.md` + evidência. S2 = React Native. S3 = Mono.
   S4 = Flutter (ADR próprio provável). As outras 24 entradas do Apêndice G fora do
   roadmap M11 permanecem registro-only; nenhuma muda de estado por tabela.

9. **Evidência golden (espelha ADR 004).** sha256 do APK e do módulo `.wasm`,
   fonte/licença, funções exportadas chamadas, fuel consumido, contagem de instruções
   WASM positiva, limites vigentes e a divulgação da bridge shim.

## Consequências

- O pipeline M11 fica provado cedo com risco mínimo; a ordem diverge da listagem da
  PARTE 4 e fica justificada aqui.
- A bridge JNI shim é limitada aos símbolos fixos do APK demo próprio — não é uma
  ponte "execute-anything" nem suporte a bridges de terceiros.
- +1 dependência (`wasmi`) com blast radius declarado; builds mínimos continuam
  sem ela (feature off por padrão).
- S2–S4 só começam após o merge do S1 (evita sobreposição e valida o padrão primeiro).
- Sem mudança no protocolo MCP/contagem de tools; `observe`/`wait_for` existentes
  leem o resultado do golden flow.

## Alternativas rejeitadas

- **Ordem literal (Flutter primeiro):** maior risco primeiro; meses sem prova de
  pipeline; exige rota Unicorn + engine Dart inteira antes de qualquer golden flow.
- **Executar `.so` real (runtime WASM em C embutido, ex. wasm3) sob Unicorn:** interpretador sobre
  interpretador, fora dos orçamentos moto-e5 do ADR 008; registada como extensão.
- **Marcar engines como executáveis por detecção de assinatura:** rejeitado pelo
  ADR 003; detecção nunca autoriza execução.
- **Shim JNI genérica:** violaria o escopo honesto (Law of Gold da compatibilidade);
  a shim fica restrita ao APK demo próprio com símbolos fixos e documentados.
