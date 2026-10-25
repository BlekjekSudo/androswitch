# AndroSwitch capability inventory

| Capability | Upstream | AndroSwitch |
| --- | --- | --- |
| ZIP/AXML/ARSC/manifest | `rd-apk` implementation/tests | Shared no_std port; Java APK parsed inside the Ryujinx NRO |
| Signing schemes | Presence detection | Detection only, not cryptographic verification |
| SDK policy | Execution rejects minSdk below 26 | Inspection accepts lower minSdk; targetSdk is not minimum |
| ARM64 selection | Native library inventory | Static preflight plus program-header ELF loading in the native proof backend |
| Required splits | Raw manifest available | Detected/reported; installation waits for M10 |
| DEX interpreter | Core opcodes, calls, heap, exceptions | Shared no_std Engine/dispatcher; 12 JVM results and six negative cases verified in Ryujinx |
| Android framework/lifecycle | Host subsets implemented | Supported Application/Activity/Intent/resource/View/Canvas/Looper subset; real Java/Kotlin DEX callbacks |
| DEX parser (not execution) | `rd-dex` implementation/tests | `no_std + alloc` build; real DEX smoke matched host in Ryujinx |
| Primitive wrapper families | `rd-vm::boxes` implements a bounded eight-class subset | Same source now used by desktop and portable dispatch; 86 JVM observations pass on both, before/after GC. Exact method/static-field metadata and VM-owned valueOf identities are shared. This is not full java.lang coverage; see the upstream reuse audit. |
| JNI/native | Bounded Unicorn/Python worker | Real Horizon ARM64 execution and VM JNI callbacks; supported subset below |
| Filesystem | Java in-memory VFS | Persistent shared Java/native VFS, private package mounts and opaque OBB reads |
| SQLite | Python subprocess/subset | Actual native SQLite C engine, durable 8 MiB snapshots and bounded Android SQLiteDatabase/helper/cursor contracts; no subprocess |
| Preferences/collections/reflection | Desktop subsets | Persistent XML SharedPreferences, GC-backed boxed collections/iterators and declared DEX method reflection; exact overloads in M8/M9 guide |
| Graphics/audio/input | Headless software snapshots | CPU UI framebuffer, actual NativeActivity EGL/GLES2, input queue and PCM AudioTrack/AAudio/OpenSL ES subset |
| Registry | Host agent facilities | Immutable copied-APK revisions, update/data retention, uninstall and explicit clear-data |
| Consumer launcher | Host agent facilities | One-NRO console; scan/install/update/launch/return/uninstall and separate clear-data; ten-session Ryujinx corpus |
| Network/Play/engines/other ABIs | Detection/partial upstream APIs | Beyond offline ARM64 v0.1 |

`asctl capabilities --json` reports implemented AndroSwitch behavior. Static
preflight passing does not mean an APK runs. Markers and fixture inspection do
not prove native execution.

The bounded APK method probe is separate from the application lifecycle runtime.
It does not launch F-Droid or an Activity. Missing APIs, platform fields, monitors
and native methods fail explicitly; a recognized method descriptor is not
evidence of framework support. See [M1 E2E](M1_E2E.md).

M4's Java/C/Rust and Kotlin/C++ fixtures exercise genuine DEX↔JNI calls,
reentrancy, GC roots, thread callbacks and shared files. This does not imply
that other apps, APIs or full Activities work. The current native subset has
explicit errors for missing imports, unsupported relocations/dependency cycles,
arbitrary `System.load` paths and direct Linux SVC instructions. JNI monitors,
weak references and unpaired UTF-16 strings remain unsupported. The tested
JNI instance/static field subset is implemented in the original M4 proof backend;
the product NRO binds descriptor-checked static fields, while its instance-field
table entries remain unavailable. These are distinct execution paths, not full reflection.
JNI `FindClass` still searches the session DEX/builtin registry. Native-caller
defining-loader selection, `JNI_OnLoad` loader association and bootstrap-only
lookup on a newly attached native thread are not implemented. Java
`Class.forName`/`loadClass` provenance checks do not establish that JNI behavior;
the attached-thread proof currently looks up platform classes only.
Default-attribute pthreads are covered; general pthread attributes and broader
NDK services need additional implementation. See [M2–M4 coverage and
reproduction](M2_M4_E2E.md). Native session roots remain alive until M7 proves
thread/media quiescence, then are released with JNI tables, TLS and mappings.
`asctl` execution flags describe its inspection/package host facade; product-NRO
evidence is listed separately. The user reports bundled sample execution and
native animation/beeps on a real Switch, and subsequently confirmed that the
display fix resolves native Plus return and startup stalls. Full scenario
verification remains pending. See
[hardware feedback](HARDWARE_FEEDBACK.md) and [M0 baseline](M0_BASELINE.md) for the
passing AndroSwitch checks and failing immutable-upstream reference tests.

