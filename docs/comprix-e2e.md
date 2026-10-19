# Comprix v1.6.0 end-to-end via MCP: status, latency, and blockers

This document covers running Comprix v1.6.0 (`br.com.comprix`, x86_64)
end to end, driven entirely through `rd mcp`.

- **App:** Jetpack Compose, Room, androidx.startup, ML Kit OCR and barcode.
- **Target:** minSdk 26, target 35, a single DEX.
- **Harness:** `mcp_run.py`, which sends JSON-RPC over stdio and times each
  call from request write to parsed response with a monotonic clock.

## Status

**Round 2 (current):** `launch` no longer hangs. It runs the whole startup
plus the first Compose composition and returns in **0.28–0.45 s** with a
typed error. Before round 2 it exceeded the 60 s per-call deadline. The
activity still dies before the first frame, so no app screenshot is
available yet.

**How far the launch gets now:**
1. `Application.onCreate`, the androidx.startup initializers, and
   `MainActivity.onCreate` run, then `setContentView(ComposeView)`.
2. The window is attached and the Recomposer and coroutine dispatchers run.
3. AndroidComposeView is created, with its accessibility, autofill,
   content-capture, ViewConfiguration, text-input and focus owners.
4. The Room database is built: `ComprixDatabase_Impl` is found by
   reflection, the InvalidationTracker is set up with locks, and the
   `SQLiteOpenHelper` is configured.
5. **The first composition of the app UI runs:** `Scaffold`/`Surface`,
   `java.time` date formatting, and Material vector icons parsed from binary
   XML (`painterResource`) and tinted with a `BlendModeColorFilter`.
6. The side effects run: `WindowInsetsController` system-bar appearance and
   the permission-result callback.
7. **It stops here:** a `NullPointerException` is raised by the interpreter
   on a null dereference inside the Room/SQL coroutine path (the first DAO
   query on `Dispatchers.IO`). It surfaces as kotlinx-coroutines'
   "Fatal exception in coroutines machinery" (`S3.a`). The NPE is raised by
   the host, so it carries no guest frames. Next step: give host-raised
   exceptions the guest stack (see "What to improve").

**Round-2 launch progress chain.** Each item is a failure found and fixed in
general form:

> CAS identity → `addAll` → `getHandler` → `getResources` → Canvas →
> AccessibilityDelegate → AccessibilityManager → SparseLongArray → Autofill
> → VibratorManager → `TreeSet(Comparator)` → ViewConfiguration →
> `CursorAnchorInfo$Builder` → DEX TreeSet subclass `contains` →
> AutofillCallback → JDK interface closure (Set→Collection CCE) →
> content capture → `Object[]` ArrayStore → `ClassLoader.loadClass` →
> `Method.getAnnotations` → `ActivityManager.isLowRamDevice` →
> `Class.getPackage` → `ReentrantLock` → host `ListIterator` via the
> Iterator mref → `DateTimeFormatter` → `Resources.getXml` →
> `Xml.asAttributeSet` → `BlendModeColorFilter` →
> `Window.getInsetsController` → host **panic** in permission delivery →
> `Activity.onRequestPermissionsResult` →
> `SQLiteOpenHelper.setWriteAheadLoggingEnabled` → `Class.getAnnotation` →
> *(open)* NPE in the Room query path.

**Round 1 (history).**
- Blocker 1: the `Dex::find_class` linear scan, now an O(1) index.
- Blocker 2: the `runBlocking` spin, resolved in round 2 by the CAS
  identity and collection fixes.

## Per-action MCP latency

