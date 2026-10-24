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

The [latest shared-API startup runs](../reports/m9-classloader-startup-ryujinx-20261024.json)
use the same NRO and unmodified APK hashes. Mekorama passes monitor operations,
Executor construction, resource-name lookup, TextUtils and resource identifier
lookup and loader-aware `Class.forName`, then reports unavailable
`Application.registerActivityLifecycleCallbacks`.
Candy Crush passes Timer, inherited `getResources`, Bitmap decoding, native
constructors, `Thread.currentThread` and context loader lookup, then reports
unavailable `Thread.getDefaultUncaughtExceptionHandler()` during `onCreate`.
Both complete worker/native resource closure and return to the launcher.
Mekorama's original execution error and later `onPause` error remain recorded;
resource closure does not imply that every Java lifecycle callback succeeded.
Neither game has a verified first screen.

Independent SDK/NDK proof APKs establish [Timer execution](../reports/horizon-timer-ryujinx-20261024.json),
[Executor execution](../reports/m9-executors-horizon-proof-20261024.json),
[actual Bitmap pixels](../reports/horizon-bitmap-ryujinx-20261024.json) and
[native-suspended GC ownership](../reports/horizon-gc-owners-ryujinx-20261024.json).
These are separate framework/VM proofs, not game compatibility passes.
The earlier [NRO15 proof](../reports/m9-jni-identifiers-encoding-ryujinx-20261024.json) runs independent JNI constructor/thread checks: all
three constructor forms produce JVM-matching values, callbacks and exceptions;
named pthread attachment retains the expected identity across detach. The
session writes the exact expected UTF-8 result, displays its verified proof
screen and returns cleanly to the launcher.
The extended context-loader proof fails on NRO16 when a valid `jclass` is used
as a `java.lang.Class` receiver. The
[canonical class-reference host correction](../reports/m9-jni-class-objects-host-20261025.json)
is separate evidence and still requires a corrected NRO execution.

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
- Portable `java.lang.Enum` stores name/ordinal in budgeted, GC-traced heap
  slots, keeps DEX constructors/overrides active, and implements constant lookup,
  comparison and declaring-class identity. Generated `values()` methods use
  real shallow array clones. Three mandatory portable tests pass, including
  thirteen JVM comparisons repeated around GC and renamed enum types;
  see the [Enum receipt](../reports/m9-portable-enum-host-20261024.json).
  EnumSet/EnumMap, serialization and private Enum-field reflection remain outside
  this implementation.
- Portable `Class.getSimpleName()` reads actual DEX nesting annotations,
  including member, local and anonymous classes. Top-level dollar signs remain
  part of the name; primitive and array names follow descriptor semantics.
  Malformed metadata fails explicitly rather than guessing a nested name.
  Three [portable class-name tests](../reports/m9-portable-class-names-host.json)
  include 23 JVM comparisons repeated around GC. This is a bounded Class API,
  not a claim of complete reflection support.
- `Bundle`/`BaseBundle` and Intent extras share a typed map for all eight
  primitive types and String, preserving wide values and floating-point bits.
  Wrong-type lookups return the specified default, null keys remain valid, and
  GC roots/snapshots follow real values. Four
  [Bundle DEX tests](../reports/m9-bundle-host.json) cover two package identities,
  typed replacement, GC and bounded allocation failure. Parcel, Binder and
  additional value families remain unavailable.
- Portable `AtomicBoolean` uses a private budgeted heap slot and exclusive VM
  operations, preserving CAS state, independent identities, GC and actual DEX
  subclass dispatch. Two [atomic tests](../reports/m9-portable-atomic-host-20261024.json)
  include eighteen JVM cases repeated around GC. This does not implement Java
  Thread, other atomic classes or newer VarHandle access modes.
- Portable monitors track actual receiver/class ownership, recursion and held
  GC roots, including synchronized DEX/native calls. The
  [host monitor receipt](../reports/m9-portable-monitor-host.json) separates
  supported ownership from unavailable contention/wait behavior; actual
  Mekorama startup now advances beyond its former monitor error.
