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

The [latest shared-API startup run](../reports/m9-field-cow-startup-ryujinx-20261025.json)
uses the same NRO and unmodified APK hashes. Mekorama passes monitor operations,
Executor construction, resource-name lookup, TextUtils and resource identifier
lookup, loader-aware `Class.forName`, Application lifecycle registration and
ThreadLocal initialization. Its Executor next reports unavailable
`Class.getDeclaredField` during `DynamiteModule.getLocalVersion`.
Candy Crush passes Timer, inherited `getResources`, Bitmap decoding, native
constructors, `Thread.currentThread`, context loader and installed package
version lookup, then reports unavailable `CopyOnWriteArrayList` construction
in `ActivityHelper` during `onCreate`.
Both complete worker/native resource closure and return to the launcher.
Mekorama's original execution error and later `onPause` error remain recorded;
resource closure does not imply that every Java lifecycle callback succeeded.
Neither game has a verified first screen.

The [NRO18 SDK proofs](../reports/m9-package-threadlocal-horizon-20261025.json)
verify real Java Thread identity and separate main/worker ThreadLocal values
101/202, including stored null and collection, followed by a main Handler
callback and exact VFS transcript. The installed package proof reads the actual
version and mounted APK header, and catches typed unknown/null package errors.
Both present frames and return safely; the isolated emulator stops and restores
its original SD state. TextView multiline rendering still substitutes newline
glyphs and clips the metadata proof after its recognizable PASS label; the
complete facts are checked independently through the file transcript.
See [ThreadLocal contracts](JAVA_THREAD_LOCAL.md),
[installed metadata contracts](../fixtures/m9/package-info/README.md), and their
[host metadata receipt](../reports/m9-package-info-host-20261025.json).

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
The extended context-loader proof failed on NRO16 when a valid `jclass` was used
as a `java.lang.Class` receiver. The
[corrected NRO17 SDK run](../reports/m9-callback-jni-ryujinx-20261025.json)
verifies the canonical class-reference correction, including context value103,
all constructor values and its actual proof image. The same run verifies six
real lifecycle callbacks and a worker handler with original Thread/Throwable
identity, followed by a successful next application. Saved state was not
observed on device. The overall SDK harness retained `cleanup_failed` after
its owned emulator exceeded a ten-second exit wait; a separate, identity-checked
recovery record restores the originally absent SD folder state after process
exit. Those scoped runtime proofs do not relabel that harness result as a pass.

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
  invalid-reference failures. The corrected NRO17 SDK context proof now writes
  the expected context value103 after the earlier class-reference rejection.
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
  The [expanded dispatch receipt](../reports/m9-object-equality-guard-host-20261025.json)
  records 72 supported JVM comparisons, eight explicit unsupported assertions,
  and five production JNI tests. It also records the corrected mismatched
  wrapper-type recursion found by those extra cases.
  The independent SDK
  proof records real startup/return callbacks; save remains `not_observed`
  unless an actual save callback occurs. NRO17 verifies C/S/R and P/T/D; saved
  state remains host-tested and unobserved in that device-proof scenario.
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


The next shared host contracts add genuine
[CopyOnWriteArrayList snapshots](JAVA_COPY_ON_WRITE.md),
[DEX field reflection](JAVA_FIELD_REFLECTION.md), canonical `TimeUnit` constants
and saturating conversions. The [TimeUnit receipt](../reports/m9-time-unit-host-20261025.json)
records 3,990 JVM comparisons. The
[portable execution contract](PORTABLE_EXECUTION.md) also guards unfinished
class initialization by its actual owner and retains native local allocations
across callback GC. These scoped host passes do not establish either external
application's first screen. The [NRO19 SDK run](../reports/m9-cow-fields-timeunit-horizon-20261025.json)
verifies all three proofs, exact VFS transcripts and launcher return on one
binary. Its captures have recognizable PASS labels; existing single-line
TextView handling still clips multiline TimeUnit text.

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


