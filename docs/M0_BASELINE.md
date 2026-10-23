# M0 baseline and verification

Date: 2026-10-23. Source base:
`6388281f66d254c8e01a0fdc7a5a9a30239bc762`. Working branch:
`codex/androswitch-m0`. Machine-readable evidence is in `reports/m0.json`.

## AndroSwitch gates: passed

`scripts/check-m0.ps1` completed all nine commands. There were 144 successful
test executions: 14 AndroSwitch feature tests, one explicit real-APK test,
127 inherited APK/DEX parser unit tests and one real class-index lookup test
each with std and no_std. The default suite's ignored external-APK test was
then run explicitly and passed. Formatting, clippy and no_std checks also pass.

The multi-gigabyte sparse-file test was excluded from the Windows parser run;
it is not included in those passing counts. No native APK execution is claimed.

F-Droid 2.0.2 (2000052) is checksum-pinned and inspected unmodified. It contains
three DEX files, minSdk 24 and targetSdk 37. AndroSwitch permits that minimum
SDK for evaluation. Inspection reports execution as unavailable.

Two generic upstream issues were corrected: Unix-only positional file I/O now
has a Windows path, and the unnamespaced manifest `split` attribute is read
correctly. Its sparse-file test builder now uses portable seek/write operations.

## Immutable upstream baseline: failed, retained

The first Windows attempt failed to compile because of Unix-only `FileExt`.
For a Linux baseline, the exact original Git revision was archived outside the
fork and tested using Rust 1.97.1. An initial WSL invocation ended before a test
receipt. The completed attempt used one build job and disabled test debug
symbols to reduce memory pressure; test/debug-assertion semantics stayed intact.

The unmodified baseline returned 101. Its first failing test binary reported
78 passed and five failed:

- `mcp::tests::swipe_type_key_form_flow`: expected app listener output after
  swipe-click, but logs were empty.
- `mcp::tests::tap_and_swipe_reject_coordinates_outside_i32`
- `mcp::tests::tools_fail_typed_without_app`
- `mcp::tests::unknown_method_notification_is_silent`
- `mcp::tests::wait_for_condition_and_caps`

The latter four encountered a poisoned shared test lock after the first panic.
Cargo stopped at that failing binary; remaining workspace tests were not
reported as passed. These are findings in the pinned upstream archive, not
regressions attributed to the fork. Keep them visible until their owning runtime
subsystems are ported and repaired.

Required CI gates cover the implemented AndroSwitch and parser paths. The
inherited full-Linux suite is a nonblocking reference job with these failures
documented; the old upstream-specific audit workflow is manually triggered.

## Reproduce

```powershell
.\scripts\check-m0.ps1
python -X utf8 scripts/check_md_links.py
wsl -d Ubuntu -- bash /mnt/d/Dev/Switch/AndroSwitch/scripts/test-upstream-baseline.sh
```

Complete logs and generated receipts are retained under `target/evidence/`.
Physical Switch verification is pending. M1's later APK/interpreter emulator
proof is recorded separately in [M1 E2E](M1_E2E.md); application lifecycle and
JNI execution remain unavailable.
