# Horizon Rust std feasibility — 26 October 2026

**Decision: continue integrating the current `no_std + alloc` core. Keep the
working `std` prototype for later qualification.** Actual Rust `std` runs in a
standalone libnx NRO, but this prototype is not ready to replace the runtime's
platform boundary. The finite emulator probe finished **29 passed, 3 failed,
0 missing**. These totals are not percentages of Rust or Android compatibility.

The machine report is [horizon-std-20261026.json](../reports/horizon-std-20261026.json).
The [original journal](../reports/horizon-std-20261026/results.tsv),
[build receipt](../reports/horizon-std-20261026/build.json),
[source snapshot](../reports/horizon-std-20261026/source.json) and
[execution receipt](../reports/horizon-std-20261026/emulator-verification.json)
are retained with raw/normalized evidence hashes. Hardware was not run.

## What was built

- Real Rust `std`, built from unmodified matching Rust sources using
  `-Zbuild-std=core,alloc,std,panic_abort` and a separate custom Unix/newlib target.
- A checksum-pinned libc 0.2.189 adaptation for the actual devkitA64 file and
  pthread ABI. The probe compares **33** sizes, alignments, offsets and constants
  against C. The [exact patch](../reports/horizon-std-20261026/libc-adaptation.patch)
  is retained; it is not applied to the product or global Cargo registry.
- SDK libsysbase/libnx for filesystem, pthread, TLS, synchronization and clocks.
  Small experiment glue supplies `sysconf(_SC_PAGESIZE)` and real CSRNG entropy.
  Linux kernel emulation, Python and guest Android OS services are absent.
- A C entry point embedding Rust through one documented ABI boundary, compiled
  with Rust `-Dwarnings` and C `-Wall -Wextra -Werror`, with `panic=abort`.

The official [Rust Switch target](https://github.com/rust-lang/rust/blob/main/compiler/rustc_target/src/spec/targets/aarch64_nintendo_switch_freestanding.rs)
still declares no standard-library support. This experiment selects and adapts
existing implementations; it does not establish official target support.
Rust project, rust-lang/libc, devkitPro/newlib and switchbrew/libnx attribution
is preserved in [credits](../CREDITS.md). RustyDroid remains AndroSwitch's Android
runtime foundation.

## Executed results

| Contract | Emulator result |
| --- | --- |
| ABI layout guard | Passed, all 33 values agree |
| Allocation, Unicode strings, randomized HashMap, aligned Box | Passed |
| Binary file I/O, seeking, stat, directory listing, rename, exclusive create, errors and fsync | Passed |
| Same file written by Rust, read/appended by C, reread by Rust | Passed |
| Monotonic clock, sleep, epoch clock | Passed; actual timings in journal |
| Real thread spawn/join and return value | Passed |
| TLS isolation and destructor lifetime | Passed: 8 initial workers, then 64 additional workers and 64 destructors |
| Mutex contention, barrier, condvar wake/timeout, RwLock, OnceLock, channel, park/unpark | Passed; shared counter reached 8000 |
| Environment set/get/remove | Passed |
| Persistence in a second fresh NRO process | Passed |
| Rust return followed by C atexit, twice | Passed; suite returned its 3 failures, restart returned 0 |
| Intentional panic with abort policy | Passed: panic hook, guest exit, no return/atexit marker for the panic phase |
| Unsupported subprocess | Passed negative contract: explicit `ErrorKind::Unsupported` |
| `std::fs::copy` | **Failed**, `ENOSYS`, errno 88 |
| Dropping an unjoined `JoinHandle` | **Failed**, SDK `pthread_detach` returned 88 |
| `thread::available_parallelism()` | **Failed**, unsupported target implementation |

The detach finding is material: the release standard library does not expose
that SDK error to the caller dropping a `JoinHandle`. Thread completion alone
would therefore be insufficient evidence of resource cleanup. The observation
wrapper preserves the SDK return value; the test harness joins the completed
worker afterward to avoid retaining its native resources. That recovery is
explicitly **not** counted as working detach support.

The copy path's Rust implementation applies file permissions through `fchmod`.
That is a likely explanation of the observed `ENOSYS`, inferred from source;
this probe does not trace the exact failing C call. Rename and ordinary byte
I/O have separate passing contracts.

## Artifact and execution identity

| Item | Value |
| --- | --- |
| NRO | `androswitch-std-probe.nro`, 569400 bytes |
| NRO SHA-256 | `6bd37061d88fada2ec5a106404e4d4640cbc90969e14fe4edf1a9721828c4c1a` |
| Compiler | Rust 1.100.0-nightly, `787af2b8c80638c51a4fc8e44f84e6891f243ec7` |
| C compiler | devkitA64 GCC 15.2.0 |
| Execution source commit | `94c26c8aa26120b478da7f6b894a7b7547175bb5` |
| Final reproducible build receipt commit | `01ec6e4fddd45d36656937af4e2730e49ceae768` |
| Ryujinx | Pinned 1.3.3+e2143d4, headless SDL2, hidden owned process |
| Emulator SHA-256 | `cf03a8da21588166082735c0427413fe791032b84cba77919f38f5652218997c` |
| Run | `f7cb236e1c7d408f98e8fd7a1ccf2909`, three fresh guest processes |

The final builder change canonicalizes evidence newlines before hashing. It
reproduced **identical NRO bytes**; the report validates the build and executed
artifact hashes together. No runtime-code change is inferred from that metadata
change. SD test state was restored. Only owned emulator processes were stopped;
the emulator host can remain alive after its guest has exited.

## Why continue with the current core

The required fundamentals are feasible with `std`; the earlier concern should
not be interpreted as a technical impossibility. It can reduce future Rust
dependency and source adaptation. However, the current complete portable
reference already compiles for Horizon and has its own 707 passing host
reference tests, as scoped in the [upstream report](UPSTREAM_PARITY_REPORT.md).
The remaining primary work is binding inherited object state, persistent VFS,
preferences/databases, lifecycle/views, scheduling and JNI to one runner.
These Android-visible contracts require integration with either Rust profile.

In particular, upstream's desktop SQLite subprocess transport still needs its
native device replacement even with `std`; subprocess execution was correctly
unsupported in this experiment. Enabling desktop features cannot substitute
for that adaptation. The detach and storage findings also require real fixes
before calling this a dependable general std backend.

Proceed through the [platform integration ledger](UPSTREAM_PARITY_PLAN.md).
Reconsider migration after the std backend has qualified detach/resource
cleanup, file/permission behavior, discovery, deployment integration and the
same full Android/JNI contract corpus. This probe establishes neither improved
APK startup nor a performance advantage. Mekorama/Candy Crush first frames and
full upstream API parity remain unverified.

## Reproduce and limitations

See the [experiment README](../experiments/horizon-std/README.md) for build and
guarded emulator commands. The report parser's **6 tests** include missing,
duplicate, mismatched-artifact and incomplete-abort evidence. Source-sync audit,
Rust formatting, target warning checks and C warning checks passed. Clippy
could not run for the pinned target: its installed nightly has no clippy
component, and this experiment did not alter global toolchains.

The entire Rust standard-library test suite, all libc/socket/process contracts,
Rust unwinding, main-thread TLS teardown guarantees, Android JNI/Bionic TLS
transitions, memory/performance benchmarks and physical Switch validation were
not run. The initial compile/link failures and ABI rejection are retained as
diagnostics; none are included among the final 32 executed contracts.
