# Appendix C — literal completion program

This document tracks the decision to use the **literal** definition of
complete for the fields currently marked `partial` in
`data/api-coverage.toml`: a package is not complete because a hot-path subset
works. It is complete only when the whole registered public Android surface for
API 26–36 has behaviour, tests and documented failure modes in RustyDroid.

The registry remains the source of truth. This page is the execution ledger:
it explains what still blocks each promotion and in which order the blockers
are being removed. No status in `data/api-coverage.toml` may be flipped to
`supported` merely because this page lists a plan.

## What “literal completion program” means

Read the phrase in its ordinary, strict sense:

- **Literal** modifies the definition of _complete_. “Complete” means the
  whole registered scope is accounted for; it does not mean “good enough”,
  “the common cases work”, “the app starts”, or “the subset needed today
  works”.
- **Completion** is the final state being pursued, not the amount of effort
  already spent and not the fact that a commit, PR, checkpoint or round was
  finished.
- **Program** means a coordinated, multi-step body of work with an explicit
  ledger of remaining blockers. It is not an executable program, and the
  existence of this document is not itself evidence that anything is complete.

In short:

> A registered Appendix C scope is literally complete **only if every
> in-scope public API and Android-visible behaviour is implemented, tested
> and truthfully recorded**. Anything less is progress, not completion.

### Binding rule for future agents

Before writing or saying that a package, class or registered surface is
“done”, “complete” or `supported`, a future agent must:

1. identify the exact registered prefix and scope in
   `data/api-coverage.toml`, using the API 26–36 public Android surface;
2. inventory every in-scope class/interface/enum, constructor, method and
   field, instead of starting from only the calls used by one app or test;
3. account for every inventoried item as either implemented and tested, or
   as an explicit remaining blocker — an unaudited item counts as a blocker;
4. implement Android-visible semantics, not merely signatures: state
   transitions, return values, callback order, exceptions, permissions,
   lifecycle/threading rules, process boundaries and persistence where the
   platform exposes them;
5. test success, error, boundary and lifecycle paths, with real-APK evidence
   for hot paths where the completion gate below requires it; and
6. update the registry and generated reports so they agree with the code and
   tests in the same change.

If any in-scope item is still missing, unknown, stubbed, defaulted,
unimplemented or untested, the surface is **not literally complete**. Leave
it at `not_implemented`, `stub` or `partial`, as appropriate, and name the
remaining blockers. When in doubt, choose the lower, more honest status.

The following are useful progress signals, but **none of them is literal
completion**:

- the code compiles;
- a selected unit/integration test subset passes;
- one app reaches a screen or one golden path works;
- most or all commonly used methods work;
- class hierarchies, constants or method signatures exist;
- methods exist but return a plausible default or silently do nothing;
- a percentage estimate sounds high;
- a long-running task, checkpoint, commit or PR has finished.

### Incremental work is allowed; false completion is not

This program expects incremental rounds. An agent may correctly finish a
requested checkpoint without literally completing the whole Android surface,
but must report those as two different facts, for example:

> “This checkpoint implemented subset Y and its tests. Surface X remains
> `partial`; blockers A, B and C remain.”

It must not shorten that to “X is done” or promote X to `supported`. If a
task explicitly asks for literal completion of a scope, the agent must work
from the full inventory and either close every blocker or stop with the
remaining blockers stated explicitly. It must not silently redefine the
requested scope to whatever subset happened to be implemented.

For example, correctly implementing `put`, `get`, `size` and `clear` for a
collection class is real progress, but the class or package remains
`partial` if its views, iterators, bulk operations, equality/ordering
semantics, serialization forms, edge cases or any other in-scope members are
still missing or unaudited. Only the exact registered scope that passes the
full gate below may be called complete.

## When a surface is `not_implemented` or `stub`

Classify the **registered surface** (the exact package or class prefix in
`data/api-coverage.toml`), not the outcome of one call. A single unsupported
overload may return a typed `NOT_IMPLEMENTED(...)` error inside a `partial`
surface; that does not make the whole surface `not_implemented`. Conversely,
one hierarchy entry or constant does not make a package `partial`.

Use the narrowest honest prefix. Registry lookup uses the longest matching
prefix, so a real class can be `partial` or `supported` while its surrounding
package remains `stub` or `not_implemented`. Do not let a broad package entry
claim credit for one unrelated class.