- Portable Throwable state preserves nullable messages, subclass overrides,
  super dispatch and VM/native-generated errors in a budgeted GC slot. Five
  [Throwable tests](../reports/m9-portable-throwable-host.json) include 51 JVM
  cases. Causes, suppressed exceptions and stack APIs remain unavailable.
- Timer scheduling uses a real dedicated platform worker with wall time,
  queued/running GC roots and cancellation. Ten
  [framework tests](../reports/m9-timer-framework-20261024.json) and the separate
  [Ryujinx proof](../reports/horizon-timer-ryujinx-20261024.json) cover different
  verification layers. A shared admission barrier prevents new callback workers
  from appearing behind completed joins during shutdown; see
  [worker ownership](HORIZON_WORKERS.md). This does not establish executor pools
  or arbitrary Java Thread support.
- Shared method dispatch preserves concrete/native/abstract DEX class-method
  precedence while allowing an implemented platform method through an interface
  declaration. The [independent dispatch NRO proof](../reports/m9-inherited-dispatch-horizon-proof-20261024.json)
  and renamed host fixture qualify this behavior without package checks.
- A bounded single-worker Executor uses actual Java Thread/ThreadFactory
  objects, FIFO queueing, GC roots and platform workers. The
  [framework receipt](../reports/m9-executors-framework-20261024.json) describes
  the supported pool shape and explicit Thread API gaps; the actual SDK proof
  separately verifies execution, VFS output and safe return.
- BitmapFactory decodes supported noninterlaced 8-bit RGB/RGBA/indexed PNGs
  into real immutable pixels, using actual resource aliases, density and API
  qualifiers. Bitmap/Rect/RectF and the supported Canvas overloads preserve
  alpha, source bounds and charged pixel leases through queued draws. The
  [host receipt](../reports/m9-bitmap-host.json) specifies allocation limits,
  unsupported formats/options and non-null Paint limitations; the NRO proof
  compares every submitted image pixel with the independent fixture pattern.
- Resource-name APIs read the owning ARSC package, type and entry key, including
  `typeIdOffset`, high package IDs and alias identity. They do not infer the
  owner from the Android manifest or resolve an alias to another entry's name.
  The [resource-name receipt](../reports/m9-resource-names-framework-20261024.json)
  distinguishes application `NotFoundException` from unavailable framework
  metadata. Renamed-manifest tests retain the table's original package name.
- Interpreter frames, diagnostics and temporary allocation scopes belong to
  the actual executing native thread. GC traces suspended owners while native
  calls release the VM lease; return/unwind cannot discard another owner's
  roots. The unchanged SDK/NDK APK fails on the old NRO and passes on the
  corrected NRO; see [worker ownership](HORIZON_WORKERS.md). This proves that
  lifetime scenario, not unrestricted Java concurrency.
- JNI `NewObject`, `NewObjectA` and `NewObjectV` share initialized allocation,
  descriptor-driven arguments, real DEX constructors and rooted receivers.
  The C varargs bridge uses the compiler's AArch64 ABI; exceptions remain
  pending and failed construction returns null. The
  [constructor host proof](../reports/m9-jni-constructors-host-20261024.json)
  separates portable execution from the independent native ABI oracle.
- JNI method lookup consults actual DEX declarations and an explicit catalog
  of implemented framework methods. Static kinds and declaring owners are
  preserved; constructors are not inherited. `Thread.currentThread` adopts
  the actual platform thread, with bounded metadata and real attach/detach
  lifetime. See [metadata and thread evidence](../reports/m9-jni-method-metadata-host-20261024.json).
  This does not implement arbitrary Thread methods or arbitrary Executor pools.
- Thread context loaders use those same canonical loader objects. Ordinary
  Java Thread construction inherits the parent's actual context; newly
  JNI-attached native peers use null/bootstrap. Detach and reattach create a
  new peer without mutating a retained old Thread. Six
  [metadata/context tests and twenty JVM rows](../reports/m9-context-loaders-host-20261024.json)
  establish the host contract. A subsequent NRO run is needed for device proof;
  system/parent loader APIs and custom loader execution are not added here.
