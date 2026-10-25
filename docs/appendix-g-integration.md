# Appendix G integration validation — 2026-10-20

## Scope and provenance

AI-assisted conflict resolution and integration of `feat/appendix-g-registry-20261014`
(`30411ec12599db191b3b11f508d4448a0978fb54`) with main after PR #160
(`7ce8ba3a9849cdef09fee988a381dac591f5b8c4`).

Local integration branch: `feat/appendix-g-integration-20261020`.
This report records **local validation**, not new-head GitHub CI or human approval.
The older [2026-10-14 report](appendix-g-validation.md) is historical and its
416-test count is not the current integrated result.

## Problems resolved

- All six textual conflicts reconciled, retaining current main's CI/native/JIT
  work, documentation/history and workspace version 0.9.0.
- CLI/agent each have one feature table. Their existing `advanced-components`
  defaults and exact `native`/`jit` forwarding remain intact; LANG contracts are
  separately opt-in. Tests guard these invariants.
- MCP was 41 tools in this validation run (the catalog is 47 tools today, see docs/mcp-tools.md). On analyzed APKs, `app_info` includes all 28
  engine records without launching a VM; no tool was removed to satisfy the old 32-tool assertion.
- Generated catalog/spec/scoreboard distinguish unavailable registry adapters
  from the existing partial DEX and opt-in ELF/JNI routes. `partial_native`
  describes bounded existing ELF/JNI capability, not language attribution,
  Objective-C runtime availability, universal NDK or new engine execution.
- Optional engine analysis limits no longer break basic inspection. `inspect`/
  `app_info` return an explicit `not_analyzed` diagnostic; strict `engine detect`
  keeps its typed error. CLI/MCP tests use a 100,001-entry source-built ZIP64 APK.
- File catalog reads are bounded at 1 MiB plus one overflow-detection byte even
  when metadata is stale or the source is a growing/endless stream. Two tests
  exercise the read boundary; adapter tests still require `NOT_IMPLEMENTED`.

- MCP documentation generation now emits exactly one final newline, with
  an explicit regression assertion plus the unchanged exact-output check.
- Fixed 20 advisory Markdown issues, including a malformed JNI table and
  literal glob/bounds syntax; no lint rule was disabled.

## Local gates

| Gate | Result |
|---|---|
| Workspace all features, locked | 548 passed, zero failed; 1 explicitly ignored real-APK hook |
| Workspace default features, locked | 537 passed, zero failed; 1 explicitly ignored real-APK hook |
| Clippy all targets/all features | Passed with `-D warnings` |
| Rustfmt and whitespace | Passed |
| Default/JIT-only Clippy; native-only and JIT-only test variants | Passed |
| Pinned Markdown lint 0.23.3 / Node 22 | 29 files; zero issues |
| Nightly fuzz builds, rd-apk and rd-dex | Passed with cargo-fuzz 0.13.2 / nightly 1.101.0 (2026-10-19) |
| Minimal CLI and minimal VM | Passed |
| Rust 1.76 workspace/all features/locked | Passed |
| Generated engine/MCP reports, spec and compatibility table | Exact equality |
| Registry minimal, all features, each LANG individually | All 14 configurations passed |
| Explicit real-APK registry integration | Passed on Comprix 1.6.0, 448 ZIP entries |
| Real CLI detect vs inspect JSON | Equal; 28 rows; `execution_claimed: false` |
| Real MCP static detection | 41 tools in this run (47 today); equal CLI report; `running: false` after `app_info` |
| No-app MCP contract | 8/8 scenarios passed |
| JVM golden gate and real vectors | Gate rejects 0/149 cases; 150/150 vectors passed |
| Real native APKs | Five pinned APKs passed on each of x86_64, ARM64, ARMv7 |

The default/all-feature counts are separate configurations, not a sum of unique
tests. The ignored real-APK hook was also **executed explicitly and passed**; it
is not counted as executed in invocations that ignored it.

The first local JVM harness attempt stopped because the recovered environment
had JDK 11 while the unchanged harness requires `--release 17`. Installing JDK 21
fixed the environment; no vector was waived or threshold relaxed.

## Fresh real Comprix replays

| Mode | Completed cycles | RPC steps | Final item check |
|---|---:|---:|---|
| GC off, JIT off; independent navigation baseline | 2 | 17 | Not a typed-item campaign |
| GC on, JIT off; typed navigation | 20 | 122 | Arroz / 2 x kg / 1 item verified after forced major GC |
| GC on, JIT on; typed navigation | 20 | 122 | Arroz / 2 x kg / 1 item verified after forced major GC |

Both typed campaigns reached success and independent strict title/item audits
passed with zero errors. JIT-off telemetry showed zero compiled methods/native
calls; JIT-on showed two compiled methods and 83,776 native calls with zero
compile errors. Each typed campaign observed eight major collections. These
are completed fresh campaigns, unlike the older interrupted soak described in
previous integration evidence.

## Reproduce

```sh
bash golden/native/build.sh
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.76.0 check --workspace --all-features --locked
cargo build --release -p rd-cli --features native,jit --locked
bash golden/vm/run-golden.sh target/release/rd
bash golden-apks/native-gate.sh
RD_NATIVE_ABI=arm64-v8a bash golden-apks/native-gate.sh
RD_NATIVE_ABI=armeabi-v7a bash golden-apks/native-gate.sh
python scripts/comprix-regression.py --apk /path/to/comprix.apk --typed --gc on --jit on --cycles 20
```

Native tests require the documented cross GCC toolchains and Unicorn 2.1.4;
golden JVM vectors require JDK 17+ and the pinned D8 tool. APK binaries are
external, hash-pinned and never committed. See the existing native/GC/JIT ADRs
for supported subsets and limitations.

## What remains outside this evidence

- Publishing and the new integration head's GitHub checks remain separate from
  local gates. Prior-branch green checks do not certify this resolved tree.
- Human review of the updated integration remains mandatory before merging.
- No physical Moto E5 timing, full Android/NDK support, new framework runtime,
  camera flow, incremental/concurrent collector or ARMv7 native JIT is claimed.
- Nightly fuzz validation was build-only, not a 24-hour fuzzing campaign.
  Local CI-equivalent checks do not substitute the new GitHub checks.

Report finalized 2026-10-20T09:03:17+00:00. Raw CLI/MCP/native/JVM evidence and the complete sectioned
work log are retained in the accompanying workspace validation artifacts.
