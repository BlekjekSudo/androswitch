# Runtime SDK fixture

Build with `pwsh -NoProfile -File fixtures/m9/runtime/build.ps1` using SDK build
tools 35.0.0 and min/target API 26. The build emits original and renamed package
APKs, DEX files and independent JVM oracle rows under `target/runtime-fixture`,
plus a checksum-bearing `build-receipt.json`. No Runtime framework classes are
patched or bundled into these ordinary applications.

`RuntimeCases` checks singleton identity, identity across GC, a positive
available-processor query and the actual Runtime class name. Host tests bind
real host or controlled resource providers explicitly; the provider's numeric
result is not baked into the Java sources.

`original/runtime-proof.apk` launches
`org.androswitch.contracts.RuntimeProofActivity` in package
`org.fixture.runtimeproof`. It queries the same runtime twice, displays the
observed count and writes `/sdcard/Download/androswitch-runtime-proof.txt`.
The host proof transcript is a callback/VFS contract. Real Horizon syscall,
rendering and clean launcher return must be verified through the actual NRO.

See [Runtime resource behavior](../../../docs/JAVA_RUNTIME_RESOURCES.md) and its
separate immutable host receipt for scope and unsupported methods.
