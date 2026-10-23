# Using the M7 development alpha

This is the first one-NRO package, not the offline v0.1 compatibility release.
Host and Ryujinx evidence do not establish physical Switch compatibility.
The user reported that the four bundled samples install/open on a real Switch
and the native triangle animation and beeps work, but Plus crashes there.
This build's shutdown changes await hardware retesting; see the
[hardware feedback record](HARDWARE_FEEDBACK.md).
The runtime builds on **RustyDroid by nadabgfccvd and contributors**; retain
the packaged credits, license notices and source link when redistributing it.

## Install and open

1. Extract `switch/` from the ZIP to your SD card. Keep an existing `userdata/`,
   `Android/`, `installed/` and `logs/` directory when updating the runtime.
2. Start `switch/androswitch/androswitch.nro` with application/title override.
   This build uses a 256 MiB homebrew heap; applet mode is not the tested target.
3. Press R for APKs, select a sample and press A to install. The console selects
   that app in Installed immediately; press A again to launch it.
   Plus requests a return to the console.

There is one product NRO. No Android partition, Python installation, Unicorn
worker or Linux guest kernel is required on the SD card.

## Files and controls

| Item | Location or behavior |
| --- | --- |
| Import APKs | `switch/androswitch/apks/*.apk`; Minus rescans |
| Installed APK copies | `installed/<package>/<revision>/package.apk`; managed by the runtime |
| Private saves | `userdata/<package>/`; maps `/data/user/0/<package>` |
| App external data | `Android/data/<package>/` |
| OBB files | `Android/obb/<package>/`; opaque files, no automatic extraction |
| Shared pictures/downloads | `DCIM/`, `Download/`; aliases `/sdcard` and `/storage/emulated/0` |
| Runtime reports | `logs/session-report.json`, `teardown.log`, `last-interrupted.json`, `teardown-failure.json` |
| Lists | L/R, Up/Down; touch selects, a second tap opens |
| Install/update/launch | A; identical APK updates reuse the verified revision; changed APKs get a new revision and retain data |
| Details/back | X/B |
| Uninstall | Y, then A to confirm; saves are retained |
| Clear internal data | ZR, then A to confirm; separate from uninstall |
| Exit app / exit console | Plus returns to the console / exits homebrew |

Android's A/DPAD/Back keys are mapped from Joy-Con controls for the supported
UI. Touch events are scaled to the Android viewport. NativeActivity receives
window-sized touch coordinates through its input queue. Physical controls and
handheld/docked modes still need device validation.

## Included sample APKs

- **Java UI and Canvas:** XML layout, save counter, delayed callback, a second
  Activity and a custom Canvas view. Press Increment to persist a value.
- **Kotlin UI:** a button whose real Kotlin DEX callback changes its label.
- **Mixed Java and Native Audio:** reads one asset through Java and native,
  writes PCM via Java AudioTrack and AAudio, then OpenSL ES queue callbacks.
- **Native Graphics and Audio:** NativeActivity draws a moving triangle through
  GLES2, plays PCM and changes its background in response to input.

The samples use ordinary Android SDK/NDK APIs. Their source and build scripts
are in this repository; they have no runtime package-name exceptions.
The negative API and uncooperative-worker fixtures are development tests and
are not included in the normal ZIP.

## Compatibility limits and errors

This alpha supports the explicitly tested framework/NDK subset, not all API 26
classes. Device APK containers are currently memory-resident and limited to
64 MiB. Assets decompress incrementally; external OBB reads are positioned and
bounded. Native libraries must be ARM64; DEX-only APKs need no native ABI.

The initial audio format is 48 kHz stereo PCM16, one output at a time.
AAudio callback streams/low-latency modes, audio input, codecs and general
OpenSL effects are unavailable. EGL/GLES2 and the supported NativeActivity
callbacks do not establish complete `android_native_app_glue`/FD support.
Adaptive/vector/JPEG icons use a fallback; bounded non-interlaced RGB/RGBA PNG
icons are decoded from APK resources.

Missing API/symbol/relocation errors are compatibility gaps, not successful
launches. Share the exact report and APK/source version when reporting one.
Signature presence is detected; APK signatures are not cryptographically
verified. Native code shares the Horizon homebrew process; use trusted APKs.

Normal switching waits for native workers and media callbacks, then releases
audio, graphics, VM references, guest TLS, synchronization objects, heap and
code mappings. A stuck thread or unsafe teardown causes a process exit.
Reopen homebrew; the one-shot launch request is consumed and the console
records the interrupted package/session. Seamless recovery from a native crash
within the same process is not promised. Do not delete the logs before reporting it.

Interactive launches write a `running` session report before executing the APK.
Shutdown flushes each phase to `logs/teardown.log` before that operation, including
GPU completion on the EGL owner thread, callbacks, joins, mapping release and
launcher framebuffer reopening. `launcher_ready` means a console frame was
actually submitted after cleanup. A missing final report cannot by itself locate
a crash; keep the phase trace and the Atmosphère error report as well.
