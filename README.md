# AndroSwitch

An experimental Android compatibility runtime for **Nintendo Switch HorizonOS**.
The goal is to run unmodified APKs through shared Android API and ABI
implementations, directly from homebrew, without booting a full Android OS.

**Built on [RustyDroid](https://github.com/nadabgfccvd/RustyDroid), created by
[nadabgfccvd](https://github.com/nadabgfccvd) and the RustyDroid contributors.**
Its APK/DEX parsers, Dalvik interpreter, classpath, heap and existing runtime
code are the foundation of this project. AndroSwitch is a history-preserving
fork, with the original copyright notices, licenses and documentation retained.
See [credits and provenance](CREDITS.md) and [NOTICE](NOTICE.md).

## Current status

**Development has reached the M4 execution proof.** The `no_std + alloc` NRO
loads APK DEX, calls real ARM64 libraries through JNI and receives callbacks
into the shared RustyDroid interpreter. Java/C/Rust and Kotlin/C++ APK fixtures
use the same runtime. Physical Switch testing remains pending.

| Area | Verified status |
| --- | --- |
| Host APK inspection | Manifest, resources, DEX/native inventory and SDK/ABI/split preflight |
| Portable APK/DEX core | Shared parsers and interpreter build for the freestanding ARM64 Switch target |
| Persistent packages/storage | Copied APKs, immutable update revisions, isolated saves, uninstall retention and OBB streaming |
| Native ARM64 | Program-header ELF loader, dependencies, constructors, supported relocations/TLS and real libnx execution |
| JNI | Native registration, callbacks/reentry, strings, primitive arrays, exception/reference/GC and pthread callback proof |
| Ryujinx E2E | DEX semantics, persistent storage, native CPU oracle and mixed Java/Kotlin JNI fixtures; see milestone evidence |
| Upstream VM regression subset | 227 tests pass; separate from the recorded full-upstream baseline |
| Android lifecycle, UI/media and consumer launcher | M5–M7; not implemented yet |
| Physical Switch | Not tested yet |

This is a development project, not a general-purpose APK launcher release.
The method probe does not launch an Activity or prove that an entire app works.
Inherited RustyDroid capabilities are not automatically Switch capabilities.
Read the [compatibility inventory](docs/ANDROSWITCH_COMPATIBILITY.md) and
[M2–M4 implementation and evidence guide](docs/M2_M4_E2E.md) for the exact scope.

## Project direction

- One common Android environment: improve shared API/ABI contracts instead of
  adding package-name branches, game profiles or title-specific hooks.
- DEX interpretation and native ARM64 libraries connected
  through real JNI objects, methods and references.
- A portable `no_std + alloc` core with explicit libnx platform adapters.
- Offline, one-app-at-a-time operation for the first release; no Python,
  Unicorn or guest Linux kernel in the device build.
- A console launcher with an installed-app list and persistent Android-style
  storage. Network support and split APKs follow the initial offline release.

Android API 26 is the initial behavior reference. An APK with `minSdk <= 26`
may enter evaluation; `targetSdk` is not a minimum requirement. Passing these
static checks does not establish Android API coverage or app compatibility.

## Try the host tools

Requires Git and Rust/Cargo. The repository pins Rust **1.97.1** in
[rust-toolchain.toml](rust-toolchain.toml). Windows PowerShell commands:

```powershell
git clone https://github.com/BlekjekSudo/androswitch.git
cd androswitch
cargo run -p as-cli --locked -- capabilities --json
cargo run -p as-cli --locked -- inspect "path/to/app.apk" --json
```

To inspect the checksum-pinned real APK fixture:

```powershell
./scripts/fetch-fixtures.ps1 -ExtractDex
cargo run -p as-cli --locked -- inspect fixtures/cache/org.fdroid.fdroid_2000052.apk --json
./scripts/check-m0.ps1
```

`asctl` provides inspection and persistent package management:

```powershell
cargo run -p as-cli --locked -- install "path/to/app.apk" --root "local-artifacts/storage" --json
cargo run -p as-cli --locked -- list --root "local-artifacts/storage" --json
```

The DEX/JNI execution probe is a separate development harness; application
lifecycle execution flags in `asctl` remain false until that integration exists.
See the
[CLI reference](docs/CLI.md) and [getting started guide](docs/GETTING_STARTED.md).

## Build and test the Switch proof

The current NRO build uses Windows Rust, WSL Ubuntu with devkitPro/libnx,
PowerShell 7, a JDK and Android SDK build-tools/platform 35. The device Rust
toolchain is pinned to nightly-2026-08-26. Full setup and commands are in
[building](docs/BUILDING.md).

```powershell
./scripts/build-switch-smoke.ps1 -Toolchain nightly-2026-08-26
./scripts/test-ryujinx-m1.ps1 -SkipBuild -Ryujinx "D:/path/to/Ryujinx.exe"
./scripts/check-m1.ps1 -SkipFixtureBuild
```

The E2E runner builds the fixture and NRO, generates a fresh session, reads
actual emulator SD reports, compares them to the portable host and JVM, and
stops only its own emulator process. Outputs stay in ignored
`local-artifacts/m1/`; compact evidence and fixture source hashes are tracked.
See [testing](docs/TESTING.md) and the [M1 E2E procedure](docs/M1_E2E.md).

## Planned SD layout

The launcher/install layout below is the product target for later milestones.
The M1–M4 developer runner uses `switch/androswitch-smoke/` for its inputs;
M2–M4 persistent installations and saves already use `switch/androswitch/`.

```text
switch/androswitch/
├── androswitch.nro
├── runtime/runner.nro
├── apks/
├── Android/
│   ├── data/<package>/
│   └── obb/<package>/
├── DCIM/
├── Download/
├── installed/<package>/
├── userdata/<package>/
├── cache/
└── logs/
```

Users will import APKs through `apks/` when the M7 launcher is available.
Java and native file APIs already share the same filesystem mappings;
OBBs remain ordinary app-readable files. Persistent installation is available
through the host CLI and the device developer runner.

## Documentation and development

- [Documentation index](docs/README.md): setup, build, CLI, tests and design.
- [Roadmap](docs/ANDROSWITCH_ROADMAP.md): M0–M12 and separate host/emulator/device gates.
- [Architecture](docs/ARCHITECTURE.md): core, platform boundary, storage and lifecycle.
- [Compatibility inventory](docs/ANDROSWITCH_COMPATIBILITY.md): implemented behavior and remaining work.
- [Contributing](CONTRIBUTING.md): shared-contract fixes, evidence and review requirements.
- [Security](SECURITY.md): reporting issues in this fork.

The original [RustyDroid README](README.rustydroid.md),
[specification](RustyDroid.md), roadmap and `rd` tools are retained as upstream
material. Their Linux/headless/MCP claims and performance budgets describe
RustyDroid, not verified Horizon behavior. The full immutable upstream baseline
has known failures, recorded in [M0 baseline](docs/M0_BASELINE.md).

## Credits and license

Primary credit belongs to **nadabgfccvd and the RustyDroid contributors** for
the original runtime. AndroSwitch builds on their implementation; the Switch
port, platform integration, development harness and its compatibility evidence
are maintained in this repository.

The fork starts at RustyDroid commit
[`6388281f66d254c8e01a0fdc7a5a9a30239bc762`](https://github.com/nadabgfccvd/RustyDroid/commit/6388281f66d254c8e01a0fdc7a5a9a30239bc762).
The original Git history and license texts are preserved. The project and
AndroSwitch additions use **MIT OR Apache-2.0**, at your option:
[MIT](LICENSE-MIT), [Apache-2.0](LICENSE-APACHE), [NOTICE](NOTICE.md).
Other components retain their respective licenses; see [CREDITS](CREDITS.md).
