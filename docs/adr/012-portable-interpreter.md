# ADR 012 — share the upstream interpreter across host and Horizon

Date: 2026-10-23. Status: accepted; M1 host/Ryujinx gate verified.

`rd-vm` now has a default `std` feature for its existing desktop framework.
Without default features the **same** `Engine`, Dalvik dispatcher, classpath,
heap, value model, exception handling and numeric formatter compile with
`no_std + alloc`. This is a port of the upstream interpreter, not a second
arithmetic-only execution loop. Portable ordered maps replace host hash maps.

Desktop-only framework fields/modules, environment tracing, subprocess SQLite,
regex and global/thread-local host stores are excluded from the device graph.
Host tests and behavior remain on the default feature path. Device native and
framework APIs are unavailable until their respective milestones. Intrinsic
lookup misses fall through to actual DEX resolution and descriptor-bearing
errors, rather than successful dummy return values. Instantiating an object
requiring missing platform state fails explicitly. Missing platform static
fields fail rather than becoming a default primitive value.

Portable VM state owns canonical class objects and interned strings. Its full
mark-sweep collector traces actual statics, interpreter frame roots, host pins,
strings and class objects using the shared heap and slot reclamation. It has
no framework side tables, weak-reference implementation or generational fast
path. Future platform adapters must extend this root contract before storing
object references. Full platform/reference/GC compatibility remains a later
gate, especially JNI lifetime tests in M4.

`as-probe` and the libnx C ABI use one bounded method-probe JSON protocol. A
request selects arbitrary class/method descriptors and arguments; the runtime
contains no fixture package/method names. Fixture-specific calls live only in
the test harness. Reports identify portable vs desktop VM linkage explicitly,
since Cargo features are additive. This probe does not launch Activities and
must not mark application lifecycle or JNI support as working.

The probe caps fuel at one million instructions per invocation, depth at 16,
and guest heap accounting at 32 MiB. The native NRO heap remains 256 MiB. The
host CLI uses a 32 MiB worker stack because debug interpreter frames exhaust
Windows' default stack before the guest depth guard. These are test-runner
budgets, not an assurance that arbitrary APKs fit these resources.
