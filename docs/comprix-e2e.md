# Comprix v1.6.0 end-to-end via MCP: status, latency, and blockers

This document covers running Comprix v1.6.0 (`br.com.comprix`, x86_64)
end to end, driven entirely through `rd mcp`.

- **App:** Jetpack Compose, Room, androidx.startup, ML Kit OCR and barcode.
- **Target:** minSdk 26, target 35, a single DEX.
- **Harness:** `mcp_run.py`, which sends JSON-RPC over stdio and times each
  call from request write to parsed response with a monotonic clock.

## Status

The launch does **not** yet reach a first Compose frame, so no screenshot of
app UI is available.

**How far the launch gets:**
1. `Application.onCreate` and the androidx.startup initializers run.
2. `MainActivity.onCreate` runs, along with `setContentView(ComposeView)`.
3. The window is attached: decor view, `onAttachedToWindow`, lifecycle owners.
4. The Compose `Recomposer` is constructed.
5. The kotlinx-coroutines main dispatcher is set up, found by reflection
   (`Handler.createAsync`).
6. Kotlin Flow collection for the animation-scale `Settings` observer runs.

**Blocker 1 (fixed): the "hang" was a performance cliff.** A symbolized
`gdb` backtrace showed every instance call running hook gates
(`threads::is_thread_class`, `perms::is_context`). Each gate called
`is_subtype`, and that called `Dex::find_class`, a linear scan over about
10k ClassDefs repeated per hierarchy step. `Dex::find_class` now uses a
lazily built descriptor index, which is O(1). The same probe that ran for
more than 10 minutes now ends in 14 s.

**Blocker 2 (open).** Execution reaches kotlinx `BlockingCoroutine`
(`runBlocking`), whose event loop (`e4/g.w`/`u`) spins without making
progress. This is consistent with the coroutine waiting for work that the
VM never schedules.
- The CLI reports `VM_FUEL` at 50M instructions with the guest stack.
- Timed main-thread `LockSupport.parkNanos` now advances virtual time to the
  earliest timed green-thread sleeper, without draining the Looper. That
  did not change this stack, so the pending work is probably queued on the
  main Looper or on a scheduler not yet modelled.
- MCP `launch` uses a much larger fuel budget, so it still exceeds the
  harness's 60 s per-call deadline. MCP needs a wall-clock bound (see
  below).

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
- **Unsupported:** ML Kit OCR and barcode, which need `pthread_create` and
  direct ByteBuffers (accepted M9 limitations).

## Features not yet exercised

Because launch does not complete, the following remain untested:
- shopping list, prices, history and the "Gasto por mercado" chart;
- the store comparison and its CSV export;
- share, backup, the widget, and the camera/OCR features.

The MCP scenario files for them follow the `s01-launch.json` format.
