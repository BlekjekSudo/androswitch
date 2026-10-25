# AndroSwitch workspace guide

## Product contract

AndroSwitch is a general Android application runtime for HorizonOS. Implement
Android API/ABI behavior shared by applications. Do not introduce package-name
branches, game profiles, hard-coded game libraries, title offsets, fabricated
success responses, or proprietary payloads. The initial native ABI is ARM64;
v0.1 is offline. One application runs at a time.

The repository is a history-preserving fork of RustyDroid at
`6388281f66d254c8e01a0fdc7a5a9a30239bc762`. Keep upstream attribution and
licenses. `README.rustydroid.md`, `RustyDroid.md`, and the original roadmap and
compatibility documents describe upstream, not verified Switch capabilities.
AndroSwitch's contracts and status live in `docs/ANDROSWITCH_ROADMAP.md`,
`docs/ARCHITECTURE.md`, and `docs/ANDROSWITCH_COMPATIBILITY.md`.

## Implementation rules

- Work in milestone order. Preserve existing changes and upstream tests.
- Device core is `no_std + alloc`; host adapters may use `std`. Do not put
  Python, Unicorn, a guest Linux kernel, or desktop process APIs in device builds.
- Route OS behavior through platform interfaces; Java and native code must share
  storage and application state. Unavailable behavior returns a structured error.
- Before expanding a missing API, inspect the inherited implementation and tests.
  Prefer shared `core`/`alloc` extraction over a second implementation; record
  state, GC, thread or platform differences that require adaptation. See
  `docs/UPSTREAM_REUSE_AUDIT.md` for the current source-routing audit.
- AndroSwitch uses an API 26 reference environment. A lower APK minSdk is valid;
  targetSdk is not a minimum. This supersedes upstream's Moto E5/minSdk floor
  policy for AndroSwitch APIs. Preserve legacy `rd` behavior until explicitly ported.
- Keep APKs, generated native libraries, toolchains, and build output ignored.
  Freeze external fixture URLs/revisions/hashes in the fixture lock.
- Keep `unsafe` limited to documented native/platform FFI boundaries. Do not
  weaken the upstream workspace lint globally to add a platform bridge.
- Track implementation, host, emulator, and physical-device evidence separately.
  A successful compile or host emulator result is not hardware verification.

## Verification

Use `rg` for searches. Run focused tests first, then relevant workspace checks.
The Windows host uses Cargo; the Switch C linker/toolchain is available in WSL
Ubuntu under `/opt/devkitpro`. Keep new toolchains in task-local caches and do
not change the user's global Rust default.

Before delivery, run the AndroSwitch tests, formatting and clippy checks, inspect
the diff, and update the milestone/evidence records with commands and results.
When a test cannot run, record the reason instead of marking it passed.
