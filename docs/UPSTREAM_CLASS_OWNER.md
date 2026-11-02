# Canonical Class ownership

The canonical Class factory is extracted from RustyDroid's original
`framework.rs`, by nadabgfccvd and contributors. Both execution profiles now
delegate to that same body in `java_values`. Its heap descriptor fields,
Intent-compatible name alias and catchable allocation failures are preserved.
The existing shared String allocator supplies the original OOM formatting.

One Engine-owned descriptor cache replaces both `HostState.class_objs` and
`portable.classes`; the old maps are removed. The original common collector
traces the actual cached Class references and their ordinary heap graph on
both profiles. A Class is published only after its descriptor fields are fully
initialized. Failed construction cannot leave a partially initialized cached
entry. No disconnected reference framework owns these values.

The full product regression exposed a legacy desktop GC false root: numeric
values in an untouched default HostState could retain an unrelated tokenizer
at the same arena ID. The existing exact-default comparison, already used by
the upstream-runtime reference profile, now also guards desktop scanning. A
default framework created without a heap cannot contain actual heap references.
Any non-default state still receives the original conservative scan, while its
object table keeps its separate ephemeron traversal. This is a general root
classification correction; the failed product run remains diagnostic evidence.

Two shared unit contracts cover primitive, array and Unicode descriptors,
canonical identity through minor/major collection, the inherited descriptor
alias, Engine isolation, failed allocation and reclamation. Existing Class,
reflection, DEX, JNI and SDK scenarios remain the regression corpus. This
refactor does not add new API declarations or increase the SDK case count.

The cache remains scoped to the Engine's current classpath, as in upstream.
Independent Java loader namespaces, unloading, complete reflection metadata,
host enum constants, and framework service/state integration remain separate
open boundaries. Exact host/device evidence must identify this implementation;
the 309-case tokenizer NRO does not contain this later change. This is not a
claim of complete upstream 1:1 behavior or physical hardware qualification.
