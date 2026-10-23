# CLI reference

## asctl — the host inspection facade

Build with `cargo build -p as-cli --locked`, or prefix the commands below
with `cargo run -p as-cli --locked --`. These are the current commands;
`install`, `launch`, `stop`, `list` and `uninstall` are planned runtime APIs.

| Command | Meaning |
| --- | --- |
| `asctl inspect <APK> [--json]` | Parse an APK and report manifest, inventory and static preflight |
| `asctl dex-summary <DEX> [--json]` | Parse a raw DEX and report counts, version and checksum status |
| `asctl capabilities [--json]` | Describe the implemented `as-runtime` facade |
| `asctl --help` / `--version` | Usage / host package version |

```powershell
cargo run -p as-cli --locked -- inspect "D:/APKs/example.apk" --json
cargo run -p as-cli --locked -- capabilities --json
```

Quote paths with spaces. APK input is bounded at 512 MiB for this whole-file
inspection path; raw DEX inspection is bounded at 64 MiB. Large-asset streaming
belongs to the later storage/runtime implementation.

### Reports and policy

JSON output has `schema_version: 1`. Inspection includes `manifest`,
`inventory`, `launcher_activities`, `signing`, `preflight` and the upstream
revision. `preflight.blockers` identifies static SDK/ABI/split gaps.

- The behavior reference is API 26; `minSdk > 26` is a version blocker.
- `targetSdk` is not treated as the minimum version.
- DEX-only APKs do not require a native library. APKs with native libraries
  need an ARM64 candidate for the initial target.
- Required splits are reported as unavailable.
- Signing scheme detection is presence-only, not cryptographic verification.

Successful parsing/preflight does not mean the APK is executable. The facade
still reports `install`, `dex_execution`, `native_execution` and its device
execution verification flags as false. M1's bounded method harness is separate
from that application execution interface.

For a parser/storage error, JSON mode emits a structured error object with
`code`, `operation`, `module_id` and `cause`, inside a versioned error envelope.
An inspection containing preflight blockers is still a successful inspection;
consumers must examine `preflight`, rather than treating exit zero as an
installation/compatibility result.

| Exit code | Meaning |
| --- | --- |
| 0 | Command/report completed |
| 1 | Runtime/parser/storage operation failed |
| 2 | Invalid CLI usage, as reported by the argument parser |

### Pinned DEX example

After `./scripts/fetch-fixtures.ps1 -ExtractDex`:

```powershell
cargo run -p as-cli --locked -- dex-summary fixtures/cache/fdroid-classes.dex --json
```

The recorded fixture is DEX 037 with 11,168 classes, 65,003 methods and
62,734 strings, and a valid checksum. `bytecode_executed` is false for this
command. See [the fixture lock](../fixtures/corpus.lock.json).

## Persistent package commands

```powershell
asctl install app.apk --root path/to/androswitch --json
asctl install update.apk --root path/to/androswitch --update --json
asctl list --root path/to/androswitch --json
asctl uninstall org.example.app --root path/to/androswitch --json
asctl clear-data org.example.app --root path/to/androswitch --json
```

`install` copies the source APK into a committed revision. `--activity` selects
an enabled MAIN/LAUNCHER activity; otherwise the first enabled launcher is
recorded. A resource/library-only APK can be installed without an activity.
Updates retain private/shared files; an older versionCode is rejected.
Uninstall retains data. `clear-data` clears private data explicitly; shared
Android data/OBB is retained. JSON failures include subsystem and operation.
These commands manage packages; they do not launch an Activity.

## as-probe — the DEX/native/JNI method harness

This developer binary takes an APK, a request JSON and optional storage root, executes selected
static DEX methods through the shared interpreter and emits a report:

```powershell
cargo run -p as-switch-smoke --features host-cli --bin as-probe -- local-artifacts/m1/fixture/vm-semantics.apk local-artifacts/m1/fixture/request.json
```

Build the fixture first as described in [building](BUILDING.md). Requests
carry a schema version, session ID and calls containing a class descriptor,
method, signature and argument literals. Fixture-specific selections are in
the harness; they do not alter runtime behavior by package name.

The optional request `storage` selects install/update/installed launch, with a
package and optional expected revision. `APK=-` loads the installed copy without
requiring a source APK. Java and native see the same package VFS. Optional
`native` calls select a library, exported symbol and scalar descriptor; host
execution requires `--features host-native` and Unicorn 2.1.4.

Reports include `portable_vm`, per-call values/errors and instruction counts,
installation metadata and actual JNI load/call/callback counters.
Use `scripts/test-ryujinx-m1.ps1` for assertions and a pass/fail receipt; raw
probe JSON must be inspected for individual errors. This harness is not a
user APK launcher or lifecycle API.

See [M2–M4](M2_M4_E2E.md) for build/run commands and the supported JNI subset.

## Retained rd tools

The `rd` CLI and agent/framework tools are retained from RustyDroid. They
describe the inherited host runtime and its own policies. Use the AndroSwitch
[compatibility inventory](ANDROSWITCH_COMPATIBILITY.md), rather than a legacy
tool's feature listing, to assess Horizon support. Original authorship is
documented in [CREDITS](../CREDITS.md).