| Run | Tool | Delay | Result |
|---|---|---:|---|
| s01 | install | 14.7 ms | OK |
| s01 | app_info | 0.2 ms | OK |
| s01 | compat_gaps | 0.2 ms | OK |
| s01 | launch | 135.9 ms | ERR: NPE (`Boolean.FALSE` was null) |
| s01 | runtime_health | 0.2 ms | OK |
| s01 | logs | 0.1 ms | OK |
| s01 | ui_tree | 0.1 ms | ERR: `NOT_RUNNING` |
| s01 | screenshot | 0.1 ms | ERR: `NOT_RUNNING` |
| s01b | initialize | 22.6 ms | OK |
| s01b | install | 15.1 ms | OK |
| s01b | app_info | 0.1 ms | OK |
| s01b | compat_gaps | 0.3 ms | OK |
| s01b | launch | >60 000 ms | TIMEOUT (blocker 1: `find_class` cliff) |
| s01c | install | 14.1 ms | OK |
| s01c | app_info | 0.1 ms | OK |
| s01c | compat_gaps | 0.3 ms | OK |
| s01c | launch | >60 000 ms | TIMEOUT (blocker 2: `runBlocking` spin under the MCP fuel budget) |
| s01d (round 2) | initialize | 2.0 ms | OK |
| s01d | install | 22.5 ms | OK |
| s01d | app_info | 7.3 ms | OK |
| s01d | compat_gaps | 0.3 ms | OK |
| s01d | launch | **446.8 ms** | ERR `JAVA_EXCEPTION` (`S3.a` wrapping the Room-path NPE); was >60 s |
| s01d | runtime_health / logs | 0.1 / 0.04 ms | OK |
| s01d | ui_tree / screenshot | 0.04 / 0.03 ms | ERR `NOT_RUNNING` (activity died) |
| s02 (features) | install | 32.0 ms | OK |
| s02 | package_list / device_profile | 0.2 / 0.1 ms | OK |
| s02 | permissions_list / permissions_audit | 0.1 / 0.1 ms | OK |
| s02 | permissions_grant CAMERA | 0.07 ms | OK, granted |
| s02 | behavior_switches | 0.26 ms | OK |
| s02 | get_api_usage | 66.0 ms | OK (static DEX scan) |
| s02 | native_state (before launch) | 0.1 ms | ERR `NOT_RUNNING` |
| s02 | launch | **278.1 ms** | ERR `JAVA_EXCEPTION` (same) |
| s02 | get_coverage, shortcuts, notifications, widgets ×3, widget_screenshot, accessibility, ime, screenshot | 0.07–0.2 ms each | ERR `NOT_RUNNING` |
| s02 | runtime_health / logs | 0.09 / 0.08 ms | OK |

The raw rows are in `comprix/mcp-delays.jsonl`, outside this repository.

### What to improve, from these measurements

1. **`launch` has no wall-clock or fuel bound that covers host loops.**
   - A single MCP call can block the server forever.
   - Add a wall-clock watchdog to `launch`, `tap` and the other tools that
     run app code (for example `RD_MCP_MAX_CALL_MS`). On expiry, return a
     structured `VM_TIMEOUT` with the guest stack.
2. **MCP errors carry no guest stack.** The CLI `--trace` stack was what
   located every root cause in this run. Add a `trace` argument to `launch`.
3. **`runtime_health` should report threads, the pending Looper queue, and
   frames pumped,** so a stalled launch can be diagnosed from MCP alone.
4. **Unmodelled host static fields silently read as null or 0,** which hides
   root causes. `boxes` and `android_build` mitigate this.
5. **Traces carry no dex pc.**
6. **The session tools are fast:** install takes about 15 ms and the
   metadata tools under 1 ms. The cost is concentrated in `launch`.
7. **Post-mortem inspection.** After a launch crash, every interactive tool
   (`screenshot`, `ui_tree`, `widgets`, `accessibility`, `get_coverage`,
   `native_state`) returns `NOT_RUNNING`. Keeping the dead activity's last
   frame, view tree and coverage readable would make crash triage possible
   from MCP alone.
8. **Host-raised exceptions need guest frames.**
   - Guest-constructed Throwables now record the guest stack under
     `--trace`; the uncaught-exception log prints it.
   - An NPE raised by the interpreter on a null dereference, once caught
     and rethrown by guest code, still has no frames. That is exactly the
     current blocker.
9. **Throwable `toString` without a message prints `Class@id`.** This
   happens for some host-materialized exceptions; AOSP prints the class
   name.
10. **Error messages should name both declarer and receiver.** The
    `ListIterator.hasNext` failure was really an `Iterator.hasNext` mref on
    a host `ListIterator`. `NOT_FOUND` should print both the resolved
    declarer and the receiver class.
11. **`launch` latency is now dominated by guest work, not the VM:** about
    0.3–0.45 s for roughly 2,200 distinct DEX methods. `install` is 22–32 ms
    with the larger round-2 classpath, and `get_api_usage` is 66 ms (a full
    DEX scan, worth caching per install).