- JNI class references now point to the VM's canonical `java.lang.Class`
  objects, using the same local/global reference and GC rules as other objects.
  Class references can be receivers, arguments and results; method/field IDs
  remain opaque metadata. Four
  [production-dispatch tests](../reports/m9-jni-class-objects-host-20261025.json)
  cover identity, object operations, GC, primitive/array Class values and
  invalid-reference failures. The extended SDK context proof still requires
  a successful NRO run after the observed class-reference rejection.
- Application lifecycle observers use the actual Application, Activity and
  saved Bundle objects, dispatching at the real base Activity method's API 26
  timing. Ordered snapshots survive mutation, GC and reentry; registrations are
  owned by their Application. Ten
  [portable tests](../reports/m9-application-lifecycle-host-20261025.json) include
  duplicate/null/error cases, missing-super rules, legacy targetSdk pause
  behavior and 42 JVM comparisons. `Object.equals` and `String.endsWith` are
  shared contracts with real DEX overrides preserved. Equality dispatch now
  selects implemented String/boxed value bodies before inherited Object
  identity. Object, arrays and DEX-only inheritance to Object have the identity
  body; unimplemented builtin equality bodies, including collection equality,
  report `API_UNAVAILABLE`. True invoke-super is dispatched by the opcode;
  a method reference declaring Object does not imply a nonvirtual call.
  The independent SDK
  proof records real startup/return callbacks; save remains `not_observed`
  unless an actual save callback occurs. NRO verification is still pending.
- Thread default/per-thread uncaught exception handlers use actual Java Thread
  and Throwable identities, including virtual handler lookup and root-group
  delegation. Nine [portable tests](../reports/m9-uncaught-handlers-host-20261025.json)
  cover worker ownership, main-boundary forwarding, handler failures, Timer/GL
  callbacks, GC and reentry. The original runtime worker-stop policy remains;
  dispatching a handler does not turn a failed worker into successful execution.
  Null default handlers retain runtime diagnostics. Custom ThreadGroup hierarchy,
  hidden pre-handlers and a System.err stack-trace sink remain unavailable.
  Android API 26 behavior after thread termination is recorded separately from
  the differing desktop JVM oracle. `logs/uncaught-exceptions.json` preserves
  original and handler failures after quiescence, bound to the session identity.
- Loader-aware `Class.forName` shares the actual app/bootstrap loader provenance
  and dependency checks with `ClassLoader.loadClass`. It honors optional class
  initialization, array component visibility and failure state. Four
  [host tests](../reports/m9-class-forname-host.json) compare 74 supported JVM
  results. Initialization that still requires an uninitialized default-bearing
  interface fails explicitly before mutation; automatic default-interface
  scheduling and custom class loaders remain unavailable.
  The [listener registry correction](../reports/m9-listener-registry-host.json)
  registers explicit interface facts for five callback contracts already
  implemented by the framework. It restores strict class loading of the
  ordinary SDK UI Activity; unknown sibling listeners remain unavailable.
- `Resources.getIdentifier` uses the same actual resource-table identity as
  the name APIs, with qualified/default names, aliases and signed numeric IDs.
  The [identifier tests](../reports/m9-resource-identifiers-host.json) include
  renamed manifests and high package IDs. Unknown Android framework resource
  metadata remains explicitly unavailable.
- `String.getBytes(String)` and default encoding share checked final-array
  allocation and six implemented encoders. Unsupported encodings, nulls and
  allocation failures retain their specified errors. See
  [encoding behavior and API 26 differences](PORTABLE_ENCODINGS.md), including
  the distinction between JVM evidence and Android-source-derived vectors.
