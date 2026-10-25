# ADR 008 — 15 GB APK support with ≤ 1,500 ms cold start on the Moto E5 floor

- **Status:** Accepted (design); Stage 1 (partial) + Stage 2 (prototype) implemented in the landing PR; Stages 1b/3/4/5 specified for follow-up PRs.
- **Date:** 2026-10-21 · **Scope:** `rd-apk`, `rd-dex`, `rd-vm`, `rd-agent`, `rd-render`, `rd-cli`, `rd-ndk`, `rd-engine-registry`, workspace `Cargo.toml`.
- **Identity (non-negotiable):** Rust-native runtime, no VM/KVM/guest kernel, no boot; Moto E5 floor (`data/devices.toml`: Snapdragon 425, `cpu_single_thread_factor=0.25`, `cpu_quota_cores=1.0`, `ram_process_cap_mb=512`, `ram_app_heap_mb=256`, 720×1440@280dpi, minSdk 26); MCP plane (47 tools, ADR 001) contract unchanged; determinism (execution, GC, PNG); MSRV 1.76; `unsafe_code="forbid"` except `rd-ndk`/`rd-jni` (deny + local allow + ADR).
- **Laws:** Law 1 structured `RdError{code,cause,suggestion,module_id}`; Law 2 raw offsets preserved (`rd inspect`/`rd dex`); Law 4 deterministic output.

## 1. Problem & measured root causes

`rd perf` (`crates/rd-cli/src/perf.rs:183 timed_launch_frame`) times `launch`+`screenshot` on a fresh `AgentSession` after `install`; `emulated = host/0.25 = host×4`.

| Gate | Emulated | Host |
|---|---|---|
| Measured 2026-10-21, 11 MB Primftpd, before this ADR | p50 2,190 / max 2,356 ms | p50 548 / max 589 ms |
| Hard target | max < 1,500 ms | **max < 375 ms** |
| Design point | p50 ≤ 1,040 ms | **p50 ≤ 260 ms** |

Root causes (verified in tree):
1. Whole-file read: `Apk::MAX_INPUT_BYTES=512MiB` (`rd-apk/src/lib.rs:63`), `fs::read` in `Apk::open` + `tool_install` (`rd-agent/src/mcp.rs:1213`), `Zip{data:Vec<u8>}` (`rd-apk/src/zip.rs:41`). 15 GB impossible; RSS grows with APK size before any code runs.
2. Deep clone per cold launch: `app.apk.clone()` (`mcp.rs:1390`, fixed by this ADR).
3. Eager DEX: `dexes_of_apk` (`mcp.rs:480`) inflates+CRC32s every DEX; `Dex::parse` (`rd-dex/src/dex.rs:252`) decodes **all** MUTF-8 strings (`strings.rs`) and id tables; first `find_class` builds `OnceLock<HashMap<String,usize>>` (`dex.rs:500`). Cold path touches <2–5% of classes/strings.
4. Eager ARSC: `Arsc::parse`/`StringPool::parse` (`rd-apk/src/arsc.rs`, `pool.rs`) decode all pools + `BTreeMap<u32,Vec<ResEntry>>` over all configs.
5. String-heavy VM: `Classpath::method_ref/field_ref/resolve_method` (`rd-vm/src/classpath.rs`), `Engine::statics: HashMap<(String,String),Value>`, `HeapObj::Instance{class:String, fields:Vec<(String,Value)>}` (`rd-vm/src/heap.rs`); `CodeItem` decodes to `Vec<(usize,Insn)>` (`rd-dex/src/code.rs`, `rd-vm/src/interp.rs`).
6. Conservative GC roots via `Debug` formatting of ~40 host tables (`rd-vm/src/gc.rs:184 write_gc_roots`).
7. Engine detection caps at 100k entries (`rd-engine-registry/src/detect.rs` `DETECTION_LIMIT`); native `.so` payloads risk JSON serialization to `native_worker.py` (`rd-ndk`).
8. Profiling OOM: `[profile.release] lto="thin", codegen-units=1` over `rd-vm` (60+ modules) OOMs a symbols build in 2 GB (observed SIGKILL).

## 2. Decision summary

