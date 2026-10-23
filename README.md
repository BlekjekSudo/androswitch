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

**M7 development alpha: one `androswitch.nro`, one SD folder.** The console
installs, updates and launches supported APKs using the shared RustyDroid
interpreter and native ARM64 backend. Java/Kotlin UI, NativeActivity GLES2,
assets, PCM audio and repeated application switching have Ryujinx evidence.
The user reports that the four bundled APKs open on a physical Switch, including
native animation/beeps, but Plus crashes in the native sample. The shutdown
changes await a hardware retest; see [hardware feedback](docs/HARDWARE_FEEDBACK.md).
This is not the v0.1 compatibility release.

![AndroSwitch console after the verified Ryujinx workflow](docs/images/androswitch-m7.png)

Actual submitted console framebuffer from the M7 Ryujinx test.

| Area | Verified status |
| --- | --- |
| Host APK inspection | Manifest, resources, DEX/native inventory and SDK/ABI/split preflight |
| Portable APK/DEX core | Shared parsers and interpreter build for the freestanding ARM64 Switch target |
| Persistent packages/storage | Copied APKs, immutable update revisions, isolated saves, uninstall retention and OBB streaming |
| Native ARM64 | Program-header ELF loader, dependencies, constructors, supported relocations/TLS and real libnx execution |
| JNI | Native registration, callbacks/reentry, strings, primitive arrays, exception/reference/GC and pthread callback proof |
| Ryujinx E2E | DEX semantics, persistent storage, native CPU oracle and mixed Java/Kotlin JNI fixtures; see milestone evidence |
| Upstream VM regression subset | 227 tests pass; separate from the recorded full-upstream baseline |
| Android lifecycle/UI | Supported M5 subset; 14 Java/Kotlin UI snapshots pass in Ryujinx |
| Native graphics/audio | NativeActivity, incremental assets, input queue, EGL/GLES2; Java AudioTrack, AAudio writes and OpenSL ES PCM queue callbacks |
| Console launcher | Scan/install/update/launch/return/uninstall; persistent saves, named failures and one NRO |
| Physical Switch | User reports sample install/open, native animation/beeps; native Plus crash remains pending retest |

The launcher is usable for the supported test corpus. Arbitrary apps can still
require missing Android APIs. This alpha accepts single APKs up to **64 MiB**
on device, API 26 or lower minimum SDK, and ARM64 native libraries. OBB files
stream separately. Network, Google Play Services, split APKs, WebView and modern
engine compatibility are not established. There is no per-game configuration.
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

## Use the Switch package

Extract the development ZIP's `switch/` folder to the SD card. Open
`switch/androswitch/androswitch.nro` through homebrew application/title override.
The package includes four original sample APKs, ready to install from the APKs
list. Put additional single APK files in `switch/androswitch/apks/`.

- **L/R:** Installed/APKs list. **A:** launch or install/update. **B:** back.
- **X:** details. **Y:** uninstall with saved data retained. **ZR:** separate
  clear-internal-data action. Both deletion actions require confirmation.
- **Minus:** rescan. **Plus:** return from an app to the console; from the
  console, exit homebrew. Touch selects a row; tap it again to open it.

If native workers cannot stop safely, the runtime records the reason and exits
the Horizon process. Reopen homebrew to read the interruption notice; it does
not reuse live guest code. APKs run native code in the homebrew process, so
install APKs you trust. See [using AndroSwitch](docs/USING_ANDROSWITCH.md),
[the Indonesian quick start](docs/QUICK_START_ID.md) and
[M6/M7 scope and evidence](docs/M6_M7_E2E.md).

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

## Build the product NRO

The product uses the same portable core as the earlier proofs, plus libnx,
Switch Mesa EGL/GLES2 and PCM output. See [building](docs/BUILDING.md) for
prerequisites and [M6/M7](docs/M6_M7_E2E.md) for exact checks.

```powershell
./scripts/build-m5-fixture.ps1
./scripts/build-m6-fixture.ps1
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-launcher.ps1
./scripts/package-androswitch.ps1
```

Output: `local-artifacts/product/androswitch.nro` and a copy-ready ZIP under
`local-artifacts/releases/`. APKs and compiled binaries are generated locally;
they are not committed to the source repository.

## Reproduce the earlier Switch proof

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

## SD layout

The console creates the directories below on first boot. Installation keeps
its own APK copy, so the source in `apks/` can be moved after installation.
The earlier developer proofs use a separate `androswitch-smoke/` input folder.

```text
switch/androswitch/
├── androswitch.nro
├── runtime/
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

Java and native file APIs share the same filesystem mappings. Put OBB files in
`Android/obb/<package>/`; they remain ordinary app-readable files and are not
automatically extracted. Private saves live in `userdata/<package>/`.

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
