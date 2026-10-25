# 🦀 RustyDroid — Especificação Mestra v2.1 (GitHub-Ready)

> **Runtime Android em Rust, sem VM, headless-first, nativo para agentes de IA.**
> Executa APKs reais diretamente no Linux x86_64 — sem KVM, sem kernel convidado, sem GPU.
> Objetivo: **cobertura total do ecossistema Android** — todas as permissões, todas as APIs,
> todos os componentes e todas as linguagens — com compatibilidade medida, não prometida.
>
> **Piso de API:** `minSdkVersion 26` (Android 8.0 Oreo) · `targetSdkVersion` = última estável no build (36 hoje).
> **Piso de hardware:** Motorola Moto E5 (Snapdragon 425, Adreno 308, 2 GB RAM) — **todos os budgets de performance são calibrados nele, não em emuladores modernos.**

**Versão do documento:** 2.1 (piso API 26 + piso hardware Moto E5) · **Data:** 2026-10 · **Licença alvo:** MIT OR Apache-2.0
**Repositório alvo:** `github.com/nadabgfccvd/RustyDroid` (workspace Cargo monorepo)

---

## 📖 Como usar este documento

1. **PASSO 1 — Selecionar módulos (PARTE 0):** marque `[ ]` → `[x]` ou responda os IDs
   (`PRESET: TESTE-DEV + LANG-02 -TST-06`). Não marcado = não implementado/não baixado.
2. **PASSO 2 — Enviar a PARTE 1** (prompt mestre) + PARTE 2 (matrizes de referência)
   ao agente construtor — ou execute aqui mesmo no chat.
3. **PASSO 3 — Seguir o roadmap M0–M12** (PARTE 4) e o plano GitHub (PARTE 3).

> ⚖️ **Lei de Ouro da Compatibilidade:** *nenhuma API ausente quebra o app em silêncio.*
> Tudo que ainda não existe responde erro estruturado `NOT_IMPLEMENTED(id)` e entra na
> `compat-matrix.md`. "Funciona para tudo" é medido pela matriz, fase por fase.

---

# 📦 PARTE 0 — CHECKLIST DE MÓDULOS (seleção de instalação)

> IDs estáveis desde v1.0 — novos módulos continuam a numeração.

## 0.0 NÚCLEO — sempre incluído

- [x] **CORE-01** Parser APK: ZIP, AXML binário, assinaturas v1/v2/v3
- [x] **CORE-02** Parser `resources.arsc` completo (multi-config: locale, density, dark)
- [x] **CORE-03** Parser DEX completo + disassembler (todos os structures, multi-dex)
- [x] **CORE-04** VM Dalvik: todos os opcodes, exceções, threads, monitores, GC
- [x] **CORE-05** Framework mínimo: Context, Application, Activity, Intent, Bundle
- [x] **CORE-06** Handler/Looper/Message (main thread = UI thread)
- [x] **CORE-07** Render headless → PNG (tiny-skia) + dump de árvore de UI
- [x] **CORE-08** CLI `rustydroid` (inspect/install/launch/dump/screenshot/test)
- [x] **CORE-09** Log estruturado (tracing JSON) + logcat virtual
- [x] **CORE-10** Modo agente básico: MCP (rmcp) + JSON-RPC stdio/HTTP
- [x] **CORE-11** **Motor de permissões completo** (tabela integral do Apêndice B1 — é dado, não código: entra no núcleo desde o dia 1)
- [x] **CORE-12** **Registro de componentes** (4 componentes + serviços vinculados do Apêndice F — manifest como fonte da verdade)

## 0.1 LINGUAGENS & RUNTIMES

- [ ] **LANG-01** Java/Kotlin/Scala/Clojure/Groovy (DEX): multi-dex, invoke-custom, lambdas desugared, `const-method-handle`
- [ ] **LANG-02** C/C++ NDK: ELF loader (x86_64 nativo + interpretador ARM64/ARM32), JNI bridge completa (InvokeInterface, strings, arrays, exceções através da fronteira)
- [ ] **LANG-03** Rust em APKs (cargo-ndk/xbuild): via JNI do LANG-02
- [ ] **LANG-04** Dart/Flutter: interpretador de Dart AOT snapshot (`libapp.so`) + engine Flutter headless (widgets→árvore própria→tiny-skia) *(experimental)*
- [ ] **LANG-05** React Native (Hermes bytecode/JSC): engine JS embutida + ponte RN/Expo→views *(experimental)*
- [ ] **LANG-06** Cordova/Capacitor/Ionic/TWA/PWA: WebView headless embutida *(experimental)*
- [ ] **LANG-07** C# — .NET MAUI / Xamarin(legado): Mono/IL interpretador embutido *(experimental)*
- [ ] **LANG-08** C# — Unity (IL2CPP/Mono) + stubs de engine Unity *(experimental, longo prazo)*
- [ ] **LANG-09** C++/Lua — Unreal / Cocos2d-x / Defold / Solar2D; Python (Kivy/Chaquopy/BeeWare); Go (gomobile); Haxe; Qt/QML: pontes por engine *(experimental)*
- [ ] **LANG-10** Kotlin Multiplatform / Compose Multiplatform: shared code via DEX + Compose MP render
- [ ] **LANG-11** App Inventor/Thunkable/B4X (geram Java): cobertos por LANG-01 — apenas fixtures de teste
- [ ] **LANG-12** WebAssembly in-app (wasm modules): runtime embutida (wasmtime) + bridge JNI/JS *(experimental)*

## 0.2 UI & GRÁFICOS

- [ ] **UI-01** Layouts: LinearLayout, FrameLayout, RelativeLayout, ConstraintLayout, CoordinatorLayout, ScrollView/NestedScroll, RecyclerView(+DiffUtil), ViewPager2, SwipeRefresh
- [ ] **UI-02** Widgets completos: TextView/EditText(ime/inputType/SPAN), Button/ImageButton, ImageView(ScaleType/SVG VectorDrawable), CheckBox/RadioGroup, Spinner/AdapterView, Switch/Slider, ProgressBar/Seekbar, CardView, Toolbar/ActionBar/BottomAppBar, TabLayout/BottomNavigation, Chip/Badge, CalendarView/DatePicker/TimePicker, SearchView, TextureView/SurfaceView(software)
- [ ] **UI-03** Dialogs/Overlays: AlertDialog, BottomSheetDialog, PopupMenu/Window, Toast, SnackBar, Tooltip
- [ ] **UI-04** Fragments (lifecycle, FragmentManager, backstack) + Navigation Component(+safe-args graphs)
- [ ] **UI-05** Jetpack Compose: runtime Compose (recomposição, state, modifiers) → árvore própria + Material 3 *(experimental, longo prazo)*
- [ ] **UI-06** Theming: Material 2/3 tokens, themes.xml, dark mode, dynamic colors
- [ ] **UI-07** Canvas 2D completo: Paint, Path, Region, Bitmap, Shader, ColorFilter, Xfermode; drawables (shape/layer/level/transition/inset/clip/scale/ripple)
- [ ] **UI-08** Animações: ValueAnimator/ObjectAnimator/AnimatorSet, ViewPropertyAnimator, transitions framework, physics (spring/fling), Lottie
- [ ] **UI-09** Tipografia: TTF/OTF/Variable fonts do APK, downloadable fonts, emoji (Noto embutido), autolink, spans completos
- [ ] **UI-10** Janela desktop (winit+softbuffer): input real de teclado/mouse, DPI scaling
- [ ] **UI-11** Render WEB: frames via WebSocket → `<canvas>` + input do navegador de volta
- [ ] **UI-12** Texto complexo: shaping (HarfBuzz ou rustybuzz), bidi/RTL, quebra de linha ICU, hyphenation
- [ ] **UI-13** Acessibilidade nativa: AccessibilityNodeInfo virtual tree (amarrada ao dump do agente)

## 0.3 ARMAZENAMENTO & DADOS

- [ ] **ST-01** SharedPreferences (XML compatível) + DataStore (proto+preferences)
- [ ] **ST-02** SQLite real (rusqlite) + Room (annotations, migrations) + ContentProvider+ContentResolver completos (incl. FileProvider, OpenableColumns)
- [ ] **ST-03** Filesystem app: internal/external/cache/obb/assets/raw; SAF (Storage Access Framework) com VFS do host
- [ ] **ST-04** Keystore simulado: KeyStore/KeyGenerator/Cipher local, EncryptedSharedPreferences, biometric-bound keys (simulado)
- [ ] **ST-05** Backup/restore: Auto Backup (XML rules) → arquivo do host; BackupAgent
- [ ] **ST-06** Data migration tests: versionamento de DB, downgrade rules