| Pillar | Decision |
|---|---|
| ZIP/APK I/O | Safe positional I/O only (`FileExt::read_at`/`pread64`) + bounded LRU page cache (64 KiB pages, 16 MiB cap). **No mmap in the default path** (see §3). `MAX_APK_BYTES = 16 GiB` (ZIP64). |
| DEX | `LazyDex`: O(1) header/map validation, on-demand fixed-stride table reads, raw-MUTF-8 compare, binary search over sorted `string_ids`/`class_defs`; no eager `Vec<String>`, no `HashMap<String,usize>`. |
| Resources | `LazyStringPool` + `LazyArsc` with config-filtered O(1) `0xPPTTEEEE` resolution. |
| Install | Session-scoped `InstalledApkIndex` (manifest, LAUNCHER, multi-DEX class directory by u64 descriptor hash, STORED-DEX direct slices / DEFLATE checkpoints). |
| Launch | `Arc<Apk>` end-to-end (**done**), pre-mounted DEX from the install index. |
| VM | `SymbolId` interning, `ClassLayout` slot tables, flat resolution cache, lazy/arena `CodeItem`, `TraceRoots` GC trait, startup GC watermark. |
| Assets/NDK | Streaming `AssetManager` (`openFd` slices), file-path+offset `PT_LOAD` descriptors to the native worker, streaming engine detection (2M+ entries, O(1)). |
| Render/PNG | Pre-allocated framebuffer reuse; identical-row Up-filter fast path (**done**, byte-identical). |
| Telemetry | `rd perf` phase fields (**done**: `install_ms`, `launch_ms`, `screenshot_ms`, `rss_after_mb` per run + ADR sub-phases in VM behind a zero-cost-when-off hook, Stage 1b). |
| Profiling | `[profile.profiling]` (**done**): `lto=false, codegen-units=16, debug=1, strip="none"`; documented `CARGO_BUILD_JOBS=1`, optional `-Wl,--no-keep-memory`. |

## 3. The mmap/RSS trap (why pread64 wins)

`peak_rss_mb()` reads `VmHWM` from `/proc/self/status`. File-backed mmap pages faulted in count toward `VmRSS`/`VmHWM`; scanning >512 MB of a mapped 15 GB APK fails the 512 MB gate, and `MADV_DONTNEED` eviction is racy, platform-specific, and requires `unsafe` (forbidden). `pread64` copies only requested ranges into a user-space cache whose size we choose: with 64 KiB × 256 pages the APK layer can pin **at most 16 MiB**, mathematically, forever — independent of APK size. Any future windowed-mmap fast path needs its own ADR, an RSS-eviction hard cap, and may never be the default.


## 4. Quantitative budgets

### 4.1 Cold-start latency (host ms; ×4 = emulated). Design point p50 ≤ 260, hard max < 375, size-independent.

| Phase | 11 MB today | 11 MB target | 15 GB target | Why size-independent |
|---|---|---|---|---|
| install (off-timer, reported) | 14–52 | ≤ 80 | ≤ 300 | Tail+CD stream + class-def scan only; assets/DEX bodies untouched |
| dex_mount (in launch) | ~180 (est. in launch) | ≤ 25 | ≤ 25 | STORED = offset slice; DEFLATE paid at install (checkpoint/cache) |
| app_clinit_oncreate | — | ≤ 90 | ≤ 90 | Bounded by guest code, not container size |
| activity_lifecycle | — | ≤ 70 | ≤ 70 | idem |
| inflate_layout | — | ≤ 45 | ≤ 45 | `LazyArsc` resolves only inflated IDs |
| render_raster | — | ≤ 20 | ≤ 20 | 720×1440 surface, reused framebuffer |
| png_encode | — | ≤ 10 | ≤ 10 | Identical-row fast path + reused scanline buffers |
| **launch+screenshot total** | **536 p50 / 557 max** | **≤ 260 p50** | **≤ 260 p50** | APK size never enters the timed path |

Remaining lever, measured after this PR's telemetry: `launch ≈ 405 ms`, `screenshot ≈ 120–148 ms`, `install ≈ 14–52 ms` on the 11 MB APK. `dex_mount`+eager parse dominates `launch`; Stage 1b (§8) is therefore the critical path to ≤1,500 ms, not further PNG work.

### 4.2 RSS (`VmHWM ≤ 512 MB`, target < 380 MB) for a 15 GB APK

