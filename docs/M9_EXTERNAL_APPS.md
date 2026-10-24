# M9 independent Android application startup

The current integration target is the **first recognizable application screen**
in the unmodified Mekorama and Candy Crush APKs below, followed by a safe Plus
return to the console. This is an additional M9 gate, not a claim that arbitrary
Android applications work. Gameplay, saves, networking and all Android APIs are
separate compatibility scenarios. M9 remains incomplete until execution and
visual evidence establish the gate.

AndroSwitch continues to use the shared **RustyDroid** parsers/interpreter by
**nadabgfccvd and contributors**. Changes implement Android API/ABI contracts;
there are no package-name checks, title profiles, proprietary code additions or
game-specific native hooks. See [credits](../CREDITS.md).

## Frozen evaluation inputs

These are user-provided local files. They are not redistributed and their
download/source-build provenance has not been independently established.
The [external application lock](../fixtures/m9/startup-apks.lock.json) records
exact hashes, versions and container/DEX/native sizes.

| APK | Package | Minimum / target SDK | Combined raw DEX | Largest ARM64 library |
| --- | --- | --- | --- | --- |
| Mekorama 1.7.2, version code 44 | `com.martinmagni.mekorama` | 21 / 33 | 14,703,020 bytes | `libmekorama.so`, 2,343,584 bytes |
| Candy Crush Saga 1.332.1.1 | `com.king.candycrushsaga` | 23 / 36 | 34,874,388 bytes | `libcandycrushsaga.so`, 89,417,880 bytes |

Static inventory identifies imports and DEX calls; it cannot establish that a
path executes or that a game renders. The runtime does not select behavior
using the package/library names in this documentation.

## Shared implementation changes

- Loading uses a decoded-DEX admission estimate, checked before decoding,
  rather than the previous unconditional 32 MiB combined raw limit. With load
  budget `B`, the parser ceiling is `min(B/2, 256 MiB)` and the raw ceiling is
  half of that, capped at 128 MiB. Integrity, table bounds and duplicate string
  expansion are checked before allocating decoded tables. A live device
  allocator snapshot must retain at least 64 MiB headroom at checkpoints.
- APK containers remain file-backed with the existing 512 MiB limit. General
  metadata/resource entry materialization keeps the default 64 MiB budget.
  Native library extraction opts into a separate ceiling of 128 MiB, reduced
  on Horizon according to measured free allocator capacity and staging reserve.
  Large assets and OBBs continue to use bounded reads.
- Horizon heap selection honors the loader's supplied heap, or derives a
  bounded heap from actual process memory queries. See [memory policy and
  Autorun reference](HORIZON_MEMORY.md). Parser estimates are **not** allocator
  measurements; neither is the VM's separate 64 MiB managed heap ceiling.
- Package metadata, class loading, application directories and the read-only
  `/data/app/<package>/base.apk` alias refer to the installed immutable APK.
  Java and native file APIs share that VFS and package identity.
- Native support extends C++ allocation/guards/destructor lifetimes, Bionic
  errno boundaries, pthread keys/once, stdio/stat/directory APIs, logging and
  module enumeration. Unsupported operations still report their actual error.
  Linux SVC scanning uses validated executable sections when present; stripped
  files without those sections retain conservative executable-segment scanning.
- A backend may defer a strong unresolved **function** import to a named,
  nonreturning failure thunk. The first invocation fails with its library and
  symbol. Weak imports retain zero resolution, unresolved data remains an
  error, and unknown required libraries remain errors. Deferred thunks are not
  published as successful symbol-query results. The guest `dlopen`/`dlsym`/
  `dladdr` API family itself remains unavailable and traps if called; internal
  loader/JNI symbol resolution is a separate implemented path.
  `logs/native-compatibility.json` lists deferred functions;
  they remain unimplemented, even if unused during a successful session.