- Native support extends C++ allocation/guards/destructor lifetimes, Bionic
  errno boundaries, pthread keys/once, stdio/stat/directory APIs, logging and
  module enumeration. Unsupported operations still report their actual error.
  Linux SVC scanning uses validated executable sections when present; stripped
  files without those sections retain conservative executable-segment scanning.
  The shared `getauxval` provider exposes the 4096-byte page granule, baseline
  ARM64 FP/ASIMD, no advertised HWCAP2 extensions, and non-secure execution.
  Successful queries preserve errno, including legitimate zero values;
  unavailable types return zero with `ENOENT`. Linux process/kernel pointers
  are absent. See the [auxv contract receipt](../reports/m9-native-auxv-20261024.json).
  Native `__system_property_get` and Java SDK facts share a read-only manifest
  containing API 26 and the implemented ARM64 ABI lists. Missing properties
  return an empty value without inventing an Android device identity; bounded
  writes, terminators and errno are covered by the
  [property contract receipt](../reports/m9-native-properties-20261024.json).
- The native locale provider keeps guest handles and per-thread selection
  separate from the host's global locale. Supported names, masks, lifetimes and
  unavailable operations are bounded by the
  [locale contract receipt](../reports/m9-native-locales-20261024.json).
  Restartable UTF-8 conversion uses the Android ARM64 state and wide-character
  layouts; [multibyte contracts](../reports/m9-native-multibyte-20261024.json)
  cover partial sequences, invalid input, bounds and thread-local state.
  `rand`/`random` and their seed functions use private API 26 algorithm state;
  `rand_r` uses the separate reference algorithm. The
  [PRNG receipt](../reports/m9-native-random-20261024.json) records 160,000
  comparisons against pinned upstream implementations, concurrency and reset
  tests. This does not implement a cryptographic random API.
  Native time conversion now accepts an actual empty timezone abbreviation
  while preserving the real calendar and numeric offset. The
  [time contract receipt](../reports/m9-native-time-empty-zone.json) covers empty
  names, non-UTC calendars, normalization and malformed formatter metadata.
  The runtime does not alter the date, force a timezone or bypass application
  decisions based on time.
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
  roots stay owned until the worker joins. Pre-surface orientation negotiation
  now rotates actual native-window buffers, software presentation and touch
  coordinates; Java display/configuration metrics use the same geometry.
  Active rotation changes, mixed software overlays, GLES1/3 and arbitrary Java
  GL10 calls remain unavailable. See the [display contract](HORIZON_DISPLAY.md)
  for the scoped host tests and separate GPU execution requirement.
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
package-metadata and portable-monitor blockers and now reports unavailable
`java.io.RandomAccessFile` in `MultiDex` ZIP checksum calculation. It remains
blocked, not compatible.

Source-bound receipts cover [native ABI](../reports/m9-native-abi.json),
[portable media contracts](../reports/m9-media-host.json),
[GL worker lifetimes](../reports/horizon-glsurface-host-20261024-v2.json),
[framework callbacks](../reports/m9-framework-host.json), and the
[original APK host evaluations](../reports/m9-external-host.json). Their scopes
separate recorded host results from unexecuted Horizon graphics and hardware.

The first external candidate was tested on physical Switch. Candy Crush's
current-session report stops during native initialization at `getauxval`;
Mekorama's screenshot stops while constructing a Java enum. Candy Crush's
teardown reaches `runtime_closed` and `launcher_ready`; this is a clean startup
rejection. Old GPU captures and an interrupted-session marker from another
package do not establish either game's rendering status.

After the user authorized desktop testing again, an actual
[Ryujinx reproduction](../reports/m9-external-ryujinx-baseline-20261024.json)
confirmed both blockers with the exact delivered NRO and frozen APK hashes.
Both installations matched their launch identities and both rejected startups
returned to the launcher. Neither application reached its first screen.
The original emulator SD folder was restored after the isolated run.

Portable startup contracts now also cover Enum constants and GC-traced identity,
bounded nonlocalized `String.format`, dynamic package-targeted broadcasts and
real installed service metadata. Service discovery does not provide Binder
IPC: a failed bind dispatches no connection callback and retains only its
required unbind-cleanup reservation. Implicit/external broadcasts and unknown
platform fields report `API_UNAVAILABLE` instead of silently returning success
or an invented null. DEX-declared fields retain Java's zero/null defaults.
See the [Enum](../reports/m9-portable-enum-host-20261024.json),
[formatter](../reports/m9-portable-format-host.json) and
[communication](../reports/m9-communication-host.json) host contracts. These
fixtures use multiple package identities and real DEX callbacks; their host
passes do not establish a game first screen.