### Decision rule

Ask these questions in order:

1. **Is there production host code that intentionally owns this prefix?**
   - No → `not_implemented`.
   - Yes → continue.
2. **Does that code perform any Android-visible behaviour?** Examples are
   retaining state, computing a result from inputs, changing lifecycle,
   invoking callbacks in the platform order, enforcing a permission, or
   raising the Android exception required by the operation.
   - No; it only supplies type shape, constants or a harmless fallback →
     `stub`.
   - Yes, for a real subset → `partial`.
3. **Does the whole registered scope pass the literal completion gate
   below?**
   - Yes → `supported`.
   - No → keep `partial` and name the remaining blockers, or narrow the
     registered prefix to the scope that is actually complete.

| Question | `not_implemented` | `stub` | `partial` |
|---|---|---|---|
| Registered production owner/module? | No | Yes | Yes |
| Class/interface hierarchy or constants may resolve? | Not claimed | Yes | Usually |
| Android-visible state transition or computed result? | No | No | Yes, for a subset |
| Counts as API behaviour/support? | No | No | Only for the documented subset |
| Normal result when real semantics are requested | Structured `NOT_IMPLEMENTED(...)`, unresolved host feature, or another explicit failure | Explicit failure, or a documented no-op/default only where no semantics are being claimed | Real behaviour for implemented calls; explicit failure for the rest |

### Use `not_implemented` when there is no host model

A surface is `not_implemented` when RustyDroid has **no registered host
model or dispatch for that prefix**. The following do **not** count as an
implementation and do not, by themselves, move a surface out of
`not_implemented`:

- the API name appears in an APK/DEX, a test fixture, documentation, an
  issue, a TODO or an AOSP reference copy;
- a descriptor or package string appears in unrelated code;
- AndroidX or app code contains its own class with a similar role;
- a generic VM path can allocate an otherwise empty object shell but no code
  owns the Android semantics of the surface;
- a plan exists to implement it later.

Runtime behaviour must not fake success. When guest code needs the missing
host behaviour, it should receive the structured `NOT_IMPLEMENTED(module)`
contract (or an equally explicit unresolved-feature failure), not a
plausible null, zero, empty collection, successful return code, invented
sensor/media/location data or silent no-op.

Registry requirements for `not_implemented`:

- `module = "none"`;
- `code = []`;
- a non-empty `gap_id`;
- `notes` names the missing model rather than saying only “TODO”;
- `key_apis` may identify representative missing APIs, but they are an
  inventory of the gap, not evidence of support.

If intentional production code is added for the prefix, the entry can no
longer remain `not_implemented`: reclassify it as `stub` or `partial` in the
same change and record the real module/code paths.

### Use `stub` when only the shape or a fallback exists

A surface is `stub` when RustyDroid intentionally recognizes it well enough
for loading, linking or harmless placeholder interaction, but does not yet
model its Android-visible semantics. Typical stub-only material includes:

- built-in superclass/interface hierarchy entries so `instanceof`, casts or
  method resolution can proceed;
- public constants, enum shells or field defaults;
- an allocatable manager/view/value shell with no service, rendering,
  transport or state machine behind it;
- methods that always return the same null/zero/false/empty result or do
  nothing regardless of their inputs;
- a compatibility fallback that lets startup continue while the requested
  operation is explicitly unsupported.

A stub is **not support**. It must not be described as “works”, “complete”,
“compatible” or as execution coverage. In particular, a stub must not invent
data whose presence would make an app believe a device, renderer, network,
media pipeline, provider or system service actually operated.

A documented no-op can be real behaviour only when the Android contract for
that exact operation is itself a no-op in the relevant environment. If the
operation normally has an observable effect and RustyDroid merely ignores
it, the surface remains a stub (or `partial` if other operations are real),
and the deviation stays in the gap notes.

Registry requirements for `stub`:

- a concrete implementing `module` (never `"none"`);
- at least one real `code` path containing the hierarchy/constants/fallback;
- a non-empty `gap_id`;
- `notes` separates what shape exists from what semantics are absent;
- evidence proves only the placeholder/loadability claim; it must not be
  presented as behavioural coverage.

### Boundary examples

- **Only a `classpath.rs` hierarchy entry and constants exist:** `stub`.
- **No owner code exists; only an app DEX references the package:**
  `not_implemented`.
