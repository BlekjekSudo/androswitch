# Building AndroSwitch

M2–M4 native/JNI fixture builds additionally require Android NDK
`28.2.13676358`, Kotlin compiler dependencies in the Gradle cache and a native
Windows GCC/G++ toolchain for the JVM JNI oracle. See the explicit parameters
in `scripts/build-m4-fixture.ps1` and [M2–M4 instructions](M2_M4_E2E.md).
Keep `CARGO_BUILD_JOBS=1` on low-memory machines; serialize verification builds.

The host CLI and device proof are different build paths. The current
automated NRO workflow uses Windows Rust and the WSL Ubuntu devkitPro linker.
No full Android OS, Python runtime or Unicorn backend is included in the NRO.

## Prerequisites

| Task | Tools |
| --- | --- |
| Host CLI / parser checks | Git, Rust 1.97.1, Cargo, host linker, rustfmt, clippy |
| PowerShell scripts | PowerShell 7 |
| Link-check script | Python 3, standard library only; this is a development check |
| Java/APK fixture | JDK with `java`/`javac` on PATH; Android SDK build-tools 35.0.0 and platform android-35 |
| Switch static library | Rust nightly-2026-08-26 with rust-src |
| Switch link/package | WSL Ubuntu, make, devkitPro under `/opt/devkitpro`, devkitA64, libnx and packaging tools |
| Emulator gate | An existing Ryujinx installation/profile; executable path supplied to the test script |

The recorded M1 run used Microsoft OpenJDK 21.0.12.1 and devkitA64 GCC 16.1.0.
Source/build-tool hashes and the tested emulator hash/version are in
[the fixture lock](../fixtures/corpus.lock.json) and
[M1 report](../reports/m1-core.json). devkitPro installation is managed
separately using its [official documentation](https://devkitpro.org/).

## Host tools

From the repository root:

```powershell
cargo build -p as-cli --locked
cargo run -p as-cli --locked -- --help
```

The binary is `target/debug/asctl.exe` on Windows. The root
[toolchain file](../rust-toolchain.toml) selects Rust 1.97.1 for this checkout;
there is no need to change the machine-wide Rust default.

## Build the Java fixture and JVM reference

```powershell
./scripts/build-m1-fixture.ps1
```

The default SDK path is `$env:LOCALAPPDATA/Android/Sdk`. For a different SDK:

```powershell
./scripts/build-m1-fixture.ps1 -AndroidSdk "D:/Android/Sdk"
```

The script compiles the included [Java source](../fixtures/m1/java/org/androswitch/fixture/Probe.java)
with `javac --release 8`, feeds the same class files to D8 with min API 24,
packages a binary manifest/APK, and runs the JVM oracle. It writes the APK,
request, expected results and build receipt to `local-artifacts/m1/fixture/`.
Generated APK ZIP timestamps can differ; each output receives its own hash.

## Build the freestanding Rust library and NRO

Install the dated toolchain explicitly; keep an unrelated `nightly` default
untouched:

```powershell
rustup toolchain install nightly-2026-08-26 --profile minimal --component rust-src
./scripts/build-switch-smoke.ps1 -Toolchain nightly-2026-08-26
```

The script verifies the nightly manifest date. It builds `core` and `alloc`
with `-Zbuild-std=core,alloc` for
`aarch64-nintendo-switch-freestanding`, then invokes the libnx
[Makefile](../platform/switch-smoke/Makefile) in WSL. WSL paths are derived
from the Windows checkout path; no fixed developer checkout is needed.

| Output | Location |
| --- | --- |
| Rust static library | `target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a` |
| ELF / NACP / NRO | `local-artifacts/m1/androswitch-smoke.*` |
| Prepared SD package | `local-artifacts/m1/package/switch/androswitch-smoke/` |
| Run reports / emulator logs | `local-artifacts/m1/runs/<session>/` |

## Run with existing installations

After the explicit builds above:

```powershell
./scripts/test-ryujinx-m1.ps1 -SkipBuild -SkipFixtureBuild -Ryujinx "D:/Emulators/Ryujinx/Ryujinx.exe"
./scripts/check-m1.ps1 -SkipFixtureBuild
```

Pass `-RyujinxProfile` if the emulator uses a profile outside
`$env:APPDATA/Ryujinx`. For the full default rebuild path, omit the skip
switches. That path expects `nightly` itself to be the pinned date and the
SDK in its default location. The explicit dated-toolchain/custom-SDK steps
above avoid changing global defaults.

The E2E script writes its own inputs under the emulator's
`sdcard/switch/androswitch-smoke/`. It also assembles the local package with
the NRO, real DEX parser fixture, test APK and request. This is a developer
proof, not `androswitch.nro` or the planned install/launcher package.

## Troubleshooting

- **Nightly date rejected:** use `-Toolchain nightly-2026-08-26`, then run E2E
  with `-SkipBuild`. A different nightly is not the recorded toolchain.
- **Missing d8/aapt2/android.jar:** install the SDK packages listed above and
  build the fixture with the correct `-AndroidSdk` value.
- **Linker/tool missing in WSL:** check `/opt/devkitpro` and the Makefile's
  devkitA64/libnx paths. A host Rust build does not produce an NRO by itself.
- **WSL systemd-session warning:** the recorded environment emitted this
  warning even on successful builds. Use the actual compiler/linker exit
  status and artifacts to determine the result.
- **Emulator timeout:** inspect the run's stdout/stderr and verification
  receipt, plus `-RyujinxProfile`. An old SD report is deliberately rejected.
- **Existing E2E lock:** confirm that no test is running before removing a
  leftover `e2e.lock` from this test's SD folder. Do not remove another run's lock.

See [testing](TESTING.md) for regression gates and [M1 E2E](M1_E2E.md) for the
verification protocol and current limitations.