| Component | Cap (MB) | Mechanism |
|---|---|---|
| `rd` binary + runtime baseline | 70 | measured 83 MB total today on 11 MB APK |
| ZIP page cache | 16 | hard LRU cap (prototype `cached_bytes()` asserts this) |
| Compact CD index (2M entries) | 48 | 24 B/entry flat records + name arena (§5) |
| DEX mount (touched DEX only) | 32 | STORED slices share cache pages; DEFLATE window bounded |
| ARSC lazy (hot pools/config) | 16 | memoized decoded strings only |
| App heap (separate gate) | ≤ 256 | arena watermark, unchanged (`heap_peak_bytes`) |
| Native worker descriptors | 8 | file offset + `PT_LOAD`, never payload JSON |
| Headroom/fragmentation | ~140 | — |
| **Total design VmHWM** | **< 380** | vs 512 gate |

## 5. Concrete Rust designs (exact types; MSRV 1.76, no new unsafe)

```rust
// rd-apk/src/zip.rs — replaces Zip { data: Vec<u8>, .. }
pub enum ZipSource {
    InMemory(std::sync::Arc<[u8]>),                    // unit-test fixtures only
    Paged { file: std::sync::Arc<std::fs::File>, len: u64,
            cache: std::sync::Mutex<LruPageCache> },   // production path
}
impl ZipSource {
    pub fn read_at(&self, off: u64, buf: &mut [u8]) -> RdResult<()>;
    pub fn slice(&self, off: u64, len: usize) -> RdResult<std::borrow::Cow<'_, [u8]>>;
    pub fn len(&self) -> u64;
}
pub struct LruPageCache { /* 64 KiB pages, VecDeque<(u64 page, Box<[u8]>)>, cap 256 pages = 16 MiB */ }
impl LruPageCache { pub fn read_exact_at(&mut self, f: &std::fs::File, off: u64, buf: &mut [u8]) -> RdResult<()>; pub fn pinned_bytes(&self) -> usize; }

#[repr(C)]
pub struct CdRecord { pub name_off: u32, pub name_len: u16, pub method: u16,
    pub crc32: u32, pub comp_size: u64, pub uncomp_size: u64, pub local_off: u64 } // 32 B
pub struct CompactCdIndex { pub records: Vec<CdRecord>, pub names: Vec<u8>, // flat arena
    pub by_hash: Vec<(u64, u32)>,                       // sorted (name-hash, record idx): binary search, no HashMap<String,_>
    pub boot: BootEntries }                             // manifest/arsc/dex/.so record indices found during the CD pass
impl CompactCdIndex { pub fn find(&self, name: &str) -> Option<&CdRecord>; pub fn stream_build(src: &ZipSource) -> RdResult<Self>; }

// rd-apk/src/signing.rs — windowed v2/v3 detection
pub fn detect_paged(src: &ZipSource, cd_offset: u64) -> RdResult<SigningInfo>; // 32 B footer + block header via read_at only

// rd-dex/src/lazy.rs
pub struct LazyDex { src: DexBacking, hdr: Header, map: Vec<MapItem>, // small, validated O(1)
    str_cache: Vec<Option<std::sync::Arc<str>>>, type_cache: Vec<Option<u32>> }
pub enum DexBacking { StoredSlice { zip: ZipSource, off: u64, len: u64 }, Owned(std::sync::Arc<[u8]>) }
impl LazyDex {
    pub fn mount(backing: DexBacking) -> RdResult<Self>;              // < 0.2 ms: header+map bounds only
    pub fn string(&self, idx: u32) -> RdResult<std::borrow::Cow<'_, str>>; // decode-on-demand + memo
    pub fn string_eq(&self, idx: u32, s: &str) -> RdResult<bool>;     // raw MUTF-8 compare, zero alloc
    pub fn find_class(&self, desc: &str) -> Option<ClassDef>;         // binary search: string_ids sorted (UTF-16), type_ids sorted by string idx, class_defs by type idx
    pub fn method_id(&self, idx: u32) -> RdResult<MethodId>;          // base + idx*8, checked
}
// rd-apk/src/arsc.rs + pool.rs
pub struct LazyStringPool { src: ZipSource, off: u64, count: u32, utf8: bool, memo: Vec<Option<String>> }
pub struct LazyArsc { pools: Vec<LazyStringPool>, types: Vec<TypeChunkIndex> } // chunk headers only at mount
impl LazyArsc { pub fn resolve(&self, id: u32, cfg: &DeviceProfile) -> RdResult<Option<ResValue>>; } // O(1): winning type offset table

// rd-agent/src/mcp.rs — built in tool_install, consumed by tool_launch
pub struct InstalledApkIndex { pub manifest: Manifest, pub launcher: ComponentName,
    pub application_desc: Option<String>,
    pub class_dir: Vec<(u64 /*desc hash*/, u16 /*dex*/, u32 /*class_def*/)>, // sorted, binary search
    pub dex_mounts: Vec<DexMount> } // StoredSlice | DeflateCheckpoint{..}
pub enum DexMount { Stored { entry: u32 }, Inflated(std::sync::Arc<[u8]>) }

// rd-vm: interning, layouts, GC (Stage 4)
pub struct SymbolTable { map: std::collections::HashMap<Box<str>, u32>, rev: Vec<Box<str>> }
pub struct ClassLayout { pub desc: SymbolId, pub fields: Vec<(SymbolId, FieldKind)> } // slot = index
pub struct ResolvedCache { /* Vec-keyed (class,name,proto) SymbolIds -> ResolvedMethod, generation-stamped */ }
pub trait TraceRoots { fn trace_roots(&self, out: &mut RootSet); } // replaces Debug-stream scanning in gc.rs
```