The [NRO19 original-game run](../reports/m9-input-runtime-startup-ryujinx-20261025.json)
passes the prior Field/COW startup boundaries. Candy Crush next requires the
shared BaseInputConnection state in a DEX subclass. Mekorama reaches a
surface-created callback, then billing initialization requests Runtime.getRuntime.
Neither produces a verified game screen. Both installed identities match the
original frozen APK hashes and both return to the launcher; the test SD folder
is restored normally. Missing contracts remain failures, not no-op successes.


The later [Runtime/Mekorama NRO20 run](../reports/m9-runtime-resources-horizon-20261025.json)
verifies a genuine per-VM Runtime singleton across GC and two fresh libnx process
CoreMask queries (three allowed CPUs in this emulator process). The original
Mekorama APK then passes that initializer and reaches its billing purchases
callback; a DEX subclass now needs the shared AbstractCollection base contract.
There is still no verified game screen. Both sessions return and SD restoration
completes. Candy Crush was not rerun on NRO20; its latest observed input-connection
failure remains the separate NRO19 result.

## Earlier unified-SDK retest (2026-10-25)

After the shared conformance repairs, a separate two-APK run with the Mesa
NVC0/NVK build still has no first-screen pass. Candy Crush now reaches
`SimpleInputConnection` construction and needs genuine shared `AtomicInteger`
state. Mekorama advances into its optional Dynamite module discovery and calls
the missing `ClassLoader.getSystemClassLoader` contract. Implementing that Java
contract will not itself supply Google Play Services. Both failures release the
session and return to the console; the original test SD directory is restored.
See [exact APK/NRO identities and errors](../reports/m9-mesa-independent-apps-20261025.json).
The independent-game gate remains open alongside the separate 219-case target.

## Shared atomic, loader and provider contracts (2026-10-25)

The next startup work implements API 26 scalar `AtomicInteger` and `AtomicLong`
operations in the portable VM. Private GC-owned value slots preserve identity;
CAS, wrapping arithmetic, pre/post update results and Number conversions share
the Engine/JNI serialization gate. Functional updater callbacks and Java 9
access modes remain unavailable. The JVM differential fixture now has 62 rows
(including the existing AtomicBoolean cases), repeated before and after GC.

The application and system loaders are distinct, rooted PathClassLoader
identities with a bootstrap parent. The system loader cannot see APK DEX;
the application loader can. JNI method-ID lookup follows modeled platform
superclasses as well as DEX metadata, without inheriting constructors or accepting
an incorrect static kind or descriptor.

`PackageManager.resolveContentProvider(String,int)` supports flags zero over
the committed installation registry. It reads ZIP directories/manifests instead
of materializing APK bodies. An exact authority or semicolon-separated alias
returns a fresh, GC-owned ProviderInfo snapshot; updates, uninstall and disabled
applications/components affect subsequent queries. A missing authority returns
null only after this lookup. Resource-valued unresolved declarations, unsupported
flags and path-permission objects produce explicit errors. Provider activation,
ContentResolver transport and Google Play Services are not supplied by this
metadata query. API 26 exported defaults, empty permissions, single-user and
instant-visibility flags follow the Android PackageParser reference.

`getApplicationInfo(String,int)` shares the same installed identity and
NameNotFoundException rules. Flags zero and GET_META_DATA are supported; literal
String/boolean/int/float values retain their Bundle types, `android:resource`
retains its resource ID, and `android:value` resolves references against the
currently mounted installation's resource table. Resource resolution for another
installation and other flags remain explicitly unavailable. This does not claim
support for every ApplicationInfo field or the full application flag bitset.

The API 26 View default-focus-highlight getter/setter retain per-view state.
On the software UI renderer the setting enables/disables a visible fallback
outline on the current keyboard-selected clickable view. Touch removes that
keyboard indication. The focused tests toggle it through normal SDK DEX calls
and verify actual changed pixels in both fixture identities. Android theme
drawables, general focusable-view policy and GL/CPU overlay composition remain
outside this bounded fallback.

