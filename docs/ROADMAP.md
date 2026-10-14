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

## ✅ Apêndice G — registro e detecção completos (2026-10-14)

**Escopo aprovado: registro/detecção, não execução de engines.** M9/M11 e a
seleção de runtimes no spec/manifest permanecem inalterados.

- `data/engines.toml` + `rd-engine-registry`: 28 IDs estáveis, 12 módulos,
  dependências e estratégias; schema validado e status de runtime honesto.
- Detecção por nomes ZIP canônicos/classes/metadata do manifest; `signature`,
  `candidate`, `not_detected`; APK híbrido mantém múltiplos resultados.
  Não extrai/descomprime assets, não valida payload, não consulta rede e não
  executa libs/bytecode. DEX/ELF não atribuem linguagem de origem.
- Features opt-in `lang-01`…`lang-12`, transitivas e propagadas por CLI/MCP;
  build mínimo conserva o catálogo, sem novas dependências de runtime nativo.
- `EngineAdapter::parse`/`event`/`syscall`: 28 adaptadores indisponíveis retornam
  `NOT_IMPLEMENTED(LANG-xx)`, mesmo com feature. Nenhum processo fictício.
- `rd engine list|detect|check`, campo `engines` de `inspect --json` e MCP
  `app_info`; overrides de catálogo explícitos falham sem fallback silencioso.
- 28 fixtures golden de **detecção**, testes de contratos/paths/limites/híbridos,
  CLI/MCP, drift e feature isolation. Integração OSS explícita via `RD_TEST_APK`.
- Tabela do Apêndice G, [registro gerado](engine-registry.md) e compat-matrix
  saem da mesma fonte (`cargo xtask engine-spec|engine-report|compat-report`).

**DoD entregue:** todos os 28 registros consultáveis e testados; toda rota ausente
responde erro tipado. **DoD de execução NÃO entregue:** cada engine futura precisa
crate/adaptador real, golden flow de APK OSS e gate Moto E5 (ADR 003).

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

**Fora do M6:** nada pendente — todos os adiamentos foram entregues
(get_api_usage, timeout por passo, retries/hooks AG-07 e get_coverage abaixo).

## ✅ Adiados do M6 concluídos

### `get_api_usage` (AG-09, metade estática)

1. ✅ **Tool MCP `get_api_usage [--package]`**: análise ESTÁTICA dos DEX do app
   instalado — tipos `android.*` referenciados (`method_ids`/`field_ids`/
   superclass/interfaces), métodos por classe e contagem por domínio
   (`android.app`, `android.view`, …). Sem app/package → erro tipado.
2. ✅ Escopo honesto no resultado (`analysis: "static-dex-references"`): é o
   que o app REFERENCIA, não coverage de execução — a metade dinâmica veio
   na sequência (`get_coverage`, abaixo).
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

### `run_scenario` com retries e hooks (AG-07)

1. ✅ **Hooks por cenário**: `setup` (roda antes dos passos; falha aborta o
   cenário — passos e teardown viram `SKIP`) e `teardown` (roda depois dos
   passos MESMO se falharem — limpeza garantida, best-effort; falha no
   teardown reprova o cenário). Mesma gramática de passo (`expect`,
   `timeout_ms`, `retries`); JUnit marca a fase (`[setup 0]`, `[teardown 1]`).
2. ✅ **Retries por passo**: `retries: N` (inteiro ≥ 0, padrão 0) = N
   tentativas extras após falha (erro inesperado, asserção ou `STEP_TIMEOUT`);
   a primeira que passa encerra; tempo = soma das tentativas; log da última;
   `attempts` anotado no JUnit.
3. ✅ Validação antes de executar: `retries` negativo/fracionário/texto e
   hooks que não são listas → `INVALID_SUITE` (exit 64).
4. ✅ Docs: `docs/suite-format.md` (campos, exemplo, regras 8-9) e módulo
   `rd-agent::suite`; 8 testes novos (parse, ordem dos hooks, teardown após
   falha, setup abortando, retries, curto-circuito, JUnit).

### `get_coverage` (AG-09, metade dinâmica)

1. ✅ **Instrumentação no interpretador (rd-vm)**: `Engine::call` é o funil
   único de todo corpo DEX executado — registra `(dex_idx, method_idx)` num
   conjunto de cobertura; `coverage_snapshot()` enumera o classpath e devolve
   totais por classe. Denominador honesto: só métodos com corpo
   (`code_off != 0`); abstract/native e classes host (framework/intrinsics)
   ficam fora.
