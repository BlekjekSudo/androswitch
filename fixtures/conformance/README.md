# AndroSwitch Conformance

One normal, development-signed **Android API 26 / ARM64 APK**, with 257 original
cases and no sensor tests. Install `AndroSwitchConformance.apk` once and use
**Run all / resume**. Results persist in the app's ordinary internal storage.
This is a growing, explicit catalog, not all Android behavior or official CTS.

The 257-case development catalog adds twenty-two Collections and sixteen text/regex contracts with actual
JVM expected results, including DEX AbstractList/AbstractCollection subclasses,
bulk operations, views, iterators and comparator callbacks that collect. The
earlier 219-case std package remains frozen and has its own user-reported
[physical result](../../reports/conformance-std-hardware-20261026.json). That
result does not qualify the changed catalog or a later NRO.

| Family | Cases | Examples |
| --- | ---: | --- |
| Java / DEX | 135 | arithmetic, exceptions, UTF-16, collections, reflection, TimeUnit, ThreadLocal, atomics |
| Android / platform | 89 | CPU access, host facts, Build, clocks, orientation, Activity/Application/Intent/Handler, files, preferences, Bundle, SQLite, audio, input editor |
| JNI / native / graphics | 33 | ARM64 calling convention, references, callbacks, GC, native thread, shared files, libc/TLS/clocks, EGL/GLES rendering, Vulkan discovery |

The runtime remains a history-preserving fork of
[RustyDroid by nadabgfccvd and contributors](https://github.com/nadabgfccvd/RustyDroid).
These first-party fixture sources use the repository's MIT OR Apache-2.0 license.
No CTS code, proprietary game code, game library, or package-specific runtime
hook is embedded. See [project credits](../../CREDITS.md).

## Build and test

From the runtime repository, with JDK, Python, Android SDK platform/build-tools
35 and NDK 28.2.13676358 available:

```powershell
./fixtures/conformance/build.ps1
./scripts/test-conformance.ps1 -SkipBuild
```

The build produces `local-artifacts/conformance/AndroSwitchConformance.apk`,
`catalog.json`, a 135-row **actual JDK oracle**, signature verification and a
source/toolchain/artifact hash receipt. It signs with a local public development
key; do not package the generated private key. Pure cases are compiled with
Java release 8, DEX minSdk 26, and native code targets aarch64-linux-android26.

The host driver installs this exact APK and executes its DEX. It recreates the
app VM after a runtime failure so one unsupported API does not poison other
cases. Host execution has no native Android, GPU or PCM backend. A completed
catalog with failures is not a compatibility pass. The JDK is a reference for
Java cases only, not an Android framework reference device.

## Switch workflow

Copy the matching runtime NRO and put the APK in
`switch/androswitch/apks/`. Install and open it from the console. Use touchscreen
or the runtime's usual UI navigation. Run all, or choose a category and run it.
Running a category archives the current report and clears only that category for a fresh run.
The renderer button selects **which tests to run**; it does not change Mesa's
active driver. Previous/next result shows status and a short diagnostic.

Reports are saved under:

```text
switch/androswitch/userdata/org.androswitch.conformance/files/conformance/
  report.json
  <catalog-hash-prefix>/case-<index>.json
  <catalog-hash-prefix>/pending.txt
  <catalog-hash-prefix>/history-<timestamp>.json
```

**Export report** also writes
`Download/AndroSwitch-Conformance/report.json`. Reset archives the old report
and clears this suite's results. Existing results are skipped on resume; reset
before collecting evidence for a different runtime build. The catalog digest
isolates changed catalogs, but does not identify the NRO by itself; retain the
bundle receipt and runtime session logs with hardware results.

A case is checkpointed before execution. If a VM/native failure exits the app,
open it again: the pending case becomes **INTERRUPTED**, and autorun continues
with the next case. This is process-interruption recovery, not power-loss
atomic storage. Ordinary catchable Java failures remain in the app and do not
require reopening it. An interrupted case is never labeled PASS; retain
`logs/session-report.json` and the per-session logs for its actual cause.

## Android reference and Ryujinx

The same signed APK can be installed on an ARM64 Android device/emulator with
API >= 26. On newer Android, runtime storage permission may be needed for the
optional external export. The internal report remains available to the app;
the development APK is debuggable for `adb shell run-as` retrieval. Actual
Android reference execution must be recorded separately from JDK and Horizon.

An authorized emulator run uses one installation and repeated ordinary app
launches to recover from unsupported runtime APIs:

```powershell
./scripts/test-conformance-ryujinx.ps1 -Ryujinx <owned-headless-build.exe> `
  -RyujinxProfile <isolated-profile> -AllowDesktopEmulator
```

An optional `-ExcludeCases` list writes an explicit app-private control file.
Every excluded case remains NOT_RUN and is listed in collected evidence. For
Ryujinx's missing PCV clock service, exclude the three `host.*.clock_hz` IDs;
leave them enabled for physical-device testing. This does not change runtime
API behavior.

`-PrepareOnly` creates the request/seed/hash snapshot without launching an
emulator or changing its SD. The harness retains the original SD, operates on
its owned emulator process, restores the SD, and preserves raw results. The
collector binds APK, NRO, catalog and sessions. Unknown/failed cleanup, missing
sessions, or an unfinished journal remain visible in the report.

See [the scope and platform contracts](../../docs/CONFORMANCE_SUITE.md).
