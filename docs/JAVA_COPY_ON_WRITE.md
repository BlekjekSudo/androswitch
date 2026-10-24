# Java CopyOnWriteArrayList

The shared framework has a separate implementation of
`java.util.concurrent.CopyOnWriteArrayList`. It does not reuse mutable ArrayList
state. Each successful mutation publishes an immutable array generation;
iterators retain the generation observed when they were created. Changes to
the list do not change an existing iterator or produce
`ConcurrentModificationException` for that iterator.

The implemented subset is:

- Constructors: empty, `Object[]`, and `Collection`.
- `size`, `isEmpty`, `get`, `set`, indexed/appending `add`, indexed/object
  `remove`, `clear`, `contains`, `indexOf(Object)`, `lastIndexOf(Object)`, and
  `addIfAbsent`.
- Appending/indexed `addAll(Collection)` and both `toArray` overloads.
- `iterator`, both `listIterator` overloads, cursor queries, and forward/backward
  traversal. Iterator `remove`, `set`, and `add` throw real no-argument
  `UnsupportedOperationException`; exhausted traversal throws real no-argument
  `NoSuchElementException`. Their Java messages are null.

Null elements are supported. Constructor arrays and returned arrays are copies.
A sufficiently large typed `toArray` destination is reused, receives a null
terminator, and retains later slots. Array-store failure preserves stores already
made before the incompatible element. Collection inputs use the actual virtual
`toArray` implementation; constructors and `addAll` do not loop through a
subclass's overridden `add`.

Searches invoke the query object's virtual `equals(element)` in the proper
direction. The API 26 snapshot revalidation rules for `remove(Object)` and
`addIfAbsent` preserve mutations performed by equality callbacks before the
write phase. Reentrant callbacks during that phase follow its captured-array
behavior. Snapshot values, the query, and the receiver remain GC roots through
DEX/native reentry. No collection-state borrow crosses a guest callback.

Mutations use a separate private Java monitor, with the VM's actual owner and
reentrant depth. The VM lease serializes state access, including publication.
Cross-owner monitor contention still requires the scheduler/lease-handoff
capability that the current monitor backend reports as unavailable. It fails
explicitly without transferring ownership or mutating the list. This subset
does not claim blocking behavior for every concurrent Java program.

A same-reference `set` preserves the existing generation and the monitor's
publication fences. Invalid indexed mutations are checked before copying.
Array generations retained by old iterators and active operations count against
the existing shared 8 MiB collection value budget, including the incoming COW
buffer while `addAll` constructs its replacement. The limits are 10,000 list
elements, 8,192 collection objects, 128 active COW root scopes, 32,768 COW object
roots, and 16,384 generation-registry entries. Dead registry entries are pruned;
generation/ticket counters never wrap. Buffer reservation is fallible, while
bounded Rust map/Rc bookkeeping still uses existing infallible allocation.

GC traces complete snapshots, including elements already passed by the cursor.
An iterator does not retain its list. Unreachable lists, iterators, and their
unshared generations are removed through the shared VM collector. Collection
can retain references from active frames, so precise reclamation timing is not
promised; budget recovery is tested after those frames return and GC runs.

`subList`, collection equality/hash/string formatting, streams, serialization,
`addAllAbsent`, bulk predicates, and other unlisted operations remain explicitly
unavailable. The supported Java exception types and unchanged-state behavior
are tested; exact VM-generated index/null exception text is outside this subset.
Regular ArrayList/Set/Map behavior remains on the existing implementation.

The behavioral source is
[Android 8.0.0_r1 CopyOnWriteArrayList.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/util/concurrent/CopyOnWriteArrayList.java),
SHA-256 `a631ef22dc8ad53cb1caa237df2583bce1964810ffd68e1f4950b48285151f5e`.
Its Java implementation is a reference; no implementation source was copied
into the Rust runtime. The public contract is also described by the
[Java API reference](https://docs.oracle.com/javase/8/docs/api/java/util/concurrent/CopyOnWriteArrayList.html).

Reproduce the focused host checks with:

```powershell
./fixtures/m9/copy-on-write/build.ps1
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p as-framework --features host --test copy_on_write -- --nocapture
cargo clippy -p as-framework --features host --lib --test copy_on_write -- -D warnings
```

The [host receipt](../reports/m9-copy-on-write-host-20261025.json) distinguishes
actual portable VM/DEX checks from controlled host-native reentry and later
NRO execution. The JVM corpus has ordinary original and renamed Java
namespaces. Its normal SDK proof APK is
`target/copy-on-write-fixture/original/cow-proof.apk`, package
`org.fixture.cowproof`. The proof checks mutation during snapshot traversal,
reentrant removal, cursor/error behavior, and GC retention; it writes
`/sdcard/Download/androswitch-cow-proof.txt` and displays PASS text.
Game startup and physical-device compatibility require separate evidence.
