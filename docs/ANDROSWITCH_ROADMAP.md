# AndroSwitch roadmap

This is AndroSwitch's status, separate from the inherited RustyDroid roadmap.
Implementation and verification are independent. `pending` is not a pass.

| Milestone | Deliverable / exit gate | Implementation | Host | Horizon emulator | Physical Switch |
| --- | --- | --- | --- | --- | --- |
| M0 | Fork/provenance, frozen fixtures, `asctl inspect`, upstream baseline/evidence | complete | passed for M0 | not_applicable | not_applicable |
| M1 | Portable core/libnx C ABI; allocator/log/clock/file/APK/DEX/interpreter smoke NRO agrees with host | complete for M1 gate | passed | passed in Ryujinx | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M2 | Atomic install/update; persistent shared Java/native VFS; restart/app-isolation tests | implemented for proof gate | passed | passed in Ryujinx | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M3 | ARM64 ELF dependencies/relocations/TLS/constructors/Bionic, including stripped objects | supported subset implemented | passed native oracle/core tests | passed native execution in Ryujinx | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M4 | DEX ↔ JNI ↔ native; references/strings/arrays/exceptions/callbacks/thread attach; Java/Kotlin mixed APK proof | implemented for proof gate | passed JVM JNI oracle | passed Java/C/Rust and Kotlin/C++ in Ryujinx | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M5 | Activity/resources/layout/views/Canvas/Looper; interactive Java/Kotlin lifecycle | supported subset implemented | passed | passed, 14 UI snapshots | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M6 | NativeActivity/assets/window/input/threading/EGL/GLES2/PCM audio | supported proof subset implemented | passed shared contracts; Android reference pending | passed native/mixed media corpus | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M7 | Console launcher/verified session teardown; scan/install/launch/return/update/uninstall; one copy-ready folder | implemented for the alpha corpus; failed-startup cleanup added | passed package workflow | console workflow / ten completed sessions; startup and boundary evidence in reports | tested on physical Switch — user confirmed 2026-10-24; detailed per-scenario/binary identity not recorded |
| M8 | SharedPreferences/native SQLite/reflection/collections/permissions; independent APK integration | bounded shared contracts implemented; wider integration ongoing | contract/JVM/persistence/fault tests; see candidate report | pending for new build | SQLite readable / SharedPreferences working — user reported 2026-10-24; detailed scenario results not recorded |
| M9 | Offline v0.1 corpus/lifecycle stress/benchmarks/copy-ready package/evidence matrix; independent first-screen gate | general DEX/memory/native/GLSurfaceView startup improvements in progress; release gate incomplete | repeated candidate corpus plus scoped new API/ABI contracts | unified SDK suite: 216 PASS / 3 clock queries NOT_RUN; independent retest reaches ClassLoader.getSystemClassLoader (Mekorama) and AtomicInteger construction (Candy); both return safely, neither has a first-screen pass; see the separate 2026-10-25 evidence records | unified suite 219/219 PASS — user confirmed 2026-10-25; raw suite journal/device hash not archived; independent games remain blocked and the full M9 gate is incomplete |
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

The M9 work now includes a complete latest-upstream merge and a source omission
gate. See [upstream porting](UPSTREAM_PORTING.md). This records every upstream
file and remaining host-only original VM source. **Full Horizon behavior parity
remains pending**, independent of source synchronization and older hardware
feedback. Complete shared-state/adapter porting and inherited contract coverage
before treating the latest upstream framework as device-compatible.

