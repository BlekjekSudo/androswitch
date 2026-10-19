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
| AI-agent control plane | adb, screen scraping | **MCP + 39 structured tools** (JSON-RPC próprio — ADR 001) |
| Perf baseline | modern desktop | **Motorola Moto E5 (2 GB RAM)** floor |

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
| **M5** | ✅ **DONE** — MCP agent v1 (`rd mcp`, JSON-RPC stdio, **30 tools**: pacote (install/list/remove/clear/launch), input (tap/swipe/type/key), observação (ui_tree/screenshot/logs), wait/wait_for + asserts, permissões, runtime_health, compat_gaps, batch idempotente, sandbox+caps de segurança, `--describe-tools` + docs geradas) |
| **M6** | ✅ **DONE** — test executor: `rd test suite.json` (cenários JSON, exit codes, **JUnit XML**), compat matrix gerada do roadmap + suítes |
| **M7** | 🟡 **IN PROGRESS** — `rd perf` mede startup cold/warm + RSS no perfil moto-e5 com gate de CI sobre o app trivial (números emulados por fator de CPU); budgets da golden suite, medição em device real e gate de release ainda pendentes |
| **M8** | ✅ **DONE (v0.9.0)** — Real-world compat: RecyclerView (DEX adapter populate), Fragments (legacy+androidx), Dialogs, OkHttp shim (offline deterministic), SQLite/Room (in-memory), Services/broadcasts/deep links, WorkManager, stateful SharedPreferences — `crates/rd-vm/src/m8.rs`, ADR 003 |
| **M9** | ✅ **DONE (review pending)** — widgets (RemoteViews/AppWidgetProvider), IME (InputConnection/InputMethodService), accessibility, full notifications, shortcuts + NDK: ELF loader (x86_64/AArch64/ARM) and JNI bridge running `.so` code in an isolated Unicorn worker (feature `native`). 5 real F-Droid APKs make JNI method calls from their real DEX, each with a checked result, on x86_64, arm64-v8a and armeabi-v7a (15/15 runs), e.g. zxing-cpp renders a QR code that zbar decodes — [docs/m9.md](docs/m9.md), ADR 004 |
| **M10** | Virtual devices: camera, GPS, sensors, biometrics — programmable by the agent |
| **M11** | Engines: Flutter → React Native → Mono → WASM |
| **M12** | Full GitHub ecosystem: docs site, live compat matrix, releases → **v1.0** |

**Compatibility floor:** `minSdkVersion 26` (Android 8.0 Oreo) · **ceiling:** latest stable (`targetSdk 36`).
APKs below the floor are inspected fine but refuse to run with a structured `BELOW_FLOOR` error — never a crash.

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
package            org.fdroid.fdroid  v2.0.1 (2000051)
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
│   ├── rd-engine-*/   # language bridges (flutter/rn/mono/wasm…) feature-gated (M11)
│   ├── rd-framework/  # android.* natively in Rust: fd-permissions ✅, fd-devices ✅, fd-behavior ✅
│   ├── rd-render/     # headless / window / web renderers + UI dump     (M4)
│   ├── rd-agent/      # MCP (JSON-RPC próprio, ADR 001) + suite executor `rd test` (M6) + coverage · 39 tools
│   └── rd-cli/        # the `rd` binary: inspect / perm / device / behavior / dex / vm / app / mcp / test / perf
├── data/              # permissions.toml (175), devices.toml (6, incl. moto-e5), behavior-switches.toml (25) · api-coverage.toml (planned)
├── golden-apks/       # open-source APK test suite (download script, no binaries)
├── docs/              # ROADMAP.md, compat-matrix.md (generated by xtask), ADRs
└── xtask/             # cargo xtask compat-report (live compatibility scoreboard)
```

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
software e um **modo dedicado a agentes de IA (MCP)** com 31 ferramentas estruturadas.

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
