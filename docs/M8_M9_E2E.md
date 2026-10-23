# M8 contracts and M9 validation candidate

This candidate extends the history-preserving **RustyDroid** fork with shared
storage/framework contracts and a repeatable corpus. It is **not a completed
v0.1 release gate**: the new NRO still needs its own Horizon emulator run and
physical Switch verification. M1–M7 hardware testing was confirmed by the user
on 2026-10-24; that confirmation does not validate these later changes.

## Implemented contracts

| Area | Implemented subset | Explicit limits |
| --- | --- | --- |
| APK/storage | File-backed ZIP/ZIP64, staged install, SHA-256, lazy native dependency extraction; 128 MiB host proof | 512 MiB container, 16 MiB directory, 64 MiB materialized entry, 32 MiB combined raw DEX; decoded memory/API coverage remain separate |
| SharedPreferences | Named/default/activity preferences, typed XML values/string sets, canonical instances, editor clear/remove/commit/apply, getAll, weak listeners and GC roots | MODE_PRIVATE, one runtime process; apply is queued until tick/stop, I/O errors surface through runtime diagnostics; listener delivery is synchronous on the current VM thread, not Android's off-main dispatcher |
| SQLite | Actual native SQLite 3.54.1 C engine; Context.openOrCreateDatabase, SQLiteOpenHelper create/upgrade/open callbacks, execSQL bindings/rawQuery, typed cursor, version and nonnested transactions | 8 MiB snapshot per database, four open files, 32 MiB global SQLite C heap, 10,000 rows/8 MiB total cursor payload, 64 Java handles/cursors; no Android connection pool/WAL/multiprocess/custom factory or guest libsqlite.so export |
| Collections/boxing | ArrayList/LinkedList/Vector, HashMap/LinkedHashMap, HashSet/LinkedHashSet for the listed operations; iterator, toArray, scalar boxing/cache and value equality | 10,000 entries, 8 MiB retained values, 8192 containers; size/isEmpty/clear/get/set/add/contains/put/get/containsKey/remove-map/iterator/toArray; no complete Collections framework or Map views |
| Reflection | Canonical Class objects, forName/name/type/subtype queries, declared/public inherited DEX method lookup, flags, setAccessible, Method.invoke with widening/unboxing/boxing and wrapped target exceptions | DEX methods; no complete platform-member catalog, fields/constructor reflection, interface-method traversal or full caller/class accessibility analysis |
| Permissions | Manifest-declared SD sandbox read/write capabilities, checkSelfPermission, enforceCallingOrSelfPermission, Activity result callbacks | v0.1 host policy, not a full Android permission manager; unsupported camera/network/etc. are denied; no IME/permission prompt UI; the existing VFS sandbox is the filesystem boundary |
| Independent UI | Original AOSP HelloActivity Java/manifest/resources, LayoutInflater, EditText text rendering/input configuration | Static text/render/lifecycle scenario; Android IME and editable-buffer APIs remain unavailable |
| Android logging | Log.v/d/i/w/e/wtf(tag,message) writes actual bounded files under logs/android/ | 64 KiB entry, 1 MiB rotating sink per package; not Android logcat |

Absent overloads and services still fail with their descriptor/operation. These
are API-level implementations, without package-name branches or special game
profiles. SQLite runs in process; no Python or database subprocess is needed.
All database snapshots contain the actual `SQLite format 3` file format.
Preferences retain at most 8 MiB of cached values/pending edits and 64 live
editors. An unavailable/OOM SQLite snapshot is an error, never an empty save.

The storage adapter publishes complete snapshots through pending/previous
files in a private journal. Tests inject failures before and after durable
mutations, check recovery and accept only a complete old/new state. This is an
adapter fault model, not a claim about every SD-controller power-loss behavior.

## Corpus and measurements

`fixtures/m9/host-corpus.json` defines required Java/Kotlin UI, preferences,
renamed preferences, SQLite notes, renamed notes and original AOSP HelloActivity
scenarios. The host runner repeats launch → render → GC → stop with persistent
storage. JNI/native media retain their dedicated tests and earlier Horizon
evidence; the host UI runner does not substitute a fake native backend.