2. ✅ **Tool MCP `get_coverage`** (32ª tool): exige app lançado
   (`NOT_RUNNING` senão); responde `{package, analysis:
   "method-entry-instrumentation", classes_total, methods_total,
   methods_executed, coverage_pct, classes[]}`. A contagem é do processo
   atual — relançar o app zera (arena-por-execução, M2).
3. ✅ Testes: rd-vm (snapshot conta corpos executados; abstract fora do
   denominador) + MCP (NO_APP→NOT_RUNNING, instalado sem launch→NOT_RUNNING,
   app lançado→onCreate/`<clinit>` contabilizados, pct coerente).
4. ✅ Catálogo: 31 → 32 tools (README, docs/mcp-tools.md regenerada, asserts
   de contagem).

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

## 🔎 M8 sondagem — compat real, primeiro app (2026-10)

Sondagem com APK real dentro do piso (sem commit de binário — GH-05;
`golden-apks/fetch.sh org.primftpd`):

- **App**: `org.primftpd` 8.2 (primitive ftpd, OSS) — minSdk 26 (**piso OK**),
  target/compile 36, 11 MB, 5 DEX, 21 componentes, Kotlin/AndroidX, assinatura
  v1+v2+v3.
- ✅ **`rd inspect`**: completo (componentes, permissões 5 auto/2 runtime/1
  special, features, LAUNCHER `MainTabsActivity` + alias leanback).
- ✅ **`install` via MCP**: registrado na sessão com classificação de
  permissões correta.
- ✅ **`AtomicReference` host (entregue nesta rodada)**: o launch parava em
  `AtomicReference.<init>(Object)` NOT_FOUND; agora `<init>/get/set/getAndSet/
  compareAndSet/weakCompareAndSet/compareAndExchange` são intrínsecos (estado
  em campo de heap; CAS por identidade de referência; VM single-threaded ⇒
  CAS exato). Verificado no app real: o gap seguinte mudou.
- ✅ **Coleções `java.util` host (entregue nesta rodada)**: o gap seguinte
  era `java.util.WeakHashMap.<init>()V`. Agora `HashMap`/`WeakHashMap`
  (put/get/remove/containsKey/containsValue/size/isEmpty/clear + ctors) e
  `ArrayList`/`LinkedList` (add/add(i)/get/set/remove(i)/remove(obj)/
  contains/indexOf/size/isEmpty/clear) são intrínsecos com estado em
  `HostObj::Map/List`. Igualdade Java nas chaves/elementos: String por
  conteúdo, boxes por valor, senão identidade (equals de usuário NÃO é
  consultado — limitação documentada). Sem GC na arena ⇒ WeakHashMap ≡
  HashMap. Sem iteradores/entrySet/keySet ainda — catálogo `compat_gaps`
  atualizado para `partial`.
- ✅ **`java.lang.Enum` host (entregue nesta rodada)**: terceiro gap do
  launch (`ExceptionInInitializerError` → `Enum.<init>(String,int)`).
  `<init>`/`name`/`ordinal`/`toString` (default = name) são intrínsecos com
  estado nos campos de heap `name`/`ordinal`. Dispatch pela classe DECLARANTE
  do method ref (`call_instance_intrinsic_mref`): a super-chamada `Enum.*`
  cai no host, mas o construtor da classe de enum do app (código DEX) NÃO é
  sequestrado; `toString` só é default quando ninguém na cadeia DEX o
  declara. `values()`/`valueOf()` são estáticos sintéticos do app — rodam
  normal no interpretador.
- ✅ **Família `java.lang.ref` host (entregue nesta rodada)**: quarto gap do
  launch (`WeakReference.<init>(Object)`). `WeakReference`/`SoftReference`:
  `<init>()V`/`(Object)V`/`(Object,ReferenceQueue)V` (fila ignorada),
  `get`/`clear` — estado no campo de heap `referent`; dispatch pela classe
  declarante do method ref (super-chamada de subclasse cai no host; construtor
  da subclasse segue para o DEX) e `get`/`clear` só sem declaração na cadeia
  DEX (override de usuário vence). `ReferenceQueue`: `poll`/`remove` →
  `null` (sem GC nada é enfileirado; na JVM real `remove()` bloqueia — aqui a
  fila está permanentemente vazia, resposta honesta). Sem GC na arena ⇒
  referent vive até `clear()` (limitação documentada, mesma política do
  WeakHashMap).
