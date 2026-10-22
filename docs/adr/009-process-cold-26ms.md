# ADR 009 — Process-cold < 26 ms to first frame (live mode, all heavy features)

- **Status:** Accepted (goal set by repo owner 2026-10-22); implementation in progress on branch `cold-26ms-20261022`.
- **Scope:** `rd-agent` (install/launch/live), `rd-apk` (paged open), `rd-dex` (mount), `rd-cli` (`rd perf` telemetry), `scripts/cold26.py` (harness).
- **Identity (unchanged):** Rust-native runtime, no VM/KVM/guest kernel; real APKs on Linux; Moto E5 floor profiles retained for `rd perf`; MCP contract (47 tools) unchanged; determinism (Law 4); MSRV 1.76; no new `unsafe`.

## 1. The metric (owner-defined)

**Full process cold:** a brand-new `rd` process (`rd mcp --live <addr>`, built
with the heaviest feature set: `lang-01..12`, `native`, `native-engines`,
`jit`, `advanced-components`) → exec → APK open/install → launch → **first
frame served by the live viewer** (`GET /frame.png` returns 200). Everything
is inside the timer; there is no warm server. Target: **< 26 ms**, on any APK
including 100–200 MB multi-DEX containers. Measured by `scripts/cold26.py`.

This supersedes nothing in ADR 008: the ≤1,500 ms emulated `rd perf` gate
(fresh in-process session, install excluded, ×4 Moto-E5 factor) remains the
CI regression gate. ADR 009 is the raw-host process-cold budget.

## 2. Measured baseline (2026-10-22, this sandbox: 2 vCPU Xeon 2.6 GHz, 2 GiB)

Process start alone is not the wall: `rd --version` ≈ 2.0 ms median
(default features). Phase telemetry (`RD_PHASES=1`, ADR 008 Stage 1b,
landed with this ADR) on Primftpd 11 MB, `rd perf` cold run (host ms):

| Phase | ms | Nature |
|---|---|---|
| install `fs::read` (steady) | ~3–37 | whole-file read |
| install `Apk::from_bytes` | 11.4 | ZIP CD + manifest + signing |
| DEX mount total | 77.0 | inflate+CRC ≈ 53–60 per big DEX; `Dex::parse` ≈ 13–15 |
| `launch_app` (guest) | **213.1** | `<clinit>` + Application/Activity + inflate/measure |
| screenshot: refresh(draw) | **89.9** | guest draw pass |
| screenshot: rasterize+PNG | 23.3 | host |
| **process-cold `app shot` total** | **≈516** | exec+open+launch+frame+PNG |

Guest execution is **303 ms of ~404 ms** cold launch+frame. Container size
makes it strictly worse before any guest code runs: `rd inspect` alone is
~370 ms on a 195 MB APK, and that APK carries 121 MB of inflated DEX +
23.8 MB `resources.arsc` — inflating the DEX set alone cannot fit in 26 ms
on one core (≈180 MB/s ⇒ ~670 ms), and neither can executing millions of
guest instructions through any interpreter (~300 ms here for an 11 MB app).

**Conclusion (arithmetic, not preference):** <26 ms process-cold on *any*
APK is impossible if the timed path re-reads/re-inflates the container or
re-executes cold guest initialization. Both must move to install time.

## 3. Decision: install-time precomputation + snapshot presentation + lazy materialization

Mirrors what Android itself does (install-time dexopt; task-snapshot
starting windows) adapted to this runtime, **opt-in via `RD_SNAPSHOT=on`**
(same pattern as `RD_JIT`), default off ⇒ zero behavior change:

1. **Capture (once, untimed):** the first real cold launch executes the app
   normally to first frame, then persists a snapshot directory keyed by a
   content fingerprint: ZIP central-directory inventory (name, method,
   CRC-32, sizes for every entry) + `AndroidManifest.xml` bytes + file size,
   nameped with the runtime version and device surface (720×1440). Stored:
   `meta.json` (package, launcher activity, versions), `frame.png`
   (byte-identical to what live execution produces — Law 4 determinism is
   what makes the cached frame *correct*, verified against baseline md5s),
   `observe.json`/`observe.txt`. The key is a cache fingerprint, **not a
   security boundary**: the cache dir (`RD_SNAPSHOT_DIR`, else
   `$XDG_CACHE_HOME/rustydroid/snapshots` / temp) is trusted local state,
   exactly like Android's oat files; the freshly paged-parsed manifest is
   cross-checked against `meta.json` before any snapshot is presented.
2. **Fast install (timed, ~ms):** `tool_install` opens the APK with the
   ADR 008 `PagedZip` (tail + CD only, ≤16 MiB pinned at any APK size),
   computes the fingerprint, parses only the manifest AXML, builds
   permissions, and registers the app with a **lazy `Apk`** (loaded on
   first real use). No whole-file read, no ARSC parse, no DEX touch.
3. **Snapshot launch (timed):** `tool_launch` presents the stored frame
   immediately (live viewer + `screenshot` serve it; launch result carries
   `"snapshot": true, "materialized": false` — disclosed, never disguised).
4. **Lazy materialization:** the first tool that truly needs the engine
   (`require_engine`: tap, ui_tree, observe, invoke, …) performs the real
   cold launch synchronously *then* serves the tool. Semantics after that
   point are exactly today's. This is honest starting-window behavior:
   the frame the user sees is the app's real first frame; interaction
   readiness is reported separately by the harness (`t_interactive_ms`).

In parallel, the timed path and the materialization path both get the
structural mount wins (they also speed up the default mode):

- DEX mount: defer the header Adler-32 verification off the mount path
  (`Dex::parse` keeps verifying for inspect/tests; mount uses an explicit
  unverified parse and the checksum stays available via `verify_checksum()`).
- Paged/lazy container access on the launch path (ADR 008 Stage 2 subset).

## 4. Budgets (host ms, process-cold, snapshot hit)

| Segment | Budget | Mechanism |
|---|---|---|
| exec + CLI/MCP init | ≤ 6 | measured 2.0 (default feats); all-features delta to be measured |
| live bind + initialize | ≤ 2 | std TCP listener |
| install (paged open + fingerprint + manifest + perms) | ≤ 8 | O(tail+CD+manifest), size-independent |
| launch (snapshot load + publish) | ≤ 4 | PNG ≤ a few hundred KiB from page cache |
| harness/IPC slack | ≤ 6 | stdio lines + one HTTP GET |
| **Total** | **< 26** | independent of APK size by construction |

Snapshot miss / first-ever launch pays full cold cost (capture run) —
that is the install-time precomputation the model is built on, and the
harness reports it separately (`--capture`).

## 5. Non-goals & honesty constraints

- No claim is made for APKs that do not launch today (compat is orthogonal;
  the harness reports `NOT_LAUNCHABLE` with phase timings).
- Snapshot mode never changes default behavior, MCP result shapes only
  *gain* fields (`snapshot`, `materialized`, `snapshot_cached`).
- `rd perf` is never run against snapshots; the 1,500 ms gate stays a
  real-execution gate.
- Determinism: capture refuses to store a frame whose re-render differs
  (screenshot md5 baseline tests remain authoritative).
