# M2–M4: persistent packages and real ARM64 JNI

These milestones extend the shared RustyDroid interpreter to a persistent
package environment and native Horizon execution. This is a developer method
runner, not an Activity launcher. The console launcher is M7; UI/framework is M5.
Physical Switch testing remains **not run**.

Tracked execution records: [M2 storage report](../reports/m2-storage.json),
[M3 native report](../reports/m3-native.json) and
[M4 JNI report](../reports/m4-jni.json). Each
backend has its own result; creating an artifact is not a hardware pass.
The [host regression receipt](../reports/m2-m4-host.json) records commands,
test counts and log hashes, including the 227 upstream VM regression tests.

The observed Ryujinx suites contain seven M2 storage scenarios, seven M3
native scenarios (five CPU-oracle comparisons and two expected errors), and
29 M4 JNI scenarios (27 JVM comparisons and two named missing-API errors).

## What is implemented

### M2: packages and shared storage

`as-package` is `no_std + alloc`. It validates SDK/ABI/split policy, duplicate
ZIP entries and each DEX checksum, then copies the APK to an immutable revision:

```text
installed/<package>/<20-digit-revision>/package.apk
installed/<package>/<20-digit-revision>/commit.json
```

The installer reserves a new directory, writes and re-reads the APK, prepares
data directories and publishes a checksummed record with a rename to a new
commit marker. Readers ignore unfinished revisions. Previous committed APKs
are retained across updates. A failed update never overwrites the old APK.
The tests inject failures before 40 filesystem mutations; abrupt power loss
and faulty SD media have not been tested. The libnx adapter commits writes
before publication and reports an indeterminate publication if rollback fails.

Uninstall publishes a tombstone and removes APK payloads, retaining private
data and shared Android data/OBB. `clear-data` is a separate explicit action.
APK signatures are recorded as **not cryptographically verified**. The current
whole-APK install/read budget is 512 MiB; the method harness input is 64 MiB.
Large OBB reads are independent, bounded reads, without automatic extraction.

Java `File`, `FileInputStream`, `FileOutputStream` and native stdio all use
`as-platform::vfs::Vfs`. Internal/runtime/registry directories are excluded
from shared mounts; private mounts permit the active package only. Windows
reparse points are rejected. Package names that differ only by case are
rejected explicitly for the case-insensitive SD storage model.

### M3: native ARM64 execution

`as-native::Loader` consumes upstream `rd-ndk::elf::Elf` program headers and
dynamic metadata, including files with no section table. The same loader runs
with a host Unicorn oracle or the native libnx memory backend. Dependencies,
export resolution, constructors and supported relocations have no package or
game-name branches.

The initial relocation set is AArch64 ABS64/GLOB_DAT/JUMP_SLOT/RELATIVE,
upstream-expanded RELR and TLS relocation forms. PT_TLS templates, TP-relative
storage and TLSDESC are implemented. Android packed APS2 and other relocation
forms fail with an identified compatibility error. Dependency cycles currently
fail explicitly. This is a supported subset of Bionic, not a replacement for
every libc function. See the imports in `platform/switch-smoke/native.c` and
`bionic_io.c` for the exact implementation.

The AAPCS64 bridge handles independent integer/SIMD registers and stack spills.
Every guest→host import restores host TLS; native entry sets per-thread guest
TLS. Default-attribute pthread creation/join and JNI thread attach/detach use
real libnx threads. Unsupported pthread attributes return `ENOTSUP`.
Executable segments containing Linux SVC instructions are rejected before
execution, with library and offset. This conservative scan may also reject
matching instruction words in literal pools. Linux syscalls are not forwarded
to Horizon.

### M4: DEX ↔ JNI ↔ native

The portable Engine has a reentrant `NativeHost` interface. `System.loadLibrary`
loads APK ARM64 libraries, runs constructors and invokes `JNI_OnLoad`.
`RegisterNatives` resolves real DEX native declarations; JNI symbol lookup uses
upstream descriptor/mangling utilities. Arbitrary `System.load` VFS paths are
reported as unavailable.

JNI uses the pinned AOSP ABI header. Implemented calls include class/method
lookup, local/global references and frames, modified UTF-8/UTF-16 strings,
primitive arrays/elements/regions, pending exceptions, static/virtual callbacks
and JavaVM thread attachment. JNI object tokens refer to real Engine heap
objects. The collector includes local/global roots and pending throwable roots.
Unpaired UTF-16 surrogates currently fail explicitly because the VM stores
strings as Rust Unicode strings. Weak references, monitors, field access and
other unimplemented JNI functions have named failure traps; they do not return
fabricated success values.

