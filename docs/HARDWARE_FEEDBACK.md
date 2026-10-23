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

## Hardware retest

Update `androswitch.nro` from the new package while retaining installed APKs and
data. Install the same APK twice: its revision must remain stable and it must be
launchable immediately. Run Native Graphics and Audio, wait for animation/beeps,
then press Plus. A successful return ends `logs/teardown.log` with
`launcher_ready`; relaunch the native app and return again.

If it crashes, preserve the last phase in `teardown.log`, `session-report.json`,
`pending-session.json` and the Atmosphère crash report. Reopen homebrew to show
the interrupted-session notice. The updated build is **pending hardware retest**.
