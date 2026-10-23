# M7 physical Switch feedback — 2026-10-24

Evidence here is user-reported in the development conversation. No device crash
dump, Atmosphère result code or collected hardware trace was available during
the fix. The exact hardware, firmware and homebrew launch mode are unrecorded.
This does not certify the general APK corpus or the replacement build.

## Original alpha

The delivered original alpha NRO SHA-256 was
`3d84164e6ba3227d6916e26bafff91f7485a817925f5450b85f5811c3738236b`.
The user reported:

- All four bundled APKs install and open on a physical Switch.
- Native Graphics and Audio plays beeps and continuously animates its triangle.
- Reinstalling identical APKs unnecessarily adds revisions.
- Opening after installation requires exiting first, as experienced by the user.
- Plus in Native Graphics and Audio produces the usual Switch crash screen.
  `logs/session-report.json` was absent.

The previous interactive path only wrote session reports for errors or completed
test scripts. Absence of that file therefore does not identify the crash stage.

## Changes for retesting

Identical update requests now compare APK SHA-256 and the chosen entry Activity,
verify the stored copy and reuse its revision without filesystem mutations.
Changed APKs and reinstalls after uninstall still commit new revisions. Data
retention and atomic failure behavior stay covered by package tests.

After successful install the launcher selects the app in Installed. A opens it
immediately. Repeating an identical install displays Already installed.

EGL context unbinding now waits for outstanding GPU work with `glFinish` on the
thread that owns the current context, before context/surface/channel destruction.
This addresses an asynchronous teardown risk; it is a candidate fix, not a
confirmed explanation of the reported hardware crash. The pinned
[Mesa Switch driver](https://github.com/devkitPro/mesa/blob/switch-20.1.0-rc3/src/egl/drivers/switch/egl_switch.c)
submits swaps asynchronously and tears down the Nouveau screen at termination.

Every shutdown phase is independently flushed to `logs/teardown.log` before the
operation. Native lifecycle callbacks, EGL completion/destruction, pthread joins,
VM destruction, code/TLS release and launcher reopening have distinct phases.
The overall watchdog allows 10 seconds for GPU retirement, bounded joins and SD
logging. Failure still exits the process; the runtime never reuses live guest code.
Interactive launches create their session identity/report before APK execution.

Regression commands (retain the original M6/M7 baseline report):

```powershell
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-launcher.ps1
./scripts/test-ryujinx-launcher.ps1 -Scenario quiescence
python scripts/record-hardware-feedback-evidence.py --implementation-commit (git rev-parse HEAD)
./scripts/package-androswitch.ps1 -EvidencePath reports/m7-hardware-feedback.json
```

The compact follow-up report records exact NRO/source/fixture hashes, receipts
and the synthetic Plus teardown phases. Its physical-device status remains
pending retest. The original JVM/VM baseline was not rerun for this fix.

Focused host verification passed four package tests (including 40 failure
boundaries and no-write identical installation), the launcher workflow, both
clippy commands, Rust/C formatting and internal Markdown links. Ryujinx passed
the 65-command console workflow with ten completed app sessions and one expected
missing-API error. Both native sessions reached GPU completion, EGL termination,
thread join and resource release, then returned to the console. The stuck-worker
test still required process exit and recovered the interruption notice on reboot.

## Second hardware result: shutdown completed, crash persisted

The user supplied `session-report.json` and `teardown.log`. Their hashes and
parsed contents are retained in `reports/hardware-native-plus-20261024.json`.
The session was `console-4`, revision 1 of Native Graphics and Audio. Its status
is `stopped`, and every shutdown phase completed through `launcher_ready`.
The other three sample APKs return safely. The system message was “the software
was closed because an error occurred”, with no numeric exception code supplied.
The user also observed an initial stall before graphics became responsive.

This disproves the sufficiency of the first GPU-retirement candidate. The logs
prove one launcher submission, not continued display stability. The fault could
be asynchronous; no crash dump establishes its exact cause.

The next candidate retains one process-owned reference to nvdrv, nvmap, fences
and GPU services throughout CPU/EGL transitions. Both libnx framebufferClose and
Nouveau's device destructor otherwise release those service guards between
renderers. CPU presentation now uses three buffers, checks dequeue results,
slot bounds, compositor fences and queue results before treating a frame as
submitted. This avoids unchecked slot indexing and fatal convenience-function
errors. The sector copy follows fincs/libnx, retains its ISC license, and is
checked against independent Tegra byte-address calculations and allocation guards.
The ownership paths are in the pinned
[libnx framebuffer source](https://github.com/switchbrew/libnx/blob/v4.12.0/nx/source/display/framebuffer.c)
and [Nouveau device source](https://github.com/devkitPro/libdrm_nouveau/blob/master/source/nouveau.c).

Two GPU diagnostic dumps at frames 2/30 are now enabled only for scripted tests.
Their readback/SD writes previously ran during ordinary interactive launches,
adding startup stalls. The APK fixture itself also performs a full-frame read;
removing runtime dumps does not promise that all startup work disappears.

The emulator workflow now waits 60 launcher frames after each native Plus return.
The first eight queue submissions per return are recorded in
`logs/display-handoffs.log`; they must keep valid slots and stable service IDs.
Errors are recorded in `logs/display-error.json` with the operation/result/slot.
`launcher_stable` follows eight successful submissions. This remains an emulator
proof until the replacement NRO is retested on the physical Switch.

Use `--display-handoff` when recording this candidate, then package with
`-EvidencePath reports/m7-display-handoff.json`. Prior reports remain unchanged.

The candidate also builds the no_std device path and passes the four independent
framebuffer-copy address/guard cases and both clippy commands. The extended
67-command Ryujinx workflow verifies the two native returns with valid slot
0/1/2 cycling, stable service handles and 60 further launcher frames each;
the negative worker/next-boot recovery test remains required separately.

## Hardware retest

Update `androswitch.nro` from the new package while retaining installed APKs and
data. Install the same APK twice: its revision must remain stable and it must be
launchable immediately. Run Native Graphics and Audio, wait for animation/beeps,
then press Plus. A successful return ends `logs/teardown.log` with
`launcher_stable` after eight successful launcher frames; relaunch the native app
and return again.

If it crashes, preserve the last phase in `teardown.log`, `session-report.json`,
`display-handoffs.log`, `display-error.json` if present, `pending-session.json`
and a system crash report if available. Reopen homebrew to show
the interrupted-session notice. At the time of that candidate, hardware retesting
was pending; the subsequent report below supersedes that status.

## User confirmation and external APK failure

The user subsequently confirmed that the display handoff fix resolves the native
sample's Plus crash and startup stall. The delivered NRO was expected to be
`449917f0f53a88d6c66ecf09aa5b820e3612e7ec9bd2192aa839f3c59da4ad32`;
the running binary was not independently hashed. This is user-reported sample
evidence, not a physical-device run of the full milestone corpus.

The next report concerns Mekorama 1.7.2, not the previous native sample. Its fresh
session report identifies a missing `libstdc++.so` dependency during static class
initialization. The accompanying display/teardown traces still describe the prior
native sample and show a successful return. They do not demonstrate a new GPU
failure in Mekorama. See [startup diagnosis and recovery](STARTUP_FAILURES.md).
