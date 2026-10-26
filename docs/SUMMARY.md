# Summary

[Introduction](./index.md)

# Roadmap & Official Scoreboards

- [Roadmap (M0 → M12)](./ROADMAP.md)
- [Official Compatibility Matrix](./compat-matrix.md)
- [Android API Coverage — Appendix C](./api-coverage.md)
- [Appendix C Literal Completion Ledger](./appendix-c-literal-completion.md)
- [AndroidX / Jetpack — Appendix D](./jetpack.md)
- [Google Play Services / Firebase — Appendix E](./gms.md)
- [Appendices D + E Executable Ledger](./appendix-d-e.md)
- [Language & Engine Registry — Appendix G](./engine-registry.md)

# MCP Agent Plane & Test Runner

- [MCP Tools Reference](./mcp-tools.md)
- [Screen Observation & Live Viewer](./mcp-observe.md)
- [Reliable MCP Automation & Diagnostics](./mcp-automation.md)
- [Test Suite Format & JUnit XML (`rd test`)](./suite-format.md)

# Runtime Subsystems & Verification

- [M9 — Advanced Components & Isolated NDK](./m9.md)
- [Real C, C++ & Rust APK Execution](./native-languages.md)
- [M10 — Programmable Virtual Devices](./m10.md)
- [Generational Garbage Collector](./gc.md)
- [GC & Runtime Efficiency Verification](./performance-gc-20261020.md)
- [Fuzzing Binary Parsers](./FUZZING.md)
- [RD Sentinel Anti-Regression Bot](./rd-sentinel.md)
- [Runtime Integration Validation](./integration-validation.md)
- [Appendix G Validation (2026-10-14)](./appendix-g-validation.md)
- [Appendix G Integration Validation (2026-10-20)](./appendix-g-integration.md)
- [Comprix v1.6.0 End-to-End via MCP](./comprix-e2e.md)

# Architecture Decision Records (ADRs)

- [ADR 001 — Zero-Dependency MCP JSON-RPC Server](./adr/001-mcp-sem-rmcp-msrv.md)
- [ADR 002 — Duplicate ZIP Entries](./adr/002-zip-duplicate-entries.md)
- [ADR 003 — Engine Registry Separated from Execution](./adr/003-appendix-g-engine-registry.md)
- [ADR 003 — M8 Selective Compatibility Host](./adr/003-m8-compat-host.md)
- [ADR 004 — M9 Isolated Native Execution](./adr/004-m9-native-isolation.md)
- [ADR 005 — Real SQLite via Isolated Subprocess](./adr/005-sqlite-subprocess.md)
- [ADR 006 — Generational GC & Live-Frame Roots](./adr/006-generational-gc-and-live-frame-roots.md)
- [ADR 007 — Opt-In Cranelift Integer JIT](./adr/007-cranelift-leaf-jit.md)
- [ADR 008 — 15 GB APK & ≤ 1,500 ms Cold Start on Moto E5](./adr/008-15gb-apk-and-1500ms-cold-start.md)
- [ADR 009 — Process-Cold < 26 ms Snapshot Startup](./adr/009-process-cold-26ms.md)
- [ADR 010 — RD Sentinel Anti-Regression Bot](./adr/010-rd-sentinel-anti-regression-bot.md)
- [ADR 011 — M10 Programmable Virtual Devices](./adr/011-m10-virtual-devices.md)
- [ADR 012 — Executable Appendices D & E Registries](./adr/012-appendix-d-e-registries.md)
- [ADR 013 — M11 Engine Execution Strategy](./adr/013-m11-engine-execution-strategy.md)
- [ADR 014 — LazyArsc & LazyStringPool](./adr/014-lazyarsc-lazystringpool.md)
- [ADR 017 — Host Thread-Local State: Engine, Package and Session Boundaries](./adr/017-host-thread-local-state-engine-boundary.md)