Bundle/Intent scalar storage now also retains primitive and String arrays by
reference, including shallow snapshot aliases, exact array type checks, null
defaults and GC edges. The SDK fixture covers all nine array kinds; Parcel,
Parcelable arrays and transport remain unavailable. PackageManager's feature
queries expose the implemented offline guest profile (single-pointer touch,
fake touch and both screen orientations), with API26 version comparisons. They
do not advertise unavailable sensors, radios, multitouch or broader gamepad APIs
based on the physical console alone. Never-installed package queries produce
NameNotFoundException before unsupported field flags; retained/uninstalled
records still require implemented visibility flags and cannot use this shortcut.

The VM has no Java debugger attachment/wait backend; the two supported
`android.os.Debug` status queries report that state. Native call tracing is not
a Java debugger. JNI ExceptionDescribe clears the thread's pending exception
before callbacks, prints its actual object detail and captured DEX frames to the
shared System.err log sink, and has an explicit no-pending no-op. This does not
implement Java StackTraceElement, cause or suppressed-exception APIs. An earlier
Candy run faulted in a native destructor after an unsupported startup call;
that failed attempt is retained separately and is not a safe-return pass.

Subsequent shared contracts add process-local Binder ownership (`attachInterface`,
descriptor lookup and actual owner identity) and canonical base Locale objects.
Binder owners/descriptors are ordinary GC-scanned heap references; this creates
no external service or Binder/Parcel transport. The default Locale is the
declared `persist.sys.locale=en-US` guest profile, with per-VM `setDefault`, base
constructors/getters and ROOT. Horizon system-language discovery, script/Unicode
extensions and ICU display names remain unavailable. Five ordinary JVM Locale
cases and both renamed SDK fixture identities cover the supported base subset.

`Class.newInstance()` invokes a real, declared no-argument DEX constructor,
checks class/member access and shared initialization admission, and propagates
constructor exceptions directly. It rejects abstract/interface/primitive/array
types and does not inherit a parent's constructor. The JVM corpus covers 16
observations, including a constructor GC, private/same-package access, failed
initialization and repeat failure. Platform constructor reflection is not
advertised by this DEX-only implementation.

The product JNI table now connects GetStaticFieldID and all nine typed static
getters/setters to descriptor-checked declarations, real DEX static storage and
the explicit platform-field catalog. It supports inherited DEX fields, actual
class initialization, private JNI access, narrow/wide/FP values and reference
GC roots. Same-name static fields that the VM cannot store separately are
rejected. Platform writes and product instance-field entry points remain
unavailable. An additional NDK-to-Horizon-GCC oracle executes the production C
entry points for 19 cases, including signed narrow results and exact NaN bits;
this CPU oracle is separate from VM semantics and Horizon execution. The required
host regression list now includes this oracle (67 gates instead of the earlier 66).

The NRO34 retest advances Mekorama through Binder and Class.newInstance to a
missing Math.max(int,int) call; it returns to the launcher with its worker and
cleanup errors preserved. Candy Crush passes Locale and JNI static fields, then
requests mmap. Its abort-time native finalization faults, so that run has no
safe-return pass. The isolated harness stops only its owned emulator process
and records restoration of the original SD tree. Neither game has a first-screen
pass. NRO34 SHA256: `c085042ee2413741a646735e78ce1fee9a37bc420cea20ab720c706302ced4a9`.

Shared Math/StrictMath min/max now implement integer bounds, NaN propagation
and signed-zero ordering for all four scalar overloads. An ordinary JVM corpus
provides 1,024 observations, replayed through unchanged DEX before and after GC.

The guest native adapter now implements owned, page-aligned private anonymous
mmap/mmap64, mprotect, partial munmap and bounded madvise contracts. Backing is
zeroed and Horizon permissions are applied with svcSetMemoryPermission; NONE,
READ and READ|WRITE are supported. Partial unmaps revoke access immediately but
retain the backing allocation until its last page is unmapped. Reset restores
RW before releasing retained backing, after guest threads/finalizers quiesce.
This is eager physical allocation, not demand paging or an overcommit promise.
File/shared/fixed/executable mappings, mremap and DONTNEED on non-RW pages return
explicit unsupported errors. The Mesa SDK's separate mmap wrapper is unchanged.
The host memory test uses actual Win32 page permissions on MSYS (whose POSIX
mprotect rejects a 4 KiB middle-page range), or POSIX mmap/mprotect on Linux;
permission-failure injection checks that ownership is retained. Host results
alone do not prove Horizon mapping behavior.

