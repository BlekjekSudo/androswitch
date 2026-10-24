# Horizon display orientation

AndroSwitch negotiates one application display before rendering starts. Fixed
portrait requests change the presentation, display metrics and input coordinate
system together. Runtime behavior does not inspect an APK's package name.

The Switch output layer remains 1280 × 720. The platform accepts Android Surface
rotation values, which differ from ActivityInfo orientation request constants:

| Surface rotation | EGL/window buffer | CPU logical raster at density 2 | Presentation |
| --- | --- | --- | --- |
| 0 | 1280 × 720 | 640 × 360 | Natural landscape |
| 1 | 720 × 1280 | 360 × 640 | Rotate clockwise 90° |
| 2 | 1280 × 720 | 640 × 360 | Rotate 180° |
| 3 | 720 × 1280 | 360 × 640 | Rotate clockwise 270° |

The shared framework maps ActivityInfo requests to a Surface rotation. It commits
the requested orientation and new GraphicsCaps only after the platform accepts
the change. Display/Resources metrics, Configuration and software raster sizing
then follow the accepted geometry. Sensor-driven rotation and activity
inheritance require their own implemented policies; accepting fixed portrait does
not establish those capabilities.

## Real presentation and input

`platform/androswitch/display.c` configures the native window before buffers are
registered. For EGL it uses `nwindowSetDimensions` and `nwindowSetTransform`.
Mesa creates real 720 × 1280 buffers for portrait; the compositor rotates those
buffers into the fixed output layer. `ANativeWindow_getWidth/Height` and the
GLSurfaceView surface callback use the accepted native-buffer geometry.

CPU presentation keeps a physical 1280 × 720 framebuffer and rotates the actual
logical Frame while expanding each pixel to a 2 × 2 block. The Rust/C frame ABI
carries the retained pixel pointer, length, width and height. The blitter rejects
inconsistent or short buffers before writing. Existing compositor fence, slot
bounds, service ownership and block-linear conversion checks remain in place.

HID touch coordinates pass through the inverse presentation transform. C emits
logical software coordinates; the runtime converts those to full-resolution
pixels for GL and NativeActivity input. Diagnostic GPU captures read scanlines
using the actual surface dimensions, then apply the same presentation transform
when writing physical 1280 × 720 PPM images.

CPU and GPU diagnostic images use a bounded 3840-byte RGB row buffer,
using one write per row. A `.ppm.tmp` filename remains private while the header
and rows are written. Only complete writes and a successful close allow a rename
to the final `.ppm` name. Write, row-generation, close or rename errors leave the
image unpublished and attempt temporary-file cleanup; they preserve an existing
complete image. Crash debris can remain under `.tmp`, which evidence scanners
ignore. This publication rule prevents an early report reader from accepting a
partially written capture.
The [publication host receipt](../reports/horizon-gpu-capture-publication-host-20261024.json)
records twelve real-file publication/error cases and strict ARM64 compilation
for the first GPU writer change. Historical run observations remain separate.

CPU `last-frame.ppm` publication happens after successful queue submission. Its
source is the unchanged CPU-owned linear RGBA shadow, including the actual
rotated pixels; the writer never reads or modifies the queued block-linear GPU
buffer. Checked row extraction removes alpha and ignores stride padding. The
[CPU publication host receipt](../reports/horizon-cpu-capture-publication-host-20261024.json)
checks every RGB pixel for all four rotations, twelve malformed row cases and
the shared publication failure paths. Both writers use 721 writes for a full
720p image, replacing the previous CPU writer's 921,600 per-pixel writes plus
header. Capture I/O remains synchronous after that frame is submitted; these
host checks do not measure on-device SD latency. The later exact-NRO handoff
run below verifies complete CPU and GPU publication in the emulator; physical
SD timing remains unmeasured.

Capture ownership is independent of filesystem timestamps. After deleting the
previous session's captures, the launcher writes `logs/capture-owner.json` with
protocol version 1, the session identifier, package and installed revision.
The external E2E recorder requires that identity to match the current session
report both before and after copying a complete image. CPU `last-frame.ppm`
also requires a matching active launch request: after return, that filename can
contain the launcher instead of the application. GPU captures remain eligible
after return while their generation still matches. Old NROs without this
protocol can supply diagnostics but cannot supply verified images to this
recorder.

The [generation-bound handoff run](../reports/horizon-capture-generation-handoff-ryujinx-20261024.json)
uses NRO `3917b675…` to launch the portrait fixture, return, open the Java UI and
return again. Both installation identities and all teardown stages match.
The native session presents 305 frames; its frame-2 and frame-30 PPMs are both
complete. The Java session presents 285 frames and supplies a complete CPU
capture with the restored landscape layout. Manual inspection of losslessly
converted PNGs confirms the colored native quadrants/marker and the Java
labels, buttons and Canvas shapes. Capture protocol 1 binds each image to its
session; the CPU capture contains the application UI. This is submitted/readback
pixel evidence, not a separate compositor screenshot. No touch input or Java
button interaction was injected in that run.

The [capture ownership host receipt](../reports/session-capture-ownership-host-20261024.json)
records fourteen real-filesystem cases, including differing host/guest clocks,
identity changes during a copy, incomplete images and immutable accepted
copies. These checks exercise the production recorder without running the
emulator; subsequent exact-NRO execution must establish the runtime protocol.

## Mesa window-surface metadata

The installed devkitPro Mesa package is `20.1.0-5`. Its Switch driver allocates
window resources from `nwindowGetDimensions`, registers their native buffers and
releases the queue when the EGL surface is destroyed. The driver does not fill
the EGL surface base Width/Height fields or supply a Switch QuerySurface override.
Consequently those EGL bookkeeping fields are insufficient to describe the
native buffers.

