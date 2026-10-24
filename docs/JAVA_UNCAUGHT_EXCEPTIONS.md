# Uncaught Java exception handlers

The shared Thread implementation supports nullable default and per-thread
`UncaughtExceptionHandler` getters/setters. A thread without an explicit handler
returns a real root delegation group; that group can invoke the installed default
handler. Custom group hierarchies and unrelated ThreadGroup APIs remain unavailable.

An exception escaping a Thread/Executor task, Timer task, GL renderer/queue/config
callback, or an outer application callback invokes the selected Java handler on
the throwing owner before its callback/thread scope exits. Ordinary caught
exceptions and structured non-Java `VmExit::Error` failures do not dispatch.
A DEX override of `Thread.getUncaughtExceptionHandler()` is respected.

The handler receives the actual Thread and Throwable objects. A VM-built throwable
is materialized with the VM's existing implementation. Unique root tickets keep
the selected handler and original exception alive through GC, handler mutation,
and native reentry. No service `RefCell` borrow is held across the callback.
Handler lookup, materialization, and handler-call failures are retained separately
from the original failure. The latest 128 diagnostic reports are rooted and
available through `Android::uncaught_exception_reports()`.
The Horizon runner writes these records to the session-bound
`logs/uncaught-exceptions.json` only after all workers/callbacks are quiescent.
It acknowledges an existing worker execution error before entering native
finalizers, retaining that error as the application failure. A new destructor
or destructor-created-worker failure still refuses teardown; its error cannot
be mistaken for a successful session. Diagnostic file writes are best effort.

Main callback boundaries use owned scope tickets. Nested boundaries report only
an exception escaping the outer operation; worker exceptions forwarded by the
main pump retain their original worker delivery and are not dispatched again.
Java `performClick` uses the inner callback path, so an enclosing DEX catch can
handle its listener's exception normally.

The [host receipt](../reports/m9-uncaught-handlers-host-20261025.json) records nine
passing contracts on the actual portable interpreter and focused Clippy. SDK
call sites and a JVM reference are built with:

```powershell
./fixtures/m9/uncaught/build.ps1
cargo test -p as-framework --features host --test uncaught -- --nocapture
cargo clippy -p as-framework --features host --lib --test uncaught -- -D warnings
```

The tests cover default/per-thread precedence, clearing and null behavior,
termination, a virtual getter, a local-only exception surviving GC in that getter,
handler mutation and native reentry, separate handler failures, VM-built
exceptions, direct group delegation, GC ownership, caught/non-Java exclusion,
main lifecycle and forwarded-worker delivery, and Timer/GL queue callbacks.
The host worker identities and EGL provider are controlled test adapters;
this receipt does not establish real OS scheduling or GPU execution.

Behavior follows the pinned API 26
[Thread.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/Thread.java)
and [ThreadGroup.java](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/ThreadGroup.java).
One reference difference is deliberate: API 26 permits setting an explicit
handler after termination and returns that handler; OpenJDK 21 ignores the
terminated setter. This case is checked against the pinned Android source,
not reported as a JVM match or Android-device execution.

The current runtime preserves its terminal policy: an uncaught worker failure
still stops the application after the handler runs. This does not implement
ThreadPoolExecutor replacement-worker or arbitrary JVM process-exit behavior.
A null default uses the existing runtime diagnostic path; no Android kill hook
or System.err implementation is fabricated. Direct group diagnostic printing
without a default handler, hidden Android pre-handler APIs, custom group
hierarchies and unrelated Thread/ThreadGroup APIs remain explicit gaps.

`target/uncaught-fixture/uncaught-proof.apk` intentionally throws on a real worker.
Its handler writes `/sdcard/Download/androswitch-uncaught-proof.txt` only after
checking actual Thread/Throwable identity, callback liveness and GC-retained
payload. A handler proof followed by the expected original runtime error must
not be classified as complete application startup, gameplay, or hardware proof.
