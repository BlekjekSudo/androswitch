# RustyDroid source synchronization and Horizon port

AndroSwitch is a history-preserving fork of **RustyDroid by nadabgfccvd and
the RustyDroid contributors**. The original fork point is
`6388281f66d254c8e01a0fdc7a5a9a30239bc762`. The synchronization target is now
[`3761815dc6a2a1427e9fc66b66e8471b78d1b170`](https://github.com/nadabgfccvd/RustyDroid/commit/3761815dc6a2a1427e9fc66b66e8471b78d1b170),
the latest upstream main checked on 2026-10-26. Both histories are retained;
this is a merge, not a source dump without attribution. See [credits](../CREDITS.md)
and [notice](../NOTICE.md).

**Full upstream behavior parity on Horizon is not achieved.** Bringing source
into Git, compiling a module, reaching its dispatch, implementing its target
services, and passing its behavior contracts are separate requirements.
In particular, the original desktop `HostState`, thread-local tables, cooperative
threads, process filesystem and Unicorn backend cannot become a second,
disconnected Android environment inside the Switch runner.

## Complete source inventory

[The pinned manifest](../data/rustydroid-upstream.json) inventories **every path**
in the upstream Git tree, rather than selected class families. Each entry has
the original Git blob ID. A changed file also records its reviewed adaptation
reason and canonical SHA-256. Text canonicalization only normalizes CRLF to LF.
Missing files, duplicate entries, an incomplete manifest, an unreviewed change,
or drift after review fail the gate.

```powershell
python -X utf8 scripts/check-upstream-sync.py --check-latest --report target/evidence/upstream-sync-inventory.json
python -X utf8 -m unittest discover -s scripts/tests -p test_upstream_sync.py -v
python -X utf8 scripts/check-upstream-sync.py --require-device-parity
```

The last command is **expected to fail** while device parity is unverified.
The ordinary source gate can pass while the device gate fails; this prevents
source synchronization from being reported as runtime compatibility. The report
enumerates every original VM source excluded from device compilation. A shared
file can still contain target-specific branches: its presence does not prove
all of its methods work. The report is an omission audit, not an Android API
coverage percentage or a substitute for differential/execution tests.

For an upstream update, fetch and pin a full commit, merge it, resolve each
adaptation against the previous and new upstream source, retain the new tests,
and review the manifest entry for **each changed local adaptation**. Do not
regenerate approved hashes merely to silence a failing gate. Keep the upstream
commit available in the local history; CI checks the actual Git tree, so an
accidentally shortened manifest cannot hide an omitted module.

Original upstream README/contribution/security documents are preserved as
`*.rustydroid.md`. Upstream GitHub bot workflows, CODEOWNERS and Dependabot
configuration are preserved byte-for-byte under `docs/upstream/github/` instead
of activating upstream repository administration in the fork. Runtime source,
data registries and tests retain their original locations.

## Shared implementation and target adapters

The synchronization includes the latest APK/DEX hardening, lazy DEX/string
cache, VM/framework updates, native oracle changes, registries and original
tests. Portable parsers use `alloc`/`core`. The immutable upstream lazy caches
use a synchronized `OnceLock` adapter backed by `spin::Once` on device;
initialization only parses immutable bytes and never calls guest code. They
retain thread-safe publication instead of replacing it with an unsynchronized
cell. The positional ZIP interface is shared by Horizon VFS and the original
desktop page cache; streaming and materialization budgets remain explicit.

The current port also compiles the **latest upstream `Arrays` and `BitSet`
implementations on both VM paths**. Array formatting and object-value helpers
were extracted into shared modules. The device does not contain another fill,
sort or BitSet implementation. Exact overload metadata feeds device JNI lookup;
missing overloads remain unavailable. Array snapshots pin their copied references
across guest callbacks and GC. Callback recursion depth belongs to the actual
execution owner on Horizon. JVM observations justify reference-array store
checks and normalization of DEX's zero/null register representation.

Original BitSet and narrow-array test files are used directly as portable test
targets; [rd-testsupport](../crates/rd-testsupport/Cargo.toml) can select either
VM dependency without accidentally enabling desktop `std` in the device build.
The existing combined Java/DEX fixture gains array contracts; no extra APK is
needed for every small API addition. This includes a JVM comparison and a
separate inherited snapshot/GC contract. Mutating a sort input from its comparator
is not used as a JVM semantic oracle.

These shared families do **not** establish complete Java or Android support.
For example, upstream BitSet's empty constructor capacity differs from the JDK,
and the upstream Arrays dispatcher does not implement every JDK overload.
The inherited behavior is preserved and its limitations stay visible.

## Remaining parity work

The machine inventory is the authoritative exhaustive file list. The work is
organized by common state and dependency boundaries, rather than the next APK
failure:

1. Extract the remaining JDK/value algorithms, dispatch metadata and original
   tests into the shared core: string, buffers/I/O, collections, numeric/text,
   reflection and class loading. Avoid duplicate platform stores.
2. Move compatible framework state out of desktop `HostState` and thread-local
   tables. Both Java and JNI must refer to the same live objects, GC roots,
   VFS, resources, package registry and lifecycle on Horizon.
3. Bind UI/rendering, scheduling, clock, threading, media and native services
   to real platform adapters. Desktop scheduling, subprocess SQLite and virtual
   hardware responses cannot stand in for real Horizon behavior.
4. Port upstream contract suites alongside each module, then run the same
   scenarios on the portable VM and through the NRO. Audit constructors,
   overloads, exceptions, callbacks, GC and thread ownership; detection of a
   class name is insufficient.
5. Audit the latest AndroidX/GMS/virtual-device surfaces against the offline
   release contract. Preserve upstream reference source and tests without
   silently advertising unavailable hardware, Play services or networking.

The launcher, native Horizon backend and persistent storage adapters remain
AndroSwitch platform components. The upstream Unicorn/Python execution backend
is an optional host oracle. There are no package-specific fallback profiles.

Source/host/emulator/device results are tracked separately in the execution
report. The earlier user-reported 219/219 hardware result applies to its original
binary and fixture only; it is not evidence for this merged source or all of
upstream. Mekorama and Candy Crush remain integration probes, not API profiles.