## 0.4 REDE & COMUNICAÇÃO

- [ ] **NET-01** HTTP(S): shim OkHttp/HttpURLConnection/Volley sobre reqwest; TLS via rustls; cookie jar; cache HTTP
- [ ] **NET-02** WebSockets + SSE (shim OkHttp WS) + Netty-like patterns
- [ ] **NET-03** Interceptor de testes: record/replay, mocks por rota, latência/bandwidth/erros injetáveis, HAR import/export
- [ ] **NET-04** gRPC/protobuf (tonic) + shim grpc-android *(pesado)*
- [ ] **NET-05** Bluetooth/BLE/NFC/UWB/Wi-Fi Direct virtuais: API completa com dispositivos simulados programáveis pelo agente (GATT server/client fake, HCE/AID routing fake)
- [ ] **NET-06** Telefonia virtual: TelephonyManager (IMEI/ICCID/sinal programáveis), SmsManager (envio capturado), eSIM stub
- [ ] **NET-07** VPN/VpnService (túnel para VFS/rede do host) *(experimental)*
- [ ] **NET-08** Cast/Nearby/Discovery: mDNS (NSD) real na rede local; Cast→stub estruturado

## 0.5 MÍDIA & DISPOSITIVOS VIRTUAIS

- [ ] **MEDIA-01** Áudio out: MediaPlayer/ExoPlayer/SoundPool/AudioTrack → WAV/PCM file (headless) ou ALSA/Pulse host; AudioFocus policy
- [ ] **MEDIA-02** Vídeo: playback→frames PNG (teste) via ffmpeg se disponível; MediaExtractor/MediaCodec shims
- [ ] **MEDIA-03** Câmera virtual: Camera2/CameraX completas com cenas/fixtures programáveis, múltiplas câmeras, torch/zoom/exif
- [ ] **MEDIA-04** Microfone virtual: arquivo/gerador/sintetizador como input; AudioRecord/AudioPolicy
- [ ] **MEDIA-05** Sensores virtuais: accel/gyro/mag/luz/proximidade/step/heart-rate/GPS(localização com rota programável)/fingerprint/face (biometria simulada com sucesso/falha/lockout)
- [ ] **MEDIA-06** Midi/USB virtual: UsbManager com dispositivos fake; MIDI via alsa-seq host *(experimental)*
- [ ] **MEDIA-07** MediaSession/MediaBrowser/MediaController: sessão de mídia estruturada (visível ao agente)
- [ ] **MEDIA-08** Projeção de tela (MediaProjection): captura da própria árvore renderizada (loopback virtual)

## 0.6 SERVIÇOS DO SISTEMA & APIs ANDROID

- [ ] **SYS-01** Permissões: fluxo runtime completo (grant/deny programável, "don't ask again", revogação automática por unused-app, grupos, justificação UI)
- [ ] **SYS-02** Notificações: NotificationManager + channels + styles (BigText/Inbox/Media/Message) + actions → objetos estruturados ao agente; heads-up simulado
- [ ] **SYS-03** Services (started/bound/foreground + tipos), BroadcastReceiver (implícitos/explícitos, ordenados, sticky), PendingIntent
- [ ] **SYS-04** AlarmManager (exact/inexact) / WorkManager (constraints, chains, retry) / JobScheduler — relógio virtual acelerável
- [ ] **SYS-05** Deep links/App Links (validador), intent filters, PendingIntents de notificação
- [ ] **SYS-06** WebView interna (amarrada ao LANG-06) + WebMessage/JS bridge
- [ ] **SYS-07** Clipboard, locale mutável em runtime, configuração dinâmica (orientation/.theme/fontScale/displaySize)
- [ ] **SYS-08** AccountManager/AccountAuthenticator stub; SyncAdapter; Push (FCM→mock local programável)
- [ ] **SYS-09** Widgets de app (AppWidgetProvider, RemoteViews → render real), Live Wallpaper, Screensaver (Dream)
- [ ] **SYS-10** Serviços de sistema extensíveis: AccessibilityService, NotificationListenerService, InputMethodService(IME com teclado virtual), DeviceAdmin/ProfileOwner/DevicePolicyManager, VoiceInteraction/Assistant, CallScreening, Telecom/ConnectionService, PrintService, AutofillService, TileService(Quick Settings), Watch Face/Wear stubs
- [ ] **SYS-11** Shortcuts (estáticos/dinâmicos/pinned), App Actions stub, Sharing (ACTION_SEND completo com VFS)
- [ ] **SYS-12** PackageInstaller (instalação via API, session-based), splits/base rotated *(experimental)*
- [ ] **SYS-13** Health Connect (dados de saúde estruturados, permissões granulares por tipo de dado — API 34) *(experimental)*

## 0.7 MODO AGENTE — ferramentas MCP

- [ ] **AG-01** Input: `tap/swipe/long_press/drag/type_text/press_key/scroll_to` por `resource_id|texto|bounds|índice|content-desc`
- [ ] **AG-02** `get_ui_tree` (JSON + XML uiautomator-compat) com todos os atributos + a11y tree
- [ ] **AG-03** `wait_for(condição, timeout)` + `assert_view_state` estruturado
- [ ] **AG-04** `screenshot` + `screenshot_diff` (golden) + gravação GIF/MP4 + `get_window_hierarchy`
- [ ] **AG-05** `read_logs` filtrável + crash monitor + ANR detector (main > 5 s) + `get_strace`-lite (syscalls do app quando LANG-02)
- [ ] **AG-06** `snapshot_state`/`restore_state` (heap+UI+backstack+prefs+DB) — continuidade para agentes
- [ ] **AG-07** `run_scenario` DSL JSON (ações+asserts+hooks+retries) atômica e reproduzível
- [ ] **AG-08** `monkey`/fuzz determinístico (seed) + mutação de eventos com relatório de travadas
- [ ] **AG-09** `get_coverage` (classes/métodos/linhas DEX) + `get_api_usage` (quais android.* foram chamados)
- [ ] **AG-10** `inject_*` (sensor/gps/câmera/mic/biometria) + `set_locale/set_theme/set_orientation/set_font_scale/set_battery/set_network`
- [ ] **AG-11** `grant_permission/deny_permission/revoke/automation_identities` + fluxos runtime
- [ ] **AG-12** `get_compat_gaps` — APIs não implementadas que o app tentou usar (+stack+module_id)
- [ ] **AG-13** `--describe-tools` — documentação auto-gerada de todas as ferramentas p/ qualquer LLM
- [ ] **AG-14** `list_components/launch_activity/start_service/send_broadcast/query_provider` — controle total dos 4 componentes
- [ ] **AG-15** `install_apk(path|url)` / `uninstall` / `clear_data` / `list_installed` — ciclo de vida de pacote completo
- [ ] **AG-16** `query_intent(action,uri)` — qual app/activity responderia ( teste de intents implícitos)
- [ ] **AG-17** `fake_notification(action)` — o agente "toca" uma notificação do app
- [ ] **AG-18** `eval_dex(method_ref, args)` — invocar método direto no app (teste white-box)
- [ ] **AG-19** `perf_trace` — timeline de frames/jank/startup exportável (Perfetto-compatible JSON)
- [ ] **AG-20** `batch(operations[])` — execução encadeada com parada condicional (economia de round-trips p/ LLM)

## 0.8 FERRAMENTAS DE TESTE & QA

- [ ] **TST-01** Suíte: `rustydroid test suíte.json` → JUnit XML + resumo humano + exit code correto
- [ ] **TST-02** Deep link/intent routing (tabela de casos) + App Links verification
- [ ] **TST-03** Matriz de permissões: grant/deny × fluxos críticos
- [ ] **TST-04** Config changes: rotação, dark mode, locale, pseudolocale, RTL, fontScale, screen size (device profiles), cutout/foldables
- [ ] **TST-05** Rede ruim: offline, timeout, 5xx, payload inválido, DNS fail, slow (via NET-03)
- [ ] **TST-06** Acessibilidade: content-desc, touch target ≥ 48dp, contraste, focus order, TalkBack-path simulation
- [ ] **TST-07** Performance: startup (cold/warm/hot), FPS, jank, RSS, GC pauses, battery-proxy (CPU-time), comparação entre runs (budgets)
- [ ] **TST-08** Resiliência: monkey prolongado, kill/restart do processo, storage cheio, memória baixa, relógio mudando
- [ ] **TST-09** Segurança: componentes exported audit, cleartext traffic, backup rules, permissões sobre-privilegiadas vs uso real (via AG-09)
- [ ] **TST-10** Dados: migração de DB, backup/restore round-trip, snapshot diff de prefs/DB antes/depois
- [ ] **TST-11** Visual regression: golden screenshots por device-profile/theme/locale
- [ ] **TST-12** Notificações/widgets/shortcuts: asserts estruturados sobre SYS-02/09/11

