# Portable execution ownership and class initialization

The portable interpreter records frames and allocation scopes per actual native
thread. Suspended native calls remain roots, and each returning invocation
removes only its own frame and captured scope. Diagnostic frames also record
the actual declaring DEX class used by reflection access checks. A native
provider must supply its stable, nonzero `current_thread_id`; absent or zero
identity returns `API_UNAVAILABLE` for native invocation and unfinished class
initialization. With no native provider, the original single interpreter uses
its separate owner zero.

## Native temporaries

Every portable DEX-declared native invocation creates a host allocation scope.
Fresh heap objects allocated by the backend stay rooted while it calls back
into DEX, including callbacks that run `System.gc()` or throw. Input objects
remain roots in that invocation's frame. The scope does not create a cutoff
that preserves unrelated existing heap objects.

On return or error, the engine removes the captured owner's scope and pins
only its returned object or materialized Java Throwable into the enclosing
host scope. Other temporary native allocations become collectible. A guest
scope prevents promotion into a more distant host scope. The owner captured
on entry determines restoration and return pinning; independent native threads
need not finish in process-wide stack order. Structured VM errors retain their
original code and cause and do not become successful task results.

These internal host pins complement the native backend's real JNI reference
tables. They do not make an arbitrary raw object reference a permanent root.
Callers retaining objects after their owning scope ends still need a real
reference owner.

## Class initialization

An unfinished portable class records the owner executing its initialization.
Recursive use by that same owner is permitted and can observe the Java default
or intermediate values visible to the initializer itself. Another owner gets
an explicit `API_UNAVAILABLE` before accessing partial static state. The
original initializer can continue and publish its completed state; an attempted
foreign access does not poison it. Unknown native identity is rejected before
defaults or initializer effects are published.

Android waits for a foreign initializer to finish. Waiting is not implemented
here yet. Returning unavailable is a compatibility limitation, never evidence
that the class has initialized. A dependent initializer that has already begun
and encounters an unavailable dependency is marked failed to prevent replaying
partial side effects; its original structured error is preserved. Later use of
that failed class raises `NoClassDefFoundError`. Java exceptions still follow
the existing `ExceptionInInitializerError`/`NoClassDefFoundError` behavior. The
legacy `std` initializer route is unchanged.

The reference behavior is [JLS 12.4.2](https://docs.oracle.com/javase/specs/jls/se8/html/jls-12.html#jls-12.4.2)
and [ART at Android 8.0.0 r1](https://android.googlesource.com/platform/art/+/android-8.0.0_r1/runtime/class_linker.cc#4658):
ART distinguishes the initializing thread from another thread and waits only
for the latter. The receipt pins a downloaded reference by hash. No ART
implementation code is copied into these Rust helpers.

## Reproduction and evidence

```powershell
pwsh -NoProfile -File crates/rd-vm/tests/fixtures/portable-execution/build.ps1
cargo test -p as-switch-smoke --features host-cli,portable-tests --test portable_execution -- --nocapture
cargo test -p as-switch-smoke --features host-cli,portable-tests --test portable_throwable -- --nocapture
cargo test -p as-framework --features host --test inherited_dispatch -- --nocapture
```

Run Cargo targets separately from legacy `rd-vm` test targets: their development
dependencies can unify `std` into the engine. Every execution contract asserts
`rd_vm::PORTABLE_CORE`; zero portable tests is not a pass.

The [host receipt](../reports/portable-execution-initialization-host-20261025.json)
records twelve execution tests, five Throwable regressions, four dispatch
regressions and focused Clippy. The twelve include the prior six ownership
contracts plus real DEX/native callback GC, exact return scope, foreign/unknown
initializer admission, failed initializer cleanup and sixteen ordinary JVM
comparisons across original and renamed namespaces. The native provider in
these tests controls opaque identities on one host thread; it proves the VM
contract without claiming actual OS scheduling. Independent emulator worker
evidence remains in the [earlier GC-owner receipts](HORIZON_WORKERS.md).

These new class-initialization/native-temporary cases have not independently
run on Android, Ryujinx or a physical Switch. They do not establish game
compatibility. Frame/scope metadata allocation remains subject to the existing
infallible Rust collection growth limits; this change is not a complete memory
exhaustion or concurrency security review.