AndroSwitch's adapter binds each successfully created owned EGL window-surface
handle to the observed, registered NWindow dimensions. For that binding only,
`eglQuerySurface(EGL_WIDTH/EGL_HEIGHT)` reports this actual geometry and verifies
that the live buffer queue still matches it. Internal surface validation and APK
imports share this provider. Unknown surfaces, invalid handles, other attributes
and non-window paths delegate to Mesa's normal EGL behavior. This adapter does
not implement offscreen surfaces that the underlying driver lacks.

Ownership is recorded before post-creation validation. Failed EGL destruction
retains the handle for fail-closed cleanup. Bindings are removed only after a
successful destroy or termination; a teardown failure does not permit a new
application to start.

GLSurface cleanup waits up to six seconds for the owned worker, including real
Mesa/GPU termination and its remaining TLS/detach work. The dedicated session
watchdog starts before Rust lifecycle shutdown and waits fifteen seconds from
its own wait entry; it is an ultimate process-stop limit, not extra time granted
to every cleanup stage. Expiry still preserves owners and requires process exit.
`logs/media-stop.log` is cleared at each application launch and records actual
wait/thread/close return codes plus EGL termination begin/end ticks. EGL result
events contain raw EGLBoolean values (1 succeeds); worker wait events contain a
zero success code and the raw libnx Result in `detail`. Missing completion events
do not establish cleanup.

Primary source references:

- [libnx 4.12 native window implementation](https://github.com/switchbrew/libnx/blob/v4.12.0/nx/source/display/native_window.c):
  dimensions require an unregistered queue (lines 113–122), transforms are queued
  with each submitted buffer (lines 281–291).
- [libnx transformation definitions](https://github.com/switchbrew/libnx/blob/v4.12.0/nx/include/switch/display/types.h):
  ROT_90 is clockwise.
- [devkitPro Mesa 20.1.0-5 patch](https://github.com/devkitPro/pacman-packages/blob/c7876f72c1f3353725ab8468f0ef9180280a8c31/switch/mesa/switch-mesa-20.1.0-5.patch):
  native dimensions at line 616, buffer registration at line 648 and buffer
  release at line 566. Frozen patch SHA-256:
  `950f93d3e5b6ae9c5a42c2918623fe9a80d7ee398d92d2e73abec86b62d75916`.

No Mesa implementation code was copied into the display adapter.

## Lifetime and current limit

Once a CPU renderer starts or a GL configuration/rendering path is committed,
changing rotation requires surface and Configuration lifecycle recreation. That
active-change path is currently unavailable: the runtime returns an explicit
error and preserves the previous orientation. Repeating the same target remains
valid, including during a paused GL context. A retained GL context resumes with
the same dimensions and transform.

Cleanup joins rendering/guest workers and releases CPU/EGL buffers before
resetting native-window dimensions and transform to natural landscape. The
launcher opens its framebuffer only after this succeeds. Registered buffers or
failed EGL cleanup make reset fail closed instead of reusing uncertain state.

## Evidence

[Host receipt](../reports/horizon-portrait-host-20261024.json) records complete
pixel/input/capture checks for all four rotations, lifecycle and malformed-buffer
checks, 29 worker scenarios, and strict ARM64 C compilation with both installed
toolchains. These checks prove geometry and ownership contracts against a fake
libnx boundary. They do not prove actual compositor rendering.
The later [stop-deadline host receipt](../reports/horizon-glsurface-stop-host-20261024.json)
adds a real host-thread regression whose simulated EGL termination lasts longer
than the previous two-second cutoff. Actual Mesa termination timing remains a
separate emulator/device measurement.

The later [Ryujinx portrait/stop receipt](../reports/horizon-portrait-stop-ryujinx-20261024.json)
records real 720 × 1280 EGL buffers matching Renderer callbacks, four successful
native quadrant readbacks, 188 observed presentations and a clean geometry reset
and return to the stable launcher. Its last native file checkpoint records frame
180. The worker wait took about 0.266 s and EGL termination about 0.093 s. This
warm run also fits the previous two-second bound, so it does not isolate the
larger deadline as the cause of improvement. Actual portrait touch was not
injected in that run; host inverse-coordinate checks remain a separate scope.

The separate [portrait-to-Java handoff receipt](../reports/horizon-portrait-handoff-ryujinx-20261024.json)
records two matching application identities in one launcher workflow, both clean
shutdowns, eight successful CPU queue submissions after EGL, and the subsequent
Java application's restored 640 × 360 layout and matching software/submitted
frame hashes. That run retains `attempted_render_unconfirmed`: its portrait PPM
has a valid header but is short by 42,000 payload bytes, and there is no frame-30
checkpoint. The harness's `sampled_nonuniform=false` field was a default after
validation failed, rather than proof of an entirely uniform image. This receipt
qualifies the handoff, without importing rendering proof from another run.

Run `./scripts/test-horizon-display.ps1` with host GCC/pthread support to repeat
the focused C contracts. `-Compiler` selects a host GCC and `-OutputDirectory`
chooses an artifact directory. The default directory is ignored
`local-artifacts/tests/horizon-display`; its `result.json` records compile/run
commands, exit codes, logs and source hashes. This script starts no emulator.

The parent-owned NRO build/emulator run and later physical Switch tests must
record their own binary identities, real surface dimensions, rendered pixels,
input and return-to-launcher evidence. M9 remains incomplete until its external
application startup gate has direct execution evidence.