NRO35 executes five successful anonymous mappings in Candy Crush (four 4 KiB
and one 270,336-byte RW allocation), then stops at fileno. Mekorama passes the
Math blocker and next requires AtomicReference. Both are still blocked; Candy's
abort finalization again faults. The new shared AtomicReference subset provides
get/set/lazySet/getAndSet and identity-based CAS/weakCAS, including null,
GC-scanned values and actual object toString callbacks. Fourteen JVM observations
cover identity versus equals, subclass dispatch, callback GC and replacement;
host tests additionally prove that replaced/unrooted values are collected.

Native FILE objects now have session-owned descriptors for fileno, fstat/fstat64,
read/write, pread/pwrite, lseek and close. They use the same unbuffered VFS position;
positioned I/O preserves it, closed descriptors are not reused within the session,
and stale IDs return EBADF. A recursive VM lease serializes descriptor lookup,
stdio position updates and close without holding the native metadata lock while
entering the VFS. Transfers return bounded short counts. Standard console streams
retain IDs 0/1/2; terminal reads and their synthetic stat records are unavailable.
This remains a path-backed VFS: open-file behavior across rename/unlink, dup,
open/fdopen and general POSIX file-descriptor APIs are not advertised by these
entry points. No host/newlib descriptor escapes into an APK. Contract tests cover
FILE/FD interleaving, positioned access, metadata layout, closure and stale IDs.

NRO36 passes AtomicReference and next finds android.util.Pair in Mekorama.
Candy's separate attempt times out before an application-session report or DEX
memory-admission record, so it provides no new API diagnosis; the original SD
tree is restored. Pair now has ordinary GC-scanned first/second fields,
constructor/create, null-safe component equality, XOR component hashes and
toString callbacks. Tests use two package identities, callback GC, lifetime
release, Unicode String hashing and asymmetric equals to preserve API26's exact
argument-component-first delegation. Component behavior delegates to the VM;
an unavailable component method still fails explicitly. See
[AOSP API26 Pair](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-8.0.0_r4/core/java/android/util/Pair.java).

These are shared framework contracts, exercised through ordinary SDK DEX calls
and renamed package fixtures. They do not add package-specific startup branches.
Original-game execution and the remaining first-screen gate are recorded in the
[startup retest](../reports/m9-atomic-loader-provider-20261025.json), separately
from the user's 219/219 physical-Switch result for the earlier binary.

## Upstream reuse correction (2026-10-26)

The [source-routing audit](UPSTREAM_REUSE_AUDIT.md) identifies existing upstream
implementations excluded from the device feature graph. Work now starts with
those implementations and their tests, porting coherent API families before
adding independent replacements. Existing single-threaded/snapshot assumptions
are not automatically valid for native callbacks or Android contracts.

The inherited `rd-vm::boxes` source file now compiles for both feature paths,
with `alloc` imports and shared device/JNI dispatch metadata. It provides the
implemented subsets of all eight primitive wrappers. Framework boxing calls
from reflection and SharedPreferences delegate to it. The duplicate device
wrapper allocation/cache, conversion and equality implementation was removed;
the bounded valueOf cache is now Engine-owned and traced by both collectors.
Boolean constants retain their actual static identity; constructors allocate
separate objects. Static-field lookup uses exact descriptors, including TYPE.

The numeric parsers were extracted from inherited `numerics.rs` into shared
`primitive_parse.rs`. Differential checks correct byte/short/char comparison
differences, floating-to-int saturation before byte/short narrowing, constructor
range/whitespace/null errors, invalid repeated/special-value float suffixes, and
direct f32 decimal rounding. Unported Unicode-digit and hexadecimal-floating
input domains return named capability errors. Broader numeric utilities,
Character Unicode APIs, wrapper parsing overloads and full Java float-format
compatibility are not implied by this extraction.

