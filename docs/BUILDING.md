# Building AndroSwitch

## Current device profile: Horizon std

The device product now builds real Rust `std` with the pinned compiler commit
`787af2b8c80638c51a4fc8e44f84e6891f243ec7` and a task-local Horizon/newlib
source overlay. Installed toolchain sources and global defaults are unchanged.
Use an already installed selector with matching rust-src (`-Toolchain`).
`build-androswitch.ps1` prepares the isolated libc/std adapters automatically.

```powershell
./scripts/build-androswitch.ps1 -Toolchain nightly -LinkBackend Windows `
  -MesaSdk local-artifacts/dependencies/mesa-switch-0ff52079/sdk/opt/devkitpro/portlibs/switch
```

The unified Mesa build uses LLVM objcopy from NDK 28.2.13676358 by default;
`-MesaObjcopy` accepts another explicit path. It isolates Mesa's older Rust
personality in copied archives. Product objects use
`local-artifacts/product-mesa-std-windows`; the Rust library is under
`target/switch/aarch64-androswitch-newlib/release`. `-TargetDirectory` overrides
that cache. `-RustOnly` builds the same std library without linking a product.
The parser smoke builder delegates to this profile as well. Earlier M1
freestanding core/alloc commands and results below are historical; current
builders use std. See [qualification and limits](HORIZON_STD_MIGRATION.md).

M2–M4 native/JNI fixture builds additionally require Android NDK
`28.2.13676358`, Kotlin compiler dependencies in the Gradle cache and a native
Windows GCC/G++ toolchain for the JVM JNI oracle. See the explicit parameters
in `scripts/build-m4-fixture.ps1` and [M2–M4 instructions](M2_M4_E2E.md).
Keep `CARGO_BUILD_JOBS=1` on low-memory machines; serialize verification builds.

The host CLI and device proof are different build paths. The current
automated NRO workflow uses Windows Rust and the WSL Ubuntu devkitPro linker.
The product builder also accepts `-LinkBackend Windows` with an existing native
devkitPro installation. It uses a separate `local-artifacts/product-windows`
object directory and records the linker backend in `product-build.json`.
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

If WSL is unavailable, the equivalent product build can use the installed
Windows toolchain without changing the WSL distribution:

```powershell
./scripts/build-androswitch.ps1 -LinkBackend Windows -WindowsDevkitPro C:/devkitPro -WindowsMake C:/w64devkit/bin/make.exe
```

The 2026-10-24 fallback used native devkitA64 GCC 15.2.0, because an external
Ubuntu compaction made WSL reject startup with `E_ILLEGAL_STATE_CHANGE`. It built
the entire C object set separately, then linked the same freestanding Rust
library. This is recorded as a Windows build, separately from prior WSL builds.

The recorded M1 run used Microsoft OpenJDK 21.0.12.1 and devkitA64 GCC 16.1.0.
Source/build-tool hashes and the tested emulator hash/version are in
[the fixture lock](../fixtures/corpus.lock.json) and
[M1 report](../reports/m1-core.json). devkitPro installation is managed
separately using its [official documentation](https://devkitpro.org/).

## Host tools

M8's framework now links native SQLite. Before building framework/launcher/NRO
targets, run `python scripts/fetch-sqlite.py`. It downloads the pinned official
amalgamation and validates its hash. Host SQLite builds need a GNU C compiler
and `ar` (Windows defaults: `C:/w64devkit/bin/gcc.exe` and `ar.exe`; override
with `CC`/`AR`). The Switch Makefile compiles the same source with devkitA64.
There is no Python dependency on the device. The host inspection CLI alone
does not require this database backend.

The consolidated host regression command is `./scripts/check-m7-m9.ps1` after
building the fixtures listed in [M8/M9](M8_M9_E2E.md). It does not open Ryujinx.
Pass `-NativeHostBackend Msys` to select the installed POSIX host compiler for
native C checks explicitly; see [native host backends](HOST_NATIVE_TESTS.md).
`-ContinueAfterFailure` records remaining independent checks, then returns a
failure if any required check failed. It cannot turn a partial run into a pass.

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

## Build the historical parser smoke with the std profile

Use an installed selector with the matching compiler and rust-src. The helper
validates commit `787af2b8c` and creates its independent source overlay; it does
not change the global default or install components:

```powershell
./scripts/build-switch-smoke.ps1 -Toolchain nightly
```

The script delegates to the same pinned product Rust builder. It builds
`core`, `alloc`, `std` and `panic_abort` with `-Zbuild-std` for the custom
`aarch64-androswitch-newlib` target, then invokes the libnx
[Makefile](../platform/switch-smoke/Makefile) in WSL. WSL paths are derived
from the Windows checkout path; no fixed developer checkout is needed.

| Output | Location |
| --- | --- |
| Rust static library | `target/switch/aarch64-androswitch-newlib/release/libas_switch_smoke.a` |
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

## M7 product, fixtures and package

Install the devkitPro Switch Mesa/GL portlibs in the same WSL toolchain as
libnx. The recorded packages are libnx `4.12.0-1`, Switch Mesa `20.1.0-5`,
and `switch-libdrm_nouveau 1.0.1-2`. Mesa includes a C++ shader compiler, so
the product linker also includes libstdc++. These are the historical M7 tools;
the current unified SDK/std profile is documented above.
Use `cargo +nightly ...` only for the freestanding build; do not change the
global toolchain. This workspace's installed nightly corresponds to the pinned
manifest date documented above.

```powershell
./scripts/build-m5-fixture.ps1
./scripts/build-m6-fixture.ps1
./scripts/build-androswitch.ps1 -Toolchain nightly-2026-08-26
./scripts/test-ryujinx-launcher.ps1
./scripts/test-ryujinx-launcher.ps1 -Scenario quiescence
./scripts/package-androswitch.ps1
```

`build-m6-fixture.ps1` uses NDK `28.2.13676358`, compile API 26 and SDK
build-tools 35; it builds ordinary NativeActivity and mixed Java/JNI media APKs.
An installed Python with only its standard library produces the original PNG
test icon. Kotlin M5 fixtures use the compiler/cache versions recorded by the
existing M4/M5 builder. The output NRO is
`local-artifacts/product/androswitch.nro`; release ZIPs and their hashes appear
under `local-artifacts/releases/`. Package generation collects full linked
dependency notices and retains RustyDroid's attribution. Large outputs stay
ignored. The package has one product NRO; no runner NRO is required.

To keep device Cargo output on a faster drive, pass an absolute task-local
directory, for example
`./scripts/build-androswitch.ps1 -TargetDirectory C:/Users/Administrator/.cache/androswitch/switch-target`.
The default remains `target/switch`. The NRO output location stays the same,
and `target/evidence/product-build.json` records the selected target directory.

See [M6/M7](M6_M7_E2E.md) for verification scope and [using the alpha](USING_ANDROSWITCH.md)
for SD layout, controls, data retention and the current compatibility limits.

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