- `GLSurfaceView` uses a real platform EGL worker and actual DEX renderer/
  configuration-chooser callbacks, with real EGL config handles, GLES2, queued
  Runnables, dirty/continuous scheduling and pause/context lifetimes. JNI/GC
  roots stay owned until the worker joins. The first display contract supports
  one full-screen landscape GL surface; mixed software overlays, portrait
  transforms, GLES1/3 and arbitrary Java GL10 calls remain unavailable.
- OpenSL ES volume/mute scales the actual submitted PCM samples. The
  `libmediandk.so` provider implements the listed API21 `AMediaFormat` scalar,
  string, buffer and diagnostic-string operations with owned storage. This
  does not provide media codecs, extractors or video playback.

## Verification and reproduction

Host parsing of both frozen APKs reaches the entry Activity's class initializer
and `System.loadLibrary`, where the host runner correctly reports that its
native backend is unavailable. Observed parser admission estimates were
54,122,782 bytes for Mekorama and 120,862,904 bytes for Candy Crush, below the
host policy ceiling of 268,435,456 bytes. These are **blocked execution results**,
not first-screen passes. An evaluation-only corpus now reports
`evaluation_only` rather than a vacuous required-case pass.

Host contract tests separately cover memory admission, malformed expansion,
native imports/ABI, worker lifecycle and framework callbacks. A successful C
compile, host recording backend or native assembly oracle does not establish
Horizon EGL behavior or recognizable application rendering.

The refreshed required host corpus passes seven scenarios over 12 cycles each
(84 sessions). The independent Vector Pinball evaluation advances past the old
package-metadata blocker and now reports the unavailable portable monitor
implementation in `MultiDex.doInstallation`. It remains blocked, not compatible.

Source-bound receipts cover [native ABI](../reports/m9-native-abi.json),
[portable media contracts](../reports/m9-media-host.json),
[GL worker lifetimes](../reports/horizon-glsurface-host-20261024-v2.json),
[framework callbacks](../reports/m9-framework-host.json), and the
[original APK host evaluations](../reports/m9-external-host.json). Their scopes
separate recorded host results from unexecuted Horizon graphics and hardware.

The user selected physical Switch testing first for this build; the current
desktop emulator run was not attempted. The harness can stage only
the supplied paths and does not change desktop window configuration:

```powershell
./scripts/test-runtime-media.ps1
./scripts/test-native-abi.ps1
cargo test -p rd-dex --test memory_budget --no-default-features --locked
cargo test -p as-framework --features host --test glsurface --locked -- --include-ignored
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-external.ps1 -ApkPaths D:/Download/mekorama-1-7-2.apk,D:/Download/candycrush.apk -PrepareOnly
# Execution is separately opt-in after the earlier desktop-window incident:
./scripts/test-ryujinx-external.ps1 -ApkPaths D:/Download/mekorama-1-7-2.apk,D:/Download/candycrush.apk -AllowDesktopEmulator
```

After passing the recorded checks/corpus and committing the implementation,
freeze a fresh report without overwriting the original M8/M9 observations:

```powershell
python scripts/record-m8-m9-evidence.py --output reports/m9-external-candidate-20261024.json --physical-test-first
./scripts/package-androswitch.ps1 -Stage m9-candidate -EvidencePath reports/m9-external-candidate-20261024.json
```

The harness requires matching installation/session identities, completed runtime
execution, fresh nonuniform GPU captures and return to the launcher. Its best
automatic result is `execution_verified_visual_review_required`; a person must
verify that the captured image is a recognizable application screen. A generic
nonblank framebuffer is insufficient. Physical Switch results for this new
implementation remain unrun, separate from earlier user-reported M1–M9 tests.

Collect `session-report.json`, `native-compatibility.json`,
`memory-admission.json`, `memory.log`, `teardown.log`, Android logs and fresh
frame captures. Retain the exact NRO/APK hashes. Failed startup is evidence of
a remaining API/ABI gap; it must not be converted into a compatibility pass.