## 0.9 PERFORMANCE & OBSERVABILIDADE

- [ ] **PRF-01** Cache de resolução + fast-path strings + arena GC com pause < 2 ms
- [ ] **PRF-02** JIT Cranelift seletivo (métodos quentes) com fallback transparente
- [ ] **PRF-03** Flamegraph integrado + contadores de opcode + `rustydroid profile run`
- [ ] **PRF-04** Determinismo total: seed global, relógio virtual, scheduler reproduzível (replay exato)
- [ ] **PRF-05** Multi-app: 2+ apps simultâneos com IPC entre eles (intents/providers reais entre instâncias)
- [ ] **PRF-06** **Piso de performance Moto E5** (Apêndice A2): profile `moto-e5` com budgets de CPU/RAM/fill-rate/storage, throttling de agendador e **gate de release obrigatório**
- [ ] **PRF-07** **Comutadores de comportamento por targetSdk** (behavior changes 26→36): matriz `data/behavior-switches.toml` — o mesmo APK com target 26 vs 36 tem comportamentos distintos validados

## 0.10 DISTRIBUIÇÃO & PROJETO (GitHub)

- [ ] **GH-01** Monorepo Cargo + `xtask` + CI GitHub Actions completa (fmt/clippy/test/fuzz/bench/deny/audit/coverage/MSRV)
- [ ] **GH-02** Docs site (mdBook + GH Pages): guide, MCP tool ref auto-gerada, compat-matrix viva publicada a cada CI nightly
- [ ] **GH-03** Comunidade: README PT/EN, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY.md, templates de issue (bug/feature/**compat-gap**/perf), PR template, labels, Discussions, FUNDING
- [ ] **GH-04** Releases: semver, changelog (Keep a Changelog), cargo-dist/release-please, publish crates.io das libs, MSRV policy documentada
- [ ] **GH-05** Golden APK suite: coleção open-source licenciada por nível de features (com script de download, nunca commit binário no repo)
- [ ] **GH-06** Benchmark tracking contínuo (criterion + github-action-benchmark, regressão > 10% falha o CI)

---

## ⚙️ PRESETS

| Preset | Conteúdo | Uso |
|---|---|---|
| `MÍNIMO` | Só núcleo (0.0) | Smoke test de apps Java/Kotlin simples |
| `TESTE-DEV` ⭐ | Núcleo + LANG-01,10 + UI-01..04,06,07,09 + ST-01..03 + NET-01..03 + SYS-01..05,07 + AG-01..20 + TST-01..12 + PRF-01..07 + GH-01..04 | **Recomendado**: testar APK próprio de ponta a ponta |
| `COMPLETO` | Tudo (incl. experimentais: Flutter/RN/Unity/NDK/Compose) | Pesquisa & máxima compatibilidade |

**Formato de resposta:** `PRESET: TESTE-DEV` + linhas `+ ID, ID` (adicionar) / `- ID` (remover) + `Observações:` livre.

---

# 📜 PARTE 1 — PROMPT MESTRE v2.0 (enviar ao agente construtor)

```markdown
# PROJETO: RustyDroid — runtime Android em Rust, sem VM, com modo dedicado a agentes de IA

## PAPEL
Você é engenheiro de sistemas sênior em VMs de bytecode, formatos binários Android
(DEX/ARSC/AXML/ELF) e infraestrutura para agentes de IA. Vamos construir o
**RustyDroid**: um runtime que executa APKs Android REAIS em Linux x86_64 SEM máquina
virtual, SEM KVM, SEM kernel convidado — uma "Wine para Android" 100% Rust, com modo
de controle por agentes de IA via MCP. Este é um projeto open-source sério: o código
que você escreve vai para o GitHub com CI, docs e comunidade.

## MISSÃO
Dado um APK real, o RustyDroid deve:
1. Parsear (ZIP, AndroidManifest.xml binário, resources.arsc, classes*.dex)
2. Executar o bytecode Dalvik numa VM própria (interpretador → JIT Cranelift)
3. Implementar nativamente em Rust o subconjunto de android.* que o app usar
4. Rodar TODAS as linguagens possíveis num APK conforme checklist (Apêndice G)
5. Modelar o sistema completo: permissões (Apêndice B), componentes (Apêndice F),
   serviços de sistema, dispositivos virtuais (câmera/GPS/sensores/biometria)
6. Renderizar por software: headless (PNG/dump), janela (winit) e web (canvas)
7. Expor MCP (rmcp) + JSON-RPC + CLI com ~40 ferramentas de agente (Apêndice I)

## FONTES DE VERDADE (consulte antes de implementar cada domínio)
- Formatos: source.android.com (dex-format, apk signature scheme, arsc)
- APIs: developer.android.com/reference (pacote android.* — Apêndice C)
- Permissões: tabela integral do Apêndice B (é DATA no projeto: permissions.toml)
- Comportamento: developer.android.com/behavior-changes por targetSdk 26→36 (Apêndice A — behavior-switches.toml)
- Piso de hardware: profile `moto-e5` em data/devices.toml (Apêndice A2 — contrato de budgets)
- Compatibilidade: docs/compat-matrix.md é o placar oficial do projeto

## ESCALAÇÃO DO CHECKLIST (PRIMEIRO PASSO OBRIGATÓRIO)
1. Gere `rustydroid.manifest.json` com os módulos marcados (IDs, deps, features)
2. Só implemente/baixe o que está no manifest — nada além
3. Dependência faltando entre módulos → AVISE e proponha inclusão
4. Módulos não marcados = STUB que responde `NOT_IMPLEMENTED(module_id)` estruturado

## RESTRIÇÕES FÍSICAS (NÃO NEGOCIÁVEIS)
- Alvo: 2 vCPU x86_64, 2,7 GB RAM, ~1,1 GB disco, Debian 13, kernel 5.10, container
  (sem KVM/VMX/SVM/binder/ashmem/Wayland/GPU)
- SEM SO convidado; exceção única: interpretador ARM p/ libs .so (se LANG-02)
- Processo: < 800 MB RAM apps médios (modo normal); binário+deps < 200 MB
- App trivial: instalar+iniciar < 5 s; UI ≥ 20 FPS telas simples
- **Piso de API:** executa APKs com minSdk 26–36; APK com minSdk < 26 é parseado e
  inspecionado, mas a execução responde `BELOW_FLOOR(minSdk=xx)` estruturado
- **Modo piso E5 (PRF-06):** processo ≤ 512 MB (heap do app ≤ 256 MB, equiv.
  dalvik.vm.heapgrowthlimit de um device de 2 GB); telas simples ≥ 60 FPS / médias
  ≥ 30 FPS sob budget de fill-rate equivalente ao Adreno 308; **gate de release**

## ARQUITETURA — Cargo workspace (dependências só apontam para baixo)
```
rustydroid/
├── crates/
│   ├── rd-apk/        # ZIP, AXML, arsc, assinaturas, manifest model completo
│   ├── rd-dex/        # DEX 100% + disassembler + fuzz targets
│   ├── rd-vm/         # VM Dalvik completa + GC arena + threads
│   ├── rd-jni/        # JNI bridge (LANG-02/03/09): ABI estável, exceções, strings
│   ├── rd-ndk/        # ELF loader x86_64 + interpretador ARM64/32 (feature-gated)
│   ├── rd-engine-*/   # pontes por linguagem (flutter/rn/mono/wasm...) feature-gated
│   ├── rd-framework/  # dados/motores transversais + fd-api-coverage
│   │                  #   (Apêndice C); domínios fd-* mapeiam hoje para o
│   │                  #   host android.* em rd-vm até extração futura
│   ├── rd-render/     # headless/winit/web renderers + uiautomator dump + a11y tree
│   └── rd-agent/      # MCP (rmcp) + JSON-RPC + scenario DSL + coverage + batch
├── data/
│   ├── permissions.toml      # Apêndice B inteiro como dado versionado
│   ├── api-coverage.toml     # prefixos android.* → domínio/status/módulo (Apêndice C)
│   ├── devices.toml          # profiles de device (phone/tablet/foldable/tv/wear) — INCLUI piso: moto-e5 (Apêndice A2)
│   └── behavior-switches.toml # behavior changes por targetSdk 26→36 (PRF-07)
├── golden-apks/       # script de download de APKs OSS por feature-tier (GH-05)
├── docs/              # ROADMAP.md, compat-matrix.md, ADRs/, guide/ (mdBook)
├── xtask/             # bench, compat-report, golden-run, mcp-doc-gen
├── .github/           # workflows, ISSUE_TEMPLATE, labels, FUNDING
└── site/              # landing page (opcional)
```

## LEIS DO PROJETO (invioláveis)
1. **Sem falha silenciosa:** API ausente → `NOT_IMPLEMENTED(id)` estruturado + registro
   automático na compat-matrix
2. **Manifest é a verdade:** tudo que o app declara (componentes, permissões, features,
   intents) é modelado e consultável pelo agente desde a instalação
3. **Permissões são dados:** tabela integral em permissions.toml; o motor cobre os 3
   níveis de proteção + flags + roles + appops (Apêndice B) desde M0
4. **Determinismo disponível sempre:** qualquer run pode ser reproduzido com seed
5. **Compat > elegância:** em ambiguidade, escolha o que mantém o app real rodando
6. **Zero código copiado** de ATL/Robolectric (inspiração conceitual apenas)
7. **Feature-gates Everywhere:** cada módulo do checklist = cargo feature; build mínimo
   compila sem nenhum módulo opcional
8. **Piso E5 é contrato:** todo recurso precisa caber nos budgets do profile `moto-e5`
   (Apêndice A2); estourar budget = bug bloqueante de release, não otimização futura
9. **targetSdk comuta comportamento:** runtime aplica behavior changes por targetSdk
   (26→36) a partir de behavior-switches.toml — nunca "comportamento único fixo"

## ROADMAP POR FASES (código funcional a cada fase, nunca só teoria)
- M0 Fundação: workspace+CI(GH-01); rd-apk inspect completo; permissions.toml +
  devices.toml (profiles: phone/tablet/foldable/tv/**moto-e5**) + behavior-switches.toml +
  motor de permissões (Apêndice B) consultável via CLI
- M1 DEX 100%: parser+disassembler validados vs baksmali (3 APKs); fuzz limpo
- M2 VM mínima: opcodes núcleo, invocações, exceções, strings, fields; métodos puros
  de APK real executam corretos
- M3 Framework essencial: lifecycle, Handler/Looper, LayoutInflater XML real,
  Resources multi-config; app trivial roda headless de ponta a ponta
- M4 Render+dump: tiny-skia desenha; screenshot correto; get_ui_tree com ids certos
- M5 AGENTE v1: MCP com ~20 ferramentas (install/launch/input/ui_tree/screenshot/
  logs/assert/permissions/notifications)
- M6 Testes: run_scenario + JUnit XML + compat-matrix automatizada + coverage (AG-09)
- M7 Performance: caches, profile, multi-thread Looper, JIT seletivo; benches no CI;
  **gate piso E5**: release só publica com a suíte golden dentro dos budgets do
  profile moto-e5 (Apêndice A2)
- M8 Compat real: RecyclerView, Fragments, Dialogs, OkHttp shim, SQLite/Room,
  Services/broadcasts/deep links, WorkManager; app OSS médio navega funcional
- M9 Componentes avançados (SYS-09/10): widgets, IME, accessibility, notifications
  completas, shortcuts; NDK (LANG-02) com 5 APKs reais com .so rodando
- M10 Dispositivos virtuais (MEDIA-*): câmera/GPS/biometria programáveis via agente
- M11 Engines (LANG-04..08, na ordem de demanda): Flutter → RN → Mono → wasm
- M12 Ecossistema GH (GH-02..06): docs site, comunidade, releases, benchmark tracking,
  compat-matrix viva publicada
- A CADA FASE: ROADMAP.md + compat-matrix.md + worklog atualizados; DoD explícito

## DECISÕES TÉCNICAS JÁ TOMADAS (siga; divergir → escreva ADR antes)
- Dispatch loop com cache de instrução decodificada (fmt 10t..51l)
- GC: arena por thread + mark-sweep global simples (pausas < 2 ms)
- Thread/Looper/Handler → threads nativas + filas (sem async verde na UI)
- tiny-skia software (zero GPU); rustybuzz p/ text shaping; rmcp p/ MCP; tokio só no rd-agent
- Erros: thiserror nas libs, anyhow no binário; tracing JSON
- MIT OR Apache-2.0; MSRV estável documentado; sem unsafe fora de rd-ndk/rd-jni (auditado)

## COMO TRABALHAR
1. Confirme a fase antes de codar; uma fase por vez, testes verdes ao fim
2. Rust idiomático: clippy clean, sem unwrap em paths de usuário
3. Cada módulo: teste unitário + integração com fixture real
4. NUNCA simplifique "só uma interface": apps reais usam reflection/JNI/classes internas
5. Ao tocar permissões/APIs: atualize permissions.toml / api-coverage.toml (são dados!)
6. Módulos não selecionados = stubs NOT_IMPLEMENTED (nunca crash)

## INÍCIO
Comece AGORA pela M0: gere o manifest, o workspace, o CI e o `rd-apk inspect`
funcional sobre um APK real + permissions.toml carregado do Apêndice B.
Pergunte SOMENTE se um bloqueio físico impedir o progresso.
```