## General RustyDroid additions made for this app

None of these additions is specific to Comprix. Each implements the AOSP or
JDK behaviour of a platform API, keyed on the declaring class, so DEX
subclasses are covered too.

| Module | Surface |
|---|---|
| `boxes` | Wrapper statics: `Boolean.TRUE`/`FALSE`, `<Wrapper>.TYPE`, canonical boxing |
| `jlsystem` | `System` statics, virtual clock, `Class.forName` |
| `threads` | Green threads; `UncaughtExceptionHandler` (per thread → default → platform `KillApplicationHandler`, which logs `FATAL EXCEPTION`) |
| `android_text` | `TextUtils`; `Log` routed to app logs |
| `perms` | Runtime permissions, synced with MCP grant/revoke |
| `window`, `view_props`, `view_attach` | Decor window, LayoutParams, view property store, attach lifecycle, `View.getContext` |
| `atomics`, `vto`, `choreographer` | Atomics, ViewTreeObserver, Choreographer plus a frame pump |
| `android_build`, `activity_callbacks`, `activity_extras` | `Build` statics (SDK 34), lifecycle callbacks, OnBackInvokedDispatcher |
| `bundle` | Bundle family; Intent extras |
| `jlstring` | UTF-16 String surface, regex, charsets, `String.format` subset |
| `jlclass`, `reflect` | Class introspection; `getMethod`/`getDeclaredMethod`/`getConstructor`; `Method.invoke`; `Constructor.newInstance` |
| `handler_msg` | Full Handler/Message messaging, in AOSP dispatch order |
| `numerics` | Integer/Long/Float/Double/Boolean/Character/Math statics |
| `arrays` | `java.util.Arrays`, with a stable merge sort and guest `equals`/`hashCode`/`compareTo` |
| `collections` | `Collections.newSetFromMap`/`max`/`min`/`reverse`/`singletonMap`/`emptyIterator`/`addAll`/`list`; `IdentityHashMap` |
| `content` | `Settings.Global`/`System`/`Secure` table, ContentResolver observers, `ContentObserver`, `Uri` accessors |
| `throwable` | `initCause` rules, `addSuppressed`, stack traces, `printStackTrace`, `(String, Throwable)` constructors |
| classpath | About 30 JDK exception families (`IndexOutOfBounds` subclasses now catchable correctly) |
| intrinsics | `AtomicLong`/`IntegerFieldUpdater` `getAndIncrement`/`getAndDecrement` |
| rd-dex | O(1) `Dex::find_class` (lazy descriptor index): a general launch-time fix |
| threads | Timed main-thread park advances virtual time to the earliest timed sleeper |
| `jcollections` | Collection bulk ops plus AbstractList/AbstractCollection templates for DEX subclasses; host List/ListIterator (also through the `Iterator` mref) |
| `android_res` | Resources, DisplayMetrics, Configuration, Theme, TypedArray, TypedValue, Locale/LocaleList; arsc `INT_COLOR` fix |
| `graphics` | Canvas/Paint/Path/Rect/Matrix display lists, a glyph atlas, enums, and ColorFilter values (BlendMode, PorterDuff, Lighting, ColorMatrix) |
| `platform_base` | Platform base classes; Accessibility, Autofill, Vibrator(Manager), value builders; ActivityManager (memory info, low-RAM); WindowInsetsController |
| `android_util`, `jsorted` | SparseArray family, ArrayMap/ArraySet; TreeMap/TreeSet with a Comparator, including DEX subclasses |
| `view_layout` | Measure/layout model, MeasureSpec, ViewConfiguration, focus/touch stubs, content capture; `Engine::draw_frame` (used by MCP `screenshot`) |
| `jlocks` (new) | ReentrantLock, ReentrantReadWriteLock (+Read/WriteLock), Lock/Condition, AbstractOwnableSynchronizer |
| `jtime` (new) | `java.time`: LocalDate, LocalDateTime, Instant, ZoneId, ZonedDateTime, YearMonth, Duration, ChronoUnit; DateTimeFormatter pattern engine with pt/en names |
| `xml_res` (new) | `Resources.getXml`/`getLayout`/`getAnimation` give an XmlPullParser+AttributeSet over binary XML; `obtainAttributes`/`obtainStyledAttributes(set, …)` fill real TypedArrays; `Xml.asAttributeSet` |
| `jlsystem` | ClassLoader `loadClass`/`getParent`; `Class.getPackage`, Package; `Class.getAnnotation*` |
| intrinsics (reflection) | `AccessibleObject` on Method/Constructor: annotation queries and `setAccessible` |
| classpath | JLS rule "every reference type is a subtype of Object"; JDK collection-interface closure for DEX implementers |
| m8 (SQLite) | Helper/database journal and config knobs (`setWriteAheadLoggingEnabled`, `enableWriteAheadLogging`, `getPath`, …) |
| perms | **Host panic fixed:** permission-result delivery wrote into an unsized array |
| activity_extras | Base no-ops: `onRequestPermissionsResult`, `onLowMemory`, `onWindowFocusChanged`, … |
| throwable, threads | Under `--trace`, guest-constructed Throwables record the guest stack; FATAL EXCEPTION logs print it |

