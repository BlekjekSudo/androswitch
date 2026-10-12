# ROADMAP — RustyDroid

> Espelho executável da PARTE 4 da spec (`RustyDroid.md`). Status real, não aspiração.
> **Regra:** cada fase termina com código funcional + testes verdes + docs atualizadas.

## Status atual

| Fase | Entrega central | DoD (done quando…) | Status |
|---|---|---|---|
| **M0** | Workspace + CI + rd-apk + permissions.toml + devices.toml (moto-e5) + behavior-switches.toml + motor de permissões | inspect de APK real imprime componentes/permissões/features completos; CI verde | ✅ **implementado (2026-10)** |
| M1 | rd-dex 100% + disassembler | idêntico ao baksmali em 3 APKs; fuzz 24h sem crash | ✅ |
| M2 | VM Dalvik mínima | métodos puros de APK real retornam valores corretos | ✅ **implementado (2026-10)** |
| M3 | Framework essencial | app trivial (activity+botão+texto) roda headless ponta a ponta | ✅ **implementado (2026-10, rounds 1+2)** |
| M4 | Render+dump | screenshot correto; get_ui_tree com ids certos | ✅ |
| M5 | Agente MCP v1 (30 tools + controles) | LLM instala, navega e assera estado só com as tools | ✅ |
| M6 | Executor de testes | suíte JSON → JUnit XML; compat-matrix automatizada | ✅ **implementado (2026-10)** |
| M7 | Performance + gate piso E5 | startup < 2 s app trivial; suíte golden passa nos budgets moto-e5 | 🟡 **parcial (2026-10)** — `rd perf` + gate de CI no app trivial (estimativa); golden pendente |
| M8 | Compat real | app OSS médio navega funcional; v0.9 | ⬜ |
| M9 | Componentes avançados + NDK | 5 APKs com .so rodando; widgets/IME/notificações ok | ⬜ |
| M10 | Dispositivos virtuais | câmera/GPS/biometria programáveis pelo agente | ⬜ |
| M11 | Engines | Flutter → RN → Mono → wasm: 1 app de cada em golden flow | ⬜ |
| M12 | Ecossistema GH | docs site, compat-matrix viva, releases; v1.0 | ⬜ |

## O que o M0 entregou (verificado em APK real: F-Droid 2.0.1)

- **Workspace Cargo** com 9 crates + xtask, dependências só apontam para baixo,
  MSRV 1.76, perfil release calibrado para hardware fraco.
- **rd-apk**: leitor ZIP próprio (STORED/DEFLATE/ZIP64, CRC-32 verificado),
  parser AXML completo (string pools UTF-8/16, resource map, typed values,
  dimensões/frações), parser ARSC mínimo-correto (config default, referências
  encadeadas), detecção de assinatura v1/v2/v3/v3.1, modelo tipado do manifest
  (Apêndice F: 4 componentes + aliases + providers + queries + features +
  meta-data + instrumentação) com árvore crua preservada (Lei 2).
- **Motor de permissões** (`fd-permissions`): dado versionado com 175 permissões
  (43 dangerous/12 grupos, 15 specials, 78 normais puras + 15 com flag special = 93 normal-level, 36 signature, 3 internal) +
  13 roles + 7 appops; estado instalável/mutável (grant/deny/revoke) e auditoria
  estática (exported sem guarda, debuggable, cleartext, desconhecidas).
- **fd-devices**: 6 perfis de device; `moto-e5` é o piso contratual (512 MB RSS /
  256 MB heap / 25% single-thread / Adreno 308 fill-rate).
- **fd-behavior**: 25 comutadores targetSdk 26→36 (PRF-07), consultáveis por target.
- **CLI `rd`**: `inspect` (humano + `--json`), `perm list|info|audit`,
  `device list|show`, `behavior --target`, `dex` (contrato `NOT_IMPLEMENTED`
  funcionando com exit code 2). Dados resolvidos de `--data` > `RD_DATA_DIR` >
  `./data` > embutido no binário.
- **CI**: fmt + clippy `-D warnings` + test + MSRV 1.76 + smoke do binário.
- **Fixture GH-05**: `golden-apks/fetch.sh` baixa APKs OSS (nunca commitados).

## ✅ M1 concluído — rd-dex (2026-10)

1. ✅ Parser DEX 100% (header, maps, strings MUTF-8, types, protos, fields,
   methods, classes, code items, debug info, annotations, call sites,
   method handles) — **45.643 classes dos 3 APKs golden renderizam sem
   falha (0 pulos)**.
