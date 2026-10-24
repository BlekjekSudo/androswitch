# java.lang.Runtime resources

The shared runtime implements `Runtime.getRuntime()` and
`Runtime.availableProcessors()`. Every application VM owns one real Runtime
object, retained as a framework GC root. Repeated lookup returns that object,
including after collection. The private Java constructor is not exposed as a
supported constructor. This implementation has no package or engine branches.

Processor counts come from an explicitly bound `as_platform::RuntimeResources`
provider. `getRuntime()` can return its singleton without a provider;
`availableProcessors()` reports `API_UNAVAILABLE` until one is bound. Each call
queries the provider again. Query failures retain their actual structured error;
zero or values outside a positive Java `int` are rejected rather than replaced
with a guessed count. No resource-state borrow is held while calling a provider,
so rebinding during a query does not panic or corrupt the singleton.

## Platform observations

On Horizon, `as_platform_available_processors` calls public libnx
`svcGetInfo(InfoType_CoreMask, CUR_PROCESS_HANDLE, 0)` and counts the bits in
that process's allowed core mask. It does not count all physical cores, use the
current thread's pinned-core count, or assume a fixed Switch CPU count. A failed
query or empty mask leaves the output untouched and returns an error. The Rust
device adapter is bound before application launch. The same C implementation is
linked into the product and focused smoke NROs.

The host provider calls `std::thread::available_parallelism()` on each query.
This is the standard library's OS-derived parallelism estimate, with its
[documented platform limitations](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html).
It is not a claim to independently measure every host affinity, processor-group,
container or scheduler restriction. Host adapters and tests bind their provider
explicitly; there is no implicit host or device fallback.

API 26's [Runtime implementation](https://android.googlesource.com/platform/libcore/+/android-8.0.0_r1/ojluni/src/main/java/java/lang/Runtime.java#698)
describes a positive available processor count that callers may poll for changes.
Its Android/Linux backend queries configured CPUs with `sysconf`; the Horizon
backend uses the process permissions supplied by its own kernel. Numeric counts
from different hosts are not expected to match. The tests compare stable Java
singleton/type/positive-count behavior with the JVM and separately check actual
provider binding and dynamic results.

## Bounded API surface

Memory statistics (`freeMemory`, `totalMemory`, `maxMemory`), Runtime-specific
collection/finalization, process execution, exit/halt, native library loading and
shutdown-hook methods remain unsupported. They are not advertised in supported
method metadata and do not return successful placeholders. Existing
`System.gc()` still uses the actual portable collector.

Allocator free-space estimates and process heap capacity do not describe the
same thing as Java committed/free heap memory. This change does not relabel
those measurements as Runtime memory values or alter existing memory admission,
heap policy, worker counts, teardown or object lifetime rules.

## Reproduction and evidence

```powershell
pwsh -NoProfile -File fixtures/m9/runtime/build.ps1
pwsh -NoProfile -File scripts/test-runtime-resources.ps1
cargo test -p as-platform --features std -- --nocapture
cargo test -p as-framework --features host --test runtime_resources -- --nocapture
cargo test -p as-framework --features host --test glsurface -- --ignored --nocapture
```

The [host receipt](../reports/m9-runtime-resources-host-20261025.json) binds five
Runtime tests, four platform tests, six explicitly executed GLSurface regressions
and focused Clippy. Original and renamed SDK APKs execute the same implementation;
sixteen ordinary DEX results match the independent JVM oracle. Tests cover GC
identity, collection of unrelated garbage, missing/failing/changing providers,
invalid results and receivers, unsupported APIs, provider reentry and shared VFS.
The production C wrapper also executes against a fake kernel query on Windows,
and compiles strictly against real libnx headers for both ARM64 build variants.
Those C host results are not real Horizon syscall execution.

The ordinary SDK proof APK is `target/runtime-fixture/original/runtime-proof.apk`
with package `org.fixture.runtimeproof`. It queries the provider twice around GC,
checks the Runtime identity, displays the observed count and writes
`/sdcard/Download/androswitch-runtime-proof.txt`. The immutable host receipt keeps
the exact tested APK/DEX/oracle copies. A real NRO/emulator run and physical
Switch validation require separate evidence; these host tests do not establish
external game first-screen compatibility.


The [NRO20 SDK execution](../reports/m9-runtime-resources-horizon-20261025.json)
observes three actual process-allowed CPUs, genuine singleton identity after GC,
matching VFS output, a recognizable PASS capture and safe launcher return. The
following original Mekorama attempt advances to AbstractCollection admission
but still has no first-screen pass. This diagnostic-emulator result is separate
from stock Ryujinx and physical hardware.
