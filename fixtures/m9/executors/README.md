# Serial executor and Thread contracts

Build with `pwsh -File fixtures/m9/executors/build.ps1`, then run
`cargo test -p as-framework --features host --test executors -- --test-threads=1`.
The same twelve validation cases run on the host JVM and as DEX in two APKs
with different package names. Generated APKs and the JVM results are under
`target/executor-fixture/` and stay ignored.

`executor-proof.apk` is an ordinary Android SDK application. A custom
`ThreadFactory` delegates to the default factory and names the returned Thread.
Three FIFO tasks record the actual Java Thread identity/name and absent worker
Looper to `/sdcard/Download/androswitch-executor-proof.txt`. The last task calls
`shutdown()` and posts a visible success message through a main Handler. An
actual Horizon run must also establish a dedicated native callback thread and
successful thread/TLS cleanup; deterministic host delivery does not prove that.

This runtime layer supports a serial `ThreadPoolExecutor` with core=max=1,
`TimeUnit.SECONDS`, and an initialized `LinkedBlockingQueue`. A new worker owns
its first command directly; subsequent commands are observable in `getQueue()`.
Graceful shutdown drains accepted commands. Enabled core timeout measures idle
time with the monotonic clock; a later submission creates a new factory Thread.
There are at most 32 live Thread workers, 128 records per object family, 4096 queued
elements in total, and 4096 bytes per requested Thread name. Platform limits may
be lower and fail explicitly.

The pool calls the application's factory and requires an unstarted Thread that
preserves the supplied worker Runnable and base `Thread.run/start` behavior.
Factories that replace/wrap that Runnable or override the worker continuation
fail before an OS thread starts or a command is accepted. Standalone Thread
targets and standalone subclass `run()` methods execute normally once on their
actual worker. `Thread.currentThread()` identifies that Java Thread during
native callbacks and through TLS teardown; its binding is removed only when the
platform reports the worker stopped.

Thread names are Java metadata. OS thread naming, ThreadGroup, priorities,
daemon process-exit policy, interruption, blocking joins/awaits, Futures, custom
uncaught handlers and worker replacement after an uncaught error are outside
this implementation. Unsupported operations fail explicitly. An uncaught task,
VM or platform failure follows AndroSwitch's shared app-fatal policy: preserve
the exact error, stop/quiesce the real workers, and retain roots until cleanup.
There is no Binder, Dynamite, analytics or package-specific bypass.
