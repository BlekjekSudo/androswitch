# ADR 011 — M10 virtual devices, sensors, media & deterministic agent controls

## Context

Milestone M10 in `RustyDroid.md` (PARTE 3 + PARTE 4) specifies virtual peripheral
subsystems and runtime agent controls so Android apps that depend on sensors,
GPS, camera, biometrics, audio/media, cellular/BLE/NFC, battery/power, and
dynamic configuration changes execute deterministically in headless mode:

- **Audio & Media (`MEDIA-01..03`, `MEDIA-05`)**: `AudioManager`, `AudioFocusRequest`,
  `AudioRecord`, `AudioTrack`, `MediaPlayer`, `MediaSession`/`MediaController`, and
  `MediaProjectionManager`/`MediaProjection`/`VirtualDisplay`.
- **Camera2 (`MEDIA-04`)**: `CameraManager`, `CameraCharacteristics`, `CameraDevice`,
  `CameraCaptureSession`, `CaptureRequest`, `TotalCaptureResult`, `ImageReader`,
  `Image`, `Image.Plane`, and `java.nio.ByteBuffer`.
- **Sensors & Location (`MEDIA-06..07`)**: `SensorManager`, `Sensor`, `SensorEvent`,
  `SensorEventListener`, `LocationManager`, `Location`, `LocationListener`, and
  `com.google.android.gms.location.FusedLocationProviderClient` (`LocationRequest`,
  `LocationCallback`, `LocationResult`, `Task`).
- **Biometrics (`MEDIA-08`)**: `android.hardware.biometrics.BiometricManager`,
  `android.hardware.biometrics.BiometricPrompt`, `androidx.biometric.BiometricManager`,
  `androidx.biometric.BiometricPrompt`, and legacy `FingerprintManager`.
- **Connectivity (`NET-05..06`)**: `TelephonyManager`, `SmsManager`,
  `BluetoothManager`/`BluetoothAdapter`/`BluetoothLeScanner` (`ScanCallback`), and
  `NfcManager`/`NfcAdapter`/`Tag`/`NdefMessage`/`NdefRecord`.
- **System & Configuration (`SYS-07`)**: `BatteryManager` (including sticky
  `Intent.ACTION_BATTERY_CHANGED`), `PowerManager` (`WakeLock`), `UiModeManager`
  (dark mode), `LocaleManager` / `Locale` / `Configuration` / `DisplayMetrics`
  (locale, font scale, portrait/landscape orientation), and `ClipboardManager`.
- **Agent Controls (`AG-10`)**: live MCP controls to query and mutate virtual
  hardware state and trigger guest DEX callbacks deterministically.

## Options Considered

1. **Host OS passthrough** (Linux ALSA/V4L2/BlueZ/GeoClue): violates Law 1
   (headless determinism), breaks Moto E5 / CI reproducibility, and leaks host
   state into guest execution.
2. **No-op stubs returning fixed constants**: insufficient for real apps whose
   UI and state machines wait on callbacks (`onSensorChanged`,
   `onLocationChanged`, `CameraDevice.StateCallback.onOpened`,
   `OnImageAvailableListener.onImageAvailable`,
   `BiometricPrompt.AuthenticationCallback.onAuthenticationSucceeded`,
   `MediaPlayer.OnPreparedListener.onPrepared`, `ScanCallback.onScanResult`,
   `Activity.onConfigurationChanged`).
3. **Deterministic per-`Engine` virtual peripherals (`crates/rd-vm/src/m10/`) +
   multiplexed `device_profile` MCP controls (`crates/rd-agent/src/m10_tools.rs`)**:
   every peripheral lives in `FrameworkState::m10` using ordered collections
   (`BTreeMap`/`BTreeSet`/`Vec`), enforces `perms.rs` permission checks, and
   delivers synchronous guest DEX callbacks when mutated by guest code or agent
   actions.

## Decision

We adopt **Option 3**, gated behind Cargo feature `virtual-devices` (enabled by
default in `rd-vm`, `rd-agent`, and `rd-cli`, mirroring `advanced-components`
from M9):

1. **Zero `thread_local!` / global mutable state**: all M10 state lives in
   `Engine.fw.m10` (`M10State`), ensuring multi-instance isolation and bitwise
   deterministic replay across runs.