---

# 📚 PARTE 2 — MATRIZES DE REFERÊNCIA (o "tudo" que o RustyDroid cobre)

## Apêndice A — Versões Android suportadas (piso API 26 → teto 36)

**Piso de compatibilidade: `minSdkVersion 26` (Android 8.0 Oreo — Android de fábrica do Motorola Moto E5).**
**Teto: `targetSdkVersion` = última estável no momento do build (hoje: 36 — Android 16).**

| Tier | API levels | Android | Status no RustyDroid |
|---|---|---|---|
| T1 | 26–28 | 8.0–9 | **Piso obrigatório** — canais de notificação, autofill framework, PiP, background execution limits, adaptive icons, `invoke-custom` na VM (ART só suporta invokedynamic desde a API 26!) |
| T2 | 29–33 | 10–13 | Suporte completo — scoped storage, roles, bubbles, POST_NOTIFICATIONS runtime, exact alarms |
| T3 | 34–36 | 14–16 | Suporte evolutivo — foto picker, partial media access, Health Connect, 16K pages |

> APKs com **minSdk < 26** (ex.: legados de API 21): são parseados e inspecionados
> normalmente (M0), mas a execução responde `BELOW_FLOOR(minSdk=xx)` estruturado — nunca crash.
> Google Play exige **target API 36** a partir de 31/08/2026 — o parser normaliza
> targetSdk 26→36 e aplica **comutadores de comportamento por targetSdk** (behavior
> changes): scoped storage @29, roles @29, notifications runtime @33, exact alarms
> @31/33/34, foreground-service types @34, edge-to-edge @35, Health Connect @34…
> Matriz em `data/behavior-switches.toml` — consultável pelo agente (PRF-07).

### A2 — Piso de hardware: Motorola Moto E5 (perfil `moto-e5` — contrato de validação)

| Recurso | Moto E5 real | Tradução para o RustyDroid (profile) |
|---|---|---|
| CPU | Snapdragon 425 — 4× Cortex-A53 @ 1,4 GHz | orçamento de CPU: app executa com ≤ 1 core do host; single-thread calibrada ≈ 1/4 core (agendador + quota) |
| GPU | Adreno 308 (GLES 3.1) | renderer software com **budget de fill-rate ≈ Adreno 308** validado por overdraw sintético (sem GPU real no pipeline) |
| RAM | 2 GB (SO consome ~1 GB) | processo RustyDroid ≤ **512 MB** no modo piso; heap do app ≤ **256 MB** (equiv. `dalvik.vm.heapgrowthlimit` de um device de 2 GB) |
| Storage | 16–32 GB | VFS com quota configurável (8/16 GB) e taxas de I/O simuladas (eMMC lento) |
| Tela | 5,7" 1440×720, 18:9, ~282 ppi | device profile 720×1440, density `280dpi`/tvdpi-ish, sem cutout, sem notch |
| SO de fábrica | Android 8.0 (API 26) | comportamentos de sistema do profile fixados no nível 26, exceto o que o targetSdk do APK comutar (PRF-07) |

> **Gate de release (PRF-06):** toda release roda a suíte golden no profile `moto-e5`.
> Budgets: startup cold/warm por classe de app · FPS ≥ 60 (telas simples) / ≥ 30
> (médias) · RSS ≤ 512 MB · jank ≤ 1% em telas simples · sem OOM em heap 256 MB.
> **Estourou budget = release travada** (workflow de CI bloqueia a tag).

## Apêndice B — PERMISSÕES COMPLETAS (motor de permissões = dado versionado)

### B1. Níveis de proteção (base + flags)
- `normal` (concessão automática) · `dangerous` (runtime grant) · `signature` ·
  `signatureOrSystem` · `internal` (system-only)
- Flags: `runtime` (via dialog), `development`, `systemApp`, `appop`, `instant`,
  `revokeWhenRequested`, `role`, `softRestriction`, `hardRestricted`, `immutable`