2. ✅ Disassembler smali fiel (alvo baksmali 2.5.2): **24.913 classes
   comparadas por método — 100% idênticas na sequência de instruções**
   (comparador differential: `/tmp/compare.py`, artefato de validação).
3. ✅ Fuzz targets (`cargo-fuzz`): `zip`, `axml`, `arsc`, `apk_full`, `dex`;
   CI compila os targets em nightly (execução 24h fora do CI — ver
   `docs/FUZZING.md`).
4. Gate de saída: idêntico ao baksmali nos 3 APKs ✅; fuzz 24h sem crash —
   smoke local limpo, campanha 24h pendente de runner dedicado (não bloqueia
   M2; parsers são fuzz-hardened por construção, Lei nº 1).

## ✅ M2 concluído — rd-vm (2026-10)

1. ✅ Interpretador Dalvik mínimo: dispatch por OPCODE EXATO (tabela canônica
   validada vs baksmali no M1) — move/const/return, aritmética int/long/float/
   double com semântica Java exata (wrapping, saturação, NaN, div por zero →
   ArithmeticException), shifts (contador int mesmo em long — pego em APK
   real), branches/switch (packed+sparse via payload), arrays com bounds,
   iget/iput/sget/sput, new-instance/new-array, invocações
   static/direct/virtual/super com resolução por superclasse, try/catch de
   exceções em QUALQUER instrução, strings (const-string, hashCode spec,
   concat, StringBuilder) e exceções de plataforma materializadas.
2. ✅ Arena heap do piso moto-e5: heap ≤ 256 MB (PRF-06) + teto de 1M objetos;
   OOM é erro tipado `VM_OOM` (Lei 1); arena-por-execução = o "GC" contratual
   do M2. Fuel anti-loop (200M default) e profundidade máxima de pilha.
3. ✅ Critério de saída — **150/150 golden vectors PASS vs execução real**:
   pipeline `golden/vm/` compila casos Java (javac no CI / ecj local), dexa
   com D8 `--min-api 26` e compara `rd vm exec --json` contra a execução
   real na JVM (GoldenRunner via reflexão) — resultado byte-a-byte igual
   (inclusive Float/Double.toString, MIN/-1 wrapping, NaN/Infinity).
4. ✅ Métodos puros de APK real executados e verificados à mão contra o smali:
   `ContainerHelpersKt.idealByteArraySize` (116/20/1048564),
   `idealLongArraySize` (cadeia invoke-static → 126), `IntIntPair.getFirst-impl`
   (empacotamento Kotlin: 1), `ScatterMapKt.loadedCapacity` (9).
5. CLI: `rd vm exec <apk|dex> --class --method --sig [--args] [--json]
   [--fuel/--depth/--heap-mb]` — contrato JSON {status: ok|exception|error}.

## ✅ M3 concluído — framework essencial (2026-10, rounds 1+2)

**Round 1 — núcleo host:**

1. ✅ Activity/Window/View/TextView/Button/LinearLayout/FrameLayout headless como
   classes HOST (`rd-vm::framework`) — estado fora do heap Dalvik, dispatch
   igual ao dos intrinsics.
2. ✅ Handler/Looper/Message com clock virtual determinístico (post/postDelayed
   + `advance_clock`); single-thread = UI thread.
3. ✅ Lifecycle (onCreate/onStart/onResume/onPause) dirigido por `launch_app`;
   navegação via `Intent(Context, Class)` + `startActivity` (const-class host).
4. ✅ Touch simulado: hit-test top-down + dispatch de onClick via resolução DEX.
5. ✅ CLI `rd app run` com script do agente (`tap X,Y` · `wait MS` · `dump`) e
   dump textual da árvore (seed do UI dump do M4).
6. ✅ Critério de saída ATINGIDO: app trivial (activity+botão+texto) roda
   ponta a ponta e responde a toques simulados (testes de integração M3).

**Round 2 (M3.2) — LayoutInflater de layout XML real:**

1. ✅ `setContentView(I)` funcional: resid → `@layout/key` (ARSC
   `resolve_name`) → `res/layout/key.xml` (ZIP, fallback configs) →
   `axml::parse` (rd-apk) → árvore de views host recursiva.
2. ✅ Resources injetados na Engine (`Engine::set_resources(Apk)`); o CLI
   `rd app run` abre o APK UMA vez (dex + arsc + layouts do mesmo `Apk`,
   antes lia o zip 3×); `getPackageName` vem do manifest.
3. ✅ Atributos aplicados: `id` (resid → findViewById + dump `@id/nome`),
   `text` (string ou `@string/x` via ARSC), `orientation`,
   `visibility` (0/1/2), `enabled`, `onClick` (método na ACTIVITY),
   `layout_height` de folha com dimensão (dp/sp/px → px, DENSITY 2.0 =
   viewport 360dp); attrs não modelados ignorados com log (RD_FW_DEBUG);
   tags sem host (ImageView/…) falham TIPADAS (INFLATE).