Callbacks and root DEX execution acquire one reentrant VM lease. Foreign native
calls release it, permitting callbacks from other threads, and reacquire it on
return. State borrows are released before foreign calls or nested DEX execution.
The Engine allocation is stable for the runner lifetime. Native sessions and
code mappings are owned by the process, preventing unmapping code still used
by native threads. Shutting down the VM disables further callbacks. Switching
applications will use fresh runner processes in M7.

## Reproduce the evidence

Use the toolchains from [building](BUILDING.md), plus Android NDK
`28.2.13676358`. The local JVM native oracle uses native Windows GCC/G++
(`C:/w64devkit/bin` in the recorded setup). The Kotlin compiler and dependencies
are resolved from the Gradle cache by the fixture script. Kotlin 2.2.20 uses
language/metadata version 2.0 for compatibility with the pinned D8 35.0.0.
No APK is patched during a test. The same source implements JVM and ARM64 JNI.

```powershell
$env:CARGO_BUILD_JOBS = '1'
./scripts/build-m1-fixture.ps1
./scripts/build-m2-fixture.ps1
./scripts/build-m3-fixture.ps1
./scripts/build-m4-fixture.ps1
./scripts/build-switch-smoke.ps1
./scripts/test-ryujinx-m2.ps1 -SkipBuild
./scripts/test-ryujinx-probe.ps1 -SkipBuild -Apk local-artifacts/m3/fixture/native-loader.apk -Request local-artifacts/m3/fixture/request.json -Expected local-artifacts/m3/fixture/expected.json
./scripts/test-ryujinx-probe.ps1 -SkipBuild -SkipHost -Milestone M4 -Apk local-artifacts/m4/fixture/java-jni.apk -Request local-artifacts/m4/fixture/java-request.json -Expected local-artifacts/m4/fixture/java-expected.json
./scripts/test-ryujinx-probe.ps1 -SkipBuild -SkipHost -Milestone M4 -Apk local-artifacts/m4/fixture/kotlin-jni.apk -Request local-artifacts/m4/fixture/kotlin-request.json -Expected local-artifacts/m4/fixture/kotlin-expected.json
./scripts/test-ryujinx-probe.ps1 -SkipBuild -SkipHost -Milestone M4 -Apk local-artifacts/m4/fixture/java-jni-emutls.apk -Request local-artifacts/m4/fixture/emutls-request.json -Expected local-artifacts/m4/fixture/java-expected.json
```

`-SkipHost` here skips the pure-native Unicorn harness, which is not the M4
JNI oracle. M4 expected results come from actual JVM JNI execution during
`build-m4-fixture.ps1`; Rust, C and C++ DLLs are built from the included sources.
The M3 pure-native probe compares the same ARM64 images against Unicorn 2.1.4.
Python/Unicorn are host-only optional dependencies, absent from the NRO.

Each run owns an exclusive SD lock, assigns a fresh session ID, archives old
reports and starts/stops only its own Ryujinx process. It checks individual
results/errors, rather than treating file creation or emulator launch as a
pass. Receipts, fresh emulator JSON, host/oracle results and logs remain under
`local-artifacts/m2`, `m3` and `m4`; compact evidence is tracked in `reports/`.

M4 Java tests cover mixed scalars, register/stack overflow arguments,
floating-point callbacks, native registration, reentrancy, NUL/emoji strings,
array COMMIT/ABORT behavior, references across GC, exceptions in both directions,
pthread callback/TLS isolation, Java/native file sharing, Rust JNI and a named
missing-API error. The independent Kotlin/C++ APK exercises instance callback
lookup and `CallIntMethodA` through the same backend.

## Remaining scope

`scripts/package-m4-proof.ps1` packages the verified NRO, fixture APK/request,
expected results, instructions and attribution into separate Java, Kotlin and
NDK-emulated-TLS ZIPs under `local-artifacts/m4/hardware/`. These are developer
proof packages. Copy one ZIP's `switch/` contents to SD and run the NRO via
application/title override; collect the two JSON reports before testing the
next package. They preserve installation/save files under `switch/androswitch/`.

These proofs do not establish whole-app compatibility. Activity lifecycle,
resources/UI, NDK windows/graphics/audio, the installed-app launcher, SQLite,
networking and physical-device validation retain their roadmap milestones.
The pinned NDK samples and Vector Pinball remain independent future integration
targets; they are not marked passed by the fixture results. Original authorship
and imported component licenses are preserved in [credits](../CREDITS.md) and
[third-party notices](../third_party/README.md).
