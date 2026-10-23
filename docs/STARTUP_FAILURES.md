# APK budgets and failed initialization

The M7 alpha loads the complete APK container into memory. Its 64 MiB cap reserves
headroom within the 256 MiB homebrew heap for decoded DEX, VM objects, extracted
native images, executable mappings and media. It is a current implementation
budget, not an Android APK format or Switch file-size limit. Host inspection can
accept 512 MiB. Supporting larger APKs requires using positioned/streamed storage
through the package loader and runtime, then measuring peak memory. Increasing
the constant alone does not establish that larger APKs can launch safely.

OBBs remain opaque files read in bounded chunks. Moving an APK into an OBB does
not change how Android code/resources are loaded.

## Mekorama 1.7.2 diagnosis

The supplied APK is 13,099,584 bytes (about 12.49 MiB), SHA-256
`9b8a5ef80de69095769c679bf65d5b339564478736f78b24700a4f0826bfec01`.
Its package is `com.martinmagni.mekorama`, versionCode 44, minSdk 21, targetSdk 33,
with ARM64 libraries. These static checks permit installation; they do not prove
that the required Android APIs and native contracts exist.

The physical Switch session report identifies:

```text
ExceptionInInitializerError
libBarcodeScannerLib.so: dependency requires libstdc++.so
at Lcom/martinmagni/mekorama/Mekorama;-><clinit>()V
```

ELF inspection confirms that dependency. `libmekorama.so` also depends on the
barcode library, GLES2, OpenSL ES, Android, logging, math, dl and libc. This is the
first observed blocker; later requirements have not been executed. The runtime
does not currently supply the requested legacy C++ library. Adding its name to
an allowlist or fabricating successful imports would not implement its ABI.

The accompanying `teardown.log` and `display-handoffs.log` have no session ID.
They show NativeActivity shutdown, `launcher_stable` and eight valid CPU
submissions, consistent with an earlier Native Graphics and Audio return.
Mekorama's error occurs before its graphics initialization, so these traces are
treated as stale; they do not establish that the earlier display bug recurred.

## Recovery contract

Previously, a launch error after creating a native session dropped the app while
the retained JNI session still existed, and the C loop exited the Horizon process.
Even a missing dependency that had never executed native code took this path.

Initialization now retains an app owner on failure. C closes that owner through
the existing watchdog and quiescence checks before returning to the console.
Abort cancels Java tasks and closes PCM resources without running lifecycle
callbacks on partly initialized Java objects. NativeActivity allocation is owned
before entering native onCreate. A worker or media callback that cannot stop
keeps its VM/mappings rooted and requires process exit. The original startup
error is retained alongside any cleanup error.

Mekorama remains **unsupported**. Safe error reporting is separate from gameplay
compatibility. No package-specific runtime behavior or APK modification is used.

## Reproduce

Desktop emulator runs are now opt-in. During this investigation the user reported
that other application windows disappeared while Ryujinx testing was active;
the cause is undetermined. Testing was stopped, the original SD directory was
restored, and the completed console report was checked with the shared validator
without reopening any app. The normal stuck-worker scenario was not rerun on
the latest NRO; its prior-build result is retained separately. The failed-onload
worker and Mekorama recovery scenarios did complete on the latest NRO.

The commands below intentionally enable desktop emulator execution. To validate
an existing interrupted workflow without launching apps, use
`scripts/verify-saved-launcher-run.ps1 -RunId <recorded-run-id>` instead.

```powershell
./scripts/build-startup-failure-fixtures.ps1
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario startup-failure -ExternalApk D:/Download/mekorama-1-7-2.apk
./scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario startup-quiescence
./scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator
./scripts/test-ryujinx-launcher.ps1 -AllowDesktopEmulator -Scenario quiescence
python scripts/record-startup-recovery-evidence.py --implementation-commit (git rev-parse HEAD)
./scripts/package-androswitch.ps1 -EvidencePath reports/m7-startup-recovery.json
```

The startup suite checks Java static initialization, NativeActivity entry lookup,
the same missing dependency with a different package name, and the unmodified
external APK when supplied. Successful Java → native graphics/audio → Java
sessions follow in the same runtime process. The negative startup test starts a
worker in JNI_OnLoad, returns an invalid JNI version, and requires process exit
and next-boot interruption reporting. External APKs/libraries are excluded from
Git and release packages. Exact inputs/results are recorded separately in
`reports/m7-startup-recovery.json`; hardware recovery remains pending retest.

Static inspection used the project's `asctl`, ZIP extraction and NDK
`llvm-readelf -d --dyn-syms`. REA's artifact inventory was available, but its
Android semantic inspector was unavailable because the configured Jadx helper
was absent. Archive inventory is not execution evidence; shell/SDK output is
recorded as such. The original supplied logs and APK hashes are retained in
[the hardware/static evidence record](../reports/mekorama-startup-20261024.json).
