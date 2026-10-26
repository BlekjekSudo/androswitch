# Complete RustyDroid API port

The active requirement is behavior parity with every Android/Java/native API
implemented by RustyDroid at `3761815dc6a2a1427e9fc66b66e8471b78d1b170`.
External APK startup failures are integration evidence, not the porting backlog.
RustyDroid remains the implementation source, credited in README, CREDITS,
NOTICE, and the inherited licenses.

## Completion criteria

The final report must say **not yet 1:1** whenever any inherited API is missing,
unroutable, backed by disconnected state, or lacks behavior verification. Source
merges, compilation, small fixtures, and test registration do not independently
close this requirement.

Account for each original module's declarations, dispatch, state, GC edges,
dependencies, and platform services. Report desktop reference, portable semantics,
Horizon build, emulator, and physical-device verification independently. A backend
replacement needs comparison of its actual Android-visible contracts.

## Implementation sequence

1. Preserve the whole upstream tree and history; reject omitted files and
   unreviewed adaptations using `check-upstream-sync.py`.
2. Compile the complete framework dependency closure on the Horizon std profile
   (the earlier core/alloc proof is retained as historical evidence),
   including AndroidX, GMS, advanced components, and reference devices.
3. Reuse the original dispatcher and GC. Make module tables Engine-owned while
   retaining actual native execution-owner roots and scopes.
4. Run every original VM integration suite against the portable implementation.
   Missing/duplicate test routes fail the gate. Zero-case native/JIT targets
   remain unverified.
5. Bind the common runtime to Horizon services. Java, JNI, and the runner must
   share filesystem, preferences, databases, collections, class identities,
   lifecycle, views, and callbacks. Replace desktop transports at platform
   boundaries, retaining the upstream API algorithms.
6. Verify the complete corpus through the product runner and Ryujinx, then
   produce a copyable package and report every remaining gap. Physical evidence
   applies only to the exact binary tested.

## Development path

The device now enables real Rust std independently of desktop execution; see
[ADR 018](adr/018-horizon-std-and-execution-backends.md) and the
[migration report](HORIZON_STD_MIGRATION.md). This removes the library-profile
restriction, while the state/platform integration ledger below still applies.

`as-vm/upstream-runtime` is explicit work in progress. The shipping runner does
not enable it. Compiling this path is not completed Horizon support. Its
`advanced-components` and `virtual-devices` surfaces preserve upstream reference
behavior, without implying physical cameras, sensors, modems, or Google services.

The upstream in-memory filesystem is retained as a scoped parity reference
(`Engine.parity_vfs`). The product already has a persistent platform VFS. They
must be integrated before enabling the common runtime in the runner: a second
in-memory filesystem cannot serve as device persistence. Portable native SQLite
replaces Python transport; Java SQL operations and lifecycle remain upstream code.

All 50 original integration suite files are registered with
`as-vm/upstream-parity-tests`. The original fixture builders and host renderer
use the isolated portable VM. Registration, compilation, and execution are
different evidence. `as-vm-render-testsupport` is a host oracle outside the
product dependency graph.

`reports/upstream-sync-20261026.json` records the earlier NRO47 checkpoint. Its
results predate this development path. Mekorama and Candy Crush first frames
remain unverified.

## Platform integration ledger

The inherited implementation must become the common state owner. Horizon
adapters provide actual transport/services around that implementation; an
unhandled call must not switch to another independent object store.

The [shared GC pressure checkpoint](UPSTREAM_GC_PRESSURE.md) connects the
original adaptive byte/slot policy to the product collector and invalidates a
pinned live watermark once its callback unwinds. The ordinary Collections
UI/journal run now completes 34/34 host cases at 64 MiB. This does not complete
the original collector's tracing/state integration or qualify unrun device APIs.

The following [shared collector integration](UPSTREAM_GC_SHARED.md) removes the
separate product collector and builds original `gc.rs` on both profiles. Heap,
generation and weak/queue tests share unchanged upstream bodies. Platform roots
and old side-state traversal remain explicit adapters; the remaining framework
tables and public API dispatch still require integration. This closes a core
implementation boundary without certifying complete API parity.

| Boundary | Inherited reference state | Current product state | Remaining integration |
| --- | --- | --- | --- |
| Collections/iterators | `HostState.objects`, `jcollections`, `collections` | `as-framework/services/collections`, COW and map adapters | One owner for constructors, factories, views, callbacks and GC edges |
| Java/native I/O | `jfile::Vfs` reference bytes, descriptors and metadata | Persistent `GuestFileSystem` and native file handles | Retain inherited APIs while routing all byte operations, errors and durability to the common mount |
| Preferences/databases | Upstream prefs/SQL objects; native SQLite replaces host process transport | XML preference store and existing native database service | One set of live objects, collection values, paths, transactions and shutdown flushes |
| Lifecycle/UI/resources | `HostState.objects`, views, graphics and upstream lifecycle | Android adapter view tree, resources and platform display | One tree and callback/lifecycle graph presented by actual Horizon graphics/input |
| Threading/scheduling | Cooperative reference threads and virtual looper/clock | Actual execution owners, platform workers, timers and synchronization | Preserve real ownership/TLS/reentry and supply real clocks to the common algorithms |
| Classes/enums/metadata | Upstream lookup algorithms, type facts and static caches | Portable canonical Class cache, loader/enum and JNI metadata adapters | Reconcile all identities and exact overload declarations; type discovery never grants a method |
| Java collections/text/regex | Original collection algorithms, heap builder text and Matcher tables | Active collection provider, shared heap `buf`, Engine-owned regex state | Shared modules now connected; remaining collection families, metadata/semantic and platform verification still gate parity |
| Device/build/native services | Upstream reference device profile and optional native/JIT oracles | Actual Horizon observations, Bionic/JNI/native ARM64 backend | Use measured/profile-declared platform values and tested ABI replacements; no fictitious physical service |

