<!--
  🤖 AI-assisted contributions are welcome! Note in the description how AI was used.
  Contribuições assistidas por IA são bem-vindas! Indique na descrição como a IA foi usada.
-->

## What / O quê

<!-- Descrição curta da mudança / Short description -->

## Why / Por quê

<!-- Problema ou issue relacionada (ex.: Fixes #12, compat-gap #7) -->

## How / Como

<!-- Abordagem técnica; ADR link se decisão de arquitetura -->

## Checklist

- [ ] Conventional Commits (`feat:`, `fix:`, `docs:`…)
- [ ] Tests added/updated (unit + integration with real fixtures)
- [ ] `cargo fmt` + `clippy -D warnings` clean *(quando houver código)*
- [ ] No silent failures — missing APIs respond `NOT_IMPLEMENTED(module_id)`
- [ ] Fits the `moto-e5` performance budgets *(para features de runtime)*
- [ ] Docs updated (`compat-matrix.md`; Android API changes also update `data/api-coverage.toml` and regenerate Appendix C reports)
- [ ] AI usage disclosed / uso de IA declarado

## RD Sentinel anti-regression evidence

- [ ] I read the Sentinel risk areas/findings; no required campaign is being
      treated as a pass while skipped, planned, or incomplete
- [ ] High-risk runtime/parser/native/protocol changes include a regression
      test that would catch the old behavior (or the PR explains why an
      existing golden/contract/real-APK test already does)
- [ ] Performance-sensitive changes include before/after evidence (`rd perf`,
      `cold26`, paired benchmark, or Sentinel A/B artifact as applicable)
- [ ] No test/golden/fuzz/MCP evidence was removed or a budget relaxed without
      equivalent replacement evidence and maintainer baseline approval
- [ ] Dependency changes disclose license, source, MSRV, major-version, and
      downstream/blast-radius impact; bot policy files were not weakened to
      make this PR pass
- [ ] Workflow changes keep actions SHA-pinned, permissions explicit, and
      user-controlled text out of shell `run:` interpolation
- [ ] If `sentinel:test-gap` or a surviving mutant is reported, the PR adds a
      behavior test or records a dated, narrow equivalent-mutant exception
