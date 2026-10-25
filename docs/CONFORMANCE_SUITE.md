# Unified Android conformance catalog

The API26 development suite consolidates **219 cases in one APK**: 97 Java/DEX,
89 Android/platform and 33 JNI/native/graphics. It excludes sensors. Its
manifest, Activity, Application, Java and NDK code use public Android contracts.
It is not CTS, CTS Verifier, complete Android coverage or Android certification.
It does not replace unmodified independent APKs as the M9 integration gate.

The parsers, VM and original runtime foundation are credited to
[RustyDroid](https://github.com/nadabgfccvd/RustyDroid), by nadabgfccvd and its
contributors. AndroSwitch adds the portable Horizon adapters and these original
tests. Upstream attribution and licenses remain in [CREDITS](../CREDITS.md).

## Runtime acceptance target

All **219 declared cases are runtime repair targets**. Producing an APK, visiting
every case or collecting a complete journal does not satisfy this target. Every
`FAIL`, `ERROR`, `INTERRUPTED`, `UNSUPPORTED`, `BLOCKED` and `NOT_RUN` remains open;
explicit emulator exclusions do not count as coverage. The collector records
`acceptance_passed` separately from `catalog_completed`, and `--require-pass`
rejects a catalog with any non-PASS case. The package script requires that gate
unless explicitly asked for an unfinished `-Diagnostic` hardware/debug bundle.
This target still does not establish all Android behavior or game compatibility.

Shared repairs include Java text/numeric/array operations, fixed-size array-backed
`Arrays.asList`, collection virtual `toString`, nested Bundle references, real
initial launch Intents, RandomAccessFile on the persistent VFS and bound SQLite
statements on native SQLite. Additional regression contracts check overlap and
partial arraycopy, backing-array aliasing/GC, EOF/read-only file behavior, SQL
binding without interpolation, live statement-to-database GC ownership, actual
row IDs/change counts, scalar SQLite conversions and closed-handle errors.

The native system-provider libdl bridge distinguishes raw data from callable
host functions and caches TLS/errno veneers. Its current scope is linked system
libraries; loading additional guest DSOs and `RTLD_NEXT` caller lookup remain
explicit errors. Android `sysconf` CPU counts describe the cores visible to this
runtime's guest, matching its process-allowed Horizon mask. The missing-file
errno case uses a nonexistent child of `getFilesDir`; `/data/local/tmp` was an
invalid ENOENT assertion because Android permissions may instead return EACCES.
The assertion still requires ENOENT, and the catalog still contains 219 cases.

## Facts and compatibility identity

| Observation | Source / meaning |
| --- | --- |
| `Runtime.availableProcessors()` | Fresh process-allowed Horizon core mask population; not all physical chip cores |
| `androswitch.host.cpu.allowed_mask` | Actual `svcGetInfo(InfoType_CoreMask)` result |
| `androswitch.host.{cpu,gpu,memory}.clock_hz` | Read-only clkrst/legacy PCV service rate, in Hz; no overclock/setter; errors retained |
| `androswitch.host.tick.frequency_hz` | Horizon architectural system tick frequency, not CPU clock |
| `androswitch.host.os.{name,version}` | Horizon adapter and libnx-reported OS version |
| GL vendor / renderer / version / extensions | Actual current EGL/GLES implementation strings; pixel tests do real pbuffer/FBO reads |
| Vulkan physical-device properties | Actual discovered Vulkan loader/driver, when present |
| Android `Build` / `Build.VERSION` | Declared `api26-v1` Android compatibility profile; not physical Android firmware |

The optional `androswitch.host.*` keys use ordinary `System.getProperty` and an
explicit platform resource provider. On an Android reference or a host without
that provider, absence is **UNSUPPORTED**, not a guessed Horizon value. These
are documented AndroSwitch observations, not standard Android property names.
`renderer.backend` is reserved and remains unavailable until a backend can
report its identity. GPU identity is read through the real graphics APIs.

The guest profile declares ARM64, Android API26 / release8.0.0, AndroSwitch
model/manufacturer, `eng/test-keys`, no security patch date, no base OS, preview0
and unknown build timestamp0. Java Build fields and native Bionic properties
derive from `platform/switch-smoke/runtime_properties.inc`. The fields are
canonical and GC-rooted per VM. Advertising API26 is a behavior reference, not
a claim that all API26 APIs are implemented. No Google/CTS certification or
physical Android model is fabricated.

## What graphics results prove

GLES tests create and restore their own EGL context/pbuffer, perform real pixel
readback, compile a working shader and an intentionally invalid shader, and
test buffer/texture/framebuffer state. They do not terminate the runtime's
shared EGL display. Driver strings alone cannot make these contracts pass.

Vulkan tests cover loader/instance/device enumeration, queue-family discovery,
logical-device creation and reported memory heaps. **They do not prove Vulkan
Android surface creation, swapchain presentation or a rendered Vulkan frame.**
Missing `libvulkan.so`/entrypoints are unsupported, not successful empty devices.

[mesa-switch](https://github.com/danfromtico/mesa-switch) is tracked as a platform
reference in `third_party/mesa-switch-reference.json`. Its native GL paths and
direct Vulkan library need platform integration plus an Android ABI/surface
bridge. This change does not install that SDK, replace the current Mesa build,
or claim Vulkan rendering. Test selection in the APK does not switch the
runtime's active Mesa driver.

## Results and evidence

`PASS` means the declared assertion or observation succeeded. `FAIL` means a
contract assertion or Java exception failed. `UNSUPPORTED` identifies a known
missing capability. `ERROR` retains other VM errors. `BLOCKED` retains poisoned
class/linkage state. `INTERRUPTED` means a checkpoint had no result before app
termination. `NOT_RUN` means no execution evidence. A hardware crash can stop
the whole suite; no fixture can promise to recover inside a terminated process.

The journal saves one bounded result per case and aggregates JSON. The test
runner resumes after an interrupted case and preserves its status. Emulator
collection may associate a captured pending index with an identity-bound
session error; it keeps both raw evidence and the original journal status.
JSON counters compare the status field, never diagnostic substrings. The
report stores its catalog digest. NRO identity comes from the outer test or
bundle receipt and must accompany physical-device results.

The automated host run includes an independent JDK oracle for Java cases and
tests JSON escaping, surrogate pairs, interruption recovery, history/reset and
status parsing. Native C wrapper tests inject failures into the **production**
resource helper and also compile it against actual libnx ARM64 headers.
Mock C service tests are not actual Horizon service execution. Unknown libnx
HOS version (zero) returns an explicit unavailable query instead of selecting
legacy PCV. libnx version fields use major16/minor8/micro0 bit positions.

New APIs should add or strengthen cases in this catalog. Small existing focused
fixtures remain useful for regression isolation, package-renaming checks and
native ABI oracles; end users do not need to install each one. General runtime
changes must pass these checks independently of any game package name.

## Input/editor boundary

The shared `BaseInputConnection` implementation supports a bounded local full
editor, UTF-16 selection/replacement/deletion/query contracts, genuine View
Context identity and application callback/exception propagation. Editable and
connection edges participate in VM GC. It does not provide an Android IME,
keyboard presentation, key dispatch, arbitrary spans/filters/watchers or a
complete text widget. Dummy-editor delivery and styled/composing text stay
unavailable. A VM String cannot yet represent an isolated UTF-16 surrogate;
editable char access preserves that unit, while unsupported String conversion
returns an explicit error. Focused SDK/JDK source-model tests run in two
namespaces; actual Android reference-device comparison remains pending.

## Emulator clock limitation

Ryujinx's profile currently routes the native clock queries to a PCV service
command it does not implement. NRO21 and NRO22 runs stopped at that emulator
service, with partial journals preserved and the original SD restored. The
unknown-version guard is a separate general correction; it does not implement
Ryujinx's missing service. For corpus progress, the harness can explicitly
exclude `host.cpu.clock_hz`, `host.gpu.clock_hz` and `host.memory.clock_hz`.
Those cases remain **NOT_RUN**, with the excluded IDs and seed hashes recorded.
The ordinary APK accepts an optional app-private `exclude.txt`, one exact case
ID per line. Physical-device runs without this control file still execute
all queries. No package/emulator switch is added to the runtime.
