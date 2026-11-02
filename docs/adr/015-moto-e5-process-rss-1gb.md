# ADR 015: moto-e5 process RSS cap raised from 512 MB to 1024 MB

- **Status:** accepted. This is an owner decision (VortexBlade, 2026-10-25, TempChat 8873: *"quero que o rusty seja capaz de rodar apk de até 1GB"*).
- **Date:** 2026-10-25.
- **Scope:** `data/devices.toml` (`moto-e5.ram_process_cap_mb`), plus the spec, README and ROADMAP text that quotes the cap.
- **Related:**
  - ADR 008 (15 GB APK / cold-start budgets);
  - #216 (paged inspect: APKs over 512 MiB open without whole-file reads);
  - Appendix A2 of RustyDroid.md.

## Context

The `moto-e5` hardware-floor profile capped the RustyDroid process at **512 MB RSS** (`rd perf` fails closed above it; Sentinel perf campaigns read the same profile). The owner wants RustyDroid to run APKs of up to **1 GB**.

#216 already removes the *file-size* barrier: archives over 512 MiB are opened in paged mode, up to 16 GiB, and are never read whole. What remains is the *process* budget. Large real apps (big DEX sets, large decoded resources, native libraries) can need more than 512 MB of resident memory while running, even though paging keeps the APK bytes themselves out of RSS.

## Decision

- `moto-e5.ram_process_cap_mb` goes from 512 to **1024**.
- `ram_app_heap_mb` stays **256**. That is the guest Dalvik heap, which matches `dalvik.vm.heapgrowthlimit` on a 2 GB device, and it is unchanged.
- The ADR 008 *target* of < 380 MB VmHWM for the 15 GB fixture still stands as an engineering target. Only the fail-closed ceiling moves.
- `Apk::MAX_INPUT_BYTES` (512 MiB whole-file read ceiling) is **not** changed. Larger files take the paged path from #216, which keeps reads bounded.

## Consequences

- **Honesty note:** a real Moto E5 has 2 GB of RAM with about 1 GB used by the OS. A 1024 MB process ceiling therefore no longer guarantees the process would fit on the physical device alongside the OS. The profile is now a *budget ceiling* chosen by the owner, not a faithful replica of free memory on the device. Reports that claim "fits moto-e5" should quote the measured VmHWM, not only pass/fail.
- `rd perf` on `moto-e5` now fails closed above 1024 MB instead of 512 MB.
- Docs quoting the cap (RustyDroid.md §Piso/A2, README, ROADMAP) are updated in the same change. Historical ADR 008 text keeps its original 512 MB wording as a record of that decision.
