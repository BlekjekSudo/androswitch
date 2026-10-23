# M6/M7: native media and one-NRO console

AndroSwitch remains a history-preserving **RustyDroid** fork. The product uses
the real upstream parsers/interpreter/heap with the portable adapters described
in [M5](M5_UI_E2E.md) and [ADR 015](adr/015-portable-framework-and-session-boundary.md).
Compact source-pinned results live in `reports/m6-m7.json`; generated full
reports, captures and stdout remain under ignored `local-artifacts/`.
Follow-up hardware observations and shutdown/install changes are tracked in
[hardware feedback](HARDWARE_FEEDBACK.md); their regression evidence is separate
from the original source-pinned `reports/m6-m7.json` baseline.

## What the product implements

`platform/androswitch` links the portable Rust static library, libnx and Switch
Mesa into **one `androswitch.nro`**. The launcher uses `as-launcher` and the
existing immutable package manager. It scans APKs, installs/updates, lists
labels/PNG icons, launches, returns, preserves saves on uninstall, and offers
clear-internal-data separately. There are no package-name execution branches.

`as-framework` exposes real Activity/View state, incremental ZIP assets and a
PCM contract. `device_jni` maps Java AssetManager objects into the same native
asset service. Stored/DEFLATE reads use bounded output plus a 32 KiB dictionary,
verify declared size and CRC, and restart inflation for backwards seeks.
The alpha's whole APK container remains memory-resident, limited to 64 MiB on
device. External OBB reads already use positioned VFS I/O.

Standard NativeActivity class/manifest metadata chooses `android.app.lib_name`
and optional `android.app.func_name`. The activity holds a real global JNI
reference to its VM object, JavaVM/JNIEnv tables, shared asset manager and VFS
paths. Native callbacks create an input queue and native-window token; the EGL
boundary translates that token to the actual libnx window. GLES2 calls execute
through Mesa, with per-thread host/guest TLS switching. Default pthreads,
mutexes and conditions use owned libnx state instead of Bionic/newlib layout casts.

AudioTrack, AAudio blocking writes and OpenSL ES simple-buffer-queue callbacks
use the **same** real audout backend: 48 kHz stereo PCM16, one output at a time.
Submitted buffers stay valid until release or shutdown. OpenSL queue callbacks
execute on a worker after playback, never recursively inside Enqueue. Unsupported
formats, required interfaces and callback/low-latency AAudio configurations fail.
The API inventory is a bounded subset, not a complete audio framework.

## Session boundary

Normal hbloader chainloading keeps the same Horizon process. The product
therefore proves quiescence rather than claiming fresh-process isolation:

1. Deliver Android/native stop/destroy callbacks while VM references are valid.
2. Stop/join platform audio callback workers and guest native threads with the
   VM lease released. A separate teardown watchdog bounds blocked callbacks.
3. Verify no current EGL contexts or outstanding native-window references.
4. Release audio/graphics, asset handles, JNI/GC roots, guest heap, guest TLS,
   synchronization state, JNI executable tables and ELF/import mappings.
5. Recreate the console framebuffer and permit another app.

Failure to prove cleanup records an error and exits the Horizon process.
The versioned package/revision/session request is consumed once. A retained
pending marker is archived and shown on the next console boot, including after
a native crash. A same-process restart after an arbitrary crash is not promised.

## Reproduce

Prerequisites: PowerShell 7, Rust 1.97.1, the pinned device nightly with rust-src,
WSL Ubuntu/devkitA64/libnx/Mesa, JDK, Android SDK/build-tools 35, NDK
28.2.13676358, Kotlin compiler cache as in [building](BUILDING.md), and Ryujinx.
The build is serialized with one Cargo job; generated APKs are not source files.

```powershell
./scripts/build-m5-fixture.ps1
./scripts/build-m6-fixture.ps1
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-media.ps1
./scripts/test-ryujinx-launcher.ps1
./scripts/test-ryujinx-launcher.ps1 -Scenario quiescence
python scripts/record-hardware-feedback-evidence.py --implementation-commit (git rev-parse HEAD)
./scripts/package-androswitch.ps1 -EvidencePath reports/m7-hardware-feedback.json
```

The launcher test temporarily moves an existing `sdcard/switch/androswitch`
folder to a checked sibling backup, uses a fresh folder, then restores it in
`finally`. It refuses another Ryujinx process/test lock and stops only its own
PID. Retained test SD output is named `androswitch-e2e-result-<session>`.
Tests never reset unrelated emulator data or the user's package saves.

The console workflow (52 commands in the original baseline) checks installation, a corrupt APK failure,
ten completed app sessions, a named missing-service failure, repeated native
media teardown/reopen, restart/save retention, update, uninstall/reinstall,
cancelled deletion and explicit data clearing. It compares hashes after actual
console framebuffer submissions. A PNG resource icon must decode successfully.

The native-media fixture renders a moving GLES2 triangle on its own pthread,
reads/seeks a 150 KB compressed asset, receives input and plays 12,000 frames
through AAudio. The mixed APK reads the same asset via Java/JNI, then sends
2,400 Java AudioTrack + 2,400 AAudio + 4,800 OpenSL queue frames. Its queue
callbacks verify a distinct worker and persistent guest TLS.

The negative lifecycle fixture deliberately leaves a worker running. It must
produce `NATIVE_NOT_QUIESCENT`, retain interruption identity, consume the launch
request, and reach the console with the interruption notice on the next boot.
This is an expected safety-boundary failure, not a compatible app result.

## Regression and evidence limits

The import bridge now preserves SP for guest-to-host stack/variadic arguments.
A JNI callback with nine integers and nine doubles returned a wrong value on
the old NRO; the expanded suite returns the JVM result 83.25. The pthread
condition/mutex/recursive-lock fixture is also compared against the JVM native
build. Historical M4 reports retain their original corpus; newer results are
recorded separately.

Host checks cover ZIP corruption/chunking/seek, package workflows, portable
framework lifecycle and the retained VM regressions. Build/format/clippy and
the compact report record exact commands and artifacts.

- **Ryujinx:** actual Horizon NRO execution, GPU readback and released PCM
  buffers. Audio quality/acoustic output is not measured. Console captures
  come from the submitted CPU framebuffer; GLES captures use GPU readback.
  The shared GLES2 RGBA readback bridge preserves PACK_ALIGNMENT and reads
  scanlines to avoid an observed multi-row staging problem in the installed
  Mesa/emulator combination. The native APK also verifies a full-frame read
  through that public Android GLES API, separately from the diagnostic capture.
- **Input:** automation feeds the same Android/console dispatchers as platform
  input. This is not physical Joy-Con/touchscreen verification.
- **Android reference device/emulator:** not run for these framework/media APKs.
  JVM comparisons cover the stated VM/JNI semantics, not Android framework APIs.
- **Physical Switch:** the user reports all four samples install/open and native
  graphics/beeps work; Plus crashes in the native sample. The candidate fix is
  pending hardware retest. See the dated hardware feedback; the original baseline
  did not include device runs. Suspend/resume/docked/memory validation remains M12.
- **Corpus:** original SDK/NDK fixtures; frozen external NDK samples and Vector
  Pinball remain independent pending targets. NativeActivity FD watchers/full
  native_app_glue, arbitrary apps, modern engines, Compose and WebView are not
  established by these passes.