One ordinary JVM/DEX fixture supplies 86 observations, replayed before and after
GC on both the inherited desktop VM and the actual portable VM. Both pass. Ten
portable JNI metadata/constructor contracts pass, including use of this shared
module. The fixture reads Boolean constants before calling valueOf: this exposed
an additional portable sget binding gap, reproduced in a retained failing run.
The device interpreter now uses the shared descriptor-checked constant resolver
even without a FrameworkHost; the same ordered corpus then passes. Three new
required checks build this fixture and test both paths; the
registered regression catalog grows from 67 to 70. This does not retroactively
turn an earlier 67-check receipt into a complete 70-check run. The larger
framework regression, NRO execution and physical-device evidence retain their
own statuses.

The preceding NRO37 Mekorama attempt passed Pair and stopped at the missing
ConcurrentHashMap family. The already-developed portable map adapter has seven
communication tests including callback GC and reentrant mutation, with a
24-observation JVM oracle. It remains a bounded serialized map, not a claim of
the entire concurrent collections API or a completed unification of upstream
collection state. Its required differences are recorded by the reuse audit.

Two NRO37 Candy retries provided no new Android API evidence: one explicitly
ran out of host disk during APK staging; the other timed out on a dedicated
HDD profile before DEX admission. The host harness now checks staging space
before moving the SD tree. Seven completed, owned SD result directories were
moved to `local-artifacts/m9/sd-archive-20261026` with verified per-file SHA256
manifests. Their immutable original receipts retain the former C: paths; the
archive manifests provide the relocation mapping. Neither game has a verified
first screen from these attempts.

The NRO39 retest uses source `ef4c4bd5194a6d56055edd0b7e14745cff964375`
and SHA256 `05b6d79166e3cf6f638de72449d3d40ef69410fa40ee54b4f8235f274542a8e1`.
Mekorama advances past ConcurrentHashMap to CopyOnWriteArraySet, then returns
through complete teardown. Candy Crush passes the former FILE/FD boundary and
reaches fsync; its session error is recorded, but abort-time native destructors
still prevent a completed launcher workflow. Its DEX admission reaches vm_ready
with 34,874,388 raw bytes. These attempts have no first-screen pass.

NRO40 (`0e3119ee37a296c049b6dc4382544f41a3fb1f23`, SHA256
`fc206d8f46321e05e82cd6941167fec0aa1fc59066ecafa937548e3622781958`)
passes the COW Set and file-sync boundaries. Mekorama next requires EnumMap and
returns through full teardown. Candy Crush next calls sigaltstack; its failure
is recorded, but native abort finalization still faults. Neither attempt has a
guest first-screen capture. Both dedicated emulator SD trees were restored.

NRO41 (`c45552d78fd82f974881b8582fd7ad94fe21d2f0`, SHA256
`e6cf3be395013ea4b46e862fdb1389d411c3a2bfadb53f34225007bbba38422c`)
passes EnumMap. Mekorama then requires the three-argument Log.w overload and
returns through teardown, retaining the worker and cleanup errors. Candy Crush
takes the signal-unavailable ABI path and advances into Java GameView setup,
where GLSurfaceView.setEGLContextFactory remains unavailable. Resource reset
then reports -5; a complete safe-return workflow has not been verified. Both
APK hashes match the frozen originals, both emulator SD trees were restored,
and neither has a first-screen capture.

The next reuse batch extracts the existing RustyDroid Log dispatch from
android_text into android_log, compiled by both VM paths. Levels, supported
Throwable summary overloads, println, byte counting, descriptor metadata and
the bounded 64 KiB admission check are shared. Output is routed to the actual
host/device sink, with short writes and storage errors propagated. The same
four contracts execute on both VMs; an ordinary SDK fixture also verifies file
content and a DEX Throwable.toString callback that runs GC, in two namespaces.
This preserves a bounded logging subset: complete stack frames and Android's
wtf handler/termination behavior are not implemented. The device does not
advertise getStackTraceString. Two regression gates bring the catalog to 72;
a full 72-gate run is not claimed.

