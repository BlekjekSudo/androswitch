# Getting Help & Support

> 🇧🇷 [Suporte em português abaixo](#suporte-e-ajuda-pt-br)

Need help inspecting or running an APK with **RustyDroid**, driving an app over
MCP, or diagnosing a compatibility gap? Use the guide below so your question or
report lands in the right place quickly.

## ⚠️ Security Vulnerabilities

**Do NOT open public issues for security vulnerabilities** (such as parser
memory/DoS bugs on untrusted APK/DEX/AXML/ARSC/ELF inputs or sandbox escapes).
Follow the private reporting flow in [`SECURITY.md`](./SECURITY.md).

## 1. Documentation & Self-Diagnosis First

Before opening an issue, check the project's versioned reference docs:

- **Overview & Quickstart:** [`README.md`](./README.md) · [`README.en.md`](./README.en.md)
- **Master Specification & Architecture:** [`RustyDroid.md`](./RustyDroid.md) · [`docs/ROADMAP.md`](./docs/ROADMAP.md)
- **Official Compatibility Scoreboard:** [`docs/compat-matrix.md`](./docs/compat-matrix.md)
  - Android `android.*` APIs (Appendix C): [`docs/api-coverage.md`](./docs/api-coverage.md)
  - AndroidX / Jetpack shims (Appendix D): [`docs/jetpack.md`](./docs/jetpack.md)
  - Google Play Services / Firebase "no-GMS" mode (Appendix E): [`docs/gms.md`](./docs/gms.md)
  - Language & Engine Registry (Appendix G): [`docs/engine-registry.md`](./docs/engine-registry.md)
- **MCP Agent Guides:** [`docs/mcp-tools.md`](./docs/mcp-tools.md) ·
  [`docs/mcp-observe.md`](./docs/mcp-observe.md) ·
  [`docs/mcp-automation.md`](./docs/mcp-automation.md)
- **Virtual Devices & NDK:** [`docs/m10.md`](./docs/m10.md) ·
  [`docs/m9.md`](./docs/m9.md) · [`docs/native-languages.md`](./docs/native-languages.md)

### Useful CLI / MCP Diagnostic Commands

```console
# 1. Inspect manifest, permissions, API floor, signing, and detected engines
rd inspect path/to/app.apk --json

# 2. Check engine / language execution status honestly
rd engine detect path/to/app.apk

# 3. Run an APK headlessly and inspect structured RdError diagnostics
rd app run path/to/app.apk

# 4. Measure cold/warm launch & RSS against the Moto E5 hardware floor
rd perf path/to/app.apk --json

# 5. Export full MCP tool schemas for agent integrations
rd mcp --describe-tools
```

## 2. Which Issue Template Should I Use?

When opening an issue on
[GitHub Issues](https://github.com/nadabgfccvd/RustyDroid/issues/new/choose),
pick the matching form:

| Situation | Issue Form | Label |
|---|---|---|
| Missing `android.*` / AndroidX / GMS method or behavioral divergence (`NOT_IMPLEMENTED`) | **Compat gap** ([`compat_gap.yml`](./.github/ISSUE_TEMPLATE/compat_gap.yml)) | `compat-gap` |
| Incorrect bytecode execution, parser failure on valid APK, wrong UI render, CLI/MCP bug | **Bug report** ([`bug_report.yml`](./.github/ISSUE_TEMPLATE/bug_report.yml)) | `bug` |
| Cold/warm startup, RSS (`VmHWM`), or heap regression against the `moto-e5` floor | **Performance regression** ([`perf_regression.yml`](./.github/ISSUE_TEMPLATE/perf_regression.yml)) | `perf-regression` |
| New CLI command, MCP capability, virtual device feature, or developer tooling | **Feature request** ([`feature_request.yml`](./.github/ISSUE_TEMPLATE/feature_request.yml)) | `enhancement` |

## 3. What to Include for Fast Triage

- RustyDroid commit SHA (`git rev-parse --short HEAD`) and Cargo features used
  (`default`, `jit`, `native`, `native-engines`, `virtual-devices`).
- Host OS, CPU architecture, and `rustc --version`.
- Structured JSON output (`{code, cause, suggestion, module_id}`), MCP
  `compat_gaps` / `runtime_health` snapshot, or `rd perf --json` numbers.
- If reproducible with an open-source APK (e.g., F-Droid), provide the package
  ID, version code, and SHA-256 — **never attach proprietary or copyrighted APK
  binaries to issues or pull requests**.

## 4. Contributing Fixes

Want to fix the issue yourself? Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) and
[`GOVERNANCE.md`](./GOVERNANCE.md).

---

# Suporte e Ajuda (PT-BR)

Precisa de ajuda para inspecionar ou executar um APK no **RustyDroid**,
integrar um agente via MCP ou diagnosticar uma lacuna de compatibilidade?

## ⚠️ Vulnerabilidades de Segurança

**NÃO abra issues públicas para relatar falhas de segurança.** Utilize sempre o
canal privado documentado em [`SECURITY.md`](./SECURITY.md).

## 1. Documentação de Referência

- **Visão Geral e Início Rápido:** [`README.md`](./README.md) · [`README.en.md`](./README.en.md)
- **Especificação Mestra e Roadmap:** [`RustyDroid.md`](./RustyDroid.md) · [`docs/ROADMAP.md`](./docs/ROADMAP.md)
- **Placar Oficial de Compatibilidade:** [`docs/compat-matrix.md`](./docs/compat-matrix.md)
  - Superfície `android.*` (Apêndice C): [`docs/api-coverage.md`](./docs/api-coverage.md)
  - AndroidX / Jetpack (Apêndice D): [`docs/jetpack.md`](./docs/jetpack.md)
  - Google Play Services / Firebase sem GMS (Apêndice E): [`docs/gms.md`](./docs/gms.md)
  - Registro de Linguagens e Engines (Apêndice G): [`docs/engine-registry.md`](./docs/engine-registry.md)
- **Guias MCP e Dispositivos Virtuais:** [`docs/mcp-tools.md`](./docs/mcp-tools.md) ·
  [`docs/mcp-observe.md`](./docs/mcp-observe.md) ·
  [`docs/mcp-automation.md`](./docs/mcp-automation.md) ·
  [`docs/m10.md`](./docs/m10.md)

## 2. Qual Template de Issue Escolher?

- **Lacuna de API Android (`NOT_IMPLEMENTED` ou divergência com AOSP):**
  [`compat_gap.yml`](./.github/ISSUE_TEMPLATE/compat_gap.yml) (`compat-gap`)
- **Bug funcional em parser, VM, renderizador, CLI ou MCP:**
  [`bug_report.yml`](./.github/ISSUE_TEMPLATE/bug_report.yml) (`bug`)
- **Regressão de performance ou memória contra o piso `moto-e5`:**
  [`perf_regression.yml`](./.github/ISSUE_TEMPLATE/perf_regression.yml) (`perf-regression`)
- **Sugestão de funcionalidade ou ferramenta:**
  [`feature_request.yml`](./.github/ISSUE_TEMPLATE/feature_request.yml) (`enhancement`)

Inclua sempre o commit SHA, flags de compilação, erro estruturado JSON e, caso
use um APK público (como F-Droid), informe apenas nome/versão/SHA-256 — **nunca
anexe binários `.apk` proprietários no repositório**.
