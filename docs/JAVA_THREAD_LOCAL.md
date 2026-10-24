# Java ThreadLocal

The shared portable framework implements `java.lang.ThreadLocal` independently
of APK package names. Its state belongs to the actual managed or JNI-adopted
Java `Thread` object already used by `Thread.currentThread()`. Reusing a native
thread ID after termination or JNI detach creates a new Java owner and does not
reuse that owner's values.

Implemented descriptors are the no-argument constructor, `get`, `set(Object)`,
`remove`, and the base `initialValue()` method. `get` invokes a DEX initializer
virtually; DEX overrides, covariant bridges, and `super.initialValue()` retain
their ordinary dispatch precedence. The base initializer returns Java null.
Stored null is a present entry and suppresses initialization. Removing an entry
allows the next `get` to initialize it again.

The implementation follows Android API 26: a successful initializer's returned
value replaces a value installed by a reentrant `set`, without invoking an
overridden `set` a second time. If initialization throws, its actual reentrant
mutations remain, and an absent entry is retried on the next `get`. No service
borrow crosses an initializer, DEX callback, native call, or collection; exact
pending-root tickets protect the receiver, owner, and values across reentry.

The Java Thread strongly retains its stored values. ThreadLocal keys are weak
and are never made global roots. Collection expunges unreachable keys; a stale
value already marked through its Thread can survive that collection and become
reclaimable on the next collection. This is earlier stale-entry cleanup than
Android's opportunistic map scanning and does not promise identical GC timing.
Actual worker termination and JNI detach clear the owner's table even if the
application retains its terminated Thread object.

Limits are 1,024 live attached ThreadLocal objects, 1,024 entries per Java Thread,
4,096 entries in the runtime, and the existing 128 Java Thread map owners.
Admission failure reports `THREAD_LOCAL_BUDGET`. Replacing an existing entry
remains possible at capacity; failed admission does not replace another entry.
Metadata is bounded, while its Rust map allocation still uses the allocator's
existing infallible allocation path. `InheritableThreadLocal` and
`ThreadLocal.withInitial(Supplier)` remain explicitly unavailable; they are not
implemented as ordinary ThreadLocal aliases.

The reference is [Android 8.0.0_r1 ThreadLocal.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/ThreadLocal.java),
SHA-256 `2533b8e5f1c21db32e3ce48c4cf2f6869fb040d2b262feadc2d748b2d6d997cc`,
and its [Thread.exit](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/Thread.java)
cleanup. These are behavioral references; no Java implementation source was
copied into the Rust runtime.

Reproduce the host evidence with:

```powershell
./fixtures/m9/thread-local/build.ps1
$env:CARGO_INCREMENTAL='0'
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p as-framework --features host --test thread_local -- --nocapture
cargo clippy -p as-framework --features host --lib --test thread_local -- -D warnings
```

The [immutable host receipt](../reports/m9-thread-local-host-20261025.json)
records 10 passing tests, 44 comparisons to actual JVM outputs across original
and renamed Java namespaces, GC/reentry/owner-lifetime/bounds checks, and 29
passing existing worker regressions. These tests execute the actual portable
VM and DEX, using controlled native IDs and deterministic worker delivery.
They do not establish OS concurrency, Horizon execution, or game compatibility.

The normal SDK proof APK is
`target/thread-local-fixture/original/thread-local-proof.apk`, package
`org.fixture.threadlocalproof`. Its worker checks an independent value of 202
while the main Thread retains 101, verifies actual Thread identity and stored
null, runs collection, then posts a main Handler callback that writes
`/sdcard/Download/androswitch-threadlocal-proof.txt` and displays its PASS text.
Host execution of that same callback transcript is covered. Actual NRO and
physical-device evidence must be recorded separately.