## Documented deviations

- **Regex:** backreferences and lookaround raise `PatternSyntaxException`
  (the engine is the Rust `regex` crate).
- **`String.format`:** a subset of conversions is supported.
- **Reflection:**
  - lookup resolves signatures from the dex `method_ids`, so it covers
    app-defined methods and referenced platform APIs;
  - `getMethods()`/`getDeclaredMethods()` enumeration is not provided;
  - access checks are not enforced.
- **Throwables:** no guest stack frames are captured.
  `getStackTrace()` returns the last `setStackTrace` value, else an empty
  array.
- **`Collections.newSetFromMap`:** returns an independent set rather than a
  view backed by the map. This is equivalent for the usual idiom, which
  passes an empty, freshly created map.
- **`IdentityHashMap`:** compares DEX objects by identity but strings and
  boxes by value.
- **Character classification:** approximated with Rust Unicode tables.
- **Collections:** `subList` returns a copy; host ListIterators iterate a
  snapshot; TreeMap/TreeSet views are snapshots.
- **Resources:** themes are not resolved, so `?attr` references in XML are
  reported as absent; default styles are ignored. TypedArray `getValue`
  reports ints as colors when the attribute name contains "color" or "tint".
  `getBaseContext` returns the receiver.
- **Device:** moto-e5, 720×1440 at 280 dpi, locale en-US (`RD_LOCALE`
  overrides). ActivityManager models a 2 GB, non-low-RAM device.
  ViewConfiguration uses AOSP 14 constants.
- **Graphics:**
  - not implemented: shaders, mask filters, blend modes, shadows, and
    applying color filters (they are recorded only);
  - clips are rectangular and `Path.op` is approximate;
  - Bitmaps are display lists, with no text shaping or bidi;
  - the draw pass paints DEX views only.
- **Platform services:** base classes return defaults. Accessibility
  reports no services; Autofill and content capture are absent (null).
  Vibrator calls are logged as `I/RdVibrator`, insets calls as `I/RdInsets`.
  Services are per-class singletons; value builders are inert.
- **Reflection:** Classes, Methods and Constructors expose no
  runtime-visible annotations. There is a single app ClassLoader, whose
  parent is null.
- **Locks:** hold counters under cooperative green threads. Holding a lock
  across a park does not block other threads; `Condition.await` returns as
  a spurious wakeup.
- **java.time:** the device zone is UTC. The current time is the
  deterministic virtual wall clock (`RD_EPOCH_MS`). Pattern letters
  supported: `y M d H h m s S a E` and quotes.
- **SQLite:** in-memory, with an SQL subset; WAL and journal settings have
  no effect.
- **Unsupported:** ML Kit OCR and barcode, which need `pthread_create` and
  direct ByteBuffers (accepted M9 limitations).

## Features not yet exercised

The first composition does run (Scaffold, icons, dates), but the activity
dies on the first database query, before a frame is drawn. The following
therefore remain untested through MCP:
- shopping list, prices, history and the "Gasto por mercado" chart;
- the store comparison and its CSV export;
- share, backup, the widget, and the camera/OCR features.

The MCP scenario files for them follow the `s01-launch.json` format.