- ✅ **`AtomicReferenceFieldUpdater` host (entregue nesta rodada)**: quinto
  gap (`newUpdater(Class,Class,String)` — antes `NOT_IMPLEMENTED` tipado do
  catch-all `Ljava/`). `newUpdater` captura o NOME do campo (classes são
  ignoradas: sem access check; `volatile` não se aplica a VM single-thread)
  e devolve updater host; `get`/`set`/`getAndSet`/`compareAndSet`/
  `weakCompareAndSet` operam via `get_field`/`put_field` no alvo (CAS por
  identidade de referência; alvo null → NPE materializado).
- ✅ **`java.util.concurrent` host (entregue nesta rodada)**: sexto gap
  (`CopyOnWriteArraySet.<init>`). `CopyOnWriteArraySet` vira Set host
  (mesmo estado `List`, add DEDUPLICA por igualdade Java e devolve false
  se já presente); `CopyOnWriteArrayList` ≡ `ArrayList`. VM single-threaded
  ⇒ cópia-em-escrita é irrelevante. **Bug real corrigido no caminho**:
  `check-cast COWSet → java/util/Set` estourava `ClassCastException` — as
  tabelas `builtin_hierarchy`/`builtin_interfaces` do `is_subtype` não
  conheciam as classes host do M8 (coleções/atomic/ref/Enum); agora têm
  cadeias de classe e closures de interface completas (List/Set →
  Collection → Iterable etc.).
- ✅ **`LinkedHashMap` host (entregue nesta rodada)**: sétimo gap
  (`<init>()V`) — entra na guarda do Map (métodos essenciais ≡ HashMap;
  ordem de iteração NÃO garantida, documentado) e na hierarquia
  (`LinkedHashMap <: HashMap <: AbstractMap`, iface `Map`). Com isso o
  launch real atravessa TODO o grupo java.util que era gap.
- ✅ **`android.os.SystemClock` host (entregue nesta rodada)**: oitavo gap
  (`uptimeMillis()J`) — família nova (framework Android). Relógio é o
  **virtual determinístico** da VM (`HostState.clock_ms`, o mesmo que
  alimenta `postDelayed`/`advance_clock`): `uptimeMillis`,
  `elapsedRealtime`, `elapsedRealtimeNanos` (×1e6) e
  `currentThreadTimeMillis` leem o clock; `sleep(J)V` o **avança e drena o
  Looper** (callbacks agendados disparam — consistente com o contrato do
  tempo da VM; nada de tempo de parede ⇒ testes reprodutíveis).
- ✅ **`AtomicInteger` host (entregue nesta rodada)**: nono gap
  (`<init>()V`). Mesmo padrão do `AtomicReference` (campo de heap `value`):
  `<init>()V`/`(I)V`, `get`/`set`/`getAndSet`, `compareAndSet`/
  `weakCompareAndSet` (exatos — VM single-threaded),
  `incrementAndGet`/`decrementAndGet`/`getAndIncrement`/`getAndDecrement`/
  `addAndGet`/`getAndAdd` com overflow **wrapping** (semântica Java),
  conversões `Number` (`intValue`/`longValue`/`floatValue`/`doubleValue`) e
  `toString`; hierarquia `AtomicInteger <: Number <: Object` + `Serializable`.
- ✅ **`Executors`/`ExecutorService` host (entregue nesta rodada)**: décimo
  gap (`newFixedThreadPool(I,ThreadFactory)`). VM é single-threaded ⇒
  substituto honesto: `newFixedThreadPool`/`newSingleThreadExecutor`/
  `newCachedThreadPool` (com/sem ThreadFactory — ambos ignorados, como o
  tamanho do pool) devolvem executor host que roda `Runnable`/`Callable`
  **inline/síncrono** (determinístico; `execute`, `submit` ×3 sobrecargas);
  `submit` devolve `Future` **já completado** (`get`/`get(J,TimeUnit)`/
  `isDone`=true/`isCancelled`=false/`cancel`=false); `shutdown`/
  `shutdownNow` (lista vazia — nada pendente)/`isShutdown`/`isTerminated`.
  Desvio documentado: exceções do Runnable **propagam imediatamente** (em
  Java seriam capturadas no Future). `ExecutorService <: Executor` na
  hierarquia builtin.
