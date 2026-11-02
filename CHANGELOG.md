# Changelog

All notable changes to **RustyDroid** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(see [`GOVERNANCE.md`](./GOVERNANCE.md) for pre-1.0 milestone versioning and MSRV policy).

## [Unreleased]

### Added

- **M10 Virtual Devices (`#201`, [ADR 011](./docs/adr/011-m10-virtual-devices.md), [`docs/m10.md`](./docs/m10.md)):**
  Programmable deterministic virtual camera (synthetic RGBA frames + Camera2/CameraX shims),
  GPS/Location (NMEA + GPX/waypoints), sensors (accelerometer, gyroscope, light, proximity,
  rotation vector, step counter), biometrics/fingerprint (`BiometricPrompt` + `FingerprintManager`),
  media/audio/MediaSession (`AudioManager`, `AudioRecord` PCM waveforms, `MediaPlayer` virtual clock),
  and network/connectivity/system configuration (`ConnectivityManager`, `WifiManager`, `BluetoothAdapter`,
  `BatteryManager`, locale, night mode, font scale, orientation) controllable via MCP `virtual_device`.
- **Appendices D & E Executable Registries (`#197`, `#198`, [ADR 012](./docs/adr/012-appendix-d-e-registries.md)):**
  Versioned TOML catalogs (`data/jetpack.toml` — 46 AndroidX/Jetpack libraries; `data/gms.toml` —
  21 Google Play Services / Firebase services in honest "no-GMS" mode), `cargo xtask` report/spec
  generators ([`docs/jetpack.md`](./docs/jetpack.md), [`docs/gms.md`](./docs/gms.md)), CLI `rd jetpack` /
  `rd gms` subcommands, and initial AndroidX runtime shims (`crates/rd-vm/src/androidx.rs`).
- **M11 Engine Execution Strategy (`#199`, [ADR 013](./docs/adr/013-m11-engine-execution-strategy.md)):**
  Accepted execution roadmap ordering (`WASM → React Native → Mono → Flutter`), MSRV 1.76 constraints,
  documented framework bridge contract, and golden-flow evidence gates.
- **LazyArsc Architecture (`#202`, [ADR 014](./docs/adr/014-lazyarsc-lazystringpool.md)):**
  Design for on-demand `resources.arsc` and `StringPool` decoding within Moto E5 budgets.
- **Appendix C Deep AOSP Audit Series (`#192`, `#196`, `#200`, `#204`):**
  Modeled-subset AOSP conformance verification, edge-case fixes, and regression suites across 43+
  `android.*` surfaces ([`docs/api-coverage.md`](./docs/api-coverage.md),
  [`docs/appendix-c-literal-completion.md`](./docs/appendix-c-literal-completion.md)).
- **Runtime & Engine Detection Fixes (`#205`, `#207`):**
  `Context.getAssets()` / `AssetManager` reading real APK `assets/` entries, and `rd-engine-registry`
  detection rules for real Tauri (`libtauri_android.so` / `libwry.so`), Defold (`libdmengine.so`),
  and TWA (`<application>` `<meta-data>` `asset_statements`) builds.
- **M12 Community, Governance & Docs Site Foundation (`GH-02`, `GH-03`, `GH-04`):**
  [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md), [`GOVERNANCE.md`](./GOVERNANCE.md),
  [`SUPPORT.md`](./SUPPORT.md), [`README.en.md`](./README.en.md), `CHANGELOG.md`,
  [`.github/ISSUE_TEMPLATE/perf-regression.yml`](./.github/ISSUE_TEMPLATE/perf-regression.yml),
  [`.github/FUNDING.yml`](./.github/FUNDING.yml), and `mdBook` configuration (`docs/book.toml`,
  `docs/SUMMARY.md`, `docs/index.md`).

## [0.9.0] - 2026-10-21

### Added

- **M9 Advanced Components & Isolated NDK Execution ([ADR 004](./docs/adr/004-m9-native-isolation.md), [`docs/m9.md`](./docs/m9.md)):**
  - `SYS-09` Notifications (`NotificationChannel`, `Builder`, `POST_NOTIFICATIONS` API 33+, `PendingIntent` mutability, `NotificationListenerService`), IME (`InputConnection`, `InputMethodService`), and AppWidgets (`AppWidgetProvider`, `RemoteViews`).
  - `SYS-10` Accessibility (`AccessibilityService`, node trees, actions) and Shortcuts (`ShortcutManager`).
  - `LANG-02` Bounded ELF32/64 loader (`x86_64`, `AArch64`, `ARM`) + JNI table (`JNIEnv`/`JavaVM`, `RegisterNatives`, `JNI_OnLoad`) executing guest `.so` libraries inside an isolated Unicorn subprocess worker (`crates/rd-ndk`), verified across 5 real F-Droid APKs × 3 ABIs (`15/15` checks) plus source-built C/C++/Rust demonstration APKs ([`docs/native-languages.md`](./docs/native-languages.md)).
