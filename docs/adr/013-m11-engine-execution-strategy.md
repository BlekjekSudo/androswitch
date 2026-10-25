# ADR 013 — Estratégia de execução de engines do M11

- **Status:** proposto (aceito quando o dono mergear — precedente do ADR 008); implementação por PRs S1–S4, cada um com evidência própria
- **Data:** 2026-10-25 · **Escopo:** M11 (PARTE 4 do `RustyDroid.md`), Apêndice G (LANG-12, LANG-05, LANG-07, LANG-04), crates `rd-engine-*`, `data/engines.toml`, `golden/engines/`, docs
- **Relacionados:** ADR 003 (registro ≠ execução), ADR 004 (isolamento nativo + evidência golden), ADR 007 (feature opcional, off por padrão), ADR 008 (piso moto-e5, orçamentos e Leis 1/2/4)
- **Divergência da spec:** a PARTE 4 lista M11 como "Flutter → RN → Mono → wasm"; a PARTE 1 exige ADR para divergir. Este ADR inverte a ordem e fixa o critério de "rodar".
- **Delimitação do "Mono":** o item Mono do M11 é **LANG-07 (#17 MAUI/Xamarin)**. Unity (**LANG-08**, #18 Mono/#19 IL2CPP) **não** faz parte do DoD do M11; fica registro-only. As outras 24 entradas do Apêndice G fora do roadmap M11 (28 − 4 engines do M11) permanecem registro-only; nenhuma muda de estado por tabela.

## Contexto

O DoD do M11 é "1 app de cada engine rodando golden flow" com as quatro engines do
roadmap: Flutter (#11), React Native (#12), Mono (#17, MAUI/Xamarin) e WASM in-app (#28).
Hoje as 28 entradas do Apêndice G são registro/detecção (ADR 003) e apenas três
adaptadores nativos (C/C++/Rust, M9/ADR 004) executam. O ADR 003 estabelece a regra
que este ADR operacionaliza: engine futura exige crate própria `rd-engine-<nome>`,
ADR, feature, adaptador real e golden flow com APK antes de mudar o estado de runtime
— goldens de detecção não contam.

Começar pelo Flutter (ordem literal da listagem) é o maior risco primeiro: `libapp.so`
é código AOT convidado + engine/render próprios, exigindo a rota isolada do ADR 004 e
superfície Dart enorme — meses sem prova ponta a ponta do pipeline de engines.

Restrição dura herdada da review (PR #199): o CI roda
`cargo check --workspace --all-features --locked` no job **MSRV 1.76**; qualquer
feature nova (incluindo `wasm-engine`) **precisa compilar em 1.76**. Enfraquecer o
gate (tirar a feature do `--all-features`) é proibido pela política no mesmo PR.

## Decisão

1. **Ordem de execução: WASM → React Native → Mono → Flutter.**
   (a) WASM tem o menor blast radius: formato especificado, sem engine de render (a UI
   é cola Java/DEX já coberta pela VM). (b) Prova o pipeline M11 completo que os
   próximos copiam: crate engine + feature + adaptador `EngineAdapter` + APK golden
   construído do fonte + teste de integração + mudança honesta de estado no registro +
   leitura via MCP `observe` existente (sem tool nova, sem mudança de
   protocolo/contagem — ADR 001). (c) RN e Mono reutilizam o padrão com engines de
   maior superfície. (d) Flutter fica por último: Dart AOT é código convidado — reusa
   o worker isolado do ADR 004 e provavelmente exigirá ADR próprio antes do S4.

2. **Critério de "rodar" (sem overclaim).** Uma engine conta como executada somente
   quando: APK demo **próprio** construído do fonte em `golden/engines/<engine>/`
   (nunca APK de terceiro), artefato **real** da engine (ex.: módulo `.wasm` real)
   executado com contagem positiva de instruções/fuel registrada, UI Java exibindo o
   resultado computado, e asserções via suíte `rd test` (JSON → JUnit XML, exit codes)
   com `observe`/`wait_for`. Detecção não satisfaz o DoD (ADR 003); compatibilidade
   com APKs de terceiros **não** é reivindicada. O wording do registro/compat-matrix é
   **"WASM hospedado pelo RustyDroid via bridge documentada"** — nunca "runtime
   WASM in-app" genérico — para que o DoD do M11 não seja lido como suporte a WASM
   de terceiros (em apps reais, WASM tipicamente roda via WebView/JS ou runtime
   embutido dentro de um `.so`). Estado do registro muda para "runtime parcial" com
   wording limitado ao que executa (padrão `AVAILABLE_BOUNDED` +
   `execution_claimed` só com evidência positiva).

3. **Bridge LANG-12 (escolha deixada "para a implementação futura" pelo
   `engines.toml`): contrato de extensão RustyDroid documentado, implementado pelo
   framework.** O APK demo usa uma classe de bridge fixa e documentada (ex.:
   `io.rustydroid.wasm.WasmBridge`, fornecida pelo framework como as classes
   `android.*`, com assinaturas de método fixas e convenção documentada de caminho
   de asset) que carrega o `.wasm` real de `assets/` e o executa via
   `rd-engine-wasm`. **Sem** `System.loadLibrary` e **sem** checagem de nome de
   biblioteca no caminho de carga — a bridge não é um caso especial do runtime, é um
   contrato público em `docs/m11-wasm.md`. A evidência golden divulga
   explicitamente que o `.so` convidado não roda sob o worker Unicorn nesta fase —
   quem executa é o runtime WASM próprio. Executar um `.so` real com runtime WASM
   embutido sob Unicorn fica como extensão pós-M11.

4. **Runtime: interpretador WASM MVP próprio (decisão da review, finding #1).**
   `rd-engine-wasm` implementa um interpretador **do subconjunto MVP** que o golden
   flow exige (inteiros i32/i64, controle de fluxo estruturado, locals/globals,
   memória linear, chamadas diretas; sem threads/SIMD/exceções/multi-memória) em
   Rust puro, `unsafe_code = "forbid"` intacto, **zero dependências novas** — o mesmo
   caminho da casa que escreveu a VM Dalvik e os parsers ZIP/AXML/ARSC/DEX. Isso
   resolve o bloqueador MSRV: nada além do workspace compila em 1.76 e o gate
   `--all-features` continua íntegro. O interpretador tem fuel nativo (determinístico,
   Lei 4), validação completa antes da instanciação (parsers nunca confiam — Lei 1
   via `RdError` estruturado) e limites duros abaixo.
   **Fallback registrado (não plano A):** se a cobertura do subconjunto próprio
   provar insuficiente em S2+, pina-se `wasmi = "=0.31.2"` (última linha MSRV 1.76;
   de 2024-01, não-mantida) **condicionado a `cargo deny check advisories` limpo**
   na versão pinada; bump de MSRV do workspace (opção b da review) exige ADR próprio
   e decisão do dono; enfraquecer o gate `--all-features` (opção c) fica excluído.

5. **Isolamento e limites duros (in-process, seguro por construção).** Limites
   espelhando `native_execution.rs`, com justificativa moto-e5 (ADR 008:
   `ram_process_cap_mb=512`):
   - módulo `.wasm` ≤ **2 MiB** e memória linear ≤ **16 MiB** → custo máximo
     declarado ≈ 18 MiB de RSS (~3,5% do teto de processo de 512 MiB do moto-e5 —
     folga compatível com os orçamentos de RSS do ADR 008);
   - **fuel ≤ 10M por invocação** → pior caso ~1 s emulado no fator moto-e5 (0.25);
     é teto de segurança, não meta de latência; valor afinável em S1 com números
     medidos;
   - payload de chamada/resposta ≤ **16 KiB** e ≤ **16 argumentos** (idêntico ao
     `native_execution.rs`).
   Excesso/falta = `RdError` estruturado (Lei 1), nunca pânico; sem rede; sem relógio
   de parede no convidado (relógio virtual); saída determinística byte-a-byte (Lei 4)
   com a engine sendo código nosso versionado pelo workspace.

6. **Orçamentos moto-e5 (ADR 008).** Instanciação/execução da engine contam no
   cold/warm do app — não há orçamento novo separado. Números medidos vão na
   evidência; se estourar, reduz-se fuel/limites antes de relaxar orçamento.

7. **Naming/features.** Crate nova `rd-engine-wasm` (Rust puro, zero deps);
   habilitação em `rd-cli`/`rd-engine-registry` por feature `wasm-engine`
   (simétrica a `native-engines`, off por padrão — padrão ADR 007, e SEMPRE
   compilando em 1.76 dentro do `--all-features` do CI); seleção de contrato
   continua `lang-12` (ADR 003 §4). `rd engine check wasm` deixa de responder exit 2
   `NOT_IMPLEMENTED` **somente** para o que de fato executa no build com a feature.

8. **Fuzzing do parser novo (review, finding #3).** O módulo `.wasm` vem de assets
   do APK = entrada não-confiável; SECURITY.md trata parsers como críticos. O S1
   inclui `fuzz_targets/wasm_module.rs` (parse + validade + instanciação sob
   limites) e a inclusão no campaign fuzz do RD Sentinel, junto de ZIP/AXML/ARSC/DEX.

9. **Fatiamento.** S0 = este ADR. S1 = `rd-engine-wasm` (interpretador MVP) + golden
   `golden/engines/wasm/` (APK e `.wasm` construídos do fonte de forma reproduzível e
   hermética — emitter determinístico sem toolchain externa, ou ferramenta declarada
   no PR) + testes (unidade, integração, negativos: módulo grande, fuel esgotado,
   memória excedida) + `fuzz_targets/wasm_module.rs` + `docs/m11-wasm.md` (contrato
   da bridge) + evidência. S2 = React Native. S3 = Mono (LANG-07). S4 = Flutter
   (ADR próprio provável). S2–S4 só começam após o merge do S1 (evita sobreposição e
   valida o padrão primeiro).

10. **Evidência golden (espelha ADR 004).** sha256 do APK e do módulo `.wasm`,
    fonte/licença, funções exportadas chamadas, fuel consumido, contagem de
    instruções WASM positiva, limites vigentes, o wording "WASM hospedado pelo
    RustyDroid via bridge documentada" e a divulgação do contrato de extensão.

## Consequências

- O pipeline M11 fica provado cedo com risco mínimo; a ordem diverge da listagem da
  PARTE 4 e fica justificada aqui.
- Interpretador próprio = mais código nosso (~milhares de linhas seguras) em vez de
  uma dependência não-mantida pinada; em troca, zero risco de MSRV/cargo-deny e fuel
  nativo determinístico. O risco de cobertura fica confinado ao subconjunto MVP que
  o golden flow exige e é declarado por wording limitado.
- A bridge é um contrato de extensão público e documentado — não é suporte a
  bridges/JNI de terceiros nem "runtime WASM in-app" genérico.
- Nenhuma dependência nova no S1; builds mínimos e `--all-features` no MSRV 1.76
  seguem verdes.
- S2–S4 só começam após o merge do S1 (evita sobreposição e valida o padrão primeiro).
- Sem mudança no protocolo MCP/contagem de tools; `observe`/`wait_for` existentes
  leem o resultado do golden flow.

## Alternativas rejeitadas

- **Ordem literal (Flutter primeiro):** maior risco primeiro; meses sem prova de
  pipeline; exige rota Unicorn + engine Dart inteira antes de qualquer golden flow.
- **`wasmi` atual como dependência (plano A original):** bloqueado pelo MSRV 1.76 do
  CI (0.32+ exige 1.77–1.86); só a 0.31.2 compila e é não-mantida — risco de
  advisory/cargo-deny para um parser crítico. Rebaixada a fallback condicionado.
- **Bump de MSRV do workspace:** impacto global, exige ADR próprio e decisão do dono;
  fora do escopo deste ADR.
- **Manter `wasm-engine` fora do `--all-features`:** enfraquece o gate de MSRV;
  política proíbe no mesmo PR; rejeitado na review.
- **Executar `.so` real (runtime WASM em C embutido, ex. wasm3) sob Unicorn:**
  interpretador sobre interpretador, fora dos orçamentos moto-e5 do ADR 008;
  registada como extensão.
- **Marcar engines como executáveis por detecção de assinatura:** rejeitado pelo
  ADR 003; detecção nunca autoriza execução.
- **Shim JNI genérica / caso especial no caminho de carga:** rejeitado (review
  finding #2); a bridge é contrato documentado implementado pelo framework, sem
  checagem de nome de biblioteca no load path.