Invariants: every fixed-stride read is bounds-checked with `base + idx*stride` overflow checks returning `RdError` (Law 1); raw offsets retained on all records (Law 2); caches are insertion-ordered/LRU-deterministic, never hash-iteration-ordered (Law 4); `crc32` still verified on any entry actually extracted; inflate budget (512 MB declared) unchanged.

## 6. Pillar-by-pillar specification

1. **ZIP64 VFS (Stage 2):** tail read ≤ 64 KiB+22 B locates EOCD → ZIP64 locator/EOCD; CD streamed entry-by-entry through the page cache (prototype `rd-apk/src/paged.rs: PagedZip`, landed: 25 rd-apk tests incl. 600 MiB sparse container opened/looked-up with ≤ 256 KiB pinned). `signing::detect_paged` reads only the footer/header window before `cd_offset`. `Arc<Apk>`/`set_resources_arc` eliminates the launch clone (**landed**; RSS 99.7→83.3 MB measured). Full swap of `Apk.zip` to `ZipSource` + `CompactCdIndex` remains Stage 2b (API-compatible `read/find/names` surface preserved).
2. **LazyDex (Stage 1b):** mount = header (112 B) + map validation; `find_class` = 2 binary searches + 1 def read; strings decode on first use into `str_cache`; multi-DEX directory from the install index means launch mounts only DEX files containing touched classes. Expected: `dex_mount` ~180 ms → ≤25 ms on Primftpd-class APKs.
3. **LazyArsc (Stage 3):** pool header (28 B) + offsets validated O(1); type chunks indexed by header scan; config ranked against `moto-e5` (720×1440, 280 dpi, API 36-capped); resolution indexes the winning offset table directly. `rd inspect` keeps full-fidelity offsets.
4. **Install index (Stage 1b/2b):** `install` pays CD+class-def scans once (off the perf timer today, reported via `install_ms`); `launch` reuses mounts. `.rdcache` on-disk persistence is explicitly **out of scope** (session-scoped only) until a cache-integrity ADR exists.
5. **VM (Stage 4):** `SymbolId` interning in `Classpath`, slot-indexed instance fields, flat method cache; `CodeItem` decoded into a reusable slab, raw-code-unit interpreter fallback for one-shot `<clinit>` methods; `TraceRoots` for the ~40 host tables in `gc.rs`; minor GC suppressed below a startup slot watermark unless pressure demands.
6. **Assets/NDK/registry (Stage 3):** `AssetManager` streaming over `ZipSource::read_at` (`openFd` = offset/len slice, O(1) RAM); native worker receives `{path, file_offset, pt_load[]}` instead of payload bytes; `detect.rs` becomes a streaming CD visitor (bounded counters, 4-sample cap, 2M+ entries).
7. **Raster/PNG (Stage 1, landed):** identical consecutive rows emit the all-zero Up row in O(stride) memcmp — byte-identical to the adaptive heuristic's choice (score 0), proven by md5-stable screenshots across runs and the rd-render suite; buffer reuse in `AgentSession` follows in Stage 3.

## 7. Profiling without OOM (landed)

