# AndroSwitch architecture

## Product contract

Run unmodified APKs through common Android semantics. No per-package execution
profiles, title offsets, game-specific library names or fake success responses.
The initial reference environment is API 26, offline, ARM64 native libraries,
multidex, and one active application. Newer targetSdk alone is not a blocker;
minSdk above the reference environment is. Required splits wait for M10.

## Components

`as-runtime` is the inspection/package facade consumed by the host CLI. The
device console uses `as-launcher`, `as-package` and the `as-framework::App`
session adapter through documented C ABI entry points in `as-switch-smoke`.
Host CLI execution flags describe that inspection facade, not product-NRO
corpus verification. `as-platform` defines portable
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
OBBs remain opaque files with positioned reads. Asset readers share APK bytes
and inflate incrementally with bounded output and a 32 KiB dictionary. M0 host
inspection accepts up to 512 MiB. The M7 improvement path also accepts containers
up to 512 MiB using an owned positional reader through the shared ZIP parser.
Installation, registry verification, icons and execution avoid whole-container
reads. Materialized entries, total DEX input and directory/signing metadata have
separate budgets; see [storage improvements](M7_STORAGE_E2E.md).
The process heap is shared by decoded DEX, VM objects, native source
images/mappings and platform/media allocations. Its actual capacity comes from
the loader's heap override when supplied. The previous `__nx_heap_size = 256 MiB`
setting was a default for the non-override path, not a fixed cap on hbloader
sessions. The current non-override path derives a bounded heap from the process
memory limit, retaining service/kernel headroom. Allocation snapshots and
separate parser/native admission limits prevent container size from being
mistaken for available runtime memory; see [Horizon memory policy and
provenance](HORIZON_MEMORY.md). Streamed OBBs and file-backed APKs use the same
persistent root. These changes require new execution evidence and do not
complete M9's external-application compatibility gate.

## Lifecycle (M7)

`androswitch.nro` presents apps/imports and executes one guest at a time,
using a versioned request carrying package, install revision and session ID.
NRO chainloading uses the same Horizon process. M7 verifies guest threads,
media and mappings are quiescent before returning to the launcher; otherwise
exit the Horizon application and report the reason on next launch. See
[ADR 015](adr/015-portable-framework-and-session-boundary.md). Seamless recovery
from arbitrary native crashes is not promised.
One-shot requests are consumed before execution. The pending-session marker
survives an interrupted process and is archived/shown on next boot. The normal
path destroys JNI veneers/tables, guest TLS/heap, synchronization objects, ELF
mappings and media before allowing a new App. See [M6/M7](M6_M7_E2E.md).

Startup is owned by a `Running` transaction before Java initialization or native
entry. A failed startup returns an owner token to C for watchdog-bounded teardown,
not an already-dropped VM with live JNI roots. The abort path cancels Java tasks
and closes PCM tracks without invoking lifecycle methods on partly initialized
objects. Native activity allocations are retained before calling guest onCreate.
The same callback/thread/media checks gate reuse after both success and failure;
cleanup failure preserves the startup cause and requires process exit.

The Horizon platform retains process-owned nvdrv/nvmap/fence/GPU service guards
across frontend changes. Guest EGL contexts and surfaces are released per session;
their underlying shared service client is not torn down between CPU and EGL
presentation. CPU presentation uses a checked triple-buffer queue and licensed
libnx sector conversion. Native returns monitor eight successful console
submissions, with operation/result/slot diagnostics on display failures. Runtime
GPU dumps are opt-in to scripted tests, not interactive startup work.

## Verification

M8 uses `as-storage` for typed XML preferences and real native SQLite snapshots.
The shared framework owns collection/editor/helper/cursor state and reports its
object graph to the same VM collector used by JNI. Method reflection resolves
actual DEX definitions. Resource lookup selects the newest SDK variant <=26;
the inherited inspection resolver retains its original default preference.
See [M8/M9 contracts](M8_M9_E2E.md) for snapshot, API and configuration limits.

Track implementation, host, Horizon emulator and physical device separately.
CLI JSON is versioned; errors include operation, module and cause. Static
inspection checks SDK, native ABI inventory and split markers. It does not
prove API coverage, DEX/ELF validity, APK authenticity or execution. Record
fixture identity, toolchain, commands and results with each milestone.