- **A manager object can be obtained but every operation is a fixed default
  and registrations/callbacks are discarded:** `stub`.
- **A manager records registrations and dispatches a tested subset of
  callbacks, while other operations fail explicitly:** `partial`.
- **A value class stores constructor inputs and correctly implements a
  tested subset of accessors, equality, parsing or serialization:**
  `partial`; `supported` only if the registered scope is complete.
- **One unsupported overload in an otherwise real implementation returns
  typed `NOT_IMPLEMENTED(...)`:** the surface remains `partial`; record the
  overload as a blocker instead of downgrading the whole prefix.
- **One class in a package is real, but the rest of the package has no
  model:** register or retain the narrow class prefix for the real class and
  keep the broader package at its own honest status.

### Promotion and demotion rules

- `not_implemented` → `stub`: only when intentional hierarchy, constants
  or fallback code lands with an owner module and gap record. A plan, issue
  or unmerged experiment is not enough.
- `stub` → `partial`: only when the first real Android-visible behaviour
  lands and is exercised by tests. More constants or more elaborate
  fallbacks do not cross this boundary.
- `partial` → `supported`: only when every item in the completion gate
  below is true for the exact registered prefix. A complete hot path is
  still `partial` under the literal program.
- Demote a surface when an audit shows that its claimed behaviour is only
  hierarchy/default fallback, that the named code no longer owns the prefix,
  or that tests do not exercise the claimed semantics. Honest demotion is
  progress; preserving an inflated count is not.

As of 2026-10-23 the registry contains 106 surfaces: 4 `supported`, 29
`partial`, 14 `stub` and 59 `not_implemented`. The generated inventory in
`docs/api-coverage.md` and the records in `data/api-coverage.toml` are the
current lists; this section defines how those records must be classified
when they change.

## Completion gate for every package

A `partial` surface may be promoted only when all items below are true for
that exact prefix:

1. **API inventory:** every public class/interface/enum, constructor, method
   and field in the API 26–36 SDK surface is inventoried. Hidden/internal APIs
   are listed separately and either modelled or return a typed
   `NOT_IMPLEMENTED(module_id)`; they do not count as public completeness.
2. **Behaviour, not presence:** methods perform the Android-visible state
   transition, return value, callback order, exception type/message shape and
   permission/security checks. Generated hierarchy stubs and default-value
   fallbacks are blockers, not implementations.
3. **Process and persistence semantics:** lifecycle, threading/looper rules,
   binder/process boundaries where Android exposes them, storage durability,
   restart/relaunch behaviour and multi-app isolation are tested.
4. **Fixtures:** unit tests plus DEX/app-level fixtures exercise success,
   error, boundary and lifecycle paths. Real-APK evidence is required for hot
   paths already used by golden apps.
5. **No silent fallback:** unsupported overloads cannot fall through to a
   plausible default. They either work or produce the structured error
   contract; the registry gap stays until they work.
6. **Generated truth:** `rd api-coverage check`, `cargo xtask
   api-coverage-report`, `cargo xtask api-coverage-spec` and
   `cargo xtask compat-report` agree with the code and tests in the same PR.

## Current checkpoint status