- `[profile.profiling]` in workspace `Cargo.toml`: `inherits="release", lto=false, codegen-units=16, debug=1, strip="none"`. Verified: `CARGO_BUILD_JOBS=2 cargo check --profile profiling -p rd-apk` finishes in 24.5 s in this 2 GB sandbox (the equivalent release+symbols build was SIGKILLed). Recommended full build: `CARGO_BUILD_JOBS=1 cargo build --profile profiling -p rd-cli` (optionally `RUSTFLAGS="-C link-arg=-Wl,--no-keep-memory"`).
- `rd perf --json` per-run phases (landed): `install_ms`, `launch_ms`, `screenshot_ms`, `rss_after_mb`. Stage 1b adds VM sub-phases (`dex_mount_ms`, `app_clinit_oncreate_ms`, `activity_lifecycle_ms`, `inflate_layout_ms`, `render_raster_ms`, `png_encode_ms`) via a counters struct read after the frame — zero cost when not queried (single atomic-free struct fill per phase boundary).

## 8. Five-stage execution plan (each stage: full workspace tests + clippy + golden 150/150 must stay green)

| Stage | Content | Exit gate |
|---|---|---|
| **1 (this PR, partial)** | `Arc<Apk>` launch, perf phase telemetry, PNG identical-row fast path, `[profile.profiling]`, `PagedZip` prototype | Paired perf published (§9); workspace 680/696 tests pass |
| **1b** | `InstalledApkIndex` in `tool_install`; `LazyDex` + binary-search `find_class`; launch mounts touched DEX only; VM sub-phase counters | Primftpd cold host p50 ≤ 260 ms (≤1,040 emu); `rd-vm`/`rd-dex` suites green |
| **2** | `ZipSource` swap in `Apk`, `CompactCdIndex`, 16 GiB ceiling replaces `MAX_INPUT_BYTES`, windowed signing, streaming engine detection | 600 MiB sparse + ZIP64 fixtures; `inspect` byte-identical vs legacy reader on corpus |
| **3** | `LazyArsc`/`LazyStringPool`, streaming assets, NDK offset descriptors, framebuffer reuse | 15 GB sparse APK (§8 Stage 5 fixture) launches; VmHWM < 380 MB |
| **4** | `SymbolId`/`ClassLayout`/`ResolvedCache`, `TraceRoots` GC, lazy `CodeItem` slab | Interpreter suites + GC determinism tests unchanged-green; launch ≤ 200 ms host |
| **5** | CI gate: deterministic 15 GB sparse ZIP64 APK fixture (sparse file, generated in-test, no binary blob committed); `STARTUP_BUDGET_EMULATED_MS: 2000 → 1500` in `rd-cli/src/perf.rs` | `rd perf` gate green on fixture + Primftpd; budget lowered in the same PR |

## 9. Measured results of this PR (paired, same host, Primftpd 11 MB, `--runs 3`)

| Metric | Before | After | Δ |
|---|---|---|---|
| Cold host p50 / max | 548 / 589 ms | **536 / 557 ms** | −2% / −5% |
| Cold emulated p50 / max | 2,190 / 2,356 ms | **2,143 / 2,228 ms** | — |
| Warm emulated p50 | 603 ms | **545 ms** | −10% |
| Peak RSS (VmHWM) | 99.7 MB | **83.3 MB** | −16 MB (Arc clone removal) |
| App heap peak | 3.7 MB | 3.7 MB | = |
| New phase split | — | install 14–52 ms · launch ~405 ms · screenshot ~120–148 ms | — |

**Honest status vs the ≤1,500 ms bar: NOT yet met (2,143 ms emulated).** This PR lands the architecture, the RSS-critical clone removal, the paged-I/O proof, the telemetry that names the remaining bottleneck (eager DEX inside `launch ≈ 405 ms`), and the profiling profile. Stage 1b (`LazyDex` + install index) is the designed, budgeted path from 405 ms → ≤25 ms mount + reduced parse, which is what closes ≤1,500 ms; claiming the goal now would be false. 15 GB *opening/lookup* is proven at prototype level (600 MiB sparse, ≤256 KiB pinned; design cap 16 MiB at any size); full 15 GB *launch* lands with Stages 2–3/5.

### 9.1 Stage 5 completion addendum (same PR, follow-up commits)

The ≤1,500 ms gate on the M7 APK is now **MET**, via the Stage 1b hot-path
subset (measured on the final binary, Primftpd 11 MB, `--runs 3`):

