# Getting started

## What can I run today?

The current public code provides host APK inspection and a developer NRO
that executes selected DEX methods from an APK. M1's emulator evidence is
recorded in [the report](../reports/m1-core.json). Installing arbitrary APKs,
launching Android Activities and loading native ARM64 libraries through JNI
are not available yet. Follow the [roadmap](ANDROSWITCH_ROADMAP.md) for those
milestones.

## Clone and inspect

The documented local workflow uses Windows and PowerShell 7. Install Git and
Rust through rustup, with the linker required by your Rust host target. For
an MSVC Windows target this means the C++ build tools/Windows SDK. The repo
pins Rust 1.97.1, rustfmt and clippy; Android SDK and devkitPro are not needed
just to build `asctl`.

```powershell
git clone https://github.com/BlekjekSudo/androswitch.git
cd androswitch
cargo run -p as-cli --locked -- --help
cargo run -p as-cli --locked -- inspect "C:/path/to/application.apk" --json
```

Inspection lists manifest identity/SDKs, launchable activities, DEX files,
native ABIs and resource inventory. Signing scheme presence is detected;
cryptographic authenticity is not verified.

The initial behavior reference is API 26. A lower `minSdk` is valid for
evaluation; a higher `targetSdk` does not by itself reject the package.
SDK/ABI/split preflight is an eligibility check, not proof that the app runs.

## Try the pinned fixture

```powershell
./scripts/fetch-fixtures.ps1 -ExtractDex
cargo run -p as-cli --locked -- inspect fixtures/cache/org.fdroid.fdroid_2000052.apk --json
cargo run -p as-cli --locked -- dex-summary fixtures/cache/fdroid-classes.dex --json
./scripts/check-m0.ps1
```

The fetch script validates size and SHA-256 against
[the fixture lock](../fixtures/corpus.lock.json). APKs remain in ignored
`fixtures/cache/`; they are downloaded separately, not shipped in Git.

`capabilities --json` describes the current `as-runtime` facade. Its execution
and device-verification flags remain false until application lifecycle
integration exists. The separate `as-probe`/NRO DEX/JNI harness has its own evidence.
See [the CLI reference](CLI.md) for this distinction.

## Move to the device proof

Use [building](BUILDING.md) to prepare the pinned nightly, WSL/libnx, JDK and
Android SDK. Then follow [M1 E2E](M1_E2E.md) to build and run the Ryujinx proof.
Do not treat its output package as the future consumer launcher.

The current test SD folder is `switch/androswitch-smoke/`. The intended app
storage layout under `switch/androswitch/`, including `apks/`, `Android/data/`
and `Android/obb/`, is created by the M2 package manager. Use `asctl install`
and `list` with an explicit `--root`, as described in [CLI](CLI.md).
For native/JNI proofs and hardware test inputs, follow [M2–M4](M2_M4_E2E.md).

For development, continue with [testing](TESTING.md),
[contributing](../CONTRIBUTING.md) and [credits](../CREDITS.md).
