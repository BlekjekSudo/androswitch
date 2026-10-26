# Horizon std migration — 26 October 2026

AndroSwitch now builds its device runtime with the real Rust standard library.
This replaces the product's core/alloc-only build profile. It preserves the
existing embedded execution state, JNI object identities, guest/host TLS
boundary, persistent Java/native filesystem and console/media runner.

RustyDroid by **nadabgfccvd and the RustyDroid contributors** remains the runtime
foundation. The isolated `as-vm` Cargo package compiles `rd-vm/src/lib.rs`
directly; it contains no copied VM. See [credits](../CREDITS.md).

## Library support and Android execution are separate

`std` supplies Rust filesystem, clocks, collections, synchronization and threads.
`desktop-runtime` selects the inherited desktop execution backend. The latter
exists only in `rd-vm`; `as-vm` cannot enable it through Cargo feature unification.
Both packages compile the same original source tree. Default AndroSwitch
framework, storage, native, package, launcher and runner features enable std.

The full inherited `upstream-runtime` reference graph remains an explicit
feature. Migrating Rust library support does not connect that graph to every
live Android object/platform service. Complete upstream Android API behavior
parity and Mekorama/Candy Crush first frames remain separate work. The
[integration ledger](UPSTREAM_PARITY_PLAN.md#platform-integration-ledger)
retains those requirements.

## Pinned platform adaptation

- Compiler/source: `787af2b8c80638c51a4fc8e44f84e6891f243ec7`, distributed as
  nightly-2026-08-26; custom `aarch64-androswitch-newlib` target, panic=abort.
- `prepare-horizon-std-toolchain.py` validates the original source hashes and
  creates a task-local sysroot. Compiler artifacts are immutable hardlinks;
  Rust sources are independent copies. No global default or installed source
  is changed.
- Two std files change: `sys/fs/unix.rs` copies file contents without claiming
  Unix chmod exists on SD, and `sys/thread/unix.rs` queries the actual process's
  allowed core mask through the PAL. Explicit chmod remains unsupported.
- Pinned libc 0.2.189 is independently extracted and its newlib layouts adapted
  to the SDK. The probe compares 33 sizes, alignments, offsets and constants
  against C compiled with the actual SDK.
- `platform/horizon-std/support.c` uses real libnx/newlib services. It repairs
  SDK default joinability, reclaims detached pthreads with a dedicated reaper,
  queries `InfoType_CoreMask`, and supplies real CSRNG entropy. Creation is
  bounded at 256 live tracked pthreads and returns EAGAIN on exhaustion.
- Reclamation depends on the pinned libnx `__pthread_t` having its public
  `Thread` as its first member, as in
  [libnx newlib.c](https://github.com/switchbrew/libnx/blob/master/nx/source/runtime/newlib.c).
  SDK archive hashes accompany the probe build. Revalidate this private layout
  and the public ABI tests on every SDK update.
- Mesa's pinned shader compiler retains its separately versioned GNU/Linux
  Rust runtime and existing bounded SDK adapters. `prepare-mesa-std-link.py`
  namespaces its exception personality and DWARF references in copied
  archives. Linux clock/sysconf imports are renamed only in Rust objects;
  Horizon C objects keep newlib's ABI. Real AArch64 archive/link tests verify
  that split and regenerated symbol indices. The product compares actual std
  wall-clock/core observations with native `gettimeofday`/kernel observations
  at startup and writes `logs/rust-std-platform.json`. It refuses a mismatch.
  It does not ignore duplicate symbols or mix Rust crate versions.

There is no Linux guest kernel, Python, Unicorn or desktop process backend in
the NRO. Android files still pass through the common guest VFS; raw Rust std
paths do not establish Android permissions, storage mounts or lifecycle.
Networking and panic unwinding are outside this offline qualification.

## Failures retained during qualification

A final run caught a detached-thread reclamation race that an earlier 32/32
run missed. The SDK frees its pthread identity before the wrapper can retire
its metadata. Allocation can reuse that identity immediately; a retiring
record must not shadow the new thread. Lookups now exclude retiring records,
and the first detach error remains visible instead of being overwritten by a
later success. The stronger test creates 256 workers in eight batches, requiring
all completions, TLS destructors and native reclamations with zero live records.
It uses explicit 256 KiB stacks in the 128 MiB fixture heap; default-stack
creation/join tests remain separate. An earlier default-stack stress allocation
failure is retained as a failed run, not reclassified as success.

The combined build also exposed a clock ABI conflict: clock `1` is realtime in
newlib and monotonic in GNU/Linux. A process-global Mesa clock wrapper would
therefore corrupt std's wall clock. SDK Rust imports now have their own namespace,
as do Linux sysconf constants. The 633 Rust objects in the mixed Vulkan archive
and matching Rust runtime archives are adapted without changing Horizon C
imports. Startup checks test the actual combined NRO, beyond standalone smoke.

The report tool's `--require-pass` gate rejects any failed/missing std contract.
Completion markers alone are not acceptance evidence. Conformance can run
continuous bounded wait segments so the harness does not tear down a genuine
asynchronous lifecycle/Handler test before its callback completes. The APK and
its assertions are unchanged; interrupted earlier runs remain recorded.

## Reproduce

Use an already installed matching compiler and rust-src; `-Toolchain` accepts
its local selector. These scripts do not install global components.

```powershell
./scripts/build-horizon-std.ps1
./scripts/test-horizon-std-ryujinx.ps1 `
  -Nro local-artifacts/horizon-std/candidate/androswitch-std-probe.nro

./scripts/build-androswitch.ps1 -Toolchain nightly -LinkBackend Windows `
  -MesaSdk local-artifacts/dependencies/mesa-switch-0ff52079/sdk/opt/devkitpro/portlibs/switch

cargo test -p as-vm --features upstream-parity-tests --lib --tests --locked -- --test-threads=1
```

The unified Mesa build also requires LLVM objcopy (default: Android NDK
28.2.13676358); use `-MesaObjcopy` for an explicit path. Build receipts retain
the compiler, copied source patches, SDK inputs and executed artifact hashes.
The historical parser smoke builder delegates its Rust build to this same
std profile. Its ARM64 link also passes; execution of that separate utility
was not repeated. SQLite build scripts now use Cargo's target OS instead of
matching the old target name, so custom Horizon builds cannot embed host C
objects. Rebuilding after that fix reproduced the qualified product NRO
byte-for-byte. It remains a developer proof, separate from the product NRO.

## Verification

The [machine checkpoint](../reports/horizon-std-migration-20261026.json) retains
source snapshots, commands, per-suite results and original/normalized evidence
hashes. Binary qualification uses **NRO52** from `ed59460`, SHA-256
`a3a34bab32cf31e182fb72efe524bad1e06b5a69107853e9a0aba893aaef2bf8`.

| Layer | Observed result | Limit |
| --- | --- | --- |
| Standalone real std NRO | **32/32, three consecutive fresh runs** | 256 detached workers per run; finite contracts, not complete std certification |
| Inherited std reference | **704 passed; all 50 original integration routes retained** | Three old no_std-only FloatExt unit tests are not selected; zero-case native/JIT targets remain unverified |
| Existing product/desktop models | **982 passed** | Host execution; separate from target behavior |
| Source/ABI/evidence checks | **25 Python tests passed**, including real AArch64 archive separation/index tests; native C/ARM64 oracle contracts passed | SDK ABI facts and transport tests do not certify every libc function |
| Unified Conformance APK | **216 PASS, 3 NOT_RUN** on NRO52 | PCV clock commands absent in the pinned emulator; no 219/219 emulator acceptance claim |
| Console workflow | **11 completed sessions** | Java/Kotlin UI, persistence/update/uninstall, mixed/native media and display handoffs; original unknown-service fixture now completes normally |
| Normal product exit | **586 native frames, two inputs, 12,000 audio frames; std shutdown 0** | Actual C return/atexit and guest OS exit observed after native graphics/audio |
| Combined std/native observations | **3 cores versus kernel 3; wall clocks within 9 ms** | Actual observed process values, not fixed hardware assumptions |
| Physical Switch | **Not run for this std binary** | Earlier user-reported 219/219 applies only to its original artifact |

Two short conformance batches retained one genuinely interrupted lifecycle
observation. A subsequent session used the unchanged APK's ordinary Previous
category / Run category controls to clear and re-execute Lifecycle. Both async
contracts then passed. Previous journals remain in the evidence chain; result
files were not manually changed to PASS.

A long continuous conformance experiment on NRO51 reached 87 passing cases but
timed out during Java stop/journal saving. This failure is retained. Restoring
the embedded VM's original ordered indexes preserves its prior backend policy;
the short-run comparison (153 versus 150 cases) does not establish a performance
fix. Long-session performance remains unqualified. The default short-session
corpus and the explicit normal graphics/audio process-exit test are separate
successful scenarios.

The legacy console validator expected an unknown `getSystemService` query to
fail because that API had once been missing. Android's unknown-name contract
is null; the current implementation completes normally. Validation now requires
all 11 real sessions and retains the original failed validation receipt. The
persistence, native counters, teardown and display checks remain required.

The earlier 29/32 [experiment report](HORIZON_STD_REPORT.md) remains historical.
**Complete RustyDroid Android API behavior parity is still not 1:1.** The full
reference dispatcher is not enabled in the shipping runner, and 91 original VM
files remain excluded there. The common state/platform integration ledger and
M9 independent-game first-frame gates remain open.
