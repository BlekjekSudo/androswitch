# AndroSwitch capability inventory

| Capability | Upstream | AndroSwitch |
| --- | --- | --- |
| ZIP/AXML/ARSC/manifest | `rd-apk` implementation/tests | Shared no_std port; Java APK parsed inside the Ryujinx NRO |
| Signing schemes | Presence detection | Detection only, not cryptographic verification |
| SDK policy | Execution rejects minSdk below 26 | Inspection accepts lower minSdk; targetSdk is not minimum |
| ARM64 selection | Native library inventory | Static preflight plus program-header ELF loading in the native proof backend |
| Required splits | Raw manifest available | Detected/reported; installation waits for M10 |
| DEX interpreter | Core opcodes, calls, heap, exceptions | Shared no_std Engine/dispatcher; 12 JVM results and six negative cases verified in Ryujinx |
| Android framework/lifecycle | Host subsets implemented | Device framework services unavailable; M5 remains planned |
| DEX parser (not execution) | `rd-dex` implementation/tests | `no_std + alloc` build; real DEX smoke matched host in Ryujinx |
| JNI/native | Bounded Unicorn/Python worker | Real Horizon ARM64 execution and VM JNI callbacks; supported subset below |
| Filesystem | Java in-memory VFS | Persistent shared Java/native VFS, private package mounts and opaque OBB reads |
| SQLite | Python subprocess/subset | Device backend planned for M8 |
| Graphics/audio/input | Headless software snapshots | Horizon backend planned for M5/M6 |
| Registry | Host agent facilities | Immutable copied-APK revisions, update/data retention, uninstall and explicit clear-data |
| Consumer launcher | Host agent facilities | M7 pending |
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
weak references, field APIs and unpaired UTF-16 strings remain unsupported.
Default-attribute pthreads are covered; general pthread attributes and broader
NDK services need additional implementation. See [M2–M4 coverage and
reproduction](M2_M4_E2E.md). Native session lifetime follows the runner process.
`asctl` application execution verification flags remain false until lifecycle
execution is implemented/verified. Physical
Switch verification remains pending. See [M0 baseline](M0_BASELINE.md) for the
passing AndroSwitch checks and failing immutable-upstream reference tests.
