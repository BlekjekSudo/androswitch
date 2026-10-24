# Native suspension and GC owner proof

Build with `./fixtures/m9/gc-owners/build.ps1`. The generated single APK is
`target/gc-owners-fixture/gc-owners-proof.apk`, package
`org.fixture.gcownersproof`, Android API 26, native ARM64.

This independent fixture starts two ordinary Java `Thread` workers. Each owns
a new `Payload` only in its DEX method's live local registers. The objects are
never JNI arguments, global references, static fields or captured closures.

1. A enters a native barrier and releases the VM lease.
2. B enters a different native barrier and remains blocked there.
3. A resumes first, calls `System.gc()`, validates its payload and returns from
   `bodyA` completely while B's `bodyB` interpreter frame is still suspended.
4. A's outer wrapper releases B. B resumes, forces GC and validates its payload.

This exercises root publication and frame/scope unwind in an order that is
not a single process-wide stack. Native barriers use real atomics, elapsed
clock deadlines and one-millisecond sleeps, with a 15-second timeout.
The Activity's `onPause` signals cancellation, so session teardown can unblock
both native waits before joining workers. Cancellation does not publish success.

Success requires an actual execution report in the app's files directory,
`gc-owners-proof.json`: `GC_OWNERS_PROOF_PASS`, values 101 and 202, distinct
nonzero native `pthread_self` identities, and the release-after-return marker.
The Activity also posts a visible result through the main Handler. Verify the
matching session's report and clean runtime teardown; a successful build or a
stale proof file does not establish execution. This fixture proves the stated
two-worker lifetime contract, not unrestricted Java scheduling or game support.
