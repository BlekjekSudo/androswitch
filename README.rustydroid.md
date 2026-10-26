<p align="center">
  <img src="logo.png" alt="RustyDroid" width="180" />
</p>

<h1 align="center">🦀 RustyDroid</h1>

<p align="center">
  <strong>Rust-native Android runtime — run real APKs on Linux.<br/>No VM. No KVM. No guest kernel.</strong><br/><br/>
  <em>Runtime Android em Rust — executa APKs reais no Linux.<br/>Sem VM. Sem KVM. Sem kernel convidado.</em>
</p>

<p align="center">
  <a href="https://github.com/nadabgfccvd/RustyDroid/actions/workflows/ci.yml"><img src="https://github.com/nadabgfccvd/RustyDroid/actions/workflows/ci.yml/badge.svg" alt="CI"/></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0"/>
  <img src="https://img.shields.io/badge/spec-v2.1-8A2BE2" alt="Spec v2.1"/>
  <img src="https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey" alt="Platform: Linux x86_64"/>
  <img src="https://img.shields.io/badge/API-26%20%E2%86%92%2036-brightgreen" alt="Android API 26→36"/>
  <img src="https://img.shields.io/badge/built%20with-AI%20assistance-FF6F00" alt="Built with AI assistance"/>
</p>

---

## ✨ What is RustyDroid?

RustyDroid is a **"Wine for Android"**: instead of emulating an entire Android OS
(QEMU-style, slow without KVM), it parses APKs directly and executes them natively:

- 📦 **Parses** the APK (ZIP, binary `AndroidManifest.xml`, `resources.arsc`, `classes.dex`)
- ⚙️ **Executes** Dalvik bytecode in a **Rust VM** (interpreter → Cranelift JIT later)
- 🧩 **Implements** `android.*` framework APIs natively in Rust
- 🎨 **Renders** the UI in software (tiny-skia): headless PNG, desktop window, web canvas
- 🤖 **Drives everything through MCP** — built for AI agents from day one

### Why it's different

| | Traditional emulator | **RustyDroid** |
|---|---|---|
| Needs KVM/VMX | ✅ required for speed | ❌ never |
| Guest OS in RAM | 2–4 GB | **0** (no guest OS) |
| Boot time | minutes | **seconds** (no boot at all) |
| Runs in a plain container | rarely | ✅ designed for it |
| AI-agent control plane | adb, screen scraping | **MCP + 47 structured tools** + subscribable screen resources + live viewer (JSON-RPC próprio — ADR 001) |
| Perf baseline | modern desktop | **Motorola Moto E5 (2 GB RAM)** floor |

### 👀 Agents see the screen without screenshots

RustyDroid renders every frame itself, so it knows what is on screen *as
data*. `observe` returns the visible texts, tap targets and icons, derived
from the recorded display list and standard accessibility providers. Compose
`contentDescription` labels name icon-only targets too. Cached observations
cost under 1 ms in desktop runs; a fresh frame executes app drawing and
semantics and can take seconds during navigation. `tap_text "Continuar"`
taps by label. MCP resources `rustydroid://screen` (and `.txt`, `.png`) can
be subscribed to and push `notifications/resources/updated` on every real
change. For humans, `rd mcp --live 0.0.0.0:7878` serves a browser page with
the live frame and tap-target overlay. See [docs/mcp-observe.md](docs/mcp-observe.md).

### ♻️ Garbage-collected app heap

The app heap (default 256 MB budget) uses non-moving generational mark/sweep:
between MCP tools, in long waits, and **inside long Java calls**. Live-frame
roots, host-temporary protection and remembered old writes keep collection
safe; WeakHashMap keys are weak. `runtime_health` reports minor/major cycles,
weak-key clearing, heap peaks and pauses. Device budgets still need Moto E5
validation. See [docs/gc.md](docs/gc.md).

### ⚙️ Experimental integer leaf JIT

