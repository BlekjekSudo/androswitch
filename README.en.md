<p align="center">
  <img src="logo.png" alt="RustyDroid" width="180" />
</p>

<h1 align="center">🦀 RustyDroid (English Guide)</h1>

<p align="center">
  <strong>Rust-native Android runtime — run and inspect real APKs on Linux.<br/>No VM. No KVM. No guest kernel.</strong><br/><br/>
  <em>🇧🇷 Bilingual main README: <a href="./README.md">README.md</a> · Master specification: <a href="./RustyDroid.md">RustyDroid.md</a></em>
</p>

<p align="center">
  <a href="https://github.com/nadabgfccvd/RustyDroid/actions/workflows/ci.yml"><img src="https://github.com/nadabgfccvd/RustyDroid/actions/workflows/ci.yml/badge.svg" alt="CI"/></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License: MIT OR Apache-2.0"/>
  <img src="https://img.shields.io/badge/MSRV-1.76.0-orange" alt="MSRV 1.76.0"/>
  <img src="https://img.shields.io/badge/API-26%20%E2%86%92%2036-brightgreen" alt="Android API 26→36"/>
  <img src="https://img.shields.io/badge/floor-Motorola%20Moto%20E5-informational" alt="Floor: Motorola Moto E5"/>
</p>

---

## 1. Overview