### B2. Grupos DANGEROUS (runtime dialogs)
| Grupo | Permissões |
|---|---|
| LOCATION | ACCESS_FINE_LOCATION, ACCESS_COARSE_LOCATION, ACCESS_BACKGROUND_LOCATION |
| CAMERA | CAMERA |
| MICROPHONE | RECORD_AUDIO |
| CONTACTS | READ_CONTACTS, WRITE_CONTACTS, GET_ACCOUNTS |
| PHONE | READ_PHONE_STATE, READ_PHONE_NUMBERS, CALL_PHONE, ANSWER_PHONE_CALLS, READ_CALL_LOG, WRITE_CALL_LOG, ADD_VOICEMAIL, USE_SIP |
| SMS | SEND_SMS, RECEIVE_SMS, READ_SMS, RECEIVE_WAP_PUSH, RECEIVE_MMS |
| STORAGE/MEDIA | READ_EXTERNAL_STORAGE, WRITE_EXTERNAL_STORAGE, READ_MEDIA_IMAGES, READ_MEDIA_VIDEO, READ_MEDIA_AUDIO, READ_MEDIA_VISUAL_USER_SELECTED, ACCESS_MEDIA_LOCATION |
| SENSORS/BODY | BODY_SENSORS, BODY_SENSORS_BACKGROUND, ACTIVITY_RECOGNITION, HIGH_SAMPLING_RATE_SENSORS |
| CALENDAR | READ_CALENDAR, WRITE_CALENDAR |
| NEARBY_DEVICES | BLUETOOTH_SCAN, BLUETOOTH_CONNECT, BLUETOOTH_ADVERTISE, NEARBY_WIFI_DEVICES, UWB_RANGING |
| NOTIFICATIONS | POST_NOTIFICATIONS |
| HEALTH | READ_HEALTH_DATA, READ_HEALTH_DATA_IN_BACKGROUND, WRITE_HEALTH_DATA (Health Connect, API 34) |

### B3. Especiais (special app access — via AppOps/settings, não dialog comum)
SYSTEM_ALERT_WINDOW · WRITE_SETTINGS · MANAGE_EXTERNAL_STORAGE · REQUEST_INSTALL_PACKAGES ·
REQUEST_DELETE_PACKAGES · PACKAGE_USAGE_STATS · ACCESS_NOTIFICATION_POLICY ·
SCHEDULE_EXACT_ALARM / USE_EXACT_ALARM · USE_FULL_SCREEN_INTENT · PICTURE_IN_PICTURE ·
START_FOREGROUND_SERVICES_FROM_BACKGROUND · BIND_ACCESSIBILITY_SERVICE (lado serviço) ·
IGNORE_BATTERY_OPTIMIZATIONS · QUERY_ALL_PACKAGES

### B4. NORMAL (seleção — ~90 no total; tabela completa em permissions.toml)
INTERNET, ACCESS_NETWORK_STATE, ACCESS_WIFI_STATE, CHANGE_WIFI_MULTICAST_STATE, BLUETOOTH(legado),
BLUETOOTH_ADMIN(legado), VIBRATE, WAKE_LOCK, RECEIVE_BOOT_COMPLETED, FOREGROUND_SERVICE(+14 tipos:
camera, microphone, location, mediaPlayback, mediaProjection, dataSync, phoneCall,
remoteMessaging, health, specialUse, connectedDevice, shortService, systemExempted (API 34) e
mediaProcessing (API 35)), NFC, TRANSMIT_IR, USE_BIOMETRIC/USE_FINGERPRINT,
SET_ALARM, SET_WALLPAPER(+HINTS), EXPAND_STATUS_BAR, GET_PACKAGE_SIZE, KILL_BACKGROUND_PROCESSES,
MODIFY_AUDIO_SETTINGS, READ_SYNC_SETTINGS/STATS, WRITE_SYNC_SETTINGS, REORDER_TASKS,
INSTALL_SHORTCUT/UNINSTALL_SHORTCUT, BROADCAST_STICKY, NFC_TRANSACTION_EVENT,
REQUEST_COMPANION_RUN_IN_BACKGROUND/START_FOREGROUND/CREATE_USER_SESSION/COMPANION_APPROVE_WIFI_CONNECTIONS,
MANAGE_OWN_CALLS, ACCEPT_HANDOVER, WRITE_VOICEMAIL, BROADCAST_SMS/WAP_PUSH, USE_SIP(alt),
DISABLE_KEYGUARD, REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, UPDATE_PACKAGES_WITHOUT_USER_ACTION,
DELIVER_COMPANION_MESSAGES…