4. ✅ Hierarquia Android BUILTIN no classpath (`builtin_hierarchy`): APKs
   reais não definem android.* no DEX — is_subtype(View/Activity/…) resolve
   sem DEX; dispatch host funciona para views infladas.
5. ✅ `setText(int resId)` + `Activity.getString(I)` resolvem no arsc;
   **bug latente do `findViewById` corrigido** (guard exigia Obj em args[0]
   e devolvia Null SEMPRE — sem cobertura até o M3.2).
6. ✅ Fixtures determinísticos byte a byte (`tests/common/apkfix.rs`): ZIP
   STORED + string pool UTF-16 + AXML + ARSC gerados em Rust (sem fixture
   binário commitado); 6 testes de integração M3.2 (inclui DoD XML+toque e
   erros tipados). 187 testes no workspace, clippy limpo, fmt ok, MSRV 1.76 ok.

Na sequência do M3 (M4): render + UI dump rico (`get_ui_tree` com ids
certos); configs específicas (land/locale/density) selecionáveis.

## ✅ M4 concluído (2026-10) — rd-render real

1. ✅ **`get_ui_tree` com ids certos**: `Engine::view_tree()` estruturada
   (class/resource-id `package:id/nome`/text/bounds/visibility/enabled/
   clickable/children) + `rd-render::uiautomator_xml` no formato do
   `uiautomator dump` real (GONE excluído, INVISIBLE com bounds — issue #46).
2. ✅ **Screenshot correto**: software renderer determinístico (framebuffer
   RGB 720×1440) + painter por classe/estado (contêiner/texto/botão/borda de
   clickable; INVISIBLE não pinta a subárvore) + encoder PNG sem
   dependências (zlib STORED + crc32/adler32 próprios). Texto com glifos de
   bloco 3×7 (fonte tipográfica real: M8/M9 — o Android real usa skia).
3. ✅ **CLI**: `rd app dump <apk>` (uiautomator XML | `--text` | `--json`) e
   `rd app shot <apk> --out FILE.png`; ações de script `xml` e `shot FILE`.
4. ✅ Testes e2e contra fixture real: ids/bounds/clickable/text na árvore;
   pixels do PNG (TextView branco / Button cinza / borda / fundo); PNG
   determinístico (mesmo input → mesmos bytes); dump escape XML.
   Round 2 do M4: seleção de configs por device-config (land/locale) no
   resolve (o parse das 2 configs já chega no modelo — issue #47).

## ✅ M6 concluído — executor de testes (2026-10)

1. ✅ **`rd test <suite.json>`** (TST-01): suíte JSON → resumo humano + **JUnit XML**
   (`--junit`) + `--json`. Exit: `0` tudo passou · `1` houve falha · `64` suíte inválida/IO.
   Formato e regras em [`suite-format.md`](suite-format.md).
2. ✅ **Núcleo em `rd-agent::suite`**: validação antes de executar (`INVALID_SUITE`),
   uma sessão de agente nova por cenário (isolamento), parada na primeira falha,
   `expect: "ok"` ou `"error:CODIGO"` (código estável exato).
3. ✅ **Asserção reprovada conta como falha.** As tools `assert_*` e `wait_for` respondem
   sucesso na chamada com `"ok": false`/`"found": false`; o executor trata isso como falha.
   Sem essa regra, toda asserção errada passaria em verde (achado do próprio teste e2e).
4. ✅ **compat-matrix automatizada** (`cargo xtask compat-report`):
   - status das fases lido de `docs/ROADMAP.md` (a lista fixa estava defasada: M4/M5 aparecia ⬜);
   - guarda de drift: crate nova em `crates/` sem linha na tabela quebra o relatório;
   - seção "Suítes automatizadas" gerada a partir de `tests/suites/*.json`.
5. ✅ **CI**: o job Rust roda `rd test tests/suites/contract-no-app.json` (sem APK) e
   gera o JUnit XML. O job de docs continua exigindo o placar sincronizado.
6. Testes: +12 unitários em `rd-agent::suite`, +4 e2e do binário (`rd-cli/tests/test_cli.rs`:
   passa+JUnit, asserção errada → exit 1, suíte inválida → 64, arquivo ausente → 64),
   +4 do `xtask` (fases do ROADMAP, guarda de crates, contagem de suítes).

**Fora do M6 (fica para as próximas rodadas):** `get_coverage` (metade do
AG-09 que exige instrumentação do interpretador), `run_scenario` com retries e
hooks (AG-07).

## ✅ Adiados do M6 concluídos

### `get_api_usage` (AG-09, metade estática)

1. ✅ **Tool MCP `get_api_usage [--package]`**: análise ESTÁTICA dos DEX do app
   instalado — tipos `android.*` referenciados (`method_ids`/`field_ids`/
   superclass/interfaces), métodos por classe e contagem por domínio
   (`android.app`, `android.view`, …). Sem app/package → erro tipado.
2. ✅ Escopo honesto no resultado (`analysis: "static-dex-references"`): é o
   que o app REFERENCIA, não coverage de execução — `get_coverage` segue
   pendente (instrumentação, M8+).
3. Testes: unidade do scan (rd-agent::api_usage) + tool MCP (fixture:
   Activity/setContentView presentes; NO_APP e PACKAGE_NOT_INSTALLED tipados).
4. Catálogo: 30 → 31 tools (README, `--describe-tools`, `docs/mcp-tools.md`
   regenerada; testes do issue #74 travam a convergência).

### Timeout por passo no executor de suítes

1. ✅ Campo opcional `timeout_ms` (inteiro > 0) por passo da suíte: orçamento
   de wall-clock. É MEDIÇÃO, não preempção: a chamada completa e o executor
   reprova o passo com o código estável `STEP_TIMEOUT` se o teto estourar
   (vale mesmo com `expect: "error:…"`); passos seguintes são pulados.
2. ✅ Validação antes de executar: `timeout_ms` ausente = sem teto; `0`,
   negativo ou não numérico → `INVALID_SUITE` (exit 64).
3. ✅ `docs/suite-format.md` (campo + regra 7) e módulo `rd-agent::suite`
   documentados; testes de parse + execução com APK real (estouro falha e
   pula o resto; orçamento folgado passa).

## 🟡 M7 em andamento — passo 1: gate de performance (2026-10)

1. ✅ **`rd perf <apk> [--device moto-e5] [--runs N] [--json]`** (PRF-06):
   mede `launch → primeiro frame` (frame = render do view tree via `screenshot`)
   em dois regimes — **cold** (`AgentSession` nova por execução; registro do APK
   fora do cronômetro) e **warm** (relançamento na mesma sessão). CPU por thread
   via `/proc/self/task/*/schedstat`; RSS de pico via `VmHWM`. Exit `0` orçamentos
   ok · `1` orçamento falhou ou erro de execução (`PERF_FAILED`).
2. ✅ **Orçamentos do perfil `moto-e5`** (Apêndice A2): startup cold emulado
   `< 2000 ms` (DoD do M7) · RSS de pico `≤ 512 MB` (falha fechado fora do
   Linux) · heap do app `≤ 256 MB` (marca d'água da arena da VM,
   `runtime_health.heap_peak_bytes`, lida fora do cronômetro).
3. ✅ **Gate de CI**: `crates/rd-cli/tests/perf_cli.rs` roda o binário sobre o
   APK do fixture e exige `pass: true` — primeiro gate de performance bloqueante.
4. ✅ `rd-testsupport::trivial_app`: builder do app trivial movido de
   `mcp_cli.rs` para fixture compartilhado (MCP e perf usam os mesmos bytes).
5. Medição local (host de desenvolvimento, 3 execuções): cold p50 ≈ 1244 ms
   emulados, máximo ≈ 1510 ms; RSS de pico ≈ 27 MB. Tempo emulado =
   `host_ms / cpu_single_thread_factor` (moto-e5 = 0.25 ⇒ ×4) — **estimativa**:
   o fator não modela cache, IO, GPU nem o quota de 1 core.
6. ✅ **Regime warm real**: `tool_launch` reutiliza o processo (Engine) quando
   o package já está em execução — sem re-parse do DEX (`ensure_initialized`
   idempotente, `<clinit>` não repete); o resultado do launch traz `warm`.
   install/clear/remove continuam matando o processo (launch seguinte é frio).

7. ✅ **Medição do heap do app**: `rd-vm::Heap` acompanha a marca d'água
   (`peak()`, maior `used` observado); o `runtime_health` expõe
   `heap_peak_bytes`; o `rd perf` lê o pico após o primeiro frame (fora do
   cronômetro), reporta `app_heap_peak_mb` e avalia o orçamento
   `app_heap_peak ≤ ram_app_heap_mb` (falha fechado se não medido).

**Pendente para fechar o M7 (DoD):** gate golden (APKs golden não estão no
repositório) · medição em Moto E5 real · workflow de release bloqueante (hoje
o gate é um teste de CI).
