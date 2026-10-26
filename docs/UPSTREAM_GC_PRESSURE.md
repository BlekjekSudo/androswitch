# Shared RustyDroid allocation-pressure policy

AndroSwitch retains RustyDroid by nadabgfccvd and contributors as its source.
This integration extracts the byte/slot pressure calculation from original
`gc.rs` at pinned upstream `3761815dc6a2a1427e9fc66b66e8471b78d1b170`
into `gc_policy.rs`. Both the original reference collector and product collector
consume that implementation. The original history, source and licenses remain.

## Observed failure and change

The previous product predicate could collect on every interpreter instruction
after a live/pinned heap crossed half its budget. The original policy already
backs off using remaining byte and object-slot headroom. A focused reproduction
failed before extraction and passed after it.

Backoff alone exposed a second problem. A callback protects existing Rust host
temporaries while Java runs. A GC inside that callback can therefore record a
large pinned live watermark. That value becomes stale when the callback returns:
the next callback must not pin the previous callback's dead objects again before
GC revisits the watermark.

Both collectors now record whether frames were active at the previous collection.
Under pressure, the first unpinned boundary rechecks that watermark with a full
collection. An actually large live heap then records an unpinned watermark and
backs off; unchanged live data does not trigger GC on every UI turn. The Android
looper evaluates pressure before removing queued callback roots, on both software
UI and GLSurfaceView paths. Existing owner-specific frame roots, JNI/native roots,
host temporary pinning and persistent storage remain the active contracts.

The app heap remains 64 MiB. No package branch, test-result substitution, fixture
modification, injected test GC event or alternate StringBuilder store is involved.
Builder text remains the original heap `buf` field shared by both dispatch paths.

## Evidence and scope

The unchanged SDK/NDK development APK has SHA-256
`32718652d7fefd16fa8c555ae2886d3f6ef8463670b46bba72ba44675ba3fcbf`.
The catalog has 257 declared cases; Collections selects 34 of them. An ordinary
host UI run now completes all 34, saves its journal and displays the completion
message. The Text selection also completes 30/30 through the ordinary UI. All
50 original reference suite routes execute, with 710 tests passing; the native
and JIT zero-case targets remain unverified. The combined desktop/product
regressions pass 1,000 tests. All 135 actual product DEX return values also match
the frozen JVM oracle exactly. Default product VM unit tests pass 47/47;
strict workspace/reference Clippy, formatting, the core-only build and 26 Python
tooling tests pass. The earlier pressure-only and
looper-only attempts actually failed with
OutOfMemoryError during journal writes; they are retained as failure evidence.

Focused tests cover allocation growth under a pinned high-water heap, invalidation
of a watermark when its callback unwinds, one full recheck of genuinely live data,
and repeated Android looper turns retaining the pending Handler, activity fields
and listener. Original owner/GC and complete reference/product regressions must
also pass; test counts and target results belong in the checkpoint receipt.

This shares pressure policy, not the entire tracing graph. The shipping profile
still excludes 85 original VM modules. Full weak-reference, generational,
framework-side-table and state-owner integration remain in the
[complete parity ledger](UPSTREAM_PARITY_PLAN.md). Host execution, NRO build,
Ryujinx execution and physical-device evidence must be reported independently.
Earlier user-reported 219/219 physical results apply to their original binary.
Full upstream 1:1 API behavior and independent game first frames remain unverified.