As of 2026-10-23, Rounds 1–10 below are merged, not merely local branch
work. [PR #172](https://github.com/nadabgfccvd/RustyDroid/pull/172) merged
`appendix-c-literal-round10-20261023` into
`appendix-c-api-coverage-20261023` at merge commit `e16632b`. The merged
stack contains the ten checkpoint commits plus two RD Sentinel remediation
commits: `9ffc7b4` (production `expect(...)` removal) and `82cc58c`
(keeping Android dispatch off the String hot path). The final pre-merge
Sentinel run
[`39930840914`](https://github.com/nadabgfccvd/RustyDroid/actions/runs/39930840914)
passed complete with no findings.

That publication status does not change the completion bar: every surface
named below remains at the status recorded in `data/api-coverage.toml`
until its whole-package blockers are closed.

## Round 1 — storage durability foundation (merged)

- `android.database.sqlite`: named databases now serialize to the
  app-private VFS database path after committed mutations and deserialize on
  a fresh Engine launch. `:memory:`, `createInMemory` and the forced legacy
  subset fallback remain intentionally memory-only.
- Proof: `sqlite_named_database_persists_across_engine_launches` writes in
  one Engine, drops it, reopens the same named helper in a new Engine and
  reads/writes again across a third launch.
- Status after Round 1: **still `partial`**. Remaining literal blockers are
  exact SQLite result codes/extensions, WAL/checkpoint and locale/collation
  fidelity, custom scalar/aggregate/window functions, locking/concurrency and
  closed-database/reference-count nuances across every overload.

## Round 2 — Parcel/Parcelable and local Binder foundation

- `android.os.Parcel` is now a real byte buffer: obtain/recycle,
  data-size/position accounting, marshall/unmarshall/appendFrom, primitive
  and UTF-16 string forms, primitive/string/Binder arrays, interface
  tokens and exception words, `writeValue`/`readValue`, Bundles, lists/maps
  and typed Parcelable lists/arrays.
- `android.os.Parcelable`: DEX classes are materialized through their
  static `CREATOR` after byte marshalling; `Bundle.writeToParcel` and
  `Bundle.readFromParcel` use the same local Parcel stream.
- `android.os.Binder`: `attachInterface`, `queryLocalInterface`,
  `getInterfaceDescriptor`, interface transactions and Engine-local
  `transact` dispatch into a DEX stub's `onTransact` are implemented.
- Proof: `crates/rd-vm/tests/parcel_binder.rs` materializes a DEX
  Parcelable from marshalled bytes through its own Creator, round-trips a
  typed list through a supplied Creator, and runs an AIDL-shaped
  enforce-interface/read-int/write-reply transaction through a DEX Binder
  stub.
- Status after Round 2: `android.os` remains **partial**, and the former
  stub sub-surfaces `android.os.Parcel`, `android.os.Parcelable` and
  `android.os.Binder` are now honestly **partial**, not complete. Kernel
  Binder handles/processes/servicemanager, death recipients, file
  descriptors, Serializable/SparseArray forms, exact AOSP Bundle wire
  magic and every framework Parcelable layout remain blockers.

## Round 3 — Named, persistent SharedPreferences

- `Context.getSharedPreferences(name, mode)` no longer collapses every
  name into one global object/map. Each name has a canonical
  `SharedPreferencesImpl` object and an isolated typed store in this Engine.
- Stores load lazily from and persist to
  `/data/user/0/<package>/shared_prefs/<name>.xml` in the app-private VFS.
  The XML uses Android's `<map>`, `<string>`, `<int>`, `<long>`, `<float>`,
  `<boolean>` and `<set>` forms.
- `PreferenceManager.getDefaultSharedPreferences` resolves the
  `<package>_preferences` store; `Activity.getPreferences(mode)` resolves
  the activity-local store name.
- Editor string/string-set/int/long/float/boolean puts, remove and clear,
  listeners, `contains`, and boxed `getAll` values preserve their guest
  types. Typed getters now raise `ClassCastException` for a wrong stored
  type, as AOSP does.
- Proof: `shared_preferences_named_stores_persist_across_engine_launches`
  writes two isolated named stores in one Engine, reads all supported value
  types in a second Engine, removes/clears keys, and observes those changes
  in a third Engine.
- Status after Round 3: `android.content` and `android.preference` remain
  **partial**. Preference screens/default-value inflation, ContentProvider
  IPC, multi-process external-change reload, backup/restore semantics and
  complete Context services remain blockers.

## Round 4 — AOSP Bundle Parcel framing

- Every Bundle Parcel path now uses the same
  `length | BNDL magic | ArrayMap` frame as AOSP
  `BaseBundle.writeToParcelInner`: `Bundle.writeToParcel`,
  `Parcel.writeBundle`, `Parcel.writeValue`, `Parcel.writeParcelable`
  and their readers no longer diverge into an internal-only stream.
- `Parcel.writeBundle`/`writePersistableBundle` write the AOSP nullable
  present marker (`1` present, `0` null); their readers validate that
  marker. Bundle readers validate the magic and bound entry decoding by
  the declared frame length.
- Proof: `bundle_parcel_uses_aosp_length_magic_and_nullable_marker`
  checks the exact empty-Bundle bytes, rejects a corrupted magic, proves
  the null marker, and round-trips through the guest-visible
  `Bundle.writeToParcel`/`readFromParcel` methods.
- Status after Round 4: `android.os`, `android.os.Parcel` and
  `android.os.Parcelable` remain **partial**. Cross-process Binder,
  file descriptors, Serializable/SparseArray forms and byte layouts for
  every framework-owned Parcelable remain blockers.

## Round 5 — SparseArray Parcel values

- `Parcel.writeValue`/`readValue` now support the AOSP
  `VAL_SPARSEARRAY` and `VAL_SPARSEBOOLEANARRAY` tags, preserving sorted
  integer keys and typed values through marshalled bytes.
- The direct `writeSparseArray`/`readSparseArray` and
  `writeSparseBooleanArray`/`readSparseBooleanArray` Parcel methods use
  the same size/key/value streams, including the `-1` null marker.
- Host `SparseArray` state is read from the existing sorted Engine map;
  DEX subclasses fall back to their guest `size()`, `keyAt()` and
  `valueAt()` methods.
- Proof: `sparse_arrays_roundtrip_through_value_and_direct_methods`
  round-trips null, String and boxed values at negative/positive keys,
  SparseBooleanArray entries, and a null SparseArray marker.
- Status after Round 5: `android.os`, `android.os.Parcel`,
  `android.os.Parcelable` and `android.util` remain **partial**.
  Serializable values, file descriptors, cross-process Binder and every
  framework Parcelable layout remain blockers.

## Round 6 — `android.util.Base64` value codec

- `android.util.Base64.encode`, `encodeToString` and `decode` now cover
  the String and byte-array overloads, including offset/length forms.
- The codec implements AOSP flags and output details: standard and
  URL-safe alphabets, optional padding, 76-column wrapping with a final
  line break, CRLF line endings and `NO_WRAP`.
- Decoding follows Android's state machine: non-alphabet bytes are
  skipped, two- and three-sextet tails may omit padding, and malformed
  padding raises `IllegalArgumentException("bad base-64")`.
- The public flag constants are available through host static-field
  resolution as well as the constants normally inlined by compilers.
- Proof: `android_base64_flags_and_wrapping_match_aosp`,
  `android_base64_decode_skips_garbage_and_validates_padding` and
  `android_base64_static_dispatch_uses_offsets_and_constants`.
- Status after Round 6: `android.util` remains **partial**. The
  Base64 stream classes and the rest of the `android.util` catalogue
  remain blockers.

## Round 7 — `android.util.LruCache` with DEX callbacks

- `LruCache` now implements constructor validation, access-order
  `get`/`put`/`remove`, `trimToSize`, `resize`, `evictAll`, size and
  hit/miss/create/put/eviction counters, ordered `snapshot()` maps and
  the AOSP `toString()` summary shape.
- Cache operations remain final host operations, but subclass hooks are
  guest code: `create`, `sizeOf` and `entryRemoved` resolve and execute
  DEX overrides, including the AOSP collision path when `create`
  populates the requested key itself.
- Proof: `lru_cache_access_order_eviction_and_stats_match_aosp` covers
  the platform class directly, and
  `dex_lru_cache_callbacks_drive_creation_sizing_and_eviction` uses a
  DEX subclass whose entries cost 2 units, creates values on misses and
  observes the exact evicted key through `entryRemoved`.
- Status after Round 7: `android.util` remains **partial**. The rest of
  its public catalogue, including IO/JSON/XML helpers and event/log
  types, remains incomplete.

## Round 8 — `android.util.Base64` stream classes

- `Base64OutputStream` now wraps a guest or host `OutputStream`, accepts
  single-byte and byte-array slice writes, emits only complete encoded
  triplets while the stream is open, carries a one- or two-byte remainder
  across writes, preserves the selected alphabet/padding/wrapping flags,
  flushes the wrapped stream without finishing the code stream, and on
  close emits the final padded group and line break before honouring
  `NO_CLOSE`.
- `Base64InputStream` now wraps a guest or host `InputStream`, decodes its
  encoded bytes with the Round 6 AOSP decoder state machine, and serves
  `read()`, `read(byte[])`, sliced `read(byte[], int, int)`, `available()`
  after decoding has begun, EOF and close/`NO_CLOSE` behaviour.
- Both stream classes are registered in the built-in class hierarchy as
  filter streams and dispatch when referenced polymorphically through
  `java.io.InputStream`/`java.io.OutputStream`, without shadowing a DEX
  subclass override.
- Proof: `base64_output_stream_incrementally_encodes_and_finishes_on_close`
  writes `hello` one byte at a time, observes only `aGVs` after `flush()`,
  and observes `aGVsbG8=\n` after `close()`;
  `base64_input_stream_decodes_wrapped_bytes_with_offsets` decodes wrapped
  input, writes into a non-zero destination offset, and checks
  `available()`/EOF.
- Status after Round 8: `android.util` remains **partial**. The rest of its
  public catalogue, including IO/JSON/XML helpers and event/log types,
  remains incomplete.

## Round 9 — `android.util.AtomicFile`

- `AtomicFile(File)` retains the exact base `File`, derives the AOSP
  `<base>.bak` backup file, and implements `getBaseFile`, `startWrite`,
  `finishWrite`, `failWrite`, `openRead`, `readFully` and `delete` over
  the VM filesystem.
- `startWrite` follows the platform backup protocol: an existing base is
  renamed to `.bak`; if a backup already exists, that older committed
  backup is retained and the interrupted base replacement is discarded.
  A failed first stream open creates missing parent directories and
  retries, matching AOSP.
- `finishWrite` closes the replacement stream and deletes the backup;
  `failWrite` closes it, deletes the replacement and restores the backup;
  `openRead` first restores an orphaned backup left by an interrupted
  write. Missing base files surface `FileNotFoundException` from the real
  `FileInputStream` constructor path.
- Proof:
  `atomic_file_finish_fail_and_interrupted_writes_follow_aosp_backup_protocol`
  covers initial parent creation, commit, explicit rollback, a second
  `startWrite` while a backup exists, rollback on read after an
  interrupted write, recommit, delete and the missing-file error.
- Status after Round 9: `android.util` remains **partial**. The rest of its
  public catalogue, including JSON/XML helpers and event/log types,
  remains incomplete.

## Round 10 — `android.util` JSON streaming

- `JsonReader` now implements the AOSP streaming scope machine for
  objects, arrays, names, strings, numbers, booleans and null, including
  `begin*`/`end*`, `hasNext`, `peek`, typed scalar readers, `skipValue`,
  `getPath`, strict number/string validation, BOM handling and typed
  `MalformedJsonException`/`IllegalStateException` failures. Lenient mode
  additionally accepts comments, single-quoted and unquoted literals,
  alternative separators and `NaN`/infinities.
- `JsonToken` is a real host enum surface: its ten constants are cached
  singletons with Java `name`/`ordinal`, and `values()`/`valueOf(String)`
  return those same constants.
- `JsonWriter` maintains the document/object/array scope stack while it
  emits to a guest or host `Writer`. It covers deferred names, compact
  and indented output, string escaping, HTML-safe replacements,
  `serializeNulls`, fluent primitive values, boxed `Boolean`/`Number`,
  finite-number checks, flush and incomplete-document failure on close.
- Proof: `json_reader_strict_streaming_tokens_paths_and_skip_value`,
  `json_reader_lenient_mode_and_typed_errors`,
  `json_writer_pretty_prints_html_safe_output_and_skips_null_names` and
  `json_writer_boxed_values_and_json_token_enum_statics` exercise the
  classes through guest-visible `StringReader`/`StringWriter` objects.
- Status after Round 10: `android.util` remains **partial**. The rest of
  its public catalogue, including XML helpers and event/log types,
  remains incomplete.

## Remaining literal blockers by partial surface

| Domain | Surface | Main literal blockers |
|---|---|---|
| App/lifecycle | `android.app` | Complete Activity/Application/Service/task/fragment/dialog/notification/job/backup/admin semantics, process death/restart, saved-state and all manager services. |
| App/lifecycle | `android.app.job` | Persistent job store, constraints, deadlines, backoff, reschedule/stop callbacks and system-triggered execution across process death. |
| App/lifecycle | `android.content` | Full Context service surface, ContentProvider transport, grant-uri permissions, broadcast queues/priorities, service binding/process semantics. |
| App/lifecycle | `android.content.pm` | Installer sessions, verification, overlays, archives, splits, cross-profile queries and every PackageManager query flag/result shape. |
| App/lifecycle | `android.content.res` | Complete qualifier resolution, themes/styles, fonts, resource providers/loaders, configuration changes and asset path variants. |
| View/UI | `android.view` | Surface/transaction model, windows/types, full input/gesture/focus/accessibility traversal, drag/drop, autofill/content capture and display variants. |
| View/UI | `android.view.accessibility` | Full manager/service protocol, windows, magnification, gestures, actions, text/selection and external service policy. |
| View/UI | `android.view.inputmethod` | Full IME protocol, subtypes, handwriting, autofill/voice integration, restart/input view lifecycle and multi-window input. |
| View/UI | `android.widget` | The complete widget catalogue and adapter/list/recycler behaviours, remote views actions and theme/attribute fidelity. |
| Graphics | `android.graphics` | Pixel-complete Bitmap/Canvas, shaders/filters/blend/path effects, color management, fonts/text rendering and hardware-buffer paths. |
| Graphics | `android.graphics.drawable` | Vector/animated/adaptive/ripple/layer/state drawable completeness and resource/theme mutation semantics. |
| Text | `android.text` | Unicode shaping, bidi, line breaking/hyphenation, fonts/fallback, spans/editable operations and layout pixel fidelity. |
| Text | `android.text.method` | Complete movement/key/transformation method catalogue and editor interaction edge cases. |
| Text | `android.text.style` | Exact span precedence/rendering for character, paragraph, suggestion, locale and metric-affecting spans. |
| OS/IPC | `android.os` | Remaining after Rounds 2/4/5: cross-process/kernel Binder and servicemanager, death recipients/oneway scheduling, file-descriptor Parcels, Serializable forms, exact wire layouts for every framework-owned Parcelable, process/power/vibration/strict-mode/users and all manager semantics. |
| OS/IPC | `android.util` | Remaining public helper/value catalogue beyond the implemented SparseArray/Base64/LruCache/AtomicFile/JSON subsets (XML helpers, event log, printers, state sets, time/date helpers and deprecated classes). |
| Database | `android.database` | ContentProvider query/insert/update/delete/call transport, bulk operations, cursor windows/extras/notifications across apps. |
| Database | `android.database.sqlite` | See Round 1 remaining blockers above. |
| Net | `android.net` | ConnectivityService/network agents, DNS, link properties/capabilities callbacks, VPN/tethering and traffic/accounting semantics. |
| Hardware | `android.hardware.display` | VirtualDisplay creation/callbacks, multiple displays, modes/HDR/color, display events and presentation windows. |
| Services ext | `android.service.notification` | System ranking/zen/rules, listener grants/revocation, snooze contexts, bubbles and cross-app policy. |
| System UI | `android.appwidget` | Launcher host process model, provider metadata/configure flows, collections/RemoteViews actions and backup/restore. |
| System UI | `android.inputmethodservice` | Full IME service lifecycle, subtypes/switching, handwriting/voice, extraction/candidates and process isolation. |
| System UI | `android.accessibilityservice` | Global actions, gestures, magnification, fingerprint/soft-keyboard controllers, windows and service capabilities/policy. |
| Misc | `android.preference` | Legacy PreferenceActivity/Fragment XML screens, dependency/summary/order state and dialog preferences. |
| Misc | `android.icu` | Complete ICU API and locale data fidelity (collation, calendars, number/date/message formatting, transliteration and break rules). |

## Sequence

1. **Storage/IPC foundations (merged checkpoints; package blockers remain):**
   SQLite durability (Round 1), Parcel/Parcelable bytes and local Binder
   transactions (Round 2), and SharedPreferences file durability (Round 3).
   These unblock `android.os`, `android.content` and provider work.
2. **Value surfaces:** continue `android.util` beyond the merged
   SparseArray/Base64/LruCache/AtomicFile/JSON subsets, then `android.text`
   primitives and ICU data boundaries, because UI and providers depend on
   exact value semantics.
3. **App/content:** PackageManager/resources first, then provider transport
   and the remaining `android.app` managers.
4. **UI/rendering:** view/widget input and focus, drawable/graphics pixel
   fidelity, text shaping/layout.
5. **System services:** notifications, widgets, IME and accessibility policy;
   display/virtual displays; network service callbacks.
6. **Legacy/narrow surfaces:** legacy preferences and remaining catalogue
   classes.

Every round must either (a) close a named blocker with tests and update the
registry notes, or (b) explicitly record why the blocker remains. A round
that only renames statuses is not progress.