Cranelift is optional and **off by default**: build `rd-cli` with `--features
jit`, then set `RD_JIT=on`. Only hot static straight-line integer leaf methods
compile; everything else stays interpreted, with fuel enforcement unchanged.
This first backend is tested on x86_64, not an ARMv7/Moto E5 JIT claim.
See [ADR 007](docs/adr/007-cranelift-leaf-jit.md).

## C/C++/Rust APKs and full MCP execution (opt-in)

`cargo build -p rd-cli --features native-engines` enables three real, bounded
source-language-selected native adapters. Source-built Android APKs, all 47 MCP
tools, x86_64/ARM64/ARMv7 matrices, and limitations are documented in
[Real native-language execution](docs/native-languages.md). These are genuine
project-owned Android demonstration APKs with native cores and Java UI glue,
not claims of universal NDK/Rust support or third-party APK compatibility.

## Reliable MCP automation and diagnostics

See [the automation guide](docs/mcp-automation.md) for structured host-view selectors, guarded taps, presence/absence waits, session diagnostics, incremental byte-exact log reads and typed error snapshots. The original tool definitions remain intact; the live catalog now has 47 tools.


### 🤖 Built with AI — heavily

> **This project is developed with heavy AI assistance** — specification, architecture,
> code and CI. Every change passes through review gates (tests, fuzzing, benchmarks)
> before landing. We believe in being transparent about it: **AI-assisted, human-directed.**

---

## 🗺️ Roadmap (M0 → M12)

