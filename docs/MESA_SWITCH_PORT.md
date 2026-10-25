# Optional Mesa Switch integration

The portable VM remains `no_std + alloc`. This optional platform build links the
real Mesa 26.2.4 Horizon EGL/NVC0/NVK SDK; it does not emulate successful GPU work.
Its source is [danfromtico/mesa-switch](https://github.com/danfromtico/mesa-switch)
at `0ff52079ca9e9e837ed27eb95a8d239f439a1864`. Credit belongs to its contributors,
Mesa/Nouveau/NVK/Collabora/NVIDIA contributors and all upstream component authors.
RustyDroid remains the runtime foundation; see [CREDITS](../CREDITS.md).

## Reproduction

Run `scripts/fetch-mesa-sdk.ps1 -IncludeSource` with an authenticated GitHub CLI. It uses the
standard CLI download flow and never exports credentials. CI run `38329414110`,
artifact `11781503719`, supplies the static SDK. The receipt records the official
archive digest and local extracted file hashes; CLI extraction does not retain
that ZIP, so its archive digest is not claimed as locally verified. The exact
Rust component is separately downloaded from static.rust-lang.org and SHA256
verified against `third_party/mesa-switch-rust.lock.json`.

The SDK was built with Rust `1.101.0-nightly (8d1a76430 2026-10-06)` as recorded
in its ELF object producer comment. Its core/alloc/std libraries are platform
implementation dependencies of the shader compiler, not the VM or an Android
Linux kernel. No global Rust default or installed devkitPro libraries change.
The SDK is staged under `local-artifacts/dependencies/mesa-switch-0ff52079`.

Build with `scripts/build-androswitch.ps1 -LinkBackend Windows -MesaSdk
local-artifacts/dependencies/mesa-switch-0ff52079/sdk/opt/devkitpro/portlibs/switch`
and the project's usual `-TargetDirectory`. Separate object directories preserve
the legacy renderer build. `-MesaRustLibDirectory` selects the matching staged
Rust component when stored elsewhere. These Rust symbols must match the SDK;
arbitrary symbol aliases or allocator/formatter success stubs are invalid.
Packaging collects notices from that exact source checkout (or an explicitly
supplied `-MesaSourceDirectory`), the matching Rust component, and linked
expat/zlib/zstd dependencies. Source license/comment collection is conservative;
it does not claim that every source component was linked into the NRO.

## Platform boundary

The linked shader-compiler runtime uses explicit host SDK wrappers. Futex wait,
wake, address/bitset selection and actual wake counts use Horizon mutexes and
condition variables. Tickets resolve timeout/wake races. Its clock adapter maps
Linux/newlib monotonic identifiers onto the real Horizon counter, and CPU clocks
onto kernel thread ticks including retired threads. Alignment, vector writes and
error strings use real newlib behavior. Unknown syscalls, Linux stat/open64/mmap,
program-header lookup and DRM syncobj interop return explicit errors rather than
the upstream compatibility stubs' fabricated values. The Android guest syscall
adapter remains separate. AArch64 executable sections are audited for Linux
`svc #0`; no guest Linux kernel is added.

`dlopen` exposes a linked `libvulkan.so` provider only in this SDK build. Its
entrypoints and returned function pointers cross cached host TLS/errno veneers.
The initial whitelist covers instance/physical-device discovery, properties,
queues, memory and logical-device lifetime. Objects are owned and bounded per
session. Guest allocators/pNext callbacks, Android WSI, swapchains and queue
submission remain unavailable. These APIs are not advertised as full Vulkan
conformance. GLES pbuffers and actual pixel/shader/buffer assertions must be
verified by the ordinary SDK APK. Vulkan discovery/device tests do not prove
rendered Vulkan frames. Hardware evidence remains separate from Ryujinx.

Software Activity orientation now queues/coalesces requests at main-loop
boundaries, applies actual display/input geometry, and recreates an Activity
with its saved Bundle and Intent, or dispatches declared configuration handling.
Active GL rotation still requires renderer suspension/recreation and stays an
explicit unsupported boundary. Application view-hierarchy state serialization
is not yet complete; application-managed saved Bundle callbacks are supported.

## Observed baseline

The ordinary 219-case APK records 216 PASS and 3 explicit NOT_RUN clock queries
in 20 sessions, using a separate patched Ryujinx diagnostic. All nine GLES and
six Vulkan discovery/device cases pass. The emulator's fixed-address GPU
allocation bug and missing legacy thread-tick alias are documented with a
[source patch and production allocator probe](../tools/ryujinx-cfg/README.md).
Those emulator changes do not enter the Android runtime or SD package.
The user subsequently confirmed 219/219 PASS on a physical Switch;
[hardware feedback](../reports/conformance-hardware-20261025.json) is recorded
separately as an aggregate report. The raw journal/device binary hash has not
been archived. Android reference testing remains pending.
