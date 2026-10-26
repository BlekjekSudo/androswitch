# Standalone Horizon Rust `std` proof

This experiment embeds the real Rust standard library in a libnx NRO. It is
outside the product workspace and does not enable `std` in `as-vm` or replace
the Android runtime. The product remains a RustyDroid fork, with credits in
[CREDITS.md](../../CREDITS.md).

The pinned compiler is `nightly-2026-08-26`, commit
`787af2b8c80638c51a4fc8e44f84e6891f243ec7`. The custom target starts from
Rust's `aarch64-nintendo-switch-freestanding` definition and adds Unix/newlib
selection. Standard library Rust sources are unmodified. Selecting these cfgs
does not make Horizon a Linux guest.

`prepare-horizon-std.py` verifies the libc 0.2.189 archive checksum, extracts it
under ignored `local-artifacts`, then adapts the actual devkitA64 file/pthread
ABI. `abi.c` compares 33 sizes, alignments, offsets and constants with Rust at
runtime. OS calls use SDK libsysbase/libnx; `support.c` provides the missing
page-size query and actual CSRNG entropy transport. Its `pthread_detach` wrapper
observes the original return value, preserving any error. A failed detach is
joined by the harness after its worker finishes; this recovery is not a working
`std` detach implementation.

From the runtime repository in PowerShell, with Rust/Cargo and Python on PATH:

```powershell
./scripts/build-horizon-std.ps1
./scripts/test-horizon-std-ryujinx.ps1 `
  -Nro local-artifacts/horizon-std/candidate/androswitch-std-probe.nro
python -X utf8 scripts/report-horizon-std.py `
  --build local-artifacts/horizon-std/candidate `
  --run local-artifacts/horizon-std/runs/<session-id> `
  --output local-artifacts/horizon-std/report.json
```

The emulator script requires the previously pinned headless executable by hash,
uses an exclusive test lock, preserves its own SD folder, and stops only owned
processes. It runs three fresh guest processes: the contract suite, persistence
restart and an intentional panic with `panic=abort`. It never opens the stock
GUI. The emulator may stay alive after the guest exits; the owned host process
is then stopped after the guest's return/atexit evidence is captured.

The report accounts for 28 suite contracts, one persistence restart, two normal
shutdowns and one intentional abort. Missing entries are not passes. The
32-contract probe is not the Rust test suite, a complete libc ABI audit, Android
API parity, hardware validation, or an Android APK startup test. Neither full
unwinding nor guest Bionic TLS/host TLS transitions is established here.

Credits: Rust project contributors (`std`, MIT/Apache-2.0), rust-lang/libc
contributors (MIT/Apache-2.0), devkitPro/newlib and switchbrew/libnx contributors
provide the implementations. The experiment's adapter and test code belongs to
AndroSwitch. License files remain in the verified source archive/extraction;
no third-party source archive or binary is checked into the repository.