**RustyDroid** is a clean-room, Rust-native Android application runtime ("Wine
for Android") built from day one for headless automation and AI agents:

- **Direct APK & DEX parsing (`rd-apk`, `rd-dex`):** Bounds-checked parsers for
  ZIP/ZIP64, binary `AndroidManifest.xml` (AXML), `resources.arsc` (ARSC),
  v1/v2/v3 APK signatures, and Dalvik `classes*.dex` (validated against
  `baksmali 2.5.2`).
- **Deterministic Dalvik VM & Android Framework (`rd-vm`, `rd-framework`):**
  Pure-Rust bytecode execution with generational mark/sweep GC
  ([`docs/gc.md`](./docs/gc.md)), optional Cranelift integer JIT
  ([ADR 007](./docs/adr/007-cranelift-leaf-jit.md)), `Activity`/`Fragment`
  lifecycles, `LayoutInflater`, `RecyclerView`, `SharedPreferences`, offline
  `OkHttp`, `SQLite`/`Room`, notifications, IME, widgets, accessibility, and
  programmable virtual devices ([`docs/m10.md`](./docs/m10.md)).
- **Isolated Native `.so` Execution (`rd-ndk`):** Opt-in `--features native`
  loads `x86_64`, `arm64-v8a`, and `armeabi-v7a` ELF shared libraries inside an
  isolated Unicorn subprocess worker ([`docs/m9.md`](./docs/m9.md),
  [`docs/native-languages.md`](./docs/native-languages.md)) — guest native code
  never runs directly on the host CPU.
- **Deterministic Software Rendering (`rd-render`):** Headless `tiny-skia`
  rasterizer + reproducible PNG output + structured `uiautomator`-compatible UI
  trees.
- **AI-Agent Control Plane (`rd-agent`):** Zero-dependency JSON-RPC 2.0 MCP
  server (`rd mcp`) exposing **47 structured tools**, subscribable
  `rustydroid://screen` resources, semantic screen observation without
  screenshots ([`docs/mcp-observe.md`](./docs/mcp-observe.md)), and a `--live`
  browser inspector.

## 2. Core Contracts

| Contract | Value / Rule |
|---|---|
| **Android API range** | `minSdkVersion 26` (Android 8.0) → `targetSdk 36` |
| **Hardware floor** | **Motorola Moto E5** (`data/devices.toml`: 1024 MB RSS cap (ADR 015), 256 MB app heap cap, 0.25× CPU factor, 720×1440 @ 280 dpi) |
| **Error discipline (Law 1)** | Structured `RdError { code, cause, suggestion, module_id }` everywhere; unmodeled surfaces return `NOT_IMPLEMENTED` |
| **Determinism (Law 4)** | Fuel budgets, virtual clock, deterministic GC, reproducible PNG frames |
| **Memory safety** | `unsafe_code = "forbid"` across the workspace except audited boundaries in `rd-ndk` and `rd-jni` |
| **Rust MSRV** | **1.76.0** (`cargo check --workspace --all-features --locked`) |
| **License** | Dual-licensed [`LICENSE-MIT`](./LICENSE-MIT) OR [`LICENSE-APACHE`](./LICENSE-APACHE) |

## 3. Quickstart

```console
# Build the CLI (default features include M9/M10 components)
cargo build --release -p rd-cli

# Inspect an APK's manifest, permissions, signing, and engine signatures
./target/release/rd inspect path/to/app.apk

# Run an APK headlessly and dump UI state
./target/release/rd app run path/to/app.apk

# Execute a JSON scenario test suite and emit JUnit XML
./target/release/rd test tests/suites/contract-no-app.json --junit report.xml

# Benchmark startup & memory against the Moto E5 floor
./target/release/rd perf path/to/app.apk --json

# Launch the MCP server (with optional live browser viewer)
./target/release/rd mcp --live 127.0.0.1:7878
```

### Optional Cargo Features on `rd-cli`

- `--features jit` — Enables the Cranelift integer JIT backend (activate at
  runtime with `RD_JIT=on`; see [ADR 007](./docs/adr/007-cranelift-leaf-jit.md)).
- `--features native` — Enables isolated Unicorn execution for guest JNI `.so`
  libraries ([`docs/m9.md`](./docs/m9.md)).
- `--features native-engines` — Enables bounded C/C++/Rust demonstration engine
  adapters ([`docs/native-languages.md`](./docs/native-languages.md)).

## 4. Workspace Architecture

| Crate / Tool | Responsibility |
|---|---|
| `crates/rd-apk` | ZIP/ZIP64, `PagedZip`, binary AXML, ARSC, v1/v2/v3 signing, manifest & permissions |
| `crates/rd-dex` | Complete DEX 035–041 parser + `baksmali`-faithful `smali` disassembler |
| `crates/rd-framework` | Versioned Android tables: permissions, devices (`moto-e5`), behavior switches, API/Jetpack/GMS catalogs |
| `crates/rd-vm` | Dalvik VM, generational GC, Android/`androidx`/M8/M9/M10 host surfaces |
| `crates/rd-jni` | Narrow JNI descriptors and optional Cranelift JIT compilation (`jit` feature) |
| `crates/rd-ndk` | ELF32/64 loader + isolated Unicorn subprocess bridge (`native` feature) |
| `crates/rd-render` | Headless `tiny-skia` software renderer & deterministic PNG encoder |
| `crates/rd-engine-registry` | Appendix G 28-engine catalog, signature detector, and feature-gated contracts |
| `crates/rd-agent` | 47-tool JSON-RPC 2.0 MCP server, `observe`, screen resources, `--live` viewer |
| `crates/rd-cli` | `rd` command-line binary (`inspect`, `dex`, `vm`, `app`, `mcp`, `test`, `perf`, `device`, `engine`, `jetpack`, `gms`) |
| `crates/rd-testsupport` | Deterministic synthetic APK, DEX, AXML, ARSC, and ELF builders for tests |
| `xtask` | Official generator for `compat-matrix.md`, `api-coverage.md`, `engine-registry.md`, `jetpack.md`, and `gms.md` |
| `tools/rd-sentinel` | Anti-regression gate auditing diff risk, invariants, fuzzing, and performance ([`docs/rd-sentinel.md`](./docs/rd-sentinel.md)) |

## 5. Generated Scoreboards & Documentation

All compatibility claims are backed by versioned TOML catalogs in `data/` and
verified in CI:

- **Official Compatibility Matrix:** [`docs/compat-matrix.md`](./docs/compat-matrix.md)
- **Milestone Roadmap (M0–M12):** [`docs/ROADMAP.md`](./docs/ROADMAP.md)
- **Android `android.*` Surface (Appendix C):** [`docs/api-coverage.md`](./docs/api-coverage.md)
- **AndroidX / Jetpack Catalog (Appendix D):** [`docs/jetpack.md`](./docs/jetpack.md)
- **Google Play Services / Firebase "No-GMS" Mode (Appendix E):** [`docs/gms.md`](./docs/gms.md)
- **28 Language & Engine Registry (Appendix G):** [`docs/engine-registry.md`](./docs/engine-registry.md)
- **mdBook Index:** [`docs/SUMMARY.md`](./docs/SUMMARY.md) · [`docs/index.md`](./docs/index.md)

## 6. Community, Governance & Security

- **Contributing Guide:** [`CONTRIBUTING.md`](./CONTRIBUTING.md)
- **Code of Conduct:** [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md)
- **Governance & Release Policy:** [`GOVERNANCE.md`](./GOVERNANCE.md)
- **Support & Issue Triage:** [`SUPPORT.md`](./SUPPORT.md)
- **Security Policy (Private Reporting):** [`SECURITY.md`](./SECURITY.md)
- **Changelog:** [`CHANGELOG.md`](./CHANGELOG.md)