M7 improvements now use file-backed APKs with a 512 MiB container cap, bounded
copy/hash/entry reads and lazy native extraction. A 128 MiB original UI APK passes
host install/launch/asset/save/restart checks. In a later hardware test the user
reports successful installation of a Candy Crush APK larger than 65 MB, separately
from the earlier M1–M7 baseline. See [M7 storage evidence](M7_STORAGE_E2E.md).
M8 adds persistent SharedPreferences and real native SQLite, boxed collections,
DEX method reflection and a bounded permission policy. M9 adds a repeatable host
corpus, original AOSP sample, external Vector Pinball evaluation, measurements,
and a copy-ready candidate. The user now confirms working SQLite and
SharedPreferences on Switch, with no detailed scenario/binary identity supplied.
**M9 is not marked complete:** full regression evidence and broader independent
compatibility remain open. The earlier candidate blocked Candy Crush at its
32 MiB raw DEX guard. The next hardware logs admit all four DEX files and reach
native initialization, which rejects unavailable `getauxval`. Mekorama reaches
enum construction and rejects the missing portable Enum implementation.
Ryujinx reproduces both original startup failures with clean launcher return.
Shared Enum, formatter, orientation, property and communication contracts
advance startup beyond those original failures. A stock Ryujinx translator
failure on an untaken CPU-register read is reproduced separately; the isolated
[emulator diagnostic](../reports/m9-cpu-id-emulator-20261024.json) retains a
runtime fault for an actually executed unsupported instruction. The later
[corrected-time attempts](../reports/m9-time-fixed-startup-20261024.json) pass
Mekorama's native time failure and Candy's locale/UTF-8/PRNG/Class blockers.
The [next exact-NRO attempts](../reports/m9-bundle-atomic-finalization-ryujinx-20261024.json)
verify the shared Bundle and AtomicBoolean fixes: Mekorama reaches monitor
operations and Candy reaches Timer. Both complete native cleanup and return to
the launcher, including Candy's formerly pending current-thread C++ destructors.
Neither game has a recognizable first screen. Subsequent
[shared-API attempt](../reports/m9-lifecycle-threadlocal-startup-ryujinx-20261025.json)
pass monitor/Timer/Executor/resource/Bitmap/TextUtils/JNI constructor blockers,
then pass loader-aware Class.forName and Thread context lookup. They stop at
ThreadLocal construction on an analytics worker (Mekorama) and installed
package version lookup (Candy). The corrected extended SDK JNI context proof
now passes, alongside scoped lifecycle and worker-handler execution; the SDK
harness's emulator-exit timeout and explicit SD recovery remain recorded.
Earlier independent NRO proofs qualify actual
workers, native-suspended GC roots, PNG pixels and JNI constructor/thread values.
Those proofs are separate from the still-open game first-screen gate.
The original portrait fixture now has real 720x1280 buffers, four successful
GPU color readbacks, complete native frame captures and a clean launcher
return. The [Java-to-portrait run](../reports/horizon-atomic-capture-ryujinx-20261024.json)
retains its overall `attempted_render_unconfirmed` status because the Java CPU
capture was missed; both sessions' execution/cleanup and the native image
publication are verified separately. A later
[portrait-to-Java run](../reports/horizon-capture-generation-handoff-ryujinx-20261024.json)
verifies complete captures for both renderers, generation ownership, manual
fixture image review and safe return. Physical testing of these new changes is
still pending.
The immediate additional M9 gate is a recognizable first screen and safe Plus
return for both frozen Mekorama and Candy Crush APKs; it is still pending.
See [independent-app status](M9_EXTERNAL_APPS.md), [hardware feedback](HARDWARE_FEEDBACK.md) and
[M8/M9 contracts, exact limits and reproduction](M8_M9_E2E.md).

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
See [M2–M4 evidence and exact subset limits](M2_M4_E2E.md);
the user now confirms M1–M7 were tested on a physical Switch. This is recorded
as user-reported hardware testing, without inventing individual test results or
binary hashes. The later M7 improvement/M8/M9 hardware feedback is scoped to
large APK installation, SQLite and SharedPreferences; full scenario verification
remains open.
See [hardware feedback](HARDWARE_FEEDBACK.md).

M5 now executes Java/Kotlin Activities and visible software UI in Ryujinx;
see [M5 evidence](M5_UI_E2E.md). M6/M7 now integrate the supported native media
and console corpus into one NRO/folder. This is an alpha; M9's wider v0.1
corpus/benchmarks and external NDK samples remain pending. See
[M6/M7 scope and evidence](M6_M7_E2E.md).
The process-isolation assumption has been corrected in
[ADR 015](adr/015-portable-framework-and-session-boundary.md): NRO chainloading
does not create a fresh Horizon process.

## Corpus and scenarios

The general-runtime architecture does not confer general APK compatibility.
M9's release gate requires independent APK execution evidence alongside the
fixtures. Candy Crush and Mekorama remain blocked evaluation targets, not
compatibility passes. Before expanding milestones, prioritize measured DEX
memory use, general native dependency/ABI support and the shared Android APIs
required by independent apps. A larger DEX cap alone does not meet that gate.

`fixtures/corpus.lock.json` freezes F-Droid's real inspection APK, upstream JNI
C/C++/Rust sources, NDK samples and Vector Pinball sources. Future APK builds
need compiler/source provenance and actual output hashes. M1 includes a Java
semantics fixture with a JVM oracle. M4 adds original Java/Kotlin/JNI fixture
source, JVM native builds and ARM64 APK builds with recorded inputs/hashes.

Test SDK/ABI/split policy, malformed inputs, APK identity, DEX arithmetic and
exceptions, JNI/reentrancy/GC/TLS/thread callbacks, Java/native file sharing,
OBB streaming, restart/update failure/data retention, switching, UI/input,
graphics/audio, missing API diagnostics, and lifecycle memory stability.

## Unified development test APK

M9 foundation work now includes the [219-case conformance catalog](CONFORMANCE_SUITE.md)
in one normal Android SDK/NDK APK, without sensor tests. Host, diagnostic Ryujinx,
Android reference and physical Switch results remain separate. This catalog
does not close the independent Mekorama/Candy Crush first-screen gate.

The user-defined acceptance target for this suite is now **all 219 cases actually
passing**, with shared runtime/API fixes. A complete baseline journal is not the
exit gate. Non-PASS cases and emulator exclusions remain open. The 219-case gate
is additional to the independent-app and physical-device gates above.

The latest [unified-SDK baseline](../reports/conformance-mesa-20261025.json)
records **216 PASS and 3 NOT_RUN** in 20 diagnostic Ryujinx sessions. No completed
case is FAIL/ERROR/UNSUPPORTED/INTERRUPTED. The three read-only CPU/GPU/memory clock queries remain NOT_RUN on this
emulator because its PCV service is empty. The user subsequently confirmed
**219/219 PASS on a physical Switch**, including those observations. See the
[hardware record](../reports/conformance-hardware-20261025.json): it preserves
the direct aggregate report and the absence of an archived raw journal/device
binary hash. The hardware bundle contains all 219 cases and no exclusion files.
This closes the suite target at the user-reported hardware evidence level; the
independent-game and Android-reference gates remain separate.