- ✅ **`Looper.getThread`/`Thread` host (entregue nesta rodada)**: décimo
  primeiro gap. VM single-threaded ⇒ UMA thread sintética **"main"** (lazy,
  como o Looper): `Looper.getThread()` (framework) e
  `Thread.currentThread()` (intrinsics) devolvem sempre a mesma referência;
  `getName`="main", `getId`=1L, `threadId`=1, `isDaemon`=false,
  `isAlive`=true, `setName` ignorado, `toString`="Thread[main,1,main]";
  `Thread.sleep(J)V`/`(JI)V` avançam o relógio virtual (≡ SystemClock.sleep;
  ms<0 ignorado), `yield()` é no-op. Hierarquia: `Thread <: Object` +
  implements `Runnable`. `start()`/`interrupt()` seguem fora (não sondados).
- ✅ **`Enum.compareTo` + herança por enums do app (entregue nesta rodada)**:
  décimo segundo gap (`Lifecycle$State.compareTo(Ljava/lang/Enum;)I` — enum
  do APP/DEX herda métodos do `Enum` host, que não tem DEX ⇒ resolução
  falhava quando o method ref declara a SUBCLASSE). Fallback por subtipagem:
  `recv <: Enum` e ninguém na cadeia DEX declara `(name,sig)` → intrínseco
  host (`name`/`ordinal`/`toString`/`compareTo`); override de usuário vence
  (resolve `Some`). `compareTo` = diferença de ordinais com `wrapping_sub`
  (comportamento documentado da JDK); `compareTo(null)` → NPE materializado.
- ✅ **Views + iteradores host (entregue nesta rodada)**: décimo terceiro gap
  (`LinkedHashMap.entrySet`). `entrySet`/`keySet`/`values` devolvem view
  **snapshot** (classe `HashSet` host com cadeia `AbstractSet`; em Java a
  view é viva — desvio documentado) em **ordem de inserção** (estado Map é
  `Vec`); `Map.Entry` host com `getKey`/`getValue`/`setValue` (devolve o
  anterior; não propaga ao Map). `iterator()` para List/Set/views →
  `Iterator` host (`HostObj::Iter`, cursor sobre snapshot): `hasNext`/`next`
  (`NoSuchElementException` quando esgota)/`remove`
  (`UnsupportedOperationException`).
- ✅ **Bug pré-existente corrigido no caminho (achado pelo app real)**:
  `builtin_hierarchy` apontava a cadeia de `Activity` para
  `Ljava/content/Context;` — **typo** de `Landroid/content/Context;` sem
  cobertura de teste até o primftpd exigir check-cast
  `MainTabsActivity → Context` (CCE espúrio). Corrigido + teste
  `android_activity_is_android_context`.
- ✅ **`Collections.synchronized*` (entregue nesta rodada)**: décimo quarto
  gap (`synchronizedMap(Map)`). VM single-threaded ⇒ sincronização é no-op:
  o wrapper devolve a PRÓPRIA coleção (identidade) — `synchronizedMap`/
  `SortedMap`/`List`/`Set`/`SortedSet`/`Collection`; null → NPE
  materializado. `unmodifiable*`/`empty*`/`singleton*` seguem fora (não
  sondados — sem especulação).
- ✅ **`ArrayDeque` host (entregue nesta rodada)**: décimo quinto gap
  (`<init>()V`). Deque sobre o estado `List` (frente = índice 0):
  `addFirst`/`addLast`/`offerFirst`/`offerLast`/`push`/`pop`/`add`/`offer`,
  `removeFirst`/`removeLast`/`pollFirst`/`pollLast`/`getFirst`/`getLast`/
  `peekFirst`/`peekLast`, `remove`/`removeFirstOccurrence`/
  `removeLastOccurrence`, `contains`/`size`/`isEmpty`/`clear` + `iterator`
  (frente→fundo). Semântica JDK: **null rejeitado** (NPE); vazio em
  get/remove/pop → `NoSuchElementException`; poll/peek → null. Hierarquia:
  `ArrayDeque <: AbstractCollection` + `Deque`/`Queue`/`Collection`/
  `Iterable`.
