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

## Round 1 — storage durability foundation (done in this branch)

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
| OS/IPC | `android.os` | Remaining after Round 2: cross-process/kernel Binder and servicemanager, death recipients/oneway scheduling, file-descriptor Parcels, Serializable/SparseArray forms, exact framework Parcelable/Bundle wire layouts, process/power/vibration/strict-mode/users and all manager semantics. |
| OS/IPC | `android.util` | Remaining public helper/value catalogue (IO, JSON/XML, event log, printers, state sets, time/date helpers and deprecated classes). |
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

1. **Storage/IPC foundations:** SQLite durability (Round 1),
   Parcel/Parcelable bytes and local Binder transactions (Round 2), and
   SharedPreferences file durability (Round 3). These unblock `android.os`,
   `android.content` and provider work.
2. **Value surfaces:** `android.util`, `android.text` primitives and ICU data
   boundaries, because UI and providers depend on exact value semantics.
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
