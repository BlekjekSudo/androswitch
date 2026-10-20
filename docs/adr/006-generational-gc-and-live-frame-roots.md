# ADR 006 — non-moving generations and live-frame GC roots

Status: implemented; differential/regression and real-APK validation in progress.

## Context

Tool-boundary full GC cannot reclaim allocations in a long Java invocation.
Precise suspended-frame roots and protection of Rust host temporaries are
required before collection can run with an interpreter frame live. Moto E5
also benefits from tracing young objects rather than the entire old heap.

## Decision

Keep stable ObjRef slots and stop-the-world collection, with a young generation
and periodic full collections. Heap mutation access (`get_mut`, including field
and array writes) records old objects in a remembered set. Minor collections
retain old objects, trace remembered old heap objects, and conservatively scan
all old host/module side states, whose mutation APIs are not write-barriered.
Surviving young objects are promoted after each minor. A full collection runs
periodically and under pressure; explicit `gc()` remains a full collection.
Slot pressure starts at 75% of the slot cap and backs off through remaining
headroom when a live/pinned heap cannot shrink. Unchanged heaps are not
recollected on every instruction; a tool boundary can retry after pins release.
No concurrent marker, moving heap or guest Android runtime is introduced.

Interpreter frames publish only ObjRef roots before calls/class initialization
and when collecting. The current frame additionally publishes last-result and
pending-exception references. Suspended callers retain their published roots.
Host-dispatch scopes pin allocations made in Rust host code; guest re-entry
suspends allocation pinning while preserving earlier host temporaries. Guest
object/exception return values are pinned into the surrounding host scope. Top-level
Java entries conservatively pin pre-existing objects because their Rust caller
may hold references not yet stored in engine state. Scope cleanup occurs on
normal and error returns. This deliberately favors over-retention over dangling
references. Public explicit GC outside a Java entry still requires callers to
root local references in engine state.

## Consequences

Minor pauses avoid traversing unchanged old Java graphs. Old host side state is
still scanned, so pause improvements depend on the workload; do not claim Moto
E5 measurements from desktop results. Unreachable old objects and weak old keys
can persist until the next major collection. WeakHashMap values remain strong,
not ephemerons (values that reference keys retain them, matching Java).

Cranelift/JIT GC integration is not implied: compiled execution must publish
roots or use interpreter fallback before allocating/calling out.