- ✅ **`Object.getClass` + `Class` host (entregue nesta rodada)**: décimo
  sexto gap (`MainTabsActivity.getClass()`). `getClass` é **final** ⇒
  captura irrestrita é correta; devolve o objeto `Class` **canônico** por
  descritor (lazy, identidade estável — `a.getClass() == a.getClass()`).
  Estado: campo de heap `class_name` (descritor). Métodos: `getName`
  (descritor → binário: `Lcom/x/Y;` → `com.x.Y`; arrays mantêm `[I`),
  `getSimpleName` (descarta pacote e classe externa: `La/b/C$D;` → `D`),
  `toString` (`class <nome>`); guarda defensiva: só sem declaração DEX.
- ✅ **`ConcurrentHashMap` host (entregue nesta rodada)**: décimo sétimo gap
  (`<init>()V`). Single-threaded ⇒ mesmo estado/métodos do HashMap host
  (guarda estendida; entrySet/views funcionam). Hierarquia JDK correta:
  **não** estende `HashMap` (`AbstractMap` direto) + interfaces
  `Map`/`ConcurrentMap`/`Cloneable`/`Serializable`.
- ✅ **`LinkedBlockingQueue` host (entregue nesta rodada)**: décimo oitavo
  gap (`<init>()V`). FIFO unbounded sobre o estado `List` (capacidade
  ignorada — default da JDK; `remainingCapacity` = `Integer.MAX_VALUE`):
  `offer`/`add`/`put`/`poll()`/`peek`/`element`/`remove`/`contains`/`size`/
  `isEmpty`/`clear`; null rejeitado (NPE, como na JDK). **Bloqueantes
  fail-fast** (substituto honesto): `take`/`poll(J,TimeUnit)` com fila vazia
  → `IllegalStateException` materializado — bloquear sem outra thread seria
  deadlock; falha visível > pendurar. Hierarquia: `AbstractQueue →
  AbstractCollection` + `BlockingQueue`/`Queue`/`Collection`/`Serializable`.
- ✅ **`ThreadLocal`/`InheritableThreadLocal` host (entregue nesta rodada)**:
  décimo nono e vigésimo gaps (`<init>()V` de ambos). Single-threaded ⇒ um
  valor por instância (campos de heap `tl_value`+`tl_set`): `get` chama
  `initialValue()` via resolução DEX na primeira leitura e memoiza (como na
  JDK); `set`/`remove`; `<init>` só quando o mref declara a classe host
  (ctor de subclasse segue o DEX); get/set/remove por subtipagem e só sem
  declaração DEX (override vence). `InheritableThreadLocal` comporta-se
  igual (herdar entre threads não se aplica). `withInitial(Supplier)` fora
  (não sondado).
- ✅ **`System.getProperty` host (entregue nesta rodada)**: vigésimo primeiro
  gap. Tabela de propriedades coerente com a VM (os.name=Linux,
  os.arch=arm64-v8a, line.separator=LF real, file/path.separator,
  java.io.tmpdir, user.dir/language/region/timezone, java.vm.*,
  file.encoding=UTF-8, http.agent…); chave desconhecida → **null**
  (semântica JDK); variante `(key, def)` devolve **def**; key null → NPE
  materializado. `getProperties()` (Properties completo) fora — não sondado.
- ✅ **`Class.getClassLoader` host (entregue nesta rodada)**: vigésimo
  segundo gap. ClassLoader **canônico por VM** (lazy, identidade estável —
  qualquer classe devolve o mesmo objeto; runtime single-classloader, a
  distinção bootstrap=null do Java não se aplica). `ClassLoader <: Object`.
  `loadClass` fora (não sondado).
- ✅ **`System.getSecurityManager` (entregue nesta rodada)**: vigésimo
  terceiro gap. Android não tem SecurityManager (deprecated na JDK; APIs de
  segurança são no-op) ⇒ sempre **null** — semântica correta, uma linha.
- ✅ **`java.util.ServiceLoader` host (entregue nesta rodada)**: vigésimo
  quarto gap. `load(Class)` e `load(Class,ClassLoader)` criam um loader que
  implementa `Iterable`; `iterator()` devolve um `Iterator` **vazio**. A VM
  ainda não varre `META-INF/services` do APK, portanto não inventa providers:
  `hasNext()` é falso e `next()` materializa `NoSuchElementException`. `service`
  null preserva `NullPointerException`; `ClassLoader` null (bootstrap) é aceito.
- ✅ **`java.util.LinkedHashSet` host (entregue nesta rodada)**: vigésimo
  quinto gap. `<init>()`, `add`/`remove`/`contains`/`size`/`isEmpty`/`clear` e
  `iterator()` reutilizam o Set host; o `Vec` preserva a ordem de inserção e
  remover+re-adicionar desloca o elemento para o fim. A hierarquia real
  `LinkedHashSet → HashSet → AbstractSet` e interfaces Set/Collection/Iterable
  são conhecidas pelo classpath. Construtores com capacity/Collection seguem
  fora até uma sondagem exigir.