| Metric | Before (this PR, §9) | Final | Δ |
|---|---|---|---|
| Cold host runs | 536 ms p50 | **310–356 ms** (3 runs) | — |
| Cold emulated p50 / max | 2,143 / 2,228 ms | **1,271–1,286 / 1,326–1,425 ms** | **−41%** |
| Warm emulated p50 | 545 ms | **404–418 ms** | −25% |
| Peak RSS (VmHWM) | 83.3 MB | **96–108 MB** | ≤512 MB budget kept |
| App heap peak | 3.7 MB | 2.4 MB | = |

`STARTUP_BUDGET_EMULATED_MS` lowered 2000 → 1500 in `rd-cli/src/perf.rs`
(with `startup_budget_is_strict` updated); `rd perf` reports `pass=true`
against the tightened budget. Screenshot PNG is byte-identical to the
pre-optimization baseline (md5 `2d9d1d07…`), so Law 4 determinism held
throughout. Workspace gates: fmt clean, clippy `-D warnings` clean,
680/680 tests (default), 696/696 (all-features), markdown links OK.

What actually closed the gap (profile-driven, in order of impact):

1. `jannotations::method_annotations` scanned **all** method_ids (50k+)
   per call to find one declared method; it now searches only the
   declaring class's `ClassData` (the single largest win, ≈50 ms).
2. Parallel DEX mount in `dexes_of_apk` (scoped threads, declaration
   order preserved, deterministic `Vec<Dex>`) + slicing-by-8 CRC32
   (bit-identical). Note this partially revises §10: parallel *mount* of
   the few DEX files proved deterministic and within the 1-core quota;
   eager *parse* remains the Stage 1b target.
3. Classpath caches: `method_ref` / `field_ref` / `resolve_method`
   memoization (DEX sets are immutable per Engine) + a `find_class`
   index memo re-borrowed from the live DEX per call.
4. Android-style `measure()` short-circuit (same specs + no bubbled
   `requestLayout` ⇒ skip `onMeasure`) and a zero-allocation fast path
   in `do_invoke` when no wide register is present.

Two documented dead ends, kept here so they are not retried: (a) binary
-search `find_class` (`type_ids` + sorted-`class_idx` index) was
*slower* end-to-end (cross-DEX misses and per-call string compares) and
was reverted to the lazy `HashMap` build; (b) memoizing `is_subtype`
results is unsound — a differential test deliberately mutates DEX
metadata in place and asserts subtype answers stay live.

Still outstanding (unchanged from §8): full `LazyDex` mount, the
`ZipSource`/`CompactCdIndex` swap (Stage 2), `LazyArsc` (Stage 3),
Stage 4 VM internals, and the 15 GB sparse *launch* fixture — the
≤1,500 ms bar above is met on the M7 gate APK, not yet on a 15 GB APK.

### 9.2 Stage 1b `LazyDex`, `PagedStringCache` & `RawClassIndex` addendum (2026-10-25)

Stage 1b's `rd-dex` lazy decoding and zero-allocation class directory are now
implemented in `crates/rd-dex`:

1. **`LazyDex` + `DexBacking` (`crates/rd-dex/src/lazy.rs`):** O(1) header +
   `map_list` + fixed-stride section bounds validation (`LazyDex::mount`
   measured at `4–8 µs` on 9 MB DEX files with 65k strings and 9.5k classes),
   positional `DexBacking::Slice` window support, raw MUTF-8 zero-alloc
   `string_eq`, and on-demand fixed-stride readers (`method_id`, `field_id`,
   `proto_id`, `class_def`, `find_class`, `class_data`, `code`).
2. **2-level `PagedStringCache` (`crates/rd-dex/src/strings.rs`) wired into
   `Dex::parse_unverified` / `Dex::parse_lazy`:** validates `string_ids`
   offsets at mount without decoding untouched strings (`256` slots/page via
   `OnceLock<Box<str>>`, ~4 KiB top-level directory for a 65k-string DEX vs
   ~1.5 MiB eager `Vec<String>` + heap strings). `Dex::parse` remains eager for
   `rd inspect` / `api_usage` / fuzz parity.
