# AndroSwitch documentation

AndroSwitch adapts [RustyDroid](https://github.com/nadabgfccvd/RustyDroid) for
HorizonOS. Start with the [project README](../README.md) and
[credits/provenance](../CREDITS.md). M0/M1 host and Ryujinx gates are complete;
installation, JNI and Android lifecycle remain later milestones.

## Use and reproduce the current work

| Guide | Contents |
| --- | --- |
| [Getting started](GETTING_STARTED.md) | Clone, prerequisites and first APK inspection |
| [Building](BUILDING.md) | Host tools, pinned device Rust, libnx and Java fixture builds |
| [CLI reference](CLI.md) | Actual `asctl` commands, JSON, preflight and the separate method probe |
| [Testing](TESTING.md) | Focused host gates, feature isolation, CI and evidence requirements |
| [M1 Ryujinx E2E](M1_E2E.md) | Reproduce the actual APK/interpreter emulator proof |

## Design and progress

| Document | Contents |
| --- | --- |
| [Roadmap](ANDROSWITCH_ROADMAP.md) | M0–M12, exit gates and backend-specific status |
| [Architecture](ARCHITECTURE.md) | Shared core, platform boundary, planned VFS and launcher/runner |
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
physical Switch evidence. Physical Switch verification is still pending.

## Contributing and original material

Use [CONTRIBUTING](../CONTRIBUTING.md) and [SECURITY](../SECURITY.md) for this
repository. Preserve [NOTICE](../NOTICE.md) and all original license notices.

The retained [RustyDroid specification](../RustyDroid.md),
[upstream README](../README.rustydroid.md), [upstream roadmap](ROADMAP.md),
Linux performance reports, compatibility scoreboard and MCP guides describe
the upstream project. They remain useful references; their feature claims
are not the AndroSwitch support matrix. The AndroSwitch documents above are
the current source of truth for Horizon work.
