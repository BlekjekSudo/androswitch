# AndroSwitch documentation

Current M9 work: [independent application startup](M9_EXTERNAL_APPS.md) and
[Horizon memory policy / Autorun reference](HORIZON_MEMORY.md). Actual external
startup failures and shared-contract improvements have separate host, emulator
and physical-device records; neither game has a first-screen pass.

- [M2–M4 persistent storage and native JNI proof](M2_M4_E2E.md)
- [Immutable package revisions ADR](adr/013-persistent-package-revisions.md)
- [Horizon native/JNI boundary ADR](adr/014-native-horizon-jni.md)

AndroSwitch adapts [RustyDroid](https://github.com/nadabgfccvd/RustyDroid) for
HorizonOS. Start with the [project README](../README.md) and
[credits/provenance](../CREDITS.md). M0–M4 developer proof gates have host and
Ryujinx evidence. M5–M7 add supported UI/native media and a one-NRO console;
the user confirms M1–M7 testing on physical Switch, with detailed scenario and
binary identities unrecorded. New changes need their own hardware verification.

## Use and reproduce the current work

| Guide | Contents |
| --- | --- |
| [Using the M7 alpha](USING_ANDROSWITCH.md) | One-NRO SD package, controls, samples, storage, limits and recovery |
| [Mulai dalam bahasa Indonesia](QUICK_START_ID.md) | Panduan singkat penggunaan dan pengujian hardware |
| [M6/M7 E2E](M6_M7_E2E.md) | Native media, console workflow, quiescent teardown and exact evidence scope |
| [Getting started](GETTING_STARTED.md) | Clone, prerequisites and first APK inspection |
| [Building](BUILDING.md) | Host tools, pinned device Rust, libnx and Java fixture builds |
| [CLI reference](CLI.md) | Actual `asctl` commands, JSON, preflight and the separate method probe |
| [Testing](TESTING.md) | Focused host gates, feature isolation, CI and evidence requirements |
| [M1 Ryujinx E2E](M1_E2E.md) | Reproduce the actual APK/interpreter emulator proof |
| [Horizon display](HORIZON_DISPLAY.md) | Actual portrait geometry, buffer ownership, capture publication and cleanup evidence |
| [Horizon workers](HORIZON_WORKERS.md) | Dedicated VM worker transport, actual clocks, ownership and bounded teardown |
| [Native host backends](HOST_NATIVE_TESTS.md) | Explicit WSL/MSYS C tests, Android errno boundaries and ARM64 CPU-oracle scope |
| [Portable string encodings](PORTABLE_ENCODINGS.md) | API 26 byte encoders, alias recognition, allocation bounds and JVM/ART comparison limits |
| [Independent M9 startup](M9_EXTERNAL_APPS.md) | Frozen game APKs, general API/ABI work, actual failures and first-screen gate |

The [latest parity report](UPSTREAM_PARITY_REPORT.md) records **not yet 1:1**,
707 portable tests and 982 host regression tests.

The active foundation work is [complete RustyDroid API parity](UPSTREAM_PARITY_PLAN.md),
with [source synchronization](UPSTREAM_PORTING.md) tracked separately from
product dispatch, shared state and actual Horizon execution. The experimental
full reference core is not selected by the shipping runner.

## Design and progress

| Document | Contents |
| --- | --- |
| [Roadmap](ANDROSWITCH_ROADMAP.md) | M0–M12, exit gates and backend-specific status |
| [Architecture](ARCHITECTURE.md) | Shared core, platform boundary, persistent VFS and verified session boundary |
| [Compatibility inventory](ANDROSWITCH_COMPATIBILITY.md) | Verified functionality and unavailable APIs/backends |
| [ADR 010](adr/010-androswitch-platform-boundary.md) | AndroSwitch's platform and verification contract |
| [ADR 011](adr/011-portable-dex-smoke.md) | Portable DEX parser and bounded C ABI |
| [ADR 012](adr/012-portable-interpreter.md) | Shared interpreter and portable VM state |

## Evidence and fixtures

- [M0 baseline](M0_BASELINE.md) and [M0 results](../reports/m0.json).
- [M1 results](../reports/m1-core.json): source commit, session IDs, input/NRO
  hashes, observed method values/errors and commands.
- [Historical parser-only smoke](M1_SMOKE.md) and
  [its result](../reports/m1-dex-smoke.json): an earlier partial M1 snapshot.
- [Fixture lock](../fixtures/corpus.lock.json): source revisions and hashes.

Large generated output is kept in ignored `target/evidence/` and
`local-artifacts/`; a fresh clone contains the compact reports and source
needed to rebuild the fixtures. Reports distinguish host, emulator and
physical Switch evidence. User-reported M1–M7 and later persistence results are
scoped in [hardware feedback](HARDWARE_FEEDBACK.md); new startup/display paths
remain unverified on physical Switch.
Current console/media source, run IDs and binary/fixture hashes are recorded
in [M6/M7 results](../reports/m6-m7.json). The M5 UI proof remains historical
evidence in [its report](../reports/m5-ui.json).

## Contributing and original material

Use [CONTRIBUTING](../CONTRIBUTING.md) and [SECURITY](../SECURITY.md) for this
repository. Preserve [NOTICE](../NOTICE.md) and all original license notices.

The retained [RustyDroid specification](../RustyDroid.md),
[upstream README](../README.rustydroid.md), [upstream roadmap](ROADMAP.md),
Linux performance reports, compatibility scoreboard and MCP guides describe
the upstream project. They remain useful references; their feature claims
are not the AndroSwitch support matrix. The AndroSwitch documents above are
the current source of truth for Horizon work.
