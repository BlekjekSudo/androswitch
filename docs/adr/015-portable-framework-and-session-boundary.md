# ADR 015: portable framework and a verified application boundary

Status: implemented for the supported M5–M7 corpus; hardware validation pending.

`rd-vm::FrameworkHost` binds Android API state to the real Engine. Virtual DEX
overrides and invoke-super/direct dispatch retain their existing semantics.
Application/Activity/View state, queues and framework reference edges live in
`as-framework`, using the shared APK/AXML/ARSC readers. The portable collector
traces those edges and removes dead side-table entries. The inherited desktop
framework remains separate and retains its behavior.

The original roadmap assumed that separate launcher/runner NROs guarantee a
fresh Horizon process. That assumption is incorrect for standard hbloader:
[`loadNro`](https://github.com/switchbrew/nx-hbloader/blob/master/source/main.c)
unmaps and maps NROs using the same process handle. `envSetNextLoad` chooses
the next NRO; it does not create a process.

M7 must therefore verify quiescent guest teardown before returning to a
launcher or loading another NRO. Native workers, media resources, TLS and code
mappings must no longer be in use. If that cannot be established, record the
reason and exit the Horizon application rather than starting another guest.
Never report chainloading alone as process isolation. This correction preserves
the user's one-entry, one-folder packaging goal without claiming an unavailable
isolation guarantee. Exact implemented teardown and emulator evidence belong
in the M7 report before that milestone is marked complete.

M7 implements one product NRO with a console/App mode boundary. Its shared
package selection contains a checked install revision and a one-shot session
request. Callbacks run while VM references are valid; platform/guest workers
must stop before JNI/GC roots, media, TLS, synchronization objects, heap and
executable mappings are released. A bounded watchdog exits the Horizon process
if teardown cannot finish. A pending marker reports interruption on next boot.
The exact subset and positive/negative evidence are in [M6/M7](../M6_M7_E2E.md).