The [next execution observations](../reports/m9-external-ryujinx-progress-20261024.json)
bind intermediate builds to exact NRO hashes: the Enum/auxv candidate advances
Mekorama to orientation negotiation and Candy Crush to
`__system_property_get`. A separate property-only diagnostic advances Candy
into its native CPU resolver, where stock Ryujinx throws an unhandled
translator exception for `MRS ID_AA64PFR1_EL1`. That run has no completed guest
report and is an emulator failure, not a successful app launch or evidence that
the CPU instruction actually executed. Neither observation closes the M9 gate.

The [CPU diagnostic](../reports/m9-cpu-id-emulator-20261024.json) confirms that
the resolver's CPU-register read is behind an untaken HWCAP guard. A task-local
copy of the same emulator defers unknown `MRS` rejection until execution; an
executed unknown instruction still faults at its real PC. Twenty synthetic
guarded/executed cases pass, and all other bundled payloads remain identical.
The installed emulator and APKs are unchanged. The subsequent
[actual Candy run](../reports/m9-candy-emulator-progress-20261024.json) reaches
the shared Bionic `newlocale` boundary and returns cleanly. This diagnostic
result is recorded separately from stock-emulator and hardware evidence.
Reproduction and source/license details are in
[the emulator diagnostic tools](../tools/ryujinx-cfg/README.md).

The shared [Bionic locale implementation](../reports/m9-native-locales-20261024.json)
owns Android locale handles and thread selections separately from host libc.
Its C/POSIX and UTF-8 contracts include numeric conversions, locale conventions,
wide classification and translated Android `tm` formatting. Unknown locale
names fail explicitly; host global locale settings are never changed. Host
contracts and strict ARM64 compilation pass. Exact Android ICU table/version
equivalence and the unsupported `strftime_l` `%s` extension are outside that
evidence. Product execution is recorded separately for each NRO.

The harness stages only the supplied paths and does not change desktop window
configuration:

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

For startup faults, `-TraceNative` stages `logs/native-trace.enabled` in the
isolated test folder. The runtime then records bounded native enter/leave
addresses and scalar argument values in `native-calls.log`; `native-modules.log`
provides the actual library bases. `jni-failures.log` records bounded API lookup
identities and exception/error classes while preserving JNI exception behavior.
The trace omits Java string contents. Remove the marker to disable call tracing
on a manually configured SD folder. Each native session starts fresh diagnostic
logs. These diagnostics establish where execution stopped; a trace is not a
compatibility pass.

The harness accepts explicit `-Ryujinx` and `-RyujinxProfile` paths and
`-Headless` for the SDL2 frontend. The launcher regression harness uses the same
explicit profile arguments. Both retain their desktop opt-in guard and restore
the test SD folder after their owned process has stopped.

After passing the recorded checks/corpus and committing the implementation,
freeze a fresh report without overwriting the original M8/M9 observations:

```powershell
python scripts/record-m8-m9-evidence.py --output reports/<fresh-candidate>.json --ryujinx-evidence local-artifacts/m9/external-runs/<run-id>/verification.json
./scripts/package-androswitch.ps1 -Stage m9-candidate -EvidencePath reports/<fresh-candidate>.json
```

The harness requires matching installation/session identities, completed runtime
execution, fresh nonuniform renderer captures and return to the launcher. Its best
automatic result is `execution_verified_visual_review_required`; a person must
verify that the captured image is a recognizable application screen. A generic
nonblank framebuffer is insufficient. Every new build needs its own NRO-bound
execution evidence; earlier physical Switch observations remain separate.

Collect `session-report.json`, `native-compatibility.json`,
`memory-admission.json`, `memory.log`, `teardown.log`, Android logs and fresh
frame captures. Retain the exact NRO/APK hashes. Failed startup is evidence of
a remaining API/ABI gap; it must not be converted into a compatibility pass.