M6/M7's exact subset and negative boundary tests are in [M6/M7](M6_M7_E2E.md).
Device APK containers now use file-backed reads and a 512 MiB cap. Materialized
metadata/resource entries remain capped at 64 MiB; native extraction has a
separate adaptive ceiling up to 128 MiB. Current source replaces the fixed
32 MiB raw DEX guard with decoded-memory admission accounting (parser ceiling
up to 256 MiB, raw ceiling up to 128 MiB) and live allocator reserve checks.
New memory/native/GLSurfaceView paths have scoped Horizon execution; full
regression and external-game first screens remain pending. See
[independent-app work and remaining limits](M9_EXTERNAL_APPS.md). The 128 MiB
container proof is host-tested. The user reports installation of a Candy Crush
APK larger than 65 MB on Switch; detailed file-backed scenarios remain unrecorded.
Assets/OBBs use bounded reads. Audio is one 48 kHz stereo PCM16 output. Full native_app_glue FD support,
AAudio callback/low-latency modes, audio input/effects, arbitrary applications,
Compose/WebView/modern engines and external NDK samples remain unverified.
Only bounded RGB/RGBA PNG icons decode; other icon formats use a visible fallback.

Mekorama 1.7.2 has **no compatibility pass**. Its earlier ARM64 run failed during
class initialization because `libBarcodeScannerLib.so` required unavailable
`libstdc++.so`. Current source adds shared C++ ABI functions and a real EGL-backed
GLSurfaceView path; a new first screen has not been verified. Candy Crush's
frozen APK also reaches the native backend boundary on host after DEX admission;
no actual native game startup is implied. The alpha reports unavailable behavior
after bounded cleanup; it does not fabricate the missing ABI. See
[diagnosis and evidence](STARTUP_FAILURES.md).

M8/M9 results are scoped by scenario, not an APK-wide compatibility badge:

| Scenario | Host | Latest observed emulator | Physical Switch |
| --- | --- | --- | --- |
| Preferences / renamed preferences, restart and GC | Contract tests passed | not run in emulator | SharedPreferences works (user report); individual renamed/restart/GC results not recorded |
| Native SQLite notes / renamed notes, transaction/cursor/reflection | Contract tests passed | not run in emulator | SQLite readable (user report); individual transaction/renamed/reflection results not recorded |
| AOSP HelloActivity original source/layout | Recorded in candidate corpus; static rendering only | not run | individual result not recorded |
| Vector Pinball 1.16.0, unmodified F-Droid APK | Shared metadata, source and monitors advance startup; now blocked at RandomAccessFile in MultiDex ZipUtil.getZipCrc; no gameplay | not run | individual result not recorded |
| Mekorama 1.7.2, hash frozen in startup lock | DEX admitted; generic framework contracts have SDK/JVM tests | NRO41 passes EnumMap, then requires Log.w(String,String,Throwable) on a worker; returns through teardown retaining original/cleanup errors; no first screen | latest user screenshot shows the earlier Enum initializer blocker; binary hash unrecorded |
| Candy Crush Saga 1.332.1.1, hash frozen in startup lock | 34,874,388 raw DEX bytes admitted; generic JNI static fields have VM and ARM64 C ABI tests | NRO41 advances into Java GameView setup, then requires GLSurfaceView.setEGLContextFactory; resource reset reports -5. No first-screen pass | latest logs confirm DEX admitted, then getauxval unavailable; teardown reaches launcher_ready; binary hash unrecorded |
| External NDK hello-jni/native-activity | Build/integration pending | pending | pending |

The [candidate report](../reports/m8-m9-candidate.json) records exact fixture
hashes, repeated-run counts and host measurements. [M8/M9 contracts](M8_M9_E2E.md)
list unavailable overloads, memory budgets, SQLite limitations, permission policy
and remaining release gates. Earlier M1–M7 hardware testing and the later scoped
M7–M9 candidate feedback are recorded separately in
[hardware feedback](HARDWARE_FEEDBACK.md). Immutable candidate/package reports
retain their original not-run statuses; the later feedback supplements them.
