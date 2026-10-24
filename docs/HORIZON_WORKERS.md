# Owned background VM workers

`java.util.Timer` requires one dedicated serial execution thread per Timer. Its
constructors start that thread; scheduling and cancellation must be thread-safe,
and cancellation must not interrupt a task already executing. The framework
implements task states, queues and fixed-delay/fixed-rate arithmetic against
the [Java Timer contract](https://docs.oracle.com/javase/8/docs/api/java/util/Timer.html).
The Horizon transport does not substitute the Activity Looper for this worker.

`as_platform::BackgroundWorkers` exposes `start`, `wake` and `cancel` using
positive monotonic opaque tokens. `start` creates and starts an actual libnx
thread immediately. `wake` signals the thread's event without invoking Java on
the caller. `cancel` signals stop without waiting for, interrupting or freeing
the current callback. Missing, stopped or resource-limited workers return a
structured platform error. The current transport supports 32 retained handles;
completed workers are reaped before allocating another slot.

The framework owns every queued/running TimerTask GC root. The backend stores
stable VM, Android and JNI Session addresses owned by the running application.
It holds no strong Android reference that would form an Rc cycle. All callbacks
take the same reentrant VM lease used by JNI and the EGL worker. The device
creates a real Session for Java-only applications as well, so thread/JNI identity
comes from the executing platform thread.

The C callback protocol attaches a worker, asks the framework to run/poll one
serial task, then detaches after guest TLS destructors finish. A step returns a
relative wait in milliseconds, zero for another immediate poll, or a terminal
state. Queue mutation can signal the event during a callback; clearing the event
before polling prevents losing that signal before the subsequent wait. Callback
and unexpected event-wait failures retain their structured error, rather than
being reported as completed tasks.

Wall time and elapsed time are separate capabilities. `Clock::wall_time_millis`
reports actual Unix epoch milliseconds and defaults to `API_UNAVAILABLE` when
the provider is absent. The host uses `SystemTime`; Horizon uses
`clock_gettime(CLOCK_REALTIME)` and writes its scalar result only after successful
validation. The kernel handles relative sleeps against elapsed time. A worker
rechecks framework wall-time deadlines at least once per second, including when
a large deadline was requested. This is scheduling without real-time guarantees.

Application shutdown signals all workers, releases the VM lease and joins using
one total deadline, not a new timeout for each thread. Thread exit and handle
close must both succeed before releasing their owners. A timeout or join/close
failure retains uncertain handles and prevents reuse of that application VM.
The application repeats quiescence after native destructor rounds before final
drop. A worker cannot join itself, and simultaneous global joins are rejected.

One durable admission gate also spans all four guest/platform thread-start
paths: Timer workers, GLSurfaceView, guest `pthread_create` and OpenSL player
realization. After Java lifecycle callbacks, shutdown closes the gate before
joining anything. A start holds the common mutex through OS creation/start and
owner registration, so closing the gate waits for an earlier start to become
fully visible to its joining subsystem. Future starts fail explicitly: pthreads
receive mapped Android `ECANCELED`; Timer/GL receive a platform cancellation
error; OpenSL returns a precondition failure. Existing tasks and destructors may
finish, but cannot create another worker behind a subsystem already joined.
The gate stays closed through TLS/global destructor rounds and media cleanup.
Only the successful end of native resource reset reopens it; every failed reset
path leaves it closed. The runner watchdog and synchronously joined process
handle-copy helper do not execute guest callbacks and are outside this gate.

The separate [admission receipt](../reports/horizon-worker-admission-host-20261024.json)
records a real host-thread race with an OS worker already started but owner
registration deliberately held: shutdown cannot return before that registration
finishes. It also checks a rejected TLS-destructor start, durable closure after
joins and an admitted new lifecycle after explicit reset. GLSurface closed-gate
coverage and strict compilation of each affected C transport are recorded
separately from actual NRO execution.

Run `./scripts/test-background-workers.ps1` with host GCC/pthread support for
the production C transport against real host threads, clocks and events. Its
[host receipt](../reports/horizon-background-workers-host-20261024.json) records
the delayed callback, serial execution, wake/cancel races, TLS/detach ordering,
capacity/reaping and failure paths. These contracts do not prove Timer framework
semantics, VM GC integration, actual Horizon scheduling or external-game
compatibility; those require their own host and NRO execution receipts. M9
remains incomplete.

The later [Ryujinx Timer receipt](../reports/horizon-timer-ryujinx-20261024.json)
qualifies the SDK-built Timer proof APK on the actual ARM64 Horizon NRO
`23e53247fbb113709e612479149e0fa91a2245611d96fe6eaee71f46fdca8726`.
Three dedicated worker callbacks wrote scheduled/actual epoch timestamps through
the shared VFS; each reported no Looper. The task cancelled its Timer and posted
the visible “Timer proof PASS: 3 worker tasks, VFS saved” text through a main
Handler. The run recorded 232 presentations, matching software/submitted hashes,
closed worker admission and returned cleanly to the launcher with SD restored.
The captured PPM and visually reviewed PNG are hash-bound to that session; their
RGB pixels match exactly. The VFS proof and profile configuration snapshot were
preserved in its ignored run directory. The profile configuration was collected
after execution, not independently hashed at process launch. Later diagnostic
image replacement attempts failed rename and retained the earlier complete
reviewed image; those later publications are not claimed. This is actual
emulator worker/lifecycle evidence, with physical hardware and external-game
first-screen support still unproved.

The shared transport also serves the bounded single-worker Executor/Thread
implementation. The [Executor NRO proof](../reports/m9-executors-horizon-proof-20261024.json)
records three FIFO DEX tasks on an SDK-created Thread, no worker Looper, VFS
output, a visible Handler-updated result and clean return. The
[framework contract receipt](../reports/m9-executors-framework-20261024.json)
specifies the supported constructors, queue, shutdown and factory behavior;
additional pool shapes and Java Thread APIs are explicit compatibility gaps.

A VM lease serializes mutations, but releases while a thread executes foreign
native code. Execution frames, diagnostic labels and temporary allocation scopes
therefore belong to the actual native thread identity. The collector traces all
owners, including suspended owners, and return/unwind removes only its owner's
frames and scopes. A single process-wide LIFO stack is insufficient: independent
threads can resume and finish in a different order. The original SDK/NDK fixture
in [gc-owners](../fixtures/m9/gc-owners/build.ps1) reproduces that ordering with
two native barriers and local-only Java objects. Building or passing a host
model alone does not establish corrected device behavior; the exact same APK
must execute before and after the NRO change.

Run the portable host ownership contracts with
`cargo test -p as-switch-smoke --features host-cli,portable-tests --test portable_execution`.
Each test asserts the portable core and exercises the original VM/Heap code;
the opt-in `portable-test-support` feature is absent from the device build.
Testing `rd-vm --lib --no-default-features` alone is insufficient here: its legacy
dev-dependencies enable `std` transitively and can exclude the portable tests.

The [host owner receipt](../reports/horizon-gc-owners-host-20261024.json)
records exactly six passing portable tests, including production GC retaining
another owner's local-only object through non-LIFO publication/unwind and
reclaiming it after its own scope exits. The
[pre-fix emulator receipt](../reports/horizon-gc-owners-before-ryujinx-20261024.json)
records the unchanged SDK/NDK APK failing on an older NRO: `bodyB` reads an
object already collected at DEX PC `0x11`. Worker joins, runtime closure and
launcher return completed; the raw Java-stop error is preserved separately.
Corrected NRO execution is a separate requirement, and these records alone do
not qualify physical hardware or external games.

The later [corrected NRO receipt](../reports/horizon-gc-owners-ryujinx-20261024.json)
records that same unchanged APK passing on NRO `11391da16ac936815be9a035443b104ff7e06e82a9f6a705397ba780031670a4`.
Its two actual native thread identities differ, both local payload values remain
correct across forced GC and the non-LIFO return, and the native VFS report says
`GC_OWNERS_PROOF_PASS`. The main Handler displays the full reviewed success
text; all RGB pixels of its saved PNG match the complete original PPM. The
matching completed launcher session records no error, 271 presentations and
successful resource reset/return. The capture-time session report still says
`running` and is preserved as such; final status and teardown are bound to the
completed launcher report, not to later global logs from the second APK.
This qualifies the stated two-worker lifetime regression on the emulator.
Physical Switch validation and the external-game targets remain separate.

The later [portable execution contract](PORTABLE_EXECUTION.md) adds captured
native-temporary scopes and owner-aware class initialization. Its separate
[host receipt](../reports/portable-execution-initialization-host-20261025.json)
records twelve execution tests, including the original six. Foreign class
initialization currently returns explicit unavailable instead of Android's
waiting behavior. These added host cases do not extend the earlier emulator
receipt to untested device scenarios.
