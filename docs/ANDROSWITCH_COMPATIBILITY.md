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
| JNI/native | Bounded Unicorn/Python worker | Real Horizon ARM64 execution and VM JNI callbacks; supported subset below |
| Filesystem | Java in-memory VFS | Persistent shared Java/native VFS, private package mounts and opaque OBB reads |
| SQLite | Python subprocess/subset | Device backend planned for M8 |
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
JNI instance/static field subset is implemented; this is not full reflection.
Default-attribute pthreads are covered; general pthread attributes and broader
NDK services need additional implementation. See [M2–M4 coverage and
reproduction](M2_M4_E2E.md). Native session roots remain alive until M7 proves
thread/media quiescence, then are released with JNI tables, TLS and mappings.
`asctl` execution flags describe its inspection/package host facade; product-NRO
evidence is listed separately. Physical
Switch verification remains pending. See [M0 baseline](M0_BASELINE.md) for the
passing AndroSwitch checks and failing immutable-upstream reference tests.

M6/M7's exact subset and negative boundary tests are in [M6/M7](M6_M7_E2E.md).
Device APKs are currently limited to 64 MiB, while assets/OBBs use bounded
reads. Audio is one 48 kHz stereo PCM16 output. Full native_app_glue FD support,
AAudio callback/low-latency modes, audio input/effects, arbitrary applications,
Compose/WebView/modern engines and external NDK samples remain unverified.
Only bounded RGB/RGBA PNG icons decode; other icon formats use a visible fallback.
