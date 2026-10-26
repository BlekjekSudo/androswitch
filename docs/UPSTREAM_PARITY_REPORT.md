# RustyDroid parity checkpoint — 26 October 2026

**Answer: AndroSwitch is not yet 1:1 with every upstream API on HorizonOS.**
The complete inherited framework dependency closure now compiles as a portable
reference core and its active VM corpus passes. The shipping runner does not
yet select that complete path. Source retention, type declarations, host tests
and a successful target check are not evidence that every API runs on Switch.

## Source and implementation

- Upstream: `3761815dc6a2a1427e9fc66b66e8471b78d1b170`, checked against upstream
  main on 26 October 2026. Original base/history/authors are retained.
- Implementation commit: `9b6679e9450a8209f17aa713c82d4fda78279072`; detailed records
  are in the [machine report](../reports/upstream-parity-20261026.json).
- Entire source tree audited: **427 original files**, 278 identical, 149 reviewed
  adaptations, nine relocations; **40 added core helpers** have source/hash
  receipts. File counts are not API coverage.
- `as-vm` compiles the original RustyDroid VM source. The experimental
  `upstream-runtime` uses its dispatcher, framework and GC through `core + alloc`.
  Thirteen tables in eight modules now have Engine ownership; the portable SQL
  transport uses real native SQLite instead of a Python subprocess.
- All 50 original VM integration source files are registered. Their builders
  and the original host renderer use the isolated portable Engine. No copied
  substitute test bodies or APK-specific runtime branches are introduced.

RustyDroid by **nadabgfccvd and the RustyDroid contributors** remains the
foundation. Its original Git history and MIT OR Apache-2.0 licenses are retained.
See [credits](../CREDITS.md) and [notice](../NOTICE.md).

## Evidence and its limits

| Layer | Result | Scope |
| --- | --- | --- |
| Portable reference VM | **707 passed, 0 failed, 0 ignored** | Unit suite plus all 50 inherited integration targets; native/JIT targets contain zero cases and remain unverified |
| Desktop and existing runner models | **982 passed, 0 failed, 0 ignored** | Includes the 21 fixture-backed cases normally ignored; this is host execution, not a Switch or emulator pass |
| Source/evidence/generator checks | **17 passed** | Ten omission/drift tests, four evidence-accounting tests, three SDK declaration parser tests |
| Horizon | **Core target check passed** | `aarch64-nintendo-switch-freestanding`, `-Zbuild-std=core,alloc`, complete reference features; no NRO link/runtime claim |
| Product integration | **Incomplete** | Default runner excludes 91 original VM files; experimental declarations/compilation are recorded separately |
| Ryujinx | **Not run for this implementation** | Previous NRO47 attempts are historical; neither game has a confirmed first frame |
| Physical Switch | **Not run for this implementation** | The earlier user-reported 219/219 result applies to its original binary/fixture only |

Formatting and strict Clippy checks, commands, per-suite results, zero-case
targets, captured log text and SHA-256 values are retained with the machine report.
Versioned evidence normalizes UTF-8/LF and final blank lines only; original
capture byte hashes and unchanged parsed results are recorded separately.
Host Rust is 1.97.1 (`8bab26f4f`); the freestanding check uses the existing
1.100.0 nightly (`787af2b8c`). These are task-local/pinned toolchains; no global
Rust default was changed.

The additional float oracle caught the Windows host's
`f32::powi(-1, i32::MAX) == +1` observation. JVM `Math.pow` and the double oracle
produce `-1`, as does the portable adapter. Extreme-power expectations therefore
use the double oracle for the selected exactly representable cases; the
original failing run and the independent Rust/Java observation are retained.
This does not establish every Android math or JNI floating-point contract.

## What prevents a 1:1 result

The [platform integration ledger](UPSTREAM_PARITY_PLAN.md#platform-integration-ledger)
tracks the remaining state boundaries. The inherited implementation must own
one common graph; a fallback must not create a second filesystem, collection,
thread identity or view tree.

1. **Dispatch/state:** the full reference path is not enabled by the shipping
   runner. Lifecycle, UI/resources, collections/iterators, loaders/enums,
   callbacks and JNI metadata still require one connected implementation.
2. **Persistent storage:** upstream `jfile::Vfs` is an explicitly scoped memory
   oracle. It must use the existing persistent `GuestFileSystem` shared with
   native code. SQLite/prefs need the same live objects, durable paths and
   flush/error behavior. The native SQL transport retains explicit 8 MiB image
   and 32 MiB allocation bounds, rather than assuming desktop resource parity.
3. **Real platform behavior:** cooperative reference threads, virtual time,
   reference device/build values and optional virtual services must be bound
   to actual Horizon services and execution owners. No simulated physical
   service is advertised as hardware support.
4. **Native/JNI:** complete overload metadata and full upstream native backend
   contract comparison are unfinished. Native/JIT zero-case suites are not
   successful backend execution. Desktop Unicorn/Python/JIT are not NRO
   dependencies.
5. **Execution evidence:** there is no new linked NRO, emulator corpus or
   hardware run for this complete reference path. Mekorama/Candy Crush first
   frames and the complete M9 release gate remain open.

The inherited collector's side-state scan remains conservative. Actual owner,
platform and native roots are added, and an exactly default reference-free
framework is excluded from scalar-ID false roots. This is not a claim of a
fully precise collector. Module table lifetime is now per Engine; desktop
environment overrides are unavailable in the standalone freestanding reference
adapter, and platform profile binding remains part of the integration work.

## API inventory interpretation

The JSON report retains every original VM source route and all **106 groups**
in the upstream API catalog, including its original partial/stub/unimplemented
status and **489 example API entries (488 distinct names)**. Those examples are not an exhaustive list
of every overload. Full-family Horizon parity remains unverified for every
catalog row until its integration and behavior evidence are complete. This
does not mean all existing, separately tested device subsets are absent.

The supplemental **5,981 SDK type declarations** provide inheritance,
interfaces, modifiers and names only. Their API 35 input is pinned in
[the receipt](../data/android-sdk-types.json); they do not implement those
methods or replace the API 26 reference behavior profile.

The source-sync gate passes, while `check-upstream-sync.py
--require-device-parity` intentionally exits **1**. It must remain non-passing
until the missing integration and verification are addressed. No percentage
of API parity is inferred from source counts, test totals or two applications.

## Reproduction and next gate

The exact commands and retained outputs are in the machine report. The core
commands are:

```powershell
cargo test -p as-vm --features upstream-parity-tests --lib --tests --no-fail-fast -- --test-threads=1
cargo test -p rd-vm -p as-framework -p as-switch-smoke --features as-framework/host,as-switch-smoke/host-cli,as-switch-smoke/portable-tests --no-fail-fast -- --test-threads=1 --include-ignored
cargo +nightly check -p as-vm --features upstream-runtime,advanced-components --target aarch64-nintendo-switch-freestanding -Zbuild-std=core,alloc
python -X utf8 scripts/check-upstream-sync.py --require-device-parity
```

Host fixture locations/toolchains are documented in [testing](TESTING.md) and
[building](BUILDING.md); selected cached APK hashes are retained with the
checkpoint. The next gate is the common runtime bound to Horizon storage,
state and services, followed by the product corpus through an actual NRO and
Ryujinx. This checkpoint does not mark complete upstream parity or M9 complete.

The separate [Horizon std feasibility probe](HORIZON_STD_REPORT.md) subsequently
ran a real std/libnx NRO: 29 of 32 finite contracts passed. It does not exercise
this full Android framework path. The decision remains to integrate the common
portable runtime; upstream API behavior on the product runner is still not 1:1.