- **M8 Real-World Compatibility Host ([ADR 003](./docs/adr/003-m8-compat-host.md)):**
  Selective framework interception (`crates/rd-vm/src/m8.rs`) for `RecyclerView`, legacy + AndroidX `FragmentManager`, `AlertDialog`, deterministic offline `OkHttpClient`, in-memory + isolated subprocess `SQLite`/`Room` ([ADR 005](./docs/adr/005-sqlite-subprocess.md)), `Services`/`BroadcastReceiver`/deep links/`JobScheduler`, `WorkManager`, and stateful `SharedPreferences`.
- **Generational GC & Live-Frame Roots ([ADR 006](./docs/adr/006-generational-gc-and-live-frame-roots.md), [`docs/gc.md`](./docs/gc.md)):**
  Non-moving generational mark/sweep collector with remembered-set write barriers, weak `WeakHashMap` keys, and mid-call collection within the 256 MB Moto E5 heap cap.
- **Experimental Cranelift JIT ([ADR 007](./docs/adr/007-cranelift-leaf-jit.md)):**
  Opt-in `--features jit` (`RD_JIT=on`) backend in `rd-jni`/`rd-vm` with fuel parity and interpreter fallback.
- **Large-APK & Cold-Start Architecture ([ADR 008](./docs/adr/008-15gb-apk-and-1500ms-cold-start.md), [ADR 009](./docs/adr/009-process-cold-26ms.md)):**
  Bounded `pread64` `PagedZip` LRU cache, `< 26 ms` snapshot-backed warm/live startup, and `rd perf` phase telemetry against the Motorola Moto E5 floor (`1,500 ms` emulated cold-start budget).
- **RD Sentinel Anti-Regression Bot ([ADR 010](./docs/adr/010-rd-sentinel-anti-regression-bot.md), [`docs/rd-sentinel.md`](./docs/rd-sentinel.md)):**
  Automated diff-risk scoring, static safety invariants, protected-path enforcement, fuzz/mutation/perf campaigns, and SARIF reporting.
- **Expanded MCP Control Plane ([`docs/mcp-observe.md`](./docs/mcp-observe.md), [`docs/mcp-automation.md`](./docs/mcp-automation.md)):**
  Screen observation without screenshots (`observe`, `tap_text`, subscribable `rustydroid://screen` resources, `--live` browser viewer), guarded selectors, incremental logs, and typed diagnostics across 47 tools.
- **Appendix G Engine Registry ([ADR 003](./docs/adr/003-appendix-g-engine-registry.md), [`docs/engine-registry.md`](./docs/engine-registry.md)):**
  28 stable language/engine records (`data/engines.toml`), `rd engine list|detect|check`, and honest `NOT_IMPLEMENTED` execution boundaries.

## [0.6.0] - 2026-10-12

### Added

- **M6 Test Executor ([`docs/suite-format.md`](./docs/suite-format.md)):** `rd test suite.json` scenario runner with JUnit XML (`--junit`), JSON reports, isolated agent sessions, and `cargo xtask compat-report` scoreboard generation ([`docs/compat-matrix.md`](./docs/compat-matrix.md)).
- **M5 MCP Agent Server v1 ([ADR 001](./docs/adr/001-mcp-sem-rmcp-msrv.md), [`docs/mcp-tools.md`](./docs/mcp-tools.md)):** Zero-dependency JSON-RPC 2.0 `rd mcp` server over stdio with package management, input injection, UI tree/screenshot inspection, permissions, batch execution, and `--describe-tools`.
- **M4 Software Renderer & UI Dump:** Deterministic `tiny-skia` software rendering (`crates/rd-render`), PNG encoding, and `uiautomator`-compatible accessibility/UI hierarchy dumps.
- **M3 Essential Android Framework:** `Activity` lifecycle (`onCreate`/`onStart`/`onResume`), `Looper`/`Handler`, binary XML `LayoutInflater`, and `Resources` resolution.
- **M2 Dalvik Bytecode VM:** Pure-Rust Dalvik interpreter (`crates/rd-vm`) passing `150/150` JVM golden vectors, exception tables, virtual dispatch, fuel budgets, and 256 MB arena `VM_OOM` enforcement.
- **M1 Complete DEX Parser & Disassembler:** Fuzz-hardened `rd-dex` parser + `smali` disassembler validated against `baksmali 2.5.2` across 24,913 classes, plus `cargo-fuzz` targets ([`docs/FUZZING.md`](./docs/FUZZING.md)).
- **M0 Workspace, APK Inspector & Permission Engine:** Cargo workspace, `rd-apk` (ZIP, AXML, ARSC, v1/v2/v3 signing, duplicate-entry handling per [ADR 002](./docs/adr/002-zip-duplicate-entries.md)), `data/permissions.toml` (175 Android permissions), `data/devices.toml` (`moto-e5` floor), `data/behavior-switches.toml` (API 26→36), and GitHub Actions CI.