### B5. SIGNATURE / system (implementadas como "concedidas" para o app em análise)
> **Regra (issue #110):** a lista abaixo é uma **whitelist**. Somente as
> permissões dela são auto-concedidas ao app em análise (`auto-granted`), com
> o marcador de confiança `runtime-is-system` (o runtime se posiciona como
> system). Toda permission **signature-class fora da whitelist** recebe estado
> `denied_system`: o Android real a nega a apps de terceiros (ex.:
> `BATTERY_STATS` = signature|privileged|development), e a sessão **não** a
> concede — `grant()`/`deny()` a preservam. No dado, a whitelist é a flag
> `b5` em `data/permissions.toml`; o motor decide por ela em `initial_state`.

BIND_* (ACCESSIBILITY, DEVICE_ADMIN, INPUT_METHOD, NOTIFICATION_LISTENER, TELECOM_CONNECTION,
VOICE_INTERACTION, VPN, WALLPAPER, DREAM, APPWIDGET, PRINT, MIDI, HOST_APDU, AUTOFILL,
CALL_REDIRECTION, SCREENING, QUICK_ACCESS_WALLET…) · WRITE_SECURE_SETTINGS ·
READ_LOGS · DUMP · SET_DEBUG_APP · ACCESS_MOCK_LOCATION(dev) · READ_FRAME_BUFFER ·
CLEAR_APP_CACHE · MANAGE_DOCUMENTS · GLOBAL_SEARCH · INSTALL_LOCATION_PROVIDER ·
LOCATION_HARDWARE · CONTROL_LOCATION_UPDATES · DEVICE_POWER · REBOOT · FACTORY_TEST

### B6. Roles (API 29+) e AppOps
- Roles: DIALER, SMS, EMERGENCY, HOME, BROWSER, MUSIC, GALLERY, ASSISTANT,
  VOICE_INTERACTION, CALL_SCREENING, CALL_COMPANION, DIRECTORY_PROVIDER, NOTES(35)
- AppOps simuladas: CAMERA/MICROPHONE toggle, GET_USAGE_STATS, SYSTEM_ALERT_WINDOW,
  TAKE_AUDIO_FOCUS, PLAY_AUDIO, VIBRATE…

> O motor cobre: declaração no manifest → estado de concessão → fluxos runtime →
> revogação por unused-app → grupos → "don't ask again" → justificativas API 34+
> (photo picker partial access). Tudo programável pelo agente (AG-11).

## Apêndice C — Superfície android.* por domínio (api-coverage.toml)

A fonte executável deste apêndice é [`data/api-coverage.toml`](data/api-coverage.toml), validada por `rd-framework::fd_api_coverage` e consumida por `rd api-coverage`, pelo MCP (`get_api_usage`/`compat_gaps`) e pelos relatórios gerados. O instantâneo atual registra **106 superfícies em 17 domínios** (supported 4, partial 29, stub 14, not_implemented 59). A tabela abaixo é **gerada** por `cargo xtask api-coverage-spec`; o relatório completo sai em [`docs/api-coverage.md`](docs/api-coverage.md) (`cargo xtask api-coverage-report`). O programa de conclusão literal dos campos `partial` — critério de pacote completo, bloqueadores e ordem de execução — vive em [`docs/appendix-c-literal-completion.md`](docs/appendix-c-literal-completion.md); os Rounds 1–10 foram mergeados pelo PR #172 sem promovê-los automaticamente. Status é conservador: `supported` cobre só o subconjunto documentado da superfície, nunca um pacote AOSP inteiro; `partial` executa subconjunto real; `stub` é hierarquia/fallback; `not_implemented` continua `NOT_IMPLEMENTED(module_id)`.

<!-- BEGIN API COVERAGE -->
| Domínio | Superfícies/pacotes principais | Status das superfícies | Módulos |
|---|---|---|---|
| App/lifecycle | `android.app`, `android.app.admin`, `android.app.backup`, `android.app.job`, `android.app.slice`, `android.content`, `android.content.pm`, `android.content.res` | partial 8 | `rd-vm/android-pm`, `rd-vm/android-res`, `rd-vm/android-stub-surfaces`, `rd-vm/framework`, `rd-vm/m8` |
| View/UI | `android.view`, `android.view.accessibility`, `android.view.animation`, `android.view.inputmethod`, `android.view.textservice`, `android.view.translation`, `android.webkit`, `android.widget` | partial 8 | `rd-vm/android-stub-surfaces`, `rd-vm/android-sysservices`, `rd-vm/m9`, `rd-vm/view-layout` |
| Graphics | `android.graphics`, `android.graphics.drawable`, `android.graphics.pdf`, `android.opengl`, `android.renderscript` | partial 5 | `rd-vm/android-stub-surfaces`, `rd-vm/graphics` |
| Text | `android.text`, `android.text.format`, `android.text.method`, `android.text.style`, `android.text.util` | partial 5 | `rd-vm/android-sysservices`, `rd-vm/android-text`, `rd-vm/graphics` |
| OS/IPC | `android.os`, `android.os.Binder`, `android.os.Build`, `android.os.Bundle`, `android.os.Parcel`, `android.os.Parcelable`, `android.util` | supported 2 · partial 5 | `rd-vm/android-build`, `rd-vm/android-util`, `rd-vm/bundle`, `rd-vm/framework`, `rd-vm/parcel` |
| Database | `android.database`, `android.database.sqlite`, `android.provider`, `android.provider.Settings` | supported 1 · partial 3 | `rd-vm/android-stub-surfaces`, `rd-vm/content`, `rd-vm/jsqlite` |
| Net | `android.net`, `android.net.Uri`, `android.net.http`, `android.net.ipsec.ike`, `android.net.nsd`, `android.net.sip`, `android.net.wifi` | supported 1 · partial 6 | `rd-vm/android-stub-surfaces`, `rd-vm/android-uri`, `rd-vm/framework` |
| Media | `android.media`, `android.media.audiofx`, `android.media.browse`, `android.media.midi`, `android.media.projection`, `android.media.session`, `android.media.tv` | partial 7 | `rd-vm/android-stub-surfaces` |
| Camera | `android.hardware.Camera`, `android.hardware.camera2`, `android.hardware.camera2.params` | partial 3 | `rd-vm/android-stub-surfaces` |
| Hardware | `android.hardware`, `android.hardware.biometrics`, `android.hardware.display`, `android.hardware.fingerprint`, `android.hardware.hdmi`, `android.hardware.input`, `android.hardware.soundtrigger`, `android.hardware.usb`, `android.location` | partial 9 | `rd-vm/android-display`, `rd-vm/android-stub-surfaces`, `rd-vm/android-sysservices` |
| Telephony | `android.telecom`, `android.telephony`, `android.telephony.CellBroadcastIdRange`, `android.telephony.euicc`, `android.telephony.gsm`, `android.telephony.mbms` | partial 6 | `rd-vm/android-stub-surfaces` |
| Connectivity | `android.bluetooth`, `android.bluetooth.le`, `android.companion`, `android.nfc`, `android.nfc.cardemulation`, `android.nfc.tech` | partial 6 | `rd-vm/android-stub-surfaces` |
| Security | `android.se.omapi`, `android.security`, `android.security.identity`, `android.security.keystore`, `android.service.credentials` | partial 5 | `rd-vm/android-stub-surfaces` |
| Speech/ML | `android.service.voice`, `android.speech`, `android.speech.tts`, `android.view.textclassifier` | partial 4 | `rd-vm/android-stub-surfaces` |
| Services ext | `android.service.autofill`, `android.service.carrier`, `android.service.chooser`, `android.service.dreams`, `android.service.media`, `android.service.notification`, `android.service.quicksettings`, `android.service.textservice`, `android.service.wallpaper` | partial 9 | `rd-vm/android-stub-surfaces`, `rd-vm/m9` |
| System UI | `android.accessibilityservice`, `android.app.usage`, `android.appwidget`, `android.inputmethodservice`, `android.print` | partial 5 | `rd-vm/android-stub-surfaces`, `rd-vm/m9` |
| Misc | `android.accounts`, `android.animation`, `android.gesture`, `android.icu`, `android.mtp`, `android.preference`, `android.system`, `android.transition` | partial 8 | `rd-vm/android-stub-surfaces`, `rd-vm/android-sysservices`, `rd-vm/framework`, `rd-vm/intrinsics` |
<!-- END API COVERAGE -->

Normalização dos atalhos históricos da spec no registro: conceitos que não são pacotes (`intent`, `broadcast`, `service`, `display`, `surface`) viram APIs-chave dentro do pacote/classe real; cada prefixo tem um único domínio dono (`android.content.res` → App/lifecycle, `android.location` → Hardware, `android.service.credentials` → Security); `android.service.dream` é grafado como o pacote AOSP `android.service.dreams`; `android.exif` é a classe `android.media.ExifInterface`, sob `android.media`. Toda mudança de API Android deve atualizar `data/api-coverage.toml` no mesmo PR e regenerar spec/relatórios; tipo `android.*` sem linha no registro é gap explícito, nunca suporte implícito.

## Apêndice D — AndroidX/Jetpack (shims por módulo)

A fonte executável deste apêndice é [`data/jetpack.toml`](data/jetpack.toml), validada por `rd-framework::fd_jetpack` e consumida por `rd jetpack`, pelo MCP e pelos relatórios gerados. A tabela abaixo é **gerada** por `cargo xtask jetpack-spec`; o relatório completo sai em [`docs/jetpack.md`](docs/jetpack.md) (`cargo xtask jetpack-report`); contexto e ledger em [`docs/appendix-d-e.md`](docs/appendix-d-e.md). Cada módulo declara estratégia — `dex-passthrough` (o DEX da biblioteca executa na VM), `host-shim` (host intercepta métodos listados; resto cai no DEX) ou `host-native` — e status conservador: `supported` é só o subconjunto documentado, nunca a biblioteca inteira. Room segue via ST-02, work via SYS-04, navigation via UI-04, datastore via ST-01, media3/ExoPlayer via MEDIA-01/02, cameraX via MEDIA-03, biometric via MEDIA-05, security-crypto via ST-04, okhttp/retrofit/moshi/gson via NET-01 e coil/glide/picasso via UI-02; hilt/dagger é reflection-free init e kotlinx-coroutines/flow mapeia em threads. Firebase não é registrado aqui: vive no Apêndice E (prefixo tem um único dono). Toda mudança de shim AndroidX deve atualizar `data/jetpack.toml` no mesmo PR e regenerar spec/relatórios.

<!-- BEGIN JETPACK -->
| Categoria | Bibliotecas (implementação nativa ou shim) | Status das bibliotecas | Módulos |
|---|---|---|---|
| Foundation | appcompat, core-ktx, activity, annotation, multidex, startup | partial 4 · not_implemented 2 | `none`, `rd-vm/androidx`, `rd-vm/classpath+inflate`, `rd-vm/m9` |
| Architecture | lifecycle-runtime, lifecycle-livedata, lifecycle-viewmodel, lifecycle-savedstate, room, paging 3, work, navigation, datastore, collection, palette | partial 9 · not_implemented 2 | `none`, `rd-vm/androidx`, `rd-vm/m8` |
| UI | compose, recyclerview, viewpager2, constraintlayout, coordinatorlayout, drawerlayout, swiperefreshlayout, cardview, fragment, browser (custom tabs), emoji2, material (extra além da tabela da spec) | partial 3 · stub 7 · not_implemented 2 | `none`, `rd-vm/androidx`, `rd-vm/classpath+inflate`, `rd-vm/m8` |
| Behavior | media3/ExoPlayer, cameraX, biometric, security-crypto, credential-manager, photo-picker (privacy) | partial 1 · not_implemented 5 | `none`, `rd-vm/androidx` |
| DI/async | hilt/dagger, kotlinx-coroutines/flow, rxjava bridge | not_implemented 3 | `none` |
| 3rd-party comuns | okhttp, retrofit, moshi, gson, coil, glide, picasso, guava ListenableFuture (extra — base de work/camera) | partial 2 · not_implemented 6 | `none`, `rd-vm/androidx`, `rd-vm/m8` |
<!-- END JETPACK -->

## Apêndice E — Google Play Services / Firebase (modo "sem GMS" honesto)

- Estratégia: **GMS shim** — APIs presentes, respostas mock/estruturadas programáveis pelo agente
- Location (fused) → MEDIA-05 GPS virtual · Maps → UI-07 render de tiles estáticos (offline)
- Billing → mock de purchases (fluxo completo testável) · Auth → provedores fake
- Ads → eventos estruturados sem rede · Fit/Health → dados fake programáveis
- Cast/Nearby → NET-08 · SafetyNet/Integrity → resposta determinística fake
- Firebase: Analytics (eventos capturados), Messaging (SYS-08), Crashlytics (crashes reais capturados!), Firestore/RTDB → NET-03 record/replay

A fonte executável deste apêndice é [`data/gms.toml`](data/gms.toml), validada por `rd-framework::fd_gms` e consumida por `rd gms`, pelo MCP e pelos relatórios gerados. O modo honesto é **validado, não só documentado**: `mode = "no-gms-honest"` e `network = "none"` em todo serviço — não há processo GMS, conta Google nem backend real; status mede o shim local para o subconjunto documentado, jamais paridade com os servidores do Google. A tabela abaixo é **gerada** por `cargo xtask gms-spec`; o relatório completo (incl. o contrato programável de cada serviço) sai em [`docs/gms.md`](docs/gms.md) (`cargo xtask gms-report`); contexto e ledger em [`docs/appendix-d-e.md`](docs/appendix-d-e.md). Toda mudança de shim GMS/Firebase deve atualizar `data/gms.toml` no mesmo PR e regenerar spec/relatórios.

<!-- BEGIN GMS -->
| Categoria | Serviços (shim sem GMS) | Status dos serviços | Módulos |
|---|---|---|---|
| GMS core | GoogleApiAvailability / common, Tasks API | partial 2 | `rd-vm/gms` |
| Play Services | Location (fused), Maps, Billing, Auth (Google Sign-In), Ads, Fit/Health, Cast, Nearby, SafetyNet / Play Integrity | partial 9 | `rd-vm/gms` |
| Firebase | Firebase core (FirebaseApp), Firebase Analytics, Firebase Auth, Firebase Messaging (FCM), Firebase Crashlytics, Cloud Firestore, Realtime Database, Firebase Remote Config (extra além da spec), Firebase Storage (extra além da spec), ML Kit (extra além da spec) | partial 9 · not_implemented 1 | `none`, `rd-vm/gms` |
<!-- END GMS -->

## Apêndice F — Componentes & Manifest (modelo completo — CORE-12)

| Tipo | Elemento | Cobertura |
|---|---|---|
| 4 componentes | activity, activity-alias, service, receiver, provider | ciclo completo + permissões de chamada + exported audit |
| Extensões | appwidget-provider, wallpaper, dream, ime, accessibility-service, notification-listener, device-admin, voice-interaction, print-service, autofill-service, host-apdu, offline-device, call-redirection/screening, carrier-config, media-browser, credentials | SYS-10 |
| Manifest | uses-permission(-sdk-23), permission(-group/tree), uses-feature(→stub se ausente), uses-configuration, supports-screens/compatible-screens, supports-gl-texture, uses-library/native-library, application(+dataExtractionRules/allowBackup/usesCleartextTraffic/networkSecurityConfig), instrumentation(→hooks de teste), meta-data, queries(<intent/signature/provider> — package visibility API 30!), original-package, adopt-permission | CORE-01/12 |

## Apêndice G — TODAS as linguagens que podem gerar um APK (registro de engines)

<!-- BEGIN ENGINE REGISTRY -->
| # | Linguagem/Framework | Artefato no APK | Módulo | Estratégia | Status |
|---|---|---|---|---|---|
| 1 | Java | classes*.dex | LANG-01 | VM DEX própria | Registro ✅; runtime DEX parcial |
| 2 | Kotlin | classes*.dex | LANG-01 | VM DEX; coroutines/threads dependem da cobertura da VM | Registro ✅; runtime DEX parcial |
| 3 | Kotlin Multiplatform / Compose MP | DEX; recursos Compose; klib nem sempre é distribuída | LANG-10 | Shared DEX + futuro render Compose MP | Registro ✅; runtime NOT_IMPLEMENTED |
| 4 | Scala / Clojure / Groovy | classes*.dex | LANG-01 | VM DEX; invocações dinâmicas conforme cobertura | Registro ✅; runtime DEX parcial |
| 5 | C (NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 6 | C++ (NDK) | lib/<abi>/*.so; STL opcional | LANG-02 | ELF + JNI + STL | Registro ✅; runtime ELF/JNI parcial (native) |
| 7 | Rust (cargo-ndk/xbuild) | lib/<abi>/*.so | LANG-03 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 8 | Go (gomobile) | lib/<abi>/*.so; wrappers bind | LANG-09 | ELF + JNI + gomobile bind | Registro ✅; runtime NOT_IMPLEMENTED |
| 9 | Objective-C/C (cross via NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI; runtime ObjC se necessário | Registro ✅; runtime ELF/JNI parcial (native) |
| 10 | Zig / outras linguagens LLVM | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 11 | Dart / Flutter | libapp.so; libflutter.so; flutter_assets | LANG-04 | Dart AOT + engine Flutter headless futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 12 | JS/TS (React Native / Expo) | bundle JS; Hermes bytecode/JSC; libs RN | LANG-05 | Engine JS + RN/Expo bridge futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 13 | HTML/JS (Cordova / Capacitor / Ionic) | assets/www/ ou assets/public/; configs bridge | LANG-06 | WebView headless + bridges futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 14 | HTML/JS (TWA / PWA) | Manifest/URL; pode não haver payload web local | LANG-06 | WebView/TWA futuro; rede não consultada | Registro ✅; runtime NOT_IMPLEMENTED |
| 15 | JS/TS (NativeScript) | assets/app/; libNativeScript.so | LANG-05+LANG-02 | Engine JS + JNI + views futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 16 | JS/TS (Tauri v2) | assets web + Rust .so; markers Tauri opcionais | LANG-06+LANG-03 | WebView + Rust JNI futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 17 | C# (.NET MAUI / Xamarin) | assemblies/*.dll; Mono; blobs de assemblies | LANG-07 | Mono/IL + bridges futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 18 | C# (Unity Mono) | Assembly-CSharp.dll; libunity.so | LANG-08 | Mono + stubs/render Unity futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 19 | C# (Unity IL2CPP) | libil2cpp.so; global-metadata.dat | LANG-08 | Interp nativo + stubs/render Unity futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 20 | C++ (Unreal) | libUE4.so / libUnreal.so; pak/obb | LANG-09 | Engine/stubs Unreal futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 21 | GDScript/C# (Godot 3/4) | libgodot*.so; .pck/assets | LANG-09 | Godot headless embed futuro | Registro ✅; runtime NOT_IMPLEMENTED |
| 22 | Lua (Defold / Solar2D / Cocos2d-x) | Lua source/bytecode; assets; libs engine | LANG-09 | Lua embed + bridges específicos futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 23 | Python (Kivy / Buildozer / Chaquopy / BeeWare) | Python source/bytecode; libs; payloads compactados | LANG-09 | CPython/PyO3 + bridges específicos futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 24 | Haxe (OpenFL / Heaps) | DEX ou lib/<abi>/*.so | LANG-01+LANG-02 | VM DEX ou ELF/JNI; bridge Haxe futuro | Registro ✅; runtime NOT_IMPLEMENTED |
| 25 | Qt / QML (C++) | libQt*.so; QML assets/recursos | LANG-09 | Qt headless + JNI futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 26 | Avalonia (C#) | assemblies Avalonia; runtime managed | LANG-07 | Mono + render próprio futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 27 | B4X / App Inventor / Thunkable | classes*.dex | LANG-01+LANG-11 | VM DEX; fixtures específicas futuras | Registro ✅; runtime DEX parcial |
| 28 | WebAssembly in-app | assets/*.wasm (ou containers opacos) | LANG-12 | Runtime WASM + bridge JNI/JS futuros | Registro ✅; runtime NOT_IMPLEMENTED |
<!-- END ENGINE REGISTRY -->

**Estado verificável (2026-10-14):** registro e detecção dos **28 itens concluídos**;
execução de novos runtimes **não incluída**. DEX/ELF compartilhados são candidatos,
não identificação da linguagem de origem. Features `lang-01`…`lang-12` selecionam
contratos e dependências, não implementam engines. A seleção de runtimes da PARTE 0
permanece inalterada. A VM possui DEX/Compose Android e ELF/JNI (feature native) parciais; isso não implementa adaptadores do registro, Compose MP ou Android/NDK completo. M11 permanece pendente.

Fonte da tabela: [`data/engines.toml`](data/engines.toml), validada pela crate
`rd-engine-registry`. `cargo xtask engine-spec` regenera apenas a tabela marcada;
`cargo xtask engine-report` gera [regras e contratos](docs/engine-registry.md).
CLI: `rd engine list|detect|check`; `inspect --json` e MCP `app_info` incluem
`engines`. Detecção não valida payload nem autoriza execução (ADR 003).

> **Lei do registro:** cada engine nova = crate `rd-engine-<nome>` feature-gated, com
> contract próprio: `parse(artifact) → process`, `syscall/event → engine`, teste golden
> por engine. Nenhuma engine quebra as outras. Adaptadores ainda ausentes retornam
> `NOT_IMPLEMENTED(LANG-xx)` em parse/event/syscall, mesmo com a feature habilitada.
> Golden de detecção não satisfaz o golden flow de execução nem o gate Moto E5.

## Apêndice H — Catálogo de testes (o que "testar um APK" significa aqui)

1. **UI funcional:** fluxos por run_scenario (tap/type/assert/wait)
2. **Estrutural:** componentes exported, intents filtros, deep links, queries package-visibility
3. **Permissões:** matriz grant/deny/revogação × fluxos; 3 níveis de proteção
4. **Config:** rotação/dark/locale/pseudolocale/RTL/fontScale/devices profiles/foldables
5. **Rede:** offline/timeout/5xx/payload/HAR replay/latência
6. **Perf:** startup cold/warm, FPS, jank, RSS, GC, battery-proxy; budgets com CI fail
7. **Resiliência:** monkey seeded, kill/restart, storage cheio, OOM simulado, clock skew
8. **Segurança:** cleartext, backup rules, sobre-permissão (declared vs used — AG-09), keystore flows
9. **Dados:** migração Room/SQLite, backup/restore, snapshot diffs
10. **A11y:** árvore a11y, touch targets, contraste, focus order, screen-reader path
11. **Visual:** golden screenshots por profile (UI-11 diff)
12. **Integração de sistema:** notificações/widgets/shortcuts/broadcasts/providers entre 2 apps (PRF-05)
13. **White-box:** eval_dex (AG-18) para chamadas diretas; coverage por método
14. **Multi-idioma-engine:** APK híbrido (ex.: Kotlin + .so Rust + Hermes) com coverage unificada
15. **Piso E5 (PRF-06/07):** suíte golden roda no profile `moto-e5` com budgets de RAM/frame/startup/I-O; behavior switches por targetSdk verificados (mesmo APK com target 26 vs 36 valida comportamentos distintos)

## Apêndice I — Ferramentas MCP do agente (resumo operacional)

Grupos: **pacote** (install/launch/components/query_intent), **input** (AG-01), **observação**
(ui_tree/screenshot/logs/hierarchy/a11y), **assert** (AG-03 + wait_for), **estado**
(snapshot/restore/clear_data), **dispositivos virtuais** (AG-10 inject_*/set_*),
**permissões** (AG-11), **notificações** (AG-17), **white-box** (AG-18 eval_dex),
**qualidade** (coverage/api_usage/compat_gaps/perf_trace), **automação** (run_scenario/
monkey/batch), **meta** (describe_tools/health). Contrato: entrada/saída 100% JSON,
erros `{code, cause, suggestion, module_id}`, idempotência quando possível, `batch()`
para economizar round-trips de LLM.

---

# 🏗️ PARTE 3 — PLANO GITHUB (projeto sério desde o dia 1)

## 3.1 Estrutura do repositório
```
rustydroid/                    # monorepo Cargo (workspace)
├── crates/…                   # ver arquitetura na PARTE 1
├── data/                      # permissions.toml, api-coverage.toml, devices.toml, behavior-switches.toml
├── golden-apks/  (script apenas — bins nunca commitados; GH-05)
├── docs/  (mdBook: guide + ADRs + ROADMAP + compat-matrix gerada)
├── xtask/                     # compat-report, mcp-doc-gen, golden-run, bench
├── .github/
│   ├── workflows/ (ci.yml, fuzz.yml, nightly-compat.yml, release.yml, docs.yml)
│   ├── ISSUE_TEMPLATE/ (bug_report.yml, feature_request.yml, compat_gap.yml — perf-regression.yml planejado)
│   └── PULL_REQUEST_TEMPLATE.md (FUNDING.yml planejado; labels.json vive em docs/labels.json — issue #38)
├── CONTRIBUTING.md  SECURITY.md  docs/labels.json (sync das labels out-of-band — issue #38)
├── README.md (PT)  LICENSE (LICENSE-MIT + LICENSE-APACHE)
└── ⚠️ §3.1 descreve a estrutura ALVO (M12): SUPPORT.md, GOVERNANCE.md, CODE_OF_CONDUCT.md,
    README.en.md, CHANGELOG.md, FUNDING.yml e workflows adicionais ainda são PLANEJADOS
```

## 3.2 CI/CD (GitHub Actions)
| Workflow | Gatilho | Conteúdo |
|---|---|---|
| ci.yml | PR/push | fmt, clippy `-D warnings`, test (unit+integration), MSRV check, cargo-deny (licenças/advisories), llvm-cov→Codecov |
| fuzz.yml | nightly | cargo-fuzz nos parsers (dex/axml/arsc/zip/elf) + corpus persistido |
| nightly-compat.yml | cron | roda golden APK suite → publica compat-matrix.md + métricas no GH Pages (badge viva) |
| bench.yml | push main | criterion + github-action-benchmark (fail em regressão > 10%) |
| release.yml | tag v* | cargo-dist (bins linux), release-please (changelog/semver), publish crates.io (libs), docs deploy |

## 3.3 Governança & versões
- **Semver:** 0.x até M8; 1.0 = contrato MCP + CLI estáveis + compat-matrix Tier-1 ✅
- **MSRV:** documentado, testado no CI; semver nas libs do workspace
- **compat-gap issues:** label dedicada — o relatório do AG-12 abre issue automaticamente (via xtask) com API, stack e APK-fonte
- **ADR:** toda decisão relevante = `docs/adr/NNN-titulo.md` (contexto/opções/escolha/consequências)
- **Marcos públicos:** milestones = fases M0–M12; Projects board com colunas por fase
- **Badges:** CI, coverage, compat-matrix Tier, crates.io, MSRV, license

---

# 🗺️ PARTE 4 — ROADMAP CONSOLIDADO (fases = milestones do GitHub)

| Fase | Entrega central | DoD (done quando…) |
|---|---|---|
| M0 | Workspace+CI+rd-apk+permissions.toml+devices.toml(**moto-e5**)+behavior-switches.toml+motor permissões | inspect de APK real imprime componentes/permissões/features completos; CI verde |
| M1 | rd-dex 100% + disassembler | idêntico ao baksmali em 3 APKs; fuzz 24h sem crash |
| M2 | VM Dalvik mínima | métodos puros de APK real retornam valores corretos |
| M3 | Framework essencial | app trivial (activity+botão+texto) roda headless ponta a ponta |
| M4 | Render+UI dump | screenshot correto; ui_tree com resource-ids certos |
| M5 | Agente MCP v1 (~20 tools) | LLM instala, navega e assera estado só com as tools |
| M6 | Executor de testes | suíte JSON → JUnit XML; compat-matrix automatizada |
| M7 | Performance + **gate piso E5** | startup < 2 s app trivial; benches com gate no CI; suíte golden passa nos budgets moto-e5 (A2) |
| M8 | Compat real (views/rede/dados/serviços) | app OSS médio navega funcional; **v0.9** |
| M9 | Componentes avançados + NDK (LANG-02) | 5 APKs com .so rodando; widgets/IME/notificações ok |
| M10 | Dispositivos virtuais (MEDIA-*) | câmera/GPS/biometria programáveis pelo agente |
| M11 | Engines (Flutter→RN→Mono→wasm) | 1 app de cada engine rodando golden flow |
| M12 | Ecossistema GH completo | docs site, compat-matrix viva, releases automáticas; **v1.0** |

---

# 🧾 APÊNDICE Z — Template de resposta do checklist

```text
PRESET: <MÍNIMO | TESTE-DEV | COMPLETO>
+ <IDs para adicionar>          # ex.: LANG-02, MEDIA-03, MEDIA-05, NET-06
- <IDs para remover>            # ex.: TST-06
Observações: <livre — ex.: "APK Kotlin + Compose, usa câmera, GPS e Health Connect">
```

**Dicas de decisão:**
- APK nativo Kotlin/Java → `PRESET: TESTE-DEV` já basta
- APK com código nativo (jogos, crypto, ffmpeg) → `+ LANG-02`
- **Jetpack Compose** → `+ UI-05` (prioriza runtime Compose)
- Testar câmera/GPS/biometria → `+ MEDIA-03, MEDIA-05`
- Testar notificações/widgets → `+ SYS-02, SYS-09, SYS-11`
- Testar telefonia/SMS/BLE/NFC → `+ NET-05, NET-06`
- Flutter/RN/Unity → `+ LANG-04/05/08` (caminho longo — me avise para replanejar M11)
