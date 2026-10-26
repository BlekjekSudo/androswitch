# Original RustyDroid collector on the product profile

Status: host integration verified; Horizon qualification is in progress.
The preceding [pressure-policy checkpoint](UPSTREAM_GC_PRESSURE.md) and its
frozen NRO remain separate evidence.

The default std product and core-only VM now compile original RustyDroid
`crates/rd-vm/src/gc.rs`, rather than a second collector in `portable/gc.rs`.
The original heap graph traversal, generation/remembered-set algorithm, major
escalation, weak/phantom referent clearing, queue enqueueing, sweeping,
promotion, pressure policy and collector statistics share one implementation.
The removed product collector's pressure/lifetime scenarios remain tests against
that implementation in `portable/gc_contracts.rs`.

## State and platform boundaries

The product root adapter supplies actual execution-owner frames and host pins,
statics, boxed values, canonical strings/classes, monitors, framework roots and
JNI/native roots. Reachable platform objects expose their existing side-state
references and collection hooks. Product Matcher-to-Pattern edges stay precise;
the original reference side-table scanner remains in the reference profile.
The default runner does not allocate the independent reference `HostState`.

Platform side-state mutation currently has no complete dirty-owner key contract.
Minor collection therefore revisits old objects when a platform framework is
bound, retaining young references introduced by the actual adapter. This is a
conservative transport boundary, with a potential pause cost still requiring
measurement; it must not be replaced with assumptions about missing write
barriers. Reference-only HostState/graphics/module tables retain their original
scanner. Remaining framework state integration is still part of the full
[parity ledger](UPSTREAM_PARITY_PLAN.md).

GC timing uses the executing platform clock. The core-only profile reports
whether a bound clock exists; it does not fabricate a measured pause. The
reference profile retains its diagnostic `RD_GC` override. Product automatic
collection is independent of desktop environment variables.

## Original contracts on both profiles

Five existing upstream scenarios are shared by the original suites and the
shipping-profile integration target. Their function bodies, plus their heap
classification helper, are byte-identical to pinned RustyDroid `3761815`.
Original test routes and assertions remain; helpers reside below `tests/common`
to avoid accidental standalone integration targets.

The scenarios cover old field/array writes and promotion, a single DEX call
allocating about 1 GiB under a 4 MiB heap while preserving a Rust caller local,
fuel-window behavior, weak/soft/strong referents and one-time ReferenceQueue
enqueueing. Focused execution passes all five on the product profile. Existing
product VM unit tests pass 47/47. A real Android adapter test also preserves a
young side-state element in promoted ArrayList, HashSet and CopyOnWriteArrayList
owners during minor GC, then releases it after clear and major collection.
The core-only build passes.

Full execution passes 710 original/reference tests through all 50 original suite
routes and 1,001 combined desktop/product regressions, with no failed, ignored or
filtered tests. Native/JIT zero-case targets remain unverified. Strict workspace
and reference Clippy pass. The unchanged SDK APK completes 34/34 Collections and
30/30 Text through its ordinary host UI and journal; 135 DEX return values match
the frozen JVM oracle. The NRO and device gates are separate.

These are collector/heap contracts. They do not establish implementation of
every public `java.lang.ref` method, the product WeakHashMap family, every
framework side table, or any untested device service. Source selection now leaves
84 original VM modules outside the default product profile; that count is not an
API denominator. Full upstream API parity and new physical-device verification
remain open.
