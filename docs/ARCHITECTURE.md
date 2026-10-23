# AndroSwitch architecture

## Product contract

Run unmodified APKs through common Android semantics. No per-package execution
profiles, title offsets, game-specific library names or fake success responses.
The initial reference environment is API 26, offline, ARM64 native libraries,
multidex, and one active application. Newer targetSdk alone is not a blocker;
minSdk above the reference environment is. Required splits wait for M10.

## Components

`as-runtime` is the facade consumed by the host CLI and eventually launcher and
runner. Inspection and persistent package management are implemented; lifecycle
launch/stop follows M5–M7. `as-platform` defines portable
storage, clock and logging contracts, with host and libnx filesystem adapters. Further services
are added with their implementation milestones. The inherited `rd-*` crates
retain their upstream behavior and tests.

M1 adds a `std` feature boundary to `rd-vm` and `rd-apk`. The device uses the
same Engine, opcode dispatcher, classpath, heap and exception code as the host.
Desktop framework services remain behind std. Portable VM side state is owned
per Engine, including interned strings and canonical class objects, with a
precise full mark-sweep collector. See [ADR 012](adr/012-portable-interpreter.md).
The no_std APK path shares ZIP/AXML/ARSC/manifest code and uses bounded raw
DEFLATE inflation on both host and device. Filesystem opening stays in adapters.

`as-switch-smoke` is the libnx proof bridge; `as-probe` is its host test CLI.
They consume the same descriptor-based request, without package-specific
runtime branches. It now binds persistent packages, the shared native loader
and real JNI to the Engine. This remains separate from the future
`as-runtime` application lifecycle facade. See [M2–M4](M2_M4_E2E.md),
[ADR 013](adr/013-persistent-package-revisions.md) and
[ADR 014](adr/014-native-horizon-jni.md).

The device path uses `no_std + alloc` and C ABI adapters for libnx. OS access,
environment configuration, thread-local VM state and subprocesses must leave
the portable core. Host tools may retain `std` and Unicorn as an oracle.
Unicorn results do not verify native Horizon execution.

DEX and native execution share one class/object model, VFS, lifecycle, clock and
application identity. JNI resolves actual framework/DEX methods, with tracked
local/global references. Cross-thread callbacks and GC roots require tests.
Host and Android TLS remain distinct across bridge transitions. Android Linux
syscalls must never be forwarded unchanged to Horizon.

The loader uses ELF program headers and dynamic metadata, including stripped
objects. Dependencies, symbol resolution, relocation, TLS and constructors are
generic. Missing imports remain structured compatibility gaps. Detecting an
ELF or language marker is not execution proof.

## Package and storage behavior (M2)

Installation copies the APK into `installed/<package>/` using atomic staging.
Updates preserve data. Uninstall preserves it unless deletion is requested.
One VFS resolves paths for Java and native APIs:

| Android path | Runtime storage |
| --- | --- |
| `/sdcard/Android/data/<package>` | `Android/data/<package>` |
| `/sdcard/Android/obb/<package>` | `Android/obb/<package>` |
| `/storage/emulated/0/DCIM` | `DCIM` |
| `/storage/emulated/0/Download` | `Download` |
| `/data/user/0/<package>` | `userdata/<package>` |

Normalize guest paths before mount resolution and separate app-private storage.
OBBs remain opaque files; stream reads instead of loading complete archives.
M0 inspection uses a bounded 512 MiB whole-APK reader, not the final large-asset
runtime reader.

## Lifecycle (M7)

`androswitch.nro` presents apps/imports. `runtime/runner.nro` executes one app in
a fresh process using a versioned internal request carrying package, install
revision and run ID. Normal exit chains back to the launcher. Diagnose a native
crash on next launch; seamless recovery from arbitrary crashes is not promised.

## Verification

Track implementation, host, Horizon emulator and physical device separately.
CLI JSON is versioned; errors include operation, module and cause. Static
inspection checks SDK, native ABI inventory and split markers. It does not
prove API coverage, DEX/ELF validity, APK authenticity or execution. Record
fixture identity, toolchain, commands and results with each milestone.