The [shared I/O checkpoint](UPSTREAM_SHARED_IO.md) moves original File, file-stream,
AtomicFile and character/byte-stream algorithms onto the common storage/object
graph. `jio_chars` is now selected by the shipping std profile. File values and
stream/channel owners are heap objects; character stream state is Engine-owned
and traced through typed GC edges. The old product File/stream implementation is
removed. This is scoped integration: native descriptors/locks, complete OS/file
attributes, aggregate stream-memory pressure, preferences and SQL ownership
remain open. The enlarged SDK catalog contains 274 cases and 148 actual JVM
oracle rows; older binary/corpus evidence is not transferred.

The first post-std integration slice extracts the immutable Collections
factory overloads and three `EMPTY_*` fields from the original dispatcher into
`collection_factories`. Both reference and product dispatch use that source.
Its collection adapter creates/snapshots values in the active backend's existing
store; a bound framework cannot fall back to `HostState`. Nine factory overloads
are exposed to the device. The two inherited sorted factories return plain
Set/Map marker types, so they remain reference-only until the sorted-container
contract is integrated. `singletonMap`, other Collections utilities and the
remaining collection constructors, bulk operations, sorted containers and
iterators are still integration work.
The inherited unmodifiable wrappers are snapshots, not live Android views;
that semantic gap remains explicit. The product does not inherit the desktop
`synchronized*` identity/no-op substitution.

The [collection integration checkpoint](../reports/collection-state-20261026.json)
records source `3e37e6d`, 706 reference tests, 988 product regressions, strict
workspace/reference Clippy, and a real std NRO build. The same unmodified
Mekorama APK was attempted twice in pinned headless Ryujinx. A short run stopped
while a render callback was still starting work (`ECANCELED` after worker
admission closed); it did not prove a missing worker API. A longer run reached
`RendererWrapper.onSurfaceCreated`, started its worker, and failed at
`HashSet(Collection)`, whose inherited constructor is not yet connected to the
product store. Both attempts returned to the console and restored the test SD;
the constructor failure also appeared during Activity stop. No first frame is
claimed. Candy Crush was not rerun in this checkpoint. It established the need
for constructor/element-reader integration, addressed by the later common-owner
port below. No new game execution is implied by those API tests. The hardware ZIP
with NRO `a3a34bab...` remains its separately identified std qualification build.

Compilation of both stores is not completion of any row. The default runner
stays on its verified subset until the replacement can run the complete
inherited and product corpus without selecting disconnected state. Native/JIT
oracles remain alternative host backends, rather than dependencies shipped in
the NRO.

The subsequent [collections/text integration](UPSTREAM_COLLECTIONS_TEXT.md)
connects the inherited collection constructors, bulk/template operations,
sublist/ListIterator graph, collection object methods and eleven additional
Collections static overloads to the product owner. Original String, builder and
regex modules now compile under the shipping std profile; the duplicate product
builder map was removed. StringBuffer uses the actual VM monitor; Matcher and
MatchResult references participate in GC. The Conformance source grows from the
frozen 219-case hardware baseline to 257 development cases, with 135 actual JVM
oracle rows. Full product DEX execution of those 135 pure cases passes at this
checkpoint. Its std NRO builds, but the two recorded Collections emulator
attempts time out before completing the workflow. An ordinary host UI/journal
diagnostic also advances slowly and samples in GC. Integrating the upstream
pressure/backoff behavior is the next verification boundary. The new build has
no completed emulator or hardware qualification; the earlier hardware
confirmation is not transferred. See the
[actual checkpoint](../reports/collections-text-20261026.json).

The source-route audit now evaluates the actual shipping `std` feature
declarations, including nested `all`/`any`/`not` guards. **85 original VM files
remain excluded** in the default device profile. A compiled std source is still
reported as behavior-unverified. This count is not an API percentage, and the
full upstream dispatcher is still not enabled for the product's bound framework.

## Required final report

- Exact upstream revision, AndroSwitch commit, and artifact hash.
- Explicit answer: every upstream API 1:1, or not yet 1:1.
- Complete inventory of implemented, integrated, and unverified modules/APIs.
- Each original suite's executed/passed/failed/ignored/zero cases, command,
  backend, and retained output.
- Every semantic/platform difference and affected contracts.
- Host, Horizon build, Ryujinx, and physical-device evidence separately.
- No percentage inferred from source counts or two applications.
