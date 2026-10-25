# RustyDroid Documentation

**RustyDroid** is a Rust-native Android application runtime ("Wine for Android")
that parses and executes Android APKs on Linux without a virtual machine, KVM,
or guest kernel — designed from day one for deterministic headless verification
and AI-agent automation over MCP.

> 🇧🇷 **O RustyDroid** é um runtime Android nativo em Rust que inspeciona e
> executa APKs no Linux sem VM, sem KVM e sem kernel convidado, projetado desde
> o dia 1 para automação determinística por agentes de IA via MCP.

## Quick Navigation

- **Overview & Quickstart:** [`README.md`](../README.md) · [`README.en.md`](../README.en.md)
- **Master Specification (v2.1):** [`RustyDroid.md`](../RustyDroid.md)
- **Milestone Roadmap (M0 → M12):** [`ROADMAP.md`](./ROADMAP.md)
- **Official Compatibility Scoreboard (generated):** [`compat-matrix.md`](./compat-matrix.md)

## Generated Reference Catalogs

Every scoreboard below is generated deterministically from versioned TOML data
in `data/` via `cargo xtask` and checked against drift in CI:

- **Android `android.*` API Coverage (Appendix C):** [`api-coverage.md`](./api-coverage.md)
- **AndroidX / Jetpack Catalog (Appendix D):** [`jetpack.md`](./jetpack.md)
- **Google Play Services / Firebase "No-GMS" Mode (Appendix E):** [`gms.md`](./gms.md)
- **28 Language & Engine Registry (Appendix G):** [`engine-registry.md`](./engine-registry.md)

## Runtime Subsystems & Agent Guides

- **MCP Tools Catalog (47 tools):** [`mcp-tools.md`](./mcp-tools.md)
- **Screen Observation Without Screenshots (`observe` & live viewer):** [`mcp-observe.md`](./mcp-observe.md)
- **Reliable MCP Automation & Diagnostics:** [`mcp-automation.md`](./mcp-automation.md)
- **Scenario Test Suites (`rd test` & JUnit XML):** [`suite-format.md`](./suite-format.md)
- **M9 Advanced Components & Isolated NDK:** [`m9.md`](./m9.md) · [`native-languages.md`](./native-languages.md)
- **M10 Programmable Virtual Devices:** [`m10.md`](./m10.md)
- **Generational Garbage Collector:** [`gc.md`](./gc.md)
- **Parser Fuzzing:** [`FUZZING.md`](./FUZZING.md)
- **RD Sentinel Anti-Regression Bot:** [`rd-sentinel.md`](./rd-sentinel.md)

## Building This Book Locally

With [`mdbook`](https://rust-lang.github.io/mdBook/) installed:

```console
mdbook build docs
```

The static HTML documentation site is written to `target/mdbook/` (per
[`book.toml`](./book.toml)), while all internal markdown links and heading
anchors are continuously verified in CI by `python3 scripts/check_md_links.py`.

## Governance, Support & Security

- [`CONTRIBUTING.md`](../CONTRIBUTING.md) · [`GOVERNANCE.md`](../GOVERNANCE.md) ·
  [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) · [`SUPPORT.md`](../SUPPORT.md) ·
  [`SECURITY.md`](../SECURITY.md) · [`CHANGELOG.md`](../CHANGELOG.md)