- ✅ **`ClassLoader.getResources(String)` + `java.util.Enumeration` host
  (entregue nesta rodada)**: vigésimo sexto gap. `getResources` nunca devolve
  null (contrato JDK): retorna uma `Enumeration` host **vazia** — o runtime
  ainda não enumera entries do APK como recursos de classpath e não varre
  `META-INF/services`, logo não inventa recursos. `name` null materializa NPE
  (`Objects.requireNonNull` da JDK 9+). A `Enumeration` reutiliza o cursor do
  Iterator host com os nomes legados: `hasMoreElements()` é falso e
  `nextElement()` materializa `NoSuchElementException`.
- ✅ **`ArrayList.<init>(Ljava/util/Collection;)V` host (entregue nesta
  rodada)**: vigésimo sétimo gap. A cópia é um **snapshot** dos elementos na
  ordem do iterador da fonte host (ArrayList admite duplicatas — dedup fica a
  cargo da fonte; depois da cópia não há aliasing: mutar a fonte não toca a
  lista). Fonte null materializa NPE (`c.toArray()` na JDK); coleção do app
  sem estado host recebe erro tipado `VM_TYPE_ERROR` em vez de cópia
  silenciosamente vazia/errada. O mesmo construtor vale para
  `LinkedList`/`CopyOnWriteArrayList` via o helper compartilhado.
- ✅ **`LinkedBlockingQueue.drainTo` host (entregue nesta rodada)**: vigésimo
  oitavo gap. `drainTo(Collection)` e `drainTo(Collection,int)` drenam até
  `maxElements` (ou tudo) **na ordem FIFO** para o destino, respeitando a
  semântica de `add` dele (Set deduplica — como na JDK, que chama `c.add(e)`);
  o retorno conta os **retirados** da fila. Contratos JDK honrados: destino
  null → NPE, `drainTo(this)` → `IllegalArgumentException`, `maxElements`
  ≤ 0 → 0. Destino fora da superfície de Collection host (coleção do app) →
  `VM_TYPE_ERROR` tipado em vez de perda silenciosa; destino host ganha
  lazy-init como os demais intrínsecos.
- ✅ **`Activity.getFragmentManager()` host (entregue nesta rodada)**: vigésimo
  nono gap e o **primeiro em `android.app`** — o app completou o bootstrap
  `java.util` e entrou no framework. Devolve o `FragmentManager` **canônico**
  por VM (lazy, identidade estável — mesmo objeto em chamadas repetidas, como
  no Android real por Activity; o runtime executa uma activity por vez).
  O estado de fragments (back stack/transações) ainda não existe: métodos do
  FM seguem sendo sondados um a um.
- ✅ **`FragmentManager.findFragmentByTag`/`findFragmentById` host (entregue
  nesta rodada)**: trigésimo gap. Enquanto não há transações committadas, a
  resposta é **`null`** — contrato Android para fragment não encontrado (tag
  null também devolve null, como na `FragmentManagerImpl`). Nada de
  `NOT_IMPLEMENTED` para o caso vazio.
- ✅ **`FragmentManager.beginTransaction` + `FragmentTransaction` host
  (entregue nesta rodada)**: trigésimo primeiro gap. `beginTransaction()`
  devolve uma transação nova por chamada; `add`/`replace`/`remove` são
  fluentes e gravam **ops pendentes**; `commit`/`commitAllowingStateLoss`
  aplicam no registro de fragments da VM — com validação **antes** de mutar
  (nada de aplicação parcial: `Add` de fragment já registrada →
  `IllegalStateException`; 2º commit ou uso pós-commit → ISE; fragment null →
  NPE — a AOSP dereferencia a fragment). `commit` devolve id opaco `0` (sem
  back stack/animations/lifecycle DEX no host — documentado). A partir daí
  `findFragmentByTag`/`findFragmentById` consultam o registro (a última
  registrada vence; `containerViewId` 0 = sem container, nunca casa).
- ✅ **`Fragment.<init>()V` host (entregue nesta rodada)**: trigésimo segundo
  gap. A classe de fragment do app chama o ctor do super via `invokespecial`;
  o host não tem estado de construção, então é no-op — mesmo padrão de
  `Activity`/`Application`/`Context`. `Fragment <: Object` na hierarquia.
