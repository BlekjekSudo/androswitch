# M1 portable DEX smoke (partial milestone)

Historical parser-only proof. The later APK/interpreter gate is now recorded
in [M1 E2E](M1_E2E.md) and supersedes this partial milestone status.

The actual upstream DEX parser has a `no_std + alloc` path. Its host path keeps
the original lazy class index; its portable path builds a deterministic index
at parse time. Parser and disassembler implementations remain shared.

This standalone smoke tests Rust allocation, file input from the SD filesystem,
monotonic timing and parsing a real DEX through a libnx C ABI. It does not run
Dalvik instructions or Android applications. Full M1 also requires the real
APK-parser/interpreter port and emulator comparison; those were pending at
the time of this initial parser-only observation.

## Observed result (2026-10-23)

The NRO ran in Ryujinx and wrote a real SD-filesystem report. The verifier
matched the host's DEX 037 counts: 11,168 classes, 65,003 methods, 62,734 strings,
9,297,608 file bytes and a valid checksum. Allocation checksum: 32,640.
File read plus parse/timing path took 1,074,602 microseconds in this emulator
run; that number is not a physical-Switch performance measurement.

The smoke now requests a fixed 256 MiB heap. Its first emulator attempt used
libnx's automatic heap size and failed with a host paging-file error before
`main`; the bounded-heap attempt passed. This is a smoke-test budget, not a
future Android-app memory budget. See `reports/m1-dex-smoke.json` for hashes.

## Build and prepare

On Windows, with nightly-2026-08-26 (`nightly` is accepted when it is that exact
dated toolchain), rust-src, and the existing WSL devkitPro installation:

```powershell
.\scripts\build-switch-smoke.ps1
.\scripts\prepare-switch-smoke.ps1
```

The package is `local-artifacts/m1/package/switch/androswitch-smoke/`. It contains
the smoke NRO and the first DEX extracted from the checksum-pinned F-Droid APK.
The host receipt records expected DEX counts/checksum and APK/DEX/NRO hashes.
The application is a parser test, not an F-Droid port.

## Run and compare

Copy the package to the emulator's SD filesystem (or physical SD), then run its
NRO. It writes `switch/androswitch-smoke/smoke.json`. Press Plus to exit normally.
Verify an actual output file against the host receipt:

```powershell
.\scripts\verify-switch-smoke.ps1 -ReportPath <path-to-actual-smoke.json> -Backend horizon_emulator
```

Use `physical_device` only for a report actually obtained from a Switch. Building
the NRO or preparing expected host output does not satisfy either execution gate.
