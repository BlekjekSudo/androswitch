# Garbage collector

RustyDroid's app heap (`rd-vm` `Heap`, default budget 256 MB) uses a
**non-moving, stop-the-world generational mark/sweep** collector. ObjRef slots
stay stable and freed slots are recycled. See [ADR 006](adr/006-generational-gc-and-live-frame-roots.md).
This is a generational implementation, not incremental or concurrent marking.

## Safe points and scheduling

- Between MCP tool calls and between 512 ms slices of long waits.
- **Inside a single Java invocation**: the interpreter checks allocation
  pressure before instructions. Frames publish roots before nested calls,
  class initialization and collection; suspended callers remain rooted.
- Minor collections trace young objects, remembered old heap writes and old
  host/module side state. Survivors are promoted. A major collection normally
  runs every eighth cycle, under byte/object-slot pressure, or after a minor
  leaves the heap above half its byte budget.
- The byte watermark grows from the last live size: max(live × 2, live +
  16 MiB), capped at half the budget while live is small and halfway through
  remaining headroom when live is larger. Object-slot pressure starts at 75%
  of the 2^20-slot cap. **Inside a live frame it backs off through remaining
  slot headroom if a pinned/live heap cannot shrink**; an unchanged heap is
  not recollected at every instruction. Tool boundaries can collect again
  after temporary pins have been released. Hard allocation budgets still
  report OOM; collection is not a promise that every workload fits.
- `RD_GC=off` (or `0`) disables automatic collection. Explicit `Engine::gc()`
  and MCP `runtime_health {"gc": true}` still request a full collection.

`runtime_health` reports minor/major counts, last kind, traced/live/freed
objects, weak references/keys cleared, before/after bytes, pause and totals.
`RD_GC_TRACE=1` logs phase timing. Interpreter-internal cycles appear in these
counters; only cycles initiated by the agent's tool-boundary path produce its
`I/RdGc` app-log message.

## Roots and barriers

- **Precise Java roots**: instance fields, arrays, register ObjRefs,
  last-result and pending-exception references.
- **Host temporaries**: a host-dispatch scope pins its allocations. Guest
  re-entry suspends allocation pinning for guest allocations but preserves
  earlier host temporaries. Object/exception results returned to a host scope
  are pinned until that scope exits. Top-level Java entries conservatively
  pin pre-existing objects, since Rust callers may hold otherwise untracked
  local references. Scope cleanup covers normal and error returns; direct
  consumers that catch runtime panics should discard the engine (MCP does).
- **Engine side state — conservative**: statics, framework state, looper,
  callbacks, insets/back dispatch, content, native/JNI state and similar
  tables are rendered with `Debug`; integer tokens matching valid ObjRefs
  are roots. The parsed APK is excluded because it has no heap references.
  Scalar/id collisions can retain extra objects.
- Heap mutation access (`get_mut`, including fields and arrays) remembers
  old objects. Old host/module side states are scanned conservatively on
  minors because not all their mutation APIs have barriers.
- The host halves (`fw.objects`, `gfx.objs`, Matcher/Spannable/Reader state,
  etc.) are traced only for reachable key objects. Old side states are also
  examined during a minor. Dead side-table keys are purged before slot reuse.

External Rust callers of explicit GC must put local references in engine state
(e.g. statics) before collecting. An arbitrary Rust local is not a GC handle.

## Weak references and WeakHashMap

`WeakReference` and `PhantomReference` referents are cleared when otherwise
unreachable; registered queues are populated using GC-visible heap fields.
`SoftReference` currently remains strong. ReferenceQueue polling/removal is
non-blocking in this headless runtime.

**WeakHashMap keys are weak, values strong.** Dead object-key entries are
pruned by collection; null and scalar keys remain strong. A value that points
back to its key keeps that key alive, as on Java WeakHashMap; this is not an
ephemeron map. Old dead weak keys/referents can survive until a major cycle.

## Limits and validation

Minor marking avoids walking unchanged old Java graphs, but still scans old
host state. Marking, slot initialization/sweep and conservative roots still
have live-set/arena-size costs; long pauses are possible. This does **not**
establish a Moto E5 pause budget or ARM device performance result.

Regression tests cover old-to-young fields/arrays/host lists, one Java call
allocating roughly a GiB under a 4 MiB heap budget, guest-returned host
references, weak keys and an above-watermark pinned heap that must not enter a
collection storm. Real-APK runs and their limitations are recorded in
[Comprix end-to-end notes](comprix-e2e.md).