- ✅ **`FragmentManager.executePendingTransactions()` host (entregue nesta
  rodada)**: trigésimo terceiro gap. No host o `commit` aplica
  **sincronamente** — não existe fila pendente, logo a resposta honesta é
  `false` (AOSP: "nothing was executed"). Nunca `NOT_IMPLEMENTED`.
- ✅ **`array.clone()` host (entregue nesta rodada)**: trigésimo quarto gap.
  Arrays são `Cloneable` (JLS 10.7) e o clone é **shallow copy**: novo objeto
  com o mesmo descritor/`ElemKind` e as mesmas referências de elementos
  (mutação estrutural no clone não toca o original). Cobre o rastro clássico
  de `values()`/`$VALUES.clone()` de enums — ex.:
  `[Landroidx/lifecycle/Lifecycle$Event;->clone()`. Na mesma rodada,
  correção de semântica relacionada: `Enum.clone()` materializa
  `CloneNotSupportedException` capturável (é `final` na JDK/libcore e sempre
  lança — nunca copia).
- ✅ **Observabilidade: stack trace Java opt-in (entregue nesta rodada) —
  `VmConfig::exception_trace` + `rd app run --trace`**. Exceções Java não
  capturadas anotam `\n  at classe->metodo(proto)` por frame ao desempilhar
  (estilo JVM, innermost-first). Default OFF: nenhuma saída existente muda
  (golden vectors seguem byte-idênticos; `rd vm exec` intocado). Resultado
  real: o NPE pós-bootstrap do `org.primftpd` foi localizado em
  `androidx.fragment.app.FragmentManager.dispatchCreate()V` ←
  `FragmentController` ← `FragmentActivity.onCreate` ←
  `MainTabsActivity.onCreate` — ou seja, o app usa **androidx FragmentActivity**
  e o call site do receiver null já está identificado.
- ✅ **Semântica DEX de invokes corrigida (rodada seguinte, 36º/37º gaps) —
  tipos de bug mais profundos que os gaps declarativos de API:**  
  (1) **`invoke-super` começava a busca em `super(mref.class)`** — semântica
  DEX correta: inicia na **super da classe que declara o frame** (Dalvik).
  No launch isso pulava `androidx/activity/ComponentActivity.onCreate` inteiro
  → `dispatchOnContextAvailable` nunca rodava → `attachHost` nunca rodava →
  `mNonConfig` null → o NPE acima. `Engine::call` agora calcula
  `frame_super` do `ClassDef` do frame e o interp o usa como ponto de
  partida.  
  (2) **Variantes `/range` de invoke-virtual (0x74) e invoke-direct (0x76)
  estavam TROCADAS**: `invoke-virtual/range` despachava NÃO-virtualmente
  (classe declarada) e `invoke-direct/range` despachava VIRTUALMENTE —
  fazendo o ctor Kotlin de lambda `FunctionReferenceImpl.<init> →
  super(2, receiver, …)` (objeto de `OnBackPressedDispatcher.addCallback`)
  re-executar o próprio `<init>` em **recursão infinita** (VM_STACK_OVERFLOW
  a 512; abort nativo com pilha padrão). Máscaras corrigidas conforme o spec
  Dalvik (0x70/0x76 = direct).  
  (3) **`<init>()V` de coleções abstratas host no-op** (37º): classe DEX
  estendendo Diretamente `java.util.AbstractList` e cia. chama `super()` via
  invoke-direct; antes caía em NOT_FOUND (tipos de classe host sem bucket
  java.util triviais). Testes: 3 regressões em `vm_exec`
  (super-do-frame, range-direct/virtual, subclasse-DEX-de-AbstractList).
  Resultado real: o launch sai do NPE e do overflow e para no próximo gap
  **`Activity.getApplication()`** (NOT_FOUND tipado, sem exception).
- ✅ **`Activity.getApplication()`/`Context.getApplicationContext()` → Application
  canônica por VM (entregue nesta rodada, 38º/39º gaps reais)**: o launch cria a
  Application do `android:name` do manifest (com fallback DEFAULT do AOSP — classe
  ausente no DEX → `Landroid/app/Application;`), roda `<init>`/`onCreate` DEX dela
  ANTES do da activity e devolve SEMPRE a mesma instância; mref pela classe do app
  (ex.: `MainTabsActivity`) coberto em `call_instance_intrinsic_mref` e mref
  `Landroid/...` no framework. Wiring: CLI `app run` e MCP `tool_launch` injetam
  o descritor do manifest (`set_application_class`). Teste: ordem
  Application.onCreate(41)→Activity.onCreate(42); canônica; contexto ≡ aplicação.