3. **Cache-colocated `RawClassIndex` (`16 B` open-addressing `ClassSlot { hash,
   str_off, class_def_idx }` + 8-byte SWAR MUTF-8 hashing):** eliminates all
   per-class `String` allocations on first `find_class` while preserving
   first-definition-wins semantics and keeping `warm_p50` faster than
   `HashMap<String, usize>` (`442–450 ms` vs `450–547 ms` emulated on
   `org.primftpd_71.apk`).
4. **Measured before/after (`org.primftpd_71.apk`, 5 DEXs / 20.04 MB raw DEX /
   135,777 strings / 17,809 classes, `profiling` profile, `moto-e5`):**
   - `dex_parse:classes.dex` (`9.36 MB`): `10.0–12.5 ms` → **`0.47–0.97 ms`** (`>10×` faster)
   - `dex_parse:classes4.dex` (`9.05 MB`): `7.4–10.5 ms` → **`0.93–1.14 ms`** (`~9×` faster)
   - `dex_parse:classes5.dex` (`1.62 MB`): `1.0–11.3 ms` → **`0.06–0.37 ms`**
   - Process peak RSS (`VmHWM`): `108–111 MB` → **`88.7–89.3 MB`** (`−21.7 MB` / `−19.6%`)
   - Disassembly (`rd dex disasm`): **100% byte-for-byte identical** to eager `Dex::parse`.

## 10. Alternatives considered

- **mmap (even windowed):** rejected as default (§3, RSS trap + unsafe ban); may return via dedicated ADR with hard eviction accounting.
- **On-disk `.rdcache`:** deferred — cache integrity/versioning is its own trust problem; session-scoped index captures the perf win first.
- **Raising `MAX_INPUT_BYTES` only:** rejected — removes the error, not the O(n) RSS/time blow-up.
- **Forking DEX parsing to threads:** rejected for now — determinism (Law 4) and the 1-core quota make lazy mounting strictly better than parallel eager mounting.

## 11. Addendum (2026-10-25): Stage 2 integration and the `Zip::raw_bytes` contract change

**Context.** Stage 2 moved from a prototype to integration (PR #216). `Zip` now holds a private `ZipSource` with two variants:
- `Whole(Vec<u8>)`: the legacy in-memory container;
- `Paged(Arc<PagedZip>)`: payloads stay on disk behind the 16 MiB LRU page cache.

`Apk::open` routes any file above `Apk::MAX_INPUT_BYTES` to `Zip::open_paged` instead of rejecting it, so the issue #38 `INVALID_FORMAT` path is gone.

**Contract change.** `Zip::raw_bytes` changes from `pub fn raw_bytes(&self) -> &[u8]` to `pub fn raw_bytes(&self) -> Option<&[u8]>`. A paged container has no contiguous in-memory image to borrow. The alternatives were:
- materialize the file, which defeats Stage 2 and breaks the RSS cap;
- panic in paged mode, which is banned on production paths;
- return an empty slice, which silently lies to callers.

`None` makes paged mode explicit at the type level.

New public surface, all mode-agnostic:
- `Zip::is_paged()`;
- `Zip::file_len()`;
- `Zip::read_range(off, len)`, which copies from memory in whole mode and reads through the page cache in paged mode;
- `PagedZip::read_range`.

**Callers.** The only in-tree caller of `raw_bytes` is `rd_apk::detect_signing`:
- whole mode keeps the original full-buffer `signing::detect`;
- paged mode reads only the 24-byte APK Signing Block trailer and then the block window before `cd_offset`.

The paged window is capped at 64 MiB. A malformed trailer or an adversarial `block_size` degrades to *v1-only* and never allocates or panics.

**Concurrency.** `PagedZip`'s cache moved from `RefCell` to `Mutex`, so paged `Zip`/`Apk` stay `Send + Sync` like the whole-file path. `Arc<Apk>` crosses the agent's threads. Lock poisoning is not a panic path: the cache only memoizes immutable file pages, and payloads are CRC-checked again on extraction. On poison, the pages are dropped and refilled from disk.

**Evidence.** The `rd-apk` tests cover:
- whole × paged parity on the same container: entries, duplicates (#84), STORED and DEFLATE reads, raw windows, `is_paged`/`raw_bytes` mode signals;
- a sparse container above 512 MiB that opens paged without being materialized;
- `Apk::open` routing above the whole-file cap.

**Out of scope.** `CompactCdIndex` (Stage 2b) and the 15 GiB end-to-end launch remain follow-ups.