2. **GC-friendly object ownership (ADR 006)**:
   - Transient value/buffer objects (`Sensor`, `SensorEvent`, `Location`,
     `ByteBuffer`, `Image`, `Image.Plane`, `CaptureRequest`, `ScanResult`,
     `BluetoothDevice`, `Tag`, `NdefMessage`, `NdefRecord`, `WakeLock`) store
     their payload in heap instance fields (`rd_*`) so generational GC reclaims
     them naturally as soon as the guest drops references.
   - Only active system registrations (`SensorEventListener`s until
     `unregisterListener`, `LocationListener`s/`LocationCallback`s until
     `removeUpdates`, open `CameraDevice`s/`ImageReader`s until `close`,
     active `MediaPlayer`/`AudioRecord`/`AudioTrack`/`MediaSession`/`MediaProjection`
     sessions until `release`/`stop`, active BLE `ScanCallback`s until `stopScan`,
     and active NFC `ReaderCallback`s until `disableReaderMode`) remain rooted in
     `M10State`.
   - `M10State`'s `Debug` implementation emits exclusively genuine `ObjRef`
     roots (`collect_gc_roots()`) so `gc.rs`'s conservative `HostState` `Debug`
     scanner never pins heap slots matching scalar configuration constants
     (sensor type IDs, viewport dimensions, battery percentages, or stream volumes).
3. **Opt-in virtual hardware dispatch over Appendix C's absent-hardware profile**:
   - Raw `Engine::new()` initializes `m10.virtual_hardware_enabled = false`,
     preserving Appendix C's sensorless / camera-less / radio-less default
     behavior in unit tests.
   - Launching an app via `rd-agent` (`AgentSession`), calling
     `Engine::m10_enable_virtual_devices()`, or executing any `AG-10` mutation
     sets `virtual_hardware_enabled = true`, routing `m10` ahead of
     `android_sysservices` / `android_stub_surfaces` in `intrinsics.rs` while
     still falling through for pure AOSP value math.
4. **Preserving the 47-tool MCP contract (ADR 010)**:
   - RD Sentinel enforces `mcp_tool_count == 47` against
     `.github/rd-sentinel/baseline.json`. Rather than breaking existing MCP
     clients or baseline invariants with a 48th tool, `AG-10` extends the
     existing `device_profile` tool with optional `action` commands (`"get"`,
     `"location"`, `"sensor"`, `"battery"`, `"network"`, `"telephony"`,
     `"sms_receive"`, `"ble_advertisement"`, `"nfc_tag"`, `"camera_frame"`,
     `"biometric"`, `"audio_input"`, `"media_action"`, `"dark_mode"`,
     `"locale"`, `"font_scale"`, `"orientation"`, `"clipboard"`).
   - Calling `device_profile` with `{}` remains 100% backward-compatible (returns
     the static profile plus a `virtual_devices` snapshot).
   - Because `"device_profile"` is listed in `observe::READ_ONLY_TOOLS` for its
     default `{}` query path, mutating `action`s explicitly increment `ui_epoch`
     and set `screen.dirty = true` so snapshot caches invalidate immediately.

## Documented Deviations (Never Silent)

- **No physical hardware passthrough**: audio output is captured in bounded PCM
  counters/ring buffers (`AudioTrack`/`MediaPlayer`) and audio input reads
  deterministic synthetic silence or agent-injected PCM (`audio_input`); camera
  and screen capture emit deterministic RGBA/JPEG test patterns (or
  agent-injected frames via `camera_frame`).
- **Synchronous deterministic callbacks**: callbacks (`onOpened`, `onConfigured`,
  `onImageAvailable`, `onSensorChanged`, `onLocationChanged`,
  `onAuthenticationSucceeded`, `onPrepared`, `onScanResult`, `onTagDiscovered`)
  execute synchronously on the VM thread within the fuel budget, avoiding thread
  races while honoring Android's callback order.
- **Explicit permission errors**: denied runtime permissions (`CAMERA`,
  `RECORD_AUDIO`, `ACCESS_FINE_LOCATION` / `ACCESS_COARSE_LOCATION`,
  `BLUETOOTH_SCAN` / `BLUETOOTH_CONNECT`, `SEND_SMS`, `READ_PHONE_STATE`) raise
  `java.lang.SecurityException` via `Engine::permission_granted` rather than
  returning silent nulls.
