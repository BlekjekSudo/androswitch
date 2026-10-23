# Testing and evidence

Test the affected API/ABI contract and record the backend that actually ran
it. Host execution, a Horizon emulator run and a physical Switch run are
different evidence. The current M1–M4 physical-device status is **not run**.

## Host checks

From the repository root in PowerShell 7:

```powershell
./scripts/check-m0.ps1
./scripts/check-m1.ps1
cargo test -p as-package -p as-native --locked
cargo test -p as-native --test loader --locked -- --include-ignored
cargo test -p rd-jni --locked
cargo test -p rd-ndk --features native --test native --locked
python -X utf8 scripts/check_md_links.py
git diff --check
```

| Gate | Scope |
| --- | --- |
| `check-m0.ps1` | Inspector/platform/CLI tests, pinned real APK, APK/DEX regressions, std/no_std DEX lookup, portable checks, clippy and format |
| `check-m1.ps1` | Java fixture build, portable VM/GC/structured-error tests, real APK results against JVM, portable core and FFI lint |
| Markdown link check | Current documentation and retained upstream references |
| `git diff --check` | Patch whitespace/integrity |

M0 downloads the hash-pinned APK if absent. M1 needs the JDK and Android SDK
from [building](BUILDING.md). After a custom SDK fixture build, use
`check-m1.ps1 -SkipFixtureBuild`. Logs and command receipts are generated under
`target/evidence/`; the original published evidence remains in
[reports](../reports/m1-core.json).

The ignored native-loader tests additionally require
`scripts/build-m3-fixture.ps1`. The `rd-ndk/native` integration test requires
`scripts/build-native-oracle-fixtures.ps1` (Windows NDK), or the original
`golden/native/build.sh` (GCC plus Linux ARM64/ARM cross compilers), and the
optional host Python/Unicorn installation. Its x86/ARM32 fixtures test the
inherited host oracle; device support remains ARM64 only. Device builds
exclude Python and Unicorn.

### Cargo feature isolation

Run the portable VM tests in isolation:

```powershell
cargo test -p as-switch-smoke --features portable-tests --test portable_core --locked
```

The real-fixture test is ignored by default because its APK must be built.
`check-m1.ps1` builds it and explicitly includes the ignored test.

Cargo features are additive. A workspace-wide test can pull `rd-vm/std` in
through legacy dev-dependencies and test the desktop VM instead. The portable
tests assert which VM variant is linked; do not hide that failure. The actual
freestanding target build is also required to establish device portability.

### Focused upstream VM regressions

The recorded M1 regression subset passed 227 cases:

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p rd-vm --lib --test vm_exec --test gc --test gc_generations --test collections_gc --locked
```

The environment variables apply to the current shell's build. Disabling debug
symbols reduces memory use; test semantics/debug assertions are retained.

## Ryujinx E2E

```powershell
./scripts/test-ryujinx-m1.ps1 -Ryujinx "D:/path/to/Ryujinx.exe"
```

For a dated-toolchain/custom-SDK build, follow [building](BUILDING.md) and use
`-SkipBuild -SkipFixtureBuild`. A fresh run still gets a new session ID.

The test asserts 12 JVM reference results and six structured negative cases
against both the portable host and actual emulator output. It also checks the
real F-Droid DEX parser counts/checksum and allocator proof. Session checking,
old-report archiving and an exclusive SD lock prevent stale results from
being accepted. See [M1 E2E](M1_E2E.md) for all steps and budget limits.

## CI and the upstream baseline

M2–M4 tests add failure-injected package updates, persistent VFS/restart/OBB
checks, stripped ELF/dependency/TLS/constructor execution, native oracle
comparison and JVM-backed Java/Kotlin JNI cases. See [M2–M4](M2_M4_E2E.md).
Use `CARGO_BUILD_JOBS=1` on memory-constrained hosts and run the host and
device builds sequentially. An out-of-memory/compiler failure is a failed
attempt; only a subsequent complete run can provide a passing receipt.

[The AndroSwitch workflow](../.github/workflows/androswitch.yml) runs host
inspection/parser/portable checks on Linux and Windows. It does not run
Ryujinx or a physical Switch. The inherited full-Linux suite is a separate
nonblocking reference job; [upstream audits](../.github/workflows/ci.yml)
remain manually triggered.

The immutable RustyDroid fork base has five known failing MCP tests in the
recorded Linux baseline. Cargo stopped at that failing binary, so later
upstream tests were not reported as passing. The Windows multi-gigabyte
sparse-file test is excluded from the local parser gate. Neither finding is
hidden or counted as a successful test. See [M0 baseline](M0_BASELINE.md).
MCP work is outside the current Switch milestone scope.

## Recording results

For a milestone or compatibility change, record the source commit, exact
command, fixture source/revision/hash, backend, result and relevant artifact
hashes. Preserve failures, and mark unavailable tests as not run. Do not infer
full app compatibility from parsing, engine detection or one static method.

Tracked compact reports live in `reports/`; large outputs, generated APKs,
libraries and NROs stay ignored. External sources are frozen in
[the corpus lock](../fixtures/corpus.lock.json). Check [credits](../CREDITS.md)
when importing or extending a fixture/component.
