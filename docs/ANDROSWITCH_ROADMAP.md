# AndroSwitch roadmap

This is AndroSwitch's status, separate from the inherited RustyDroid roadmap.
Implementation and verification are independent. `pending` is not a pass.

| Milestone | Deliverable / exit gate | Implementation | Host | Horizon emulator | Physical Switch |
| --- | --- | --- | --- | --- | --- |
| M0 | Fork/provenance, frozen fixtures, `asctl inspect`, upstream baseline/evidence | complete | passed for M0 | not_applicable | not_applicable |
| M1 | Portable core/libnx C ABI; allocator/log/clock/file/APK/DEX/interpreter smoke NRO agrees with host | complete for M1 gate | passed | passed in Ryujinx | pending |
| M2 | Atomic install/update; persistent shared Java/native VFS; restart/app-isolation tests | implemented for proof gate | passed | passed in Ryujinx | not run |
| M3 | ARM64 ELF dependencies/relocations/TLS/constructors/Bionic, including stripped objects | supported subset implemented | passed native oracle/core tests | passed native execution in Ryujinx | not run |
| M4 | DEX ↔ JNI ↔ native; references/strings/arrays/exceptions/callbacks/thread attach; Java/Kotlin mixed APK proof | implemented for proof gate | passed JVM JNI oracle | passed Java/C/Rust and Kotlin/C++ in Ryujinx | not run |
| M5 | Activity/resources/layout/views/Canvas/Looper; interactive Java/Kotlin lifecycle | planned | pending | pending | pending |
| M6 | NativeActivity/assets/window/input/threading/EGL/GLES2/PCM audio | planned | pending | pending | pending |
| M7 | Console launcher/fresh-process runner; scan/install/launch/return/update/uninstall | planned | pending | pending | pending |
| M8 | SharedPreferences/native SQLite/reflection/collections/permissions; independent APK integration | planned | pending | pending | pending |
| M9 | Offline v0.1 corpus/lifecycle stress/benchmarks/copy-ready package/evidence matrix | planned | pending | pending | pending |
| M10 | DNS/socket/TLS/HTTP(S), Java/native networking and split APKs | planned | pending | pending | pending |
| M11 | Higher API/GLES3/engine ecosystem; separate ARM32/x86 research | planned | pending | pending | pending |
| M12 | Physical Switch stability, suspend/resume, handheld/docked, migrations, v1.0 | planned | pending | pending | pending |

## Order and defaults

M0 → M1 → M2 → M3 → M4 precedes launcher work. M4 proves DEX calling actual
native code and native calling actual DEX. Reuse upstream algorithms/tests;
do not substitute narrow demo interpreters. Device builds exclude Python,
Unicorn and guest Linux. API reference: 26, accepting minSdk <= 26 for evaluation.
Initial native ABI: ARM64; DEX-only APKs need no native library. v0.1 is offline,
single-file APK plus multidex. A higher targetSdk is not a higher minimum.

Each milestone records source revision, fixture identity, commands, results and
missing checks. Preserve baseline failures. Compiles and host results alone do
not verify emulator/device behavior. No release dates or universal compatibility
are inferred from scaffolding.

## Current evidence

M0's nine verification commands passed (144 test executions across feature,
real-APK, parser and std/no_std class-index checks; formatting/clippy also pass).
The immutable upstream baseline has five failing MCP tests, documented in
[the baseline report](M0_BASELINE.md); it is not reported as passing.

The M1 NRO now reads the Java fixture APK and executes the shared upstream
interpreter without std. Twelve method results match a real JVM; six error
cases agree with the portable host. The real F-Droid DEX parser smoke also
remains passing. See [M1 E2E](M1_E2E.md). M1's host/emulator exit gate is
complete. The M1 result covers bounded method execution; Activity launch is M5
and JNI is covered separately by the M4 proof below.
M2 now provides copied-APK installation, immutable revisions and persistent
Java/native storage. M3 executes real ARM64 objects on Horizon and compares
the supported native suite against Unicorn. M4 executes JNI in both directions,
including Java/Kotlin, C/C++/Rust, exceptions, GC references and pthread callbacks.
See [M2–M4 evidence and exact subset limits](M2_M4_E2E.md). M5 is next;
physical-device testing remains pending at the user's request.

## Corpus and scenarios

`fixtures/corpus.lock.json` freezes F-Droid's real inspection APK, upstream JNI
C/C++/Rust sources, NDK samples and Vector Pinball sources. Future APK builds
need compiler/source provenance and actual output hashes. M1 includes a Java
semantics fixture with a JVM oracle. M4 adds original Java/Kotlin/JNI fixture
source, JVM native builds and ARM64 APK builds with recorded inputs/hashes.

Test SDK/ABI/split policy, malformed inputs, APK identity, DEX arithmetic and
exceptions, JNI/reentrancy/GC/TLS/thread callbacks, Java/native file sharing,
OBB streaming, restart/update failure/data retention, switching, UI/input,
graphics/audio, missing API diagnostics, and lifecycle memory stability.