Vector Pinball 1.16.0 is a separate **evaluation** entry. An observed blocker is
reported as blocked execution, never converted into a compatibility pass.
The APK is unmodified and hash-pinned in `fixtures/m9/external.lock.json`.
The F-Droid binary/source-build equivalence has not been independently verified.
The external NDK samples remain frozen integration targets, not completed tests.

The corpus records per-session APK loading, VM/startup time, 30 CPU software UI
frame times, final frame hash and managed heap after GC. The launcher script
also measures elapsed time and the Windows host process's sampled peak working
set. These are unoptimized Cargo-dev host numbers at 640×360, **not Switch FPS,
GPU timings, total device memory or a native leak test**. The versioned report
in `reports/m8-m9-candidate.json` records exact observations and missing gates.

## Reproduce without opening desktop apps

Requires the documented Windows GNU Rust/C toolchain, JDK and Android SDK35.
`CC`/`AR` can override the C compiler/archive tool. See [building](BUILDING.md).

```powershell
python scripts/fetch-sqlite.py
./scripts/build-m5-fixture.ps1
./scripts/build-m6-fixture.ps1
./scripts/build-m8-fixture.ps1
./scripts/build-aosp-hello.ps1
python scripts/build-large-apk-fixture.py
cargo test -p rd-apk --lib --no-default-features --locked
cargo test -p as-storage -p as-sqlite --locked
cargo test -p as-package --locked
cargo test -p as-native --test loader --locked -- --include-ignored
cargo test -p as-launcher --features host --test workflow --locked -- --include-ignored
cargo test -p as-framework --features host --tests --locked -- --include-ignored
./scripts/test-m9-host.ps1 -Cycles 12
./scripts/build-androswitch.ps1
```

`test-m9-host.ps1` starts only an owned console child with `CreateNoWindow`.
It never enumerates, focuses, hides or closes other desktop applications.

After committing the implementation and passing the consolidated checks, freeze
the actual build/run evidence and package it:

```powershell
./scripts/check-m7-m9.ps1
python scripts/record-m8-m9-evidence.py
./scripts/package-androswitch.ps1 -Stage m9-candidate -EvidencePath reports/m8-m9-candidate.json
```

The recorder requires a clean committed implementation, passing/hash-matching
logs, at least 12 cycles per required case and a matching successful NRO build
receipt. It preserves missing Horizon/hardware gates in the candidate manifest.

## Remaining Horizon gate

Desktop Ryujinx testing stays explicitly opt-in after the reported disappearance
of unrelated app windows. The cause remains undetermined. The prepared command
below has **not** been run for this candidate:

```powershell
./scripts/test-ryujinx-launcher.ps1 -Scenario m9 -AllowDesktopEmulator
```

It prepares eight APKs and checks 24 sessions, persistent preferences/database,
the original AOSP layout, GC, native graphics/audio and complete teardown back
to the console. The existing workflow/startup/quiescence suites are also
required again for release. Script availability is not passing evidence.

The candidate ZIP uses the exact recorded NRO hash and bundles seven positive
sample APKs. It excludes Vector Pinball, Mekorama and negative test fixtures.
Copy `switch/` onto the SD while retaining existing user data. Test preferences
and notes twice, check Plus return, then repeat Java/native switching. Retain
the package manifest and logs when recording physical results.

## Contract references and attribution

- [AOSP API26 SharedPreferencesImpl](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-8.0.0_r1/core/java/android/app/SharedPreferencesImpl.java), Android Open Source Project, Apache-2.0.
- [Android SharedPreferences API](https://developer.android.com/reference/android/content/SharedPreferences) and [SQLiteDatabase API](https://developer.android.com/reference/android/database/sqlite/SQLiteDatabase).
- [SQLite official source](https://www.sqlite.org/download.html) and [public-domain statement](https://www.sqlite.org/copyright.html); exact source/archive hashes are in `third_party/sqlite.lock.json`.
- [Original AOSP HelloActivity](https://github.com/aosp-mirror/platform_development/tree/android-8.0.0_r1/samples/HelloActivity), Copyright 2007 The Android Open Source Project, Apache-2.0.
- [Vector Pinball](https://github.com/dozingcat/Vector-Pinball), dozingcat/contributors, GPL v3. Evaluated locally, not redistributed.

RustyDroid by **nadabgfccvd and contributors** remains the runtime foundation.
See [credits](../CREDITS.md), [notice](../NOTICE.md) and packaged dependency licenses.
