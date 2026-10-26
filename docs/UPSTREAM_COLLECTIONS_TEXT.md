# Shared collections and text state

The implementation comes from **RustyDroid by nadabgfccvd and contributors**,
pinned at `3761815dc6a2a1427e9fc66b66e8471b78d1b170`. All original source/history
and licenses are retained. This checkpoint connects complete inherited modules
to shared state; it does **not** close complete upstream API parity.

## State boundaries

`jcollections.rs`, `jiterator.rs` and `collections.rs` now compile in the product
VM. Their algorithms use `collection_factories::{snapshot,create,replace}`.
With a bound `FrameworkHost`, those operations exclusively use its collection
owner. The reference `HostState` is used only without a bound framework.
Unsupported owner operations fail explicitly and never allocate fallback state.

The product adapter retains its existing list/set/map store, value budgets,
modification counters and fail-fast ordinary iterators. Inherited ListIterator
snapshots use that same store. Constructors, bulk operations, array conversions,
object methods and sorting/comparison callbacks therefore observe the values
already used by the application. Sublist writes publish to their parent, including
nested views. Parent edges are actual heap fields followed by either collector;
the inherited bounded view registry is weak and is purged when slots die.

`jlstring.rs`, `jsb.rs` and `jregex.rs` also compile under the shipping std profile.
StringBuilder/StringBuffer text lives in the inherited heap `buf` field; the
separate Android adapter builder map and its duplicate dispatch were removed.
String/regex algorithms and formatting remain upstream code. Their exact
implemented overload metadata is in `shared_text.rs`; SDK class declarations
alone do not grant an implementation. Callback arguments and staged values stay
rooted through guest reentry and GC. Product StringBuffer operations enter the
actual VM monitor and preserve contention/errors and release on failure.

Regex compilation caches and Matcher/MatchResult state belong to one Engine.
The cloneable table handle was extracted from `module_tables.rs`, without a
global registry. A reachable matcher/result retains its Pattern; unreachable
entries are purged before heap slots can be reused. The cache keeps the original
bounded compilation/backtracking policy. No Python or host subprocess is used.

The shared object-to-string helper now consults the bound framework's value
method before falling back to Object identity. Appending a collection must read
the active collection owner rather than format a disconnected reference object.
Inherited DEX template lookup follows superclass facts while concrete DEX
overrides retain interpreter precedence.

## Evidence and scope

The same Conformance APK source now has **257 declared cases**, including
**135 pure Java/DEX cases with actual JVM expected results**. Twenty-two new cases
exercise collection constructors, bulk operations, sublists, list iterators,
typed arrays, DEX AbstractList/AbstractCollection subclasses, exceptions and
comparator GC. Sixteen exercise String comparisons and CharSequence operations,
regex flags/lookaround/named groups, UTF-16 match offsets, match-result snapshots,
pattern replacement, append callbacks, exceptions and GC.

Development builds use `local-artifacts/conformance-collections-text-final`, preserving
the previously qualified 219-case APK and std ZIP. The direct user report of
219/219 on Switch belongs to that earlier [std candidate](HARDWARE_FEEDBACK.md).
It is not verification of a changed APK or this source checkpoint. Host, target
build, emulator and hardware results must remain separate in the evidence report.

Host verification for this source checkpoint: all 50 original suite files are
registered and the reference run has 706 passing tests; the combined product and
desktop regression run has 995 passing tests with no failures, ignored or
filtered cases. The actual product interpreter matches all 135 JVM catalog rows.
Core-only compilation, workspace/reference strict Clippy, formatting, 26 Python
tests and the reviewed source gate pass. Zero-case native/JIT targets do not
become verified through those totals. Target execution is recorded separately.

The std NRO built from `7fca532` has SHA-256
`1a577dd42a8fe673e511800f95d020995dd15e5ff6dc822818bc1081a39aca7c`.
Two pinned headless Ryujinx Collections attempts timed out at 180 and 600 seconds.
Their aggregate journals contained 17 and 14 PASS rows respectively; neither
finished the launcher workflow. Both original SD trees were restored. No API
failure or completed emulator qualification is inferred from those attempts.
The Text group has not been run on this NRO. Host resource pressure was observed,
but an ordinary host UI/journal diagnostic also progressed slowly. It was stopped
after a debugger sample found the main thread in automatic GC. Original
`gc.rs` has pinned-allocation pressure backoff that the product collector still
needed to integrate and verify at that checkpoint; the profile sample was not a complete root-cause
proof. The partial journals, process diagnostic and raw stack remain in the
[checkpoint report](../reports/collections-text-20261026.json).

The subsequent [GC pressure integration](UPSTREAM_GC_PRESSURE.md) extracts the
original adaptive policy into one shared module and rechecks a pinned live
watermark after callbacks unwind. The ordinary SDK APK's Collections UI run now
finishes with 34/34 PASS, including its full journal writes, at the unchanged
64 MiB application heap. This closes that host UI diagnostic, not the complete
upstream collector/state integration or the Horizon/device verification gates.

The combined product regression run caught older assertions that expected
collection equality to fail and formatted integral float coordinates without
Java's `.0` suffix. These now assert actual JVM value behavior. The same run
also found that CharSequence-declared builder calls were missing the shared text
dispatcher; final text receivers now select that dispatcher. The existing
lifecycle, graphics, TextUtils and JNI contracts pass in focused reruns, including
the negative JNI check for a builtin object with no active provider state.

## Remaining contracts

- The complete upstream parity [ledger](UPSTREAM_PARITY_PLAN.md) remains active;
  collections, text and regex do not qualify the other framework modules.
- Map constructors/views/bulk operations, sorted containers, queues and the rest
  of Collections utilities still need common-owner integration. `newSetFromMap`
  and the two incorrect sorted empty factories remain reference-only.
- Inherited unmodifiable wrappers are snapshots, not live JDK views. The bounded
  sublist implementation and snapshot ListIterator are retained; ListIterator
  add/set remain explicit UnsupportedOperationException. Full JDK fail-fast view
  semantics are not established by these tests.
- The inherited singletonMap accidentally used a mutable HashMap. It now uses
  the existing immutable map marker, verified against the JVM's mutation error.
- CopyOnWriteArrayList/Set keep the product's existing generation/lock model.
  Missing bulk/equality behavior is not replaced by an unsafe reference fallback.
- Builder capacity and comparison policies retain inherited limitations.
  Isolated UTF-16 surrogate mutations still fail explicitly on the product path;
  the Rust-string heap cannot claim to preserve those code units. Monitor
  contention retains the existing explicit scheduling limitation.
- Native/JIT zero-case oracle targets and physical verification of this changed
  build remain separate, unverified surfaces. Independent games are deferred
  until the user's complete-upstream-parity requirement has been met.

The remaining Java inventory also contains inherited platform/reference policies,
not just disabled declarations. For example, `juuid` uses a deterministic seeded
generator for reference runs; its production random source must bind the Horizon
provider. `jstring_tokenizer` keeps state in HostState, and `jbreakiter` uses
reference object state with an explicitly approximate Unicode algorithm. Their
complete source and original tests are already present; integration must retain
those algorithms and expose their actual contracts through the active owner.