- ✅ **Cadeia `java.util.Collections` do `LogController` (entregue nesta rodada,
  40º–43º gaps reais):** `unmodifiableMap`/`unmodifiableSet`/`singletonList` host —
  views imutáveis de SNAPSHOT em classes-marcadoras do JDK
  (`Collections$UnmodifiableMap/$UnmodifiableSet/$SingletonList`): leituras
  delegam (get/size/contains/…); mutações → `UnsupportedOperationException`
  tipada como na JDK; null → NPE; coleção do app sem estado host →
  `VM_TYPE_ERROR` tipado (não itera/instancia surpresa); `HashSet.<init>(Collection)`
  com snapshot dedup na ordem do iterador. Desvio documentado: snapshot ≠ view
  live (as mutações da fonte após a criação não refletem — já o regime de
  synchronized*); instanciador de provider usa ctor acessível sem classe pai implícita.
- ✅ **`SharedPreferences` padrão host (entregue nesta rodada, 44º gap real):**
  `PreferenceManager.getDefaultSharedPreferences(Context)` devolve o objeto
  canônico `Landroid/content/SharedPreferencesImpl;` (AOSP) — prefs começam
  VAZIAS e não persistem em arquivo nesta milestone (getXxx → default da
  chamada; contains → false; key null → NPE do AOSP). `edit()/apply/commit`
  seguem fora (falta tipada na sondagem seguinte se o app escrever prefs).
- ✅ **Descoberta real de `ServiceLoader` via `META-INF/services` (entregue
  nesta rodada, 45º gap real — slf4j 2.x do primftpd):** `load(Class
  [,ClassLoader])` lê `META-INF/services/<nome binário>` do APK anexado
  (formato JDK: `#` comenta; linhas vazias puladas) e guarda os providers no
  estado host; `iterator()` INSTANCIA cada provider via DEX
  (`<clinit>`+`<init>()V`, helper `new_dex_instance`) no snapshot do Iter
  (eager — desvio de preguiça do JDK documentado) e devolve iterador real;
  provider ausente dos dex → `ServiceConfigurationError` tipado (JLS), sem
  skip silencioso; sem APK/sem arquivo → loader vazio (contrato, como antes).
  Resultado real da sondagem: `findServiceProviders` roda DEX, providers
  seriam descobertos se o APK embarcasse os services — o APK golden NÃO tem
  `META-INF/services` (o gradle strip dos packagingOptions) e a única factory
  disponível cai no fallback NOP do slf4j; `LogController.setActiveConfig`
  upstream casta `(LoggerContext)` INCONDICIONALMENTE → `ClassCastException`
  tipada, idêntica ao que um dispositivo real entrega com esse APK (cadeia do
  launch encerrada com comportamento fiel).
- ⬜ **Legacy gap (postergado — superseded pela cadeia androidX resolvida acima):** o
  `ClassLoader.getResources(String) → Enumeration<URL>` abaixo pertence a probe anterior
  (SLF4J/LG já passou — o launch agora roda o onCreate inteiro). Reavaliar quando a
  sequência real exigir SPI/classpath enumeration de novo.
- ⬜ **Próximo gap (re-sondado): `ClassLoader.getResources(String) →
  Enumeration<URL>`** — próximo `NOT_FOUND` após LinkedHashSet
  (`getResources("org/slf4j/impl/StaticLoggerBinder.class")` do
  LoggerFactory.findPossibleStaticLoggerBinderPathSet — Logback probe; o
  PRM continua controlado antes). Replica encontrada: suíte
  `org.primftpd.log.PrimFtpdLoggerFactoryTest`. Necessário (canto-canto):
  Enumeration minimal (`hasMoreElements`/`nextElement`,
  `NoSuchElementException`) + getResources vazio (Java 9+: NPE em name null).
  Follow-ups da Enumeration: `Collections.emptyEnumeration` e
  `Collections.list(Enumeration)` no mesmo host; `ClassLoader.getResource`
  (singular) coberto por exceção quando o launch pedir; bloco `java.net.URL`
  quando a sequência real exigir URL.