| Phase | Delivers |
|---|---|
| **M0** | ✅ **DONE** — workspace + CI + APK inspector + permission engine (full Android permission matrix as data) |
| **M1** | ✅ **DONE** — DEX parser 100% + disassembler (validated against baksmali 2.5.2, fuzzed) |
| **M2** | ✅ **DONE** — minimal Dalvik interpreter: **150/150 golden vectors PASS vs real JVM execution** + real APK pure methods verified |
| **M3** | ✅ **DONE** — Essential framework: Activity lifecycle, Handler/Looper, LayoutInflater, Resources |
| **M4** | ✅ **DONE** — software rendering determinístico + UI dump uiautomator-compatible |
| **M5** | ✅ **DONE** — MCP agent v1 (`rd mcp`, JSON-RPC stdio, **30 tools**: pacote (install/list/remove/clear/launch), input (tap/swipe/type/key), observação (ui_tree/screenshot/logs), wait/wait_for + asserts, permissões, runtime_health, compat_gaps, batch idempotente, sandbox+caps de segurança, `--describe-tools` + docs geradas; the live catalog has since grown to **47 tools**) |
| **M6** | ✅ **DONE** — test executor: `rd test suite.json` (cenários JSON, exit codes, **JUnit XML**), compat matrix gerada do roadmap + suítes |
| **M7** | 🟡 **IN PROGRESS** — `rd perf` mede startup cold/warm + RSS no perfil moto-e5 com gate de CI sobre o app trivial (números emulados por fator de CPU); budgets da golden suite, medição em device real e gate de release ainda pendentes |
| **M8** | ✅ **DONE (v0.9.0)** — Real-world compat: RecyclerView (DEX adapter populate), Fragments (legacy+androidx), Dialogs, OkHttp shim (offline deterministic), SQLite/Room (in-memory), Services/broadcasts/deep links, WorkManager, stateful SharedPreferences — `crates/rd-vm/src/m8.rs`, ADR 003 |
| **M9** | ✅ **DONE (review pending)** — widgets (RemoteViews/AppWidgetProvider), IME (InputConnection/InputMethodService), accessibility, full notifications, shortcuts + NDK: ELF loader (x86_64/AArch64/ARM) and JNI bridge running `.so` code in an isolated Unicorn worker (feature `native`). 5 real F-Droid APKs make JNI method calls from their real DEX, each with a checked result, on x86_64, arm64-v8a and armeabi-v7a (15/15 runs), e.g. zxing-cpp renders a QR code that zbar decodes — [docs/m9.md](docs/m9.md), ADR 004 |
| **M10** | ✅ **DONE** — virtual devices programmable by the agent: camera, GPS, sensors, biometrics and the other M10 surfaces ([docs/m10.md](docs/m10.md), [ADR 011](docs/adr/011-m10-virtual-devices.md), #201) |
| **M11** | ⬜ **PLANNED** — Engines, in the order accepted by [ADR 013](docs/adr/013-m11-engine-execution-strategy.md): WASM → React Native → Mono → Flutter (one real app each in a golden flow; detection alone never counts) |
| **M12** | Full GitHub ecosystem: docs site, live compat matrix, releases → **v1.0** |

**Compatibility floor:** `minSdkVersion 26` (Android 8.0 Oreo) · **ceiling:** latest stable (`targetSdk 36`).
APKs below the floor are inspected fine and reported with a structured `BELOW_FLOOR` finding — never a crash. (Known gap, 2026-10-25: `rd app run` does not yet refuse below-floor APKs; the fix is in progress.)

---

## 🚧 Status — M0→M6, M8, M9 landed · M7 started (2026-10)

The foundation is **real, tested code** — verified end-to-end against the F-Droid client APK (973 zip entries, 36 components, 31 permissions):

- ✅ `rd inspect <apk>` — full manifest decode: identity, SDKs + **API-floor check** (`BELOW_FLOOR`), features, all 4 component types + intent filters, resolved labels (chained resource refs), signing schemes v1/v2/v3, dex/native/asset inventory
- ✅ **Permission engine** over versioned data: **175 permissions** (12 dangerous groups, 15 special-access, 36 signature…), install → grant/deny/revoke state machine + static audit (`exported` without guard, debuggable, cleartext…)
- ✅ **Device profiles** incl. the contractual **`moto-e5` hardware floor** (RSS ≤ 512 MB, app heap ≤ 256 MB, 25% single-thread)
- ✅ **Behavior switches** per targetSdk 26→36 (25 entries, PRF-07)
- ✅ Structured errors everywhere: `{code, cause, suggestion, module_id}` — never a silent failure (unimplemented modules answer `NOT_IMPLEMENTED`). **Exit codes (contrato público para agents): 0 ok · 1 erro · 2 NOT_IMPLEMENTED · 4 erro VM (`rd vm exec`) · 64 usage.**
- ✅ **`rd dex` (M1)** — 100% DEX parser (header/map/MUTF-8 strings/types/protos/fields/methods/classes/code/debug/annotations/call-sites/method-handles) + **smali disassembler validated against baksmali 2.5.2: 24.913 classes across 3 golden APKs, 100% identical instruction sequences** + fuzz targets for every binary parser (ZIP/AXML/ARSC/DEX)
- ✅ **`rd vm` (M2)** — minimal Dalvik interpreter over rd-dex: core opcodes with exact Java semantics (wrapping, saturating casts, NaN/Infinity, `MIN/-1` wrapping div), exceptions (try/catch on any instruction, platform exceptions materialized), strings/StringBuilder intrinsics, static fields + `<clinit>`, virtual dispatch through superclass chains, **heap arena capped at the moto-e5 floor (256 MB, typed `VM_OOM`)**, fuel + call-depth guards. **Golden harness `golden/vm/`: 150/150 vectors byte-identical to real JVM execution** (javac→D8→`rd vm exec --json` vs reflection runner). Real APK pure methods executed and hand-verified against smali (androidx ContainerHelpers/IntIntPair/ScatterMap)
- ✅ **`rd test` (M6)** — test executor: JSON suite de cenários → resumo humano + **JUnit XML** (`--junit`) + `--json`; contrato de saída `0` tudo passou · `1` houve falha · `64` suíte inválida/IO; cada cenário roda numa sessão de agente nova (isolamento). A compat-matrix (`docs/compat-matrix.md`) é gerada por `cargo xtask compat-report` a partir do ROADMAP + suítes em `tests/suites/` (formato: [`docs/suite-format.md`](docs/suite-format.md))
- 🟡 **`rd perf` (M7, primeiro passo)** — gate de performance no perfil `moto-e5`: startup cold/warm (`launch → primeiro frame`), CPU por thread (`schedstat`), RSS de pico (`VmHWM`) e heap do app (marca d'água da arena da VM, `heap_peak_bytes`); orçamentos startup < 2 s emulados, RSS ≤ 512 MB e heap ≤ 256 MB (fail-closed). Gate de CI exige `pass: true` sobre o app trivial — **estimativa por fator de CPU** (host ≠ Moto E5); golden suite e device real pendentes

```console
$ rd inspect golden-apks/org.fdroid.fdroid.apk
package            org.fdroid.fdroid  v2.0.2 (2000052)
label              F-Droid (resolved @0x7f110045)
SDK                min 24 · target 37 · compile 37
api floor          ⚠ BELOW_FLOOR(minSdk=24, floor=26) — inspect ok; execução responde erro estruturado
signing            v1, v2, v3
permissions (31 declaradas; tabela: 175 permissões)
  auto-granted: 14 · runtime-pending: 7 · special: 5 · denied-system: 0 · unknown: 0 · not requested: 3 · app-defined: 2
    (not requested = fora da janela do targetSdk: maxSdkVersion vencido ou since_api acima do alvo)
components (36: activity×10 · activity-alias×3 · provider×4 · receiver×9 · service×10)
```

APK de referência: **F-Droid 2.0.1** (`v2.0.1 · 2000051`), baixado por `./golden-apks/fetch.sh` — `sha256 83d3fe522281c3cb89fce3bc05038f81b3c3b32c108536941f184c5f9bb53778`. Bloco regenerado em **2026-10-12** a partir do binário desta árvore; `cargo test -p rd-cli --test readme_inspect` falha se o exemplo divergir da saída real (o teste é ignorado quando o APK golden não está presente).

Build it yourself: `cargo build -p rd-cli` → `./target/debug/rd inspect your.apk`.

APKs with duplicate ZIP entries (two `classes.dex`, for instance) are **accepted but flagged**: the inventory counts each name once and `inspect` prints a `zip duplicates` warning (plus a `zip_duplicates` key in `--json`); reads follow the first occurrence. Policy and rationale: [ADR 002](docs/adr/002-zip-duplicate-entries.md).

---

## 📦 Repository layout

```
rustydroid/
├── crates/
│   ├── rd-apk/        # ZIP (+ZIP64, CRC), AXML, resources.arsc, signatures, manifest model
│   ├── rd-dex/        # DEX 100% + disassembler + fuzz targets        (M1)
│   ├── rd-vm/         # Dalvik VM in Rust (interpreter → Cranelift JIT) (M2)
│   ├── rd-jni/        # JNI mangling, descriptors, MUTF-8, ref frames   (M9)
│   ├── rd-ndk/        # ELF loader x86_64/AArch64/ARM + isolated worker (M9)
│   ├── rd-engine-registry/ # Appendix G: 28 records + static detection (not execution)
│   ├── rd-engine-*/   # language bridges (flutter/rn/mono/wasm…) feature-gated (M11)
│   ├── rd-framework/  # versioned framework data/engines: fd-permissions ✅, fd-devices ✅, fd-behavior ✅, fd-api-coverage ✅, fd-jetpack ✅, fd-gms ✅
│   ├── rd-render/     # headless / window / web renderers + UI dump     (M4)
│   ├── rd-agent/      # MCP (JSON-RPC próprio, ADR 001) + suite executor `rd test` (M6) + coverage · 47 tools · screen model, resources, live viewer
│   └── rd-cli/        # the `rd` binary: inspect / perm / device / behavior / api-coverage / jetpack / gms / engine / dex / vm / app / mcp / test / perf
├── data/              # permissions (175), devices (6), behavior-switches (25), engines (28), api-coverage (17 domains, 106 surfaces), jetpack (46 libraries), gms (21 services)
├── golden-apks/       # open-source APK test suite (download script, no binaries)
├── docs/              # ROADMAP.md, compat-matrix.md + api-coverage.md (generated by xtask), ADRs
└── xtask/             # cargo xtask compat-report/api-coverage-* (generated scoreboards)
```

## 🗂️ Appendix C — Android API surface registry

The `android.*` surface is versioned data in
[`data/api-coverage.toml`](data/api-coverage.toml): every Appendix C domain maps
package/class prefixes to the real implementing module and an honest status
(`supported` subset, `partial`, `stub`, or `not_implemented`). The current
registry snapshot has **106 surfaces in 17 domains**: 4 supported subsets
and 102 partial (0 stub, 0 not implemented). `partial` means a documented,
tested subset is modeled — **no** surface claims that the whole AOSP package
runs, and this is not execution coverage. The deep-audit series merged on
2026-10-25 ([#196](https://github.com/nadabgfccvd/RustyDroid/pull/196),
[#200](https://github.com/nadabgfccvd/RustyDroid/pull/200)) re-checked modeled
subsets against AOSP source and fixed real ordering/validation divergences.

```sh
rd api-coverage check --json
rd api-coverage show android.database.sqlite.SQLiteDatabase --json
rd api-coverage list --domain view-ui --status partial
```

MCP `get_api_usage` classifies an installed app's static DEX references against
the same registry (`api_coverage` in the result); `compat_gaps` includes its
domain/status summary, while `get_coverage` remains the dynamic execution
counter. Regenerate the human report and spec table with
`cargo xtask api-coverage-report`, `cargo xtask api-coverage-spec` and
`cargo xtask compat-report`; see the [generated API coverage report](docs/api-coverage.md)
and the [literal completion ledger](docs/appendix-c-literal-completion.md) for
the merged Rounds 1–10 checkpoints and what still blocks promoting any
`partial` package.

## 📚 Appendices D + E — Jetpack and Play Services registries

AndroidX/Jetpack libraries and Google Play Services/Firebase are versioned data
too ([ADR 012](docs/adr/012-appendix-d-e-registries.md)):

- [`data/jetpack.toml`](data/jetpack.toml): **46 libraries** in 6 categories, each
  with a strategy (`dex-passthrough`, `host-shim`, `host-native`) and an honest
  status — currently 18 partial, 2 stub, 26 not implemented. Libraries travel
  in the APK and their DEX runs on the VM; stateful host shims (lifecycle,
  LiveData, ViewModel, collections, DataStore values, WorkManager `Data`,
  navigation, startup, biometric, futures) intercept only the listed methods.
- [`data/gms.toml`](data/gms.toml): **21 Play Services/Firebase services**, all
  `not_implemented` — an explicit **"no GMS" mode**: calls fail typed
  (`NOT_IMPLEMENTED`/`NOT_FOUND`) instead of faking success.

```sh
rd jetpack list --json
rd gms check --json
```

Generated reports: [docs/jetpack.md](docs/jetpack.md), [docs/gms.md](docs/gms.md);
ledger: [docs/appendix-d-e.md](docs/appendix-d-e.md). Regenerate with
`cargo xtask jetpack-report` and `cargo xtask gms-report`.

## 🧩 Appendix G — engine registry (detection only)

All **28 languages/frameworks** are registered in [`data/engines.toml`](data/engines.toml).
The registry reports artifact/manifest evidence, module dependencies, feature
selection and the **actual** runtime status:

```sh
rd engine list --json
rd engine detect app.apk --json
rd engine check flutter --json  # exit 2: NOT_IMPLEMENTED(LANG-04)
rd inspect app.apk --json       # includes the same engines report
```

MCP `app_info` also includes `engines`, without launching the app or adding a new
tool. Shared DEX/ELF formats are **candidates**, not proof of Java/Kotlin/C/Rust;
framework **signatures** are name/manifest heuristics, not binary validation.
Hybrid APKs may match multiple engines. No assets/libraries are loaded or executed.

The catalog works with **no optional features**. Opt-in `lang-01`…`lang-12`
features select contracts/dependencies, **not runtime implementations**. Existing
DEX/Compose Android and opt-in three-ABI ELF/JNI routes remain partial; they are
not the registry adapters or full Android/NDK support. Flutter, React Native,
Mono, WASM and game-engine adapters remain pending (M11, execution plan in
[ADR 013](docs/adr/013-m11-engine-execution-strategy.md)). Existing CLI/agent
`advanced-components` defaults and opt-in `native`/`jit` features are preserved.

See the [generated registry](docs/engine-registry.md),
[ADR 003](docs/adr/003-appendix-g-engine-registry.md) and
[current integration validation](docs/appendix-g-integration.md) and
[historical branch validation](docs/appendix-g-validation.md).
Regenerate with `cargo xtask engine-report`, `cargo xtask engine-spec` and
`cargo xtask compat-report`; tests reject catalog/spec/scoreboard drift.

---

## 📄 Full specification

Everything above is specified in detail in **[`RustyDroid.md`](./RustyDroid.md)** (v2.1):

- **~85 selectable modules** with installation checklist & presets
- **Complete Android permissions matrix** (protection levels, groups, roles, AppOps)
- **`android.*` API surface** mapped by domain + Jetpack/Play Services strategy
- **28 languages/engines** that can live inside an APK — and how each one runs
- **GitHub project plan**: CI/CD, governance, issue templates, release automation

## 🤝 Contributing

PRs and issues are welcome! Start with [`CONTRIBUTING.md`](./CONTRIBUTING.md).
**Found an API that doesn't work?** Open a `compat-gap` issue — it is designed to feed the compatibility scoreboard (`docs/compat-matrix.md` is generated by `cargo xtask compat-report` from the ROADMAP + test suites since M6; automatic issue→scoreboard ingestion is planned for a later milestone).

## 🔒 Security

See [`SECURITY.md`](./SECURITY.md).

## ⚖️ License

Dual-licensed under [`MIT`](./LICENSE-MIT) OR [`Apache-2.0`](./LICENSE-APACHE) — your choice.

---

<details>
<summary>🇧🇷 Sobre (PT-BR)</summary>

**RustyDroid** é uma "Wine para Android": em vez de emular um sistema Android inteiro
(lento sem KVM), ele parseia APKs diretamente e executa o bytecode Dalvik numa **VM
escrita em Rust**, com as APIs `android.*` implementadas nativamente, renderização por
software e um **modo dedicado a agentes de IA (MCP)** com 47 ferramentas estruturadas.

- **Piso de performance:** Motorola Moto E5 (Snapdragon 425, 2 GB RAM) — todos os
  budgets de performance são calibrados nele, não em emuladores modernos
  (`rd perf` mede startup e RSS contra esses orçamentos, com gate de CI)
- **Piso de API:** `minSdkVersion 26` · **teto:** `targetSdk 36`
- **Testes de cenário:** `rd test suite.json` executa cenários JSON contra o
  agente e emite JUnit XML (M6)
- **Desenvolvido pesadamente com auxílio de IA** — especificação, código e CI, com
  revisão humana e gates de teste
- A especificação completa (v2.1) está em [`RustyDroid.md`](./RustyDroid.md)

</details>

---

<p align="center">
  <sub>🦀 RustyDroid — Android without Android. Made with 🤖 AI assistance + ☕ human review.</sub>
</p>
