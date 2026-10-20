# Garbage collector

RustyDroid's app heap (`rd-vm` `Heap`, budget 256 MB = Moto E5 floor) is
collected by a **safe-point, non-moving mark-sweep** collector
(`crates/rd-vm/src/gc.rs`).

## When it runs

- **Between MCP tool calls** (`rd-agent` `call_tool`) — no interpreter frame
  is live there, so the Dalvik registers (which live on the Rust stack) are
  not a root set.
- **Between 512 ms slices of a long `wait`** — same property; runnables still
  run in deadline order.
- **Trigger** (`Engine::maybe_gc`): heap used ≥ max(2 × live after the last
  cycle, live + 16 MB), capped at half the budget so the next call always has
  ≥ 50 % headroom. `RD_GC=off` disables automatic collection.
- **On demand**: MCP `runtime_health {"gc": true}` forces a cycle.
  `runtime_health` reports `gc` stats (cycles, last live/freed objects and
  bytes, heap before/after, pause, totals); each automatic cycle logs
  `I/RdGc: freed … in … ms` to the app log.

## Roots and tracing

- **Heap graph — precise**: instance fields and array elements (`Value::Obj`).
- **Engine state — conservative**: statics, the framework host state, looper
  and threads, choreographer, callbacks, insets/back dispatch, content,
  native/JNI state… are rendered with `Debug` and every integer token that is
  a valid `ObjRef` is a root. Type-agnostic, so a reference kept in any of the
  ~40 host side tables cannot be missed; the cost is some over-retention. The
  parsed APK is excluded (no refs; it made the first cycle ~11× slower).
- **Weak / ephemeron tables**: `fw.objects` (host half of host-backed objects:
  collections, views, bundles…), `gfx.objs` (Paint/Path/RenderNode/Bitmap
  display lists) and module side tables whose values hold refs (`Matcher`,
  `Spannable`, `Reader`/`Writer`) are traced only when their key object is
  reachable. After the sweep, every table keyed by `ObjRef` (also `Random`
  seeds, `Uri.Builder`, XML parsers, annotations, `BigInteger`, `BreakIterator`)
  is purged of freed keys — a recycled slot inherits no state.

## Sweep and reuse

Unmarked slots become tombstones (`Lrd/gc/Freed;`, contents dropped, bytes
returned to the budget) and go on a free list that `alloc` reuses before
growing, so the 2^20-slot cap bounds live objects rather than total
allocations.

## Known limits / next steps

- Weak/soft references are traced strongly (never cleared) — correct but
  conservative; clearing + `ReferenceQueue` enqueue is the next step.
- No collection *inside* a single long-running Java call (no frame maps); the
  slice safe points cover `wait`, other tools allocate little per call.
- Pause is proportional to live objects + host state (~20–65 ms on Comprix,
  live 30–70 k objects); generational/incremental marking would cut it.
