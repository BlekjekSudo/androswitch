# Horizon memory policy and evidence

AndroSwitch is a history-preserving fork of **RustyDroid by nadabgfccvd and
contributors**. The Android VM/parser remain the shared runtime foundation.
This document describes the Horizon adapter's memory policy; it does not mark
M9 complete or establish that an external game reaches a recognizable screen.

## Process heap ownership

The previous product symbol `__nx_heap_size = 256 * 1024 * 1024` did **not** impose
a fixed 256 MiB heap in all homebrew launches. libnx first checks the homebrew
loader's heap override. When one is supplied, that existing range takes
precedence over the symbol. A prior hardware result therefore cannot be assigned
a 256 MiB capacity without a measured heap snapshot.

The current adapter in [memory.c](../platform/androswitch/memory.c) follows that
ownership contract:

- Honor a valid loader-owned heap override, including its exact address and
  bounds. Do not call `svcSetHeapSize` over a loader allocation or trim the
  range based on a guessed physical RAM total.
- Without an override, query `InfoType_TotalMemorySize` and
  `InfoType_UsedMemorySize` for the current process. Allocate at most **1536 MiB**,
  aligned down to **2 MiB**, and leave at least **128 MiB** of the remaining
  process allowance outside newlib's heap. That space is for later kernel and
  service allocations. GPU/driver allocations made through malloc still share
  newlib's heap with DEX, Java objects and native images.
- If those queries are unavailable, use a **256 MiB fallback**. A successful
  query reporting insufficient capacity is not treated as an unavailable query
  and does not trigger a larger speculative fallback.
- An invalid override or failed initial heap allocation reports libnx's heap
  allocation failure before service/SD initialization. The running product
  requires application/title override and at least 256 MiB of actual heap;
  unsupported launch mode displays `MEMORY_LAUNCH_MODE` before loading Android.

The 1536/128 MiB choices are conservative engineering policy. They are not
measured memory requirements of Candy Crush, Mekorama or any other package.
The cap applies only to newly allocated, non-override heaps. It cannot promise
what a loader or process resource limit will make available.

## Admission and measurements

[memory_policy.h](../platform/androswitch/memory_policy.h) provides checked,
allocation-free policy arithmetic. `as_host_memory_available()` reports a
snapshot estimate of newlib's reusable free bytes plus the part of its heap
not yet acquired by the allocator. It does not report physical RAM, a largest
contiguous free block or a reservation against concurrent allocations.

`logs/memory.log` contains bounded, independently flushed JSONL checkpoints.
They record heap capacity and source, newlib arena/used/free bytes, aggregate
free estimate, process memory queries and their result codes. Kernel process
usage includes reserved heap and is kept separate from allocator usage.
`contiguous_free_known` is false. Heap-source values are `0` for queried process
limits, `1` for a loader override and `2` for unavailable-query fallback.

The framework uses the available-memory snapshot for load admission, with
separate raw/decoded DEX ceilings and headroom checks. Native extraction also
has an explicit budget covering staging costs. Parser estimates are accounting
estimates, not allocator measurements; snapshots before and after each stage
are the evidence of actual allocator usage. The VM's managed-object budget does
not cover raw DEX, decoded metadata, native images or all media allocations.
Avoid equating a successful 512 MiB APK-container check with sufficient runtime
memory. Streaming assets/OBBs and file-backed APKs keep their existing behavior.

Outstanding work includes measured native/graphics startup peaks, allocator
fragmentation and repeated-session stability on the new Horizon build. Bounded
load admission does not implement missing Java APIs, C++ ABI contracts or GLES
behavior. External application startup and safe return must be verified
separately; game input, saves and gameplay remain additional evidence.

## Autorun reference and credit

The requested reference is **Autorun by autorunhq and its contributors**, frozen
at [`1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877`](https://github.com/autorunhq/autorun/tree/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877).
The following source informed the distinction between loader-owned memory,
heap capacity, allocator usage and free-space estimates:

- [`horizon-wine/hbl/source/main.c`](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/horizon-wine/hbl/source/main.c#L116-L142)
  derives heap size from the process allowance, reserves 96 MiB, then passes the
  NRO remainder as a loader override at lines 520–558.
- [`horizon-wine/source/runtime.c`](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/horizon-wine/source/runtime.c#L1329-L1336)
  distinguishes allocator free chunks from untouched heap capacity.
- [`horizon-wine/source/native_heap.c`](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/horizon-wine/source/native_heap.c#L75-L98),
  [`backing_heap.h`](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/horizon-wine/source/backing_heap.h)
  and [`heap_snapshot.h`](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/horizon-wine/source/heap_snapshot.h)
  separate large backing allocations, allocator gaps and fragmentation evidence.

Autorun's repository identifies Wine under LGPL-2.1-or-later in its
[license notice](https://github.com/autorunhq/autorun/blob/1ac9b6cf37b90f2d5783dd7ef389bebbe1c68877/LICENSE).
AndroSwitch's memory adapter is an original implementation using public libnx
interfaces. No Wine allocator, private newlib chunk layout, custom hbloader,
low-address guest window, swap pager or title-specific memory rule was copied.

The authoritative libnx startup contract is
[`nx/source/runtime/init.c` at v4.12.0](https://github.com/switchbrew/libnx/blob/v4.12.0/nx/source/runtime/init.c#L34-L90).
Its [public SVC definitions](https://github.com/switchbrew/libnx/blob/v4.12.0/nx/include/switch/kernel/svc.h#L169-L203)
identify process memory limits separately from physical memory totals.

## Verification scope

Admission checks and managed budgets do not cap total process memory or
guarantee a contiguous allocation. Aggregate free space can be fragmented.
Existing VM metadata, register and cache paths still include infallible Rust
container growth; converting every such allocation into a recoverable Android
error remains memory-hardening work. The current checks must not be presented
as a guarantee that every admitted APK can finish loading or running.
The [scoped source review](../reports/m9-memory-source-review-20261024.json)
binds the reviewed source hashes and records those limits; it adds no execution
or hardware result.

The pure C policy test exercises alignment, reserve/cap decisions, unavailable
queries, loader overrides, unsigned bounds and free estimates. It can run on
the host without Horizon:

```powershell
gcc -std=c11 -Wall -Wextra -Werror platform/androswitch/tests/memory_policy.c -o target/test-memory-policy.exe
./target/test-memory-policy.exe
```

Cross-compilation establishes API/toolchain compatibility only. Host fake-EGL
worker tests establish ownership and callback ordering only. Neither is an
emulator rendering result or physical-device verification of these changes.
See the separately dated reports and the [roadmap](ANDROSWITCH_ROADMAP.md) for
the current M9 evidence gates. Source hashes and exact test commands are in the
[memory policy/initial worker report](../reports/horizon-memory-glsurface-host-20261024.json)
and [worker suspend/resume follow-up](../reports/horizon-glsurface-host-20261024-v2.json).
These reports preserve the tested source states; later changes need their own
verification.