NRO42 (`60abf7fab0421771ca701e2ab97e9794c11a8421`, SHA256
`b3e7a69d5005f41d8d66a0e83475fb0581b1d9e729c1560144c0aeb4a6015c1e`)
passes the logging boundary. Mekorama loads its native libraries and reports
the next missing Integer.highestOneBit from a consent SDK path; the diagnostic
stack also reaches RendererWrapper.onSurfaceCreated. This is not a presented
application frame. Candy Crush repeats the custom EGL context factory blocker.
Both SD trees were restored and neither run has a first-screen pass.

The inherited Integer/Long bit and scalar match arms are extracted from
numerics into shared integer_bits, rather than implementing highestOneBit
alone in a device class. The existing wrapper corpus now also runs 518 JVM
utility results and four caught zero-divisor cases on both VM paths, before
and after GC. It covers 32/64-bit boundary values, rotation distances including
negative/oversized shifts, unsigned arithmetic, byte/bit reversal, comparisons
and wrapping sums. Long.reverseBytes/divideUnsigned/remainderUnsigned are new
bounded additions alongside the extracted inherited algorithms. The existing
shared-boxes regression gates include these tests; the catalog remains 72.

NRO43 (`88fe9219a56e9fae554b35a735db9adc069954db`, SHA256
`e9b75cd720acf41d049d3704e19b87d2c027c0b1f347968ce622b5ee6295c976`)
passes the integer utility boundary. Mekorama next reports String.hashCode in
the same consent path. Source inspection finds that this is already implemented
in portable/file.rs: dispatch used the declared Object type and bypassed the
existing String value methods. A new 80-row JVM corpus reproduces that missing
method before the routing fix, then passes direct/Object/CharSequence calls on
the portable VM. Desktop comparison also corrects String.toString to preserve
receiver identity, matching its documented contract. Both paths retain the
86-wrapper and 518-integer utility observations. Candy Crush's NRO43 attempt
still stops at custom EGL context factory setup; both SD trees were restored.
Neither attempt presents a verified first screen.

The following batch reuses existing implementations: Set constructor
deduplication moves from upstream intrinsics into shared `collection_values`,
and portable Set calls use the existing COW list generations/lock/iterators/GC,
with add mapped to addIfAbsent. Twenty ordinary JVM scenarios run unchanged in
both renamed SDK fixture identities, including callback mutation/GC, constructor
deduplication, typed arrays and snapshot lifetime. All 11 COW tests, the existing
seven communication tests, and the inherited host COW regression pass. The
supported Set subset excludes bulk operations, equality/hash and string APIs;
List-only methods cannot be called through a Set. Its shared superclass metadata
is corrected to AbstractSet, consistent with the [Java 8 class contract](https://docs.oracle.com/javase/8/docs/api/java/util/concurrent/CopyOnWriteArraySet.html).

Native fsync/fdatasync validate the existing descriptor and route to one
VFS/FileSystem sync operation. Host uses an actual disk sync; Horizon uses the
same filesystem commit already used after positional writes and installation
publication. Guest FILE transfers are unbuffered. Backend failures propagate;
standard terminals reject sync and closed descriptors retain EBADF. These
path-backed regular-file descriptors still do not promise open-file lifetime
across rename/unlink or directory-FD support. Five platform tests and the full
native ABI host/oracle suite pass, including sync failure injection and cursor
preservation. The follow-up NRO and physical-device results remain separate.

Reference behavior: [API26 ClassLoader](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r4/ojluni/src/main/java/java/lang/ClassLoader.java),
[JNI method IDs](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/functions.html#getmethodid),
[API26 provider parsing](https://github.com/aosp-mirror/platform_frameworks_base/blob/android-8.0.0_r4/core/java/android/content/pm/PackageParser.java).
