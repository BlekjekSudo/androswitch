# M1 APK → DEX → interpreter E2E in Ryujinx

M1's host and emulator gate is complete. The device uses the actual shared
upstream APK/DEX parsers, Engine, opcode dispatcher, classpath and heap built
with `no_std + alloc`. No Python, Unicorn, guest kernel, per-package runtime
branches or second toy interpreter are linked into the NRO.

## Observed evidence

The latest run passed 18 calls from the Java APK fixture:

- Twelve results match both the portable host and a real JVM: arithmetic with
  loops/branches, arrays, object fields/constructors/virtual and super calls,
  caught division by zero, normal division, wide shifts/XOR, float/double,
  sparse-switch hit/miss, static call chain and persistent static initialization.
- Six negative cases match the portable host's exact structured errors:
  unavailable System API, platform static field, monitor operation, native
  method, instruction-fuel limit and recursive-call depth limit.

The APK declares minSdk 24 and targetSdk 35. It is accepted by the API 26
reference policy. The same compiled fixture is used without modification on
host and emulator. Test descriptors live in the harness, outside the runtime.
The JVM oracle runs the Java source compiled with `--release 8`; D8 compiles
those same class files into the APK. SDK/build-tool/source/APK hashes are in
each build receipt. APK ZIP timestamps can vary across rebuilds; output hashes
are recorded per build rather than claiming bit-reproducible APK packaging.

The checksum-pinned real F-Droid DEX parser proof also passes in the same NRO:
DEX 037, 11,168 classes, 65,003 methods, 62,734 strings, valid checksum,
allocator checksum 32,640. This does not execute F-Droid's application lifecycle.

See [machine-readable evidence](../reports/m1-core.json) for sessions, hashes,
commands, result counts and verification scope. Physical Switch: not run.

## Reproduce

On this Windows workspace, Ryujinx defaults to
`D:/Dev/Switch/RyujinX/Ryujinx.exe`; pass `-Ryujinx` or `-RyujinxProfile` to
override. Use PowerShell 7, the pinned nightly from the build script with
rust-src, WSL Ubuntu/devkitPro/libnx, JDK, Android SDK build-tools 35.0.0 and
platform android-35. Set `-AndroidSdk` on the fixture builder if needed.

```powershell
./scripts/test-ryujinx-m1.ps1
./scripts/check-m1.ps1
./scripts/check-m0.ps1
```

The E2E command builds the fixture/JVM expected output and NRO, copies its own
inputs to the emulator SD folder, archives old reports, and generates a fresh
session UUID. It runs the portable host, launches Ryujinx, waits for an actual
SD report bearing that UUID, then compares every result, instruction count and
error against host. Positive values are also compared to the JVM. It stops
only the process it launched after checking its command line/executable. An
exclusive lock prevents concurrent use of this test SD directory.

Run artifacts are in `local-artifacts/m1/runs/<session>/`: verification receipt,
host/emulator reports, DEX parser report, request and emulator stdout/stderr.
These binaries and verbose local outputs are ignored by Git. Source fixtures
and compact evidence are tracked. `-SkipBuild -SkipFixtureBuild` reuses existing
artifacts for an unchanged-input repetition; default invocation rebuilds both.

Focused upstream regression command (227 passing tests):

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p rd-vm --lib --test vm_exec --test gc --test gc_generations --test collections_gc --locked
```

The initial debug host probe exhausted Windows' default native stack during
the recursive-limit test. The host test adapter now uses an explicit 32 MiB
worker stack, and the bounded probe caps guest depth at 16. The final Ryujinx
run reports `VM_STACK_OVERFLOW` rather than crashing. Device heap: 256 MiB;
probe guest heap accounting: 32 MiB; fuel: one million instructions per call.

## Historical scope after M1

This section records the M1 boundary. Persistent storage and native/JNI
execution have since been implemented and verified separately in
[the M2–M4 evidence](M2_M4_E2E.md).

This is a method execution proof, not an Android Activity launcher. JNI,
native loading, threads/monitors, Android framework/UI/media and persistent
Java/native VFS remain in their planned milestones. The portable collector
currently supports its own exact state graph; JNI/reference lifetime and new
platform side tables require additional tests before those features ship.

The next milestone is M2: atomic APK installation/update, app registry and
persistent shared storage. The historical upstream MCP baseline is recorded
separately and is outside this gate.
