# Using the M9 validation candidate

This is a one-NRO validation candidate, not the completed offline v0.1 release.
Host and Ryujinx evidence do not establish physical Switch compatibility.
The current std migration ZIP includes one unified AndroSwitch Conformance APK.
Its 219 sensor-free cases cover the recorded project contracts; it is not
official Android CTS or a claim of complete Android compatibility. The current
std binary passed 216 cases in Ryujinx; three PCV clock queries were unavailable.
See the [std qualification](HORIZON_STD_MIGRATION.md) before hardware testing.
The user reported that the four bundled samples install/open on a real Switch
and the native triangle animation and beeps work. The user subsequently confirmed
that the display handoff fix resolves Plus return and the startup stall; see the
[hardware feedback record](HARDWARE_FEEDBACK.md).
M1–M7 hardware testing is user-confirmed. This newer file-backed/M8/M9 build
still needs its own Horizon/hardware run; see [candidate status](M8_M9_E2E.md).
The runtime builds on **RustyDroid by nadabgfccvd and contributors**; retain
the packaged credits, license notices and source link when redistributing it.

## Install and open

1. Extract `switch/` from the ZIP to your SD card. Keep an existing `userdata/`,
   `Android/`, `installed/` and `logs/` directory when updating the runtime.
2. Start `switch/androswitch/androswitch.nro` with application/title override.
   The runtime measures loader/process heap capacity and requires title mode;
   see [Horizon memory policy](HORIZON_MEMORY.md).
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
| Runtime reports | `logs/session-report.json`, `teardown.log`, `display-handoffs.log`, `display-error.json`, `last-interrupted.json`, `teardown-failure.json` |
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
- **Persistent Preferences:** typed XML settings and a launch counter.
- **Persistent Notes:** native SQLite transactions, cursor reads, reflection and
  a count that increases after reopening.
- **AOSP HelloActivity:** original external Java/layout source displaying Hello,
  World. IME/editable-buffer input is outside the tested rendering scenario.

The samples use ordinary Android SDK/NDK APIs. Their source and build scripts
are in this repository; they have no runtime package-name exceptions.
The negative API and uncooperative-worker fixtures are development tests and
are not included in the normal ZIP.

## Compatibility limits and errors

This alpha supports the explicitly tested framework/NDK subset, not all API 26
classes. Device APK containers are file-backed and capped at **512 MiB**.
Materialized metadata/resource entries are capped at 64 MiB. Native extraction
has a separate budget up to 128 MiB; DEX admission accounts for raw and decoded
metadata with an adaptive parser ceiling up to 256 MiB. See [current startup
contracts](M9_EXTERNAL_APPS.md) for exact limits and pending verification. A larger
container can still exceed decoded memory or require missing APIs. Assets/OBB
use positioned, bounded reads. Native libraries must be ARM64; DEX-only APKs
need no native ABI. M8 SQLite is limited to 8 MiB per database and does not
implement Android's full database API/connection pool. See [M8/M9 limits](M8_M9_E2E.md).

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

Failed initialization also owns its VM/JNI state until bounded cleanup finishes.
Missing native dependencies can return to the console with their original error;
an initializer that leaves a live worker still requires process exit. This does
not make the rejected APK compatible. See [startup failures](STARTUP_FAILURES.md)
for the earlier Mekorama diagnosis and the historical 64 MiB container budget.

Interactive launches write a `running` session report before executing the APK.
Shutdown flushes each phase to `logs/teardown.log` before that operation, including
GPU completion on the EGL owner thread, callbacks, joins, mapping release and
launcher framebuffer reopening. `launcher_ready` means a console frame was
actually submitted after cleanup. A missing final report cannot by itself locate
a crash; keep the phase trace and the Atmosphère error report as well.

On the native graphics return path, `launcher_stable` records eight successful
CPU queue submissions, with queue slots and stable nvmap/fence service IDs in
`display-handoffs.log`. Display failures produce `display-error.json` instead of
using libnx's fatal framebuffer convenience APIs. GPU readback dumps are enabled
only for scripted tests, to avoid diagnostic SD writes during normal startup.
