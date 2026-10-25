# Project Governance

> 🇧🇷 [Governança do Projeto em português abaixo](#governança-do-projeto-pt-br)

This document defines how technical, architectural, and release decisions are
made in **RustyDroid**, mirroring Part 3 (§3.3) of the master specification
([`RustyDroid.md`](./RustyDroid.md)) and [`CONTRIBUTING.md`](./CONTRIBUTING.md).

## 1. Roles and Decision Model

RustyDroid follows a **maintainer-led (BDFL / repository owner)** governance
model focused on engineering rigor, deterministic verification, and honest
compatibility reporting:

- **Repository Owner (`@nadabgfccvd`)** — Holds final authority over project
  direction, milestone acceptance, policy changes, baseline updates, and pull
  request merges (see [`.github/CODEOWNERS`](./.github/CODEOWNERS)).
- **Maintainers & Reviewers** — Triaging issues, auditing compatibility claims
  against AOSP specifications, running end-to-end APK verification, and
  enforcing CI and RD Sentinel gates.
- **Contributors (Human & AI-Assisted)** — Anyone submitting issues, ADRs,
  compatibility shims, parsers, tests, or documentation in accordance with
  [`CONTRIBUTING.md`](./CONTRIBUTING.md) and
  [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md).

**Human-in-the-loop rule:** RustyDroid welcomes heavy AI assistance across
design, implementation, and verification, **but no change is merged without
explicit human approval** after automated gates pass.

## 2. Inviolable Technical Laws

Every design and pull request is bound by the project laws in
[`RustyDroid.md`](./RustyDroid.md):

1. **Law 1 — No silent failures:** Unimplemented surfaces or malformed inputs
   must return structured `RdError { code, cause, suggestion, module_id }`
   diagnostics (`NOT_IMPLEMENTED` where unmodeled) — never panics or silent
   no-op success when the contract requires failure.
2. **Law 2 — Raw fidelity:** Binary parsers (`rd-apk`, `rd-dex`, `rd-ndk`)
   preserve raw offsets and byte-level inspection fidelity alongside typed
   models.
3. **Law 3 — Clean-room licensing:** Dual-licensed `MIT OR Apache-2.0`. Zero
   copied code from GPL/incompatible sources; external inspirations are
   documented conceptually only, and `cargo deny` enforces dependency licenses.
4. **Law 4 — Determinism:** Fixed fuel budgets, virtual clocks, deterministic
   GC, and byte-reproducible rendering/PNG outputs.
5. **Hardware & API Floor Contract:** Android `minSdkVersion 26` through
   `targetSdk 36`, validated against the **Motorola Moto E5** hardware floor
   (`data/devices.toml`: 1024 MB process RSS cap (ADR 015), 256 MB app heap cap, 0.25×
   single-thread CPU factor).

## 3. Architecture Decision Records (ADRs)

Any non-trivial architectural change — or any deliberate divergence from
[`RustyDroid.md`](./RustyDroid.md) — **requires an ADR** under `docs/adr/`
before or alongside the implementation:

- Format: `docs/adr/NNN-short-slug.md` (Context, Decision, Consequences,
  Alternatives Rejected).
- Examples include MCP zero-dependency JSON-RPC
  ([`001`](./docs/adr/001-mcp-sem-rmcp-msrv.md)), engine registry separation
  ([`003`](./docs/adr/003-appendix-g-engine-registry.md)), isolated Unicorn
  native execution ([`004`](./docs/adr/004-m9-native-isolation.md)),
  generational GC ([`006`](./docs/adr/006-generational-gc-and-live-frame-roots.md)),
  opt-in Cranelift JIT ([`007`](./docs/adr/007-cranelift-leaf-jit.md)),
  15 GB / 1,500 ms Moto E5 budgets
  ([`008`](./docs/adr/008-15gb-apk-and-1500ms-cold-start.md)), RD Sentinel
  ([`010`](./docs/adr/010-rd-sentinel-anti-regression-bot.md)), M10 virtual
  devices ([`011`](./docs/adr/011-m10-virtual-devices.md)), and M11 engines
  ([`013`](./docs/adr/013-m11-engine-execution-strategy.md)).

## 4. Merge Gates & Anti-Regression Policy

A pull request is eligible for merge only when:

1. **CI (`ci.yml`) is green:** `cargo fmt --check`, `cargo clippy -D warnings`,
   workspace tests, MSRV **Rust 1.76.0** (`--all-features --locked`), `unsafe`
   confinement (`rd-ndk`/`rd-jni` only), markdown link integrity, and `xtask`
   generated report sync (`docs/compat-matrix.md`, `docs/api-coverage.md`,
   `docs/engine-registry.md`, `docs/jetpack.md`, `docs/gms.md`).
2. **RD Sentinel (`rd-sentinel/gate`) passes:** Zero blocking `P0`/`P1`
   findings across static safety rules, test-shrinking checks, fuzz/perf
   campaigns, and protected-path controls (see
   [`docs/rd-sentinel.md`](./docs/rd-sentinel.md)).
3. **Protected paths & baselines:** Files listed under `protected_paths` in
   `.github/rd-sentinel/policy.json` (CI security gates, Sentinel policy and
   baselines, `deny.toml`, `CODEOWNERS`) cannot be weakened in a feature PR and
   require explicit owner approval labels (`sentinel:policy-approved` /
   `sentinel:baseline-approved`).
4. **Performance claims:** Optimization PRs must include paired before/after
   measurements on identical hardware/profiles.

## 5. Versioning, MSRV & Release Policy

- **Semantic Versioning (SemVer 2.0.0):**
  - `0.x.y` during milestone build-out (`M0`–`M11`; `0.9.0` marks `M8`
    real-world compatibility).
  - `1.0.0` (`M12` target) requires stable CLI exit codes, stable MCP tool
    schemas (`rd mcp --describe-tools`), green Tier-1 compatibility scoreboards,
    and Moto E5 budget compliance.
- **MSRV Policy:** Workspace Minimum Supported Rust Version is **1.76.0**,
  enforced in CI with `--all-features --locked`. Raising MSRV requires a
  dedicated ADR and explicit owner approval.
- **Generated Scoreboards:** `docs/compat-matrix.md`, `docs/api-coverage.md`,
  `docs/engine-registry.md`, `docs/jetpack.md`, and `docs/gms.md` are generated
  from versioned TOML catalogs in `data/` via `cargo xtask` and never hand-edited.

---

# Governança do Projeto (PT-BR)

Este documento define como decisões técnicas, arquiteturais e de release são
tomadas no **RustyDroid**, refletindo a PARTE 3 (§3.3) da especificação mestra
([`RustyDroid.md`](./RustyDroid.md)) e o guia [`CONTRIBUTING.md`](./CONTRIBUTING.md).

## 1. Papéis e Modelo de Decisão

- **Dono do Repositório (`@nadabgfccvd`)** — Autoridade final sobre rumos do
  projeto, aceitação de ADRs, baselines/políticas do Sentinel e aprovação de
  todo merge na `main` ([`.github/CODEOWNERS`](./.github/CODEOWNERS)).
- **Mantenedores e Revisores** — Triagem de issues, auditoria fiel ao AOSP,
  validação E2E em APKs reais e fiscalização dos gates.
- **Contribuidores (Humanos e Assistidos por IA)** — Contribuições assistidas
  por IA são bem-vindas e declaradas de forma transparente, mas **nenhum merge
  ocorre sem revisão e aprovação humana explícita**.

## 2. Leis Técnicas e Contratos

1. **Lei 1 — Zero falha silenciosa:** Erros sempre estruturados (`RdError`),
   `NOT_IMPLEMENTED` honesto para superfícies ainda não modeladas.
2. **Lei 2 — Fidelidade bruta:** Parsers preservam bytes e offsets originais.
3. **Lei 3 — Licenciamento limpo:** `MIT OR Apache-2.0`, zero código copiado de
   fontes incompatíveis, auditado via `cargo deny`.
4. **Lei 4 — Determinismo:** Fuel fixo, relógio virtual, GC determinístico e
   rasterização PNG reprodutível byte-a-byte.
5. **Piso de API e Hardware:** `minSdkVersion 26` → `targetSdk 36`, contrato de
   hardware **Motorola Moto E5** (`data/devices.toml`: RSS ≤ 1024 MB, heap ≤ 256 MB).

## 3. ADRs, Gates de Merge e Versionamento

- **ADRs obrigatórios:** Qualquer decisão arquitetural relevante ou divergência
  documentada da spec exige registro em `docs/adr/NNN-slug.md`.
- **Gates obrigatórios:** CI verde (incluindo MSRV **1.76.0** com
  `--all-features --locked`, `clippy -D warnings`, links markdown e relatórios
  `cargo xtask` sincronizados) + **RD Sentinel** sem bloqueios
  ([`docs/rd-sentinel.md`](./docs/rd-sentinel.md)).
- **SemVer e Releases:** `0.x` durante a construção dos marcos (`0.9.0` = M8);
  `1.0.0` no fechamento do M12 com contratos MCP/CLI estáveis e histórico em
  [`CHANGELOG.md`](./CHANGELOG.md).
