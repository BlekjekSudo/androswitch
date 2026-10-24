# TimeUnit contract fixture

This original Java 8 / Android API 26 fixture tests the actual shared TimeUnit
adapter. The builder runs the real JVM to generate 1,995 expected rows covering
every pair of seven units, signed truncation, both overflow boundaries, all seven
direct conversions, enum names/ordinals/identity/order, fresh `values()` arrays,
and invalid/null arguments. A second APK changes only the manifest identity.

Run `pwsh -File fixtures/m9/time-unit/build.ps1`, then
`cargo test -p as-framework --features host --test time_unit -- --test-threads=1`.
The builder uses the existing SDK build-tools 35.0.0, not an emulator.

The normal SDK Activity displays a visible result and writes
`/sdcard/Download/androswitch-timeunit-proof.txt`. Compare the entire file,
including its JVM-produced numeric digest, to `target/time-unit-fixture/proof.txt`.
Host success does not establish Horizon/emulator/physical-device execution.

The adapter implements canonical constants, `values`, `TimeUnit.valueOf`,
`convert`, and the seven `to*` methods. Inherited Enum methods use actual Enum
heap state. Generic `Enum.valueOf(Class, String)` discovery of host enums,
constant-specific anonymous implementation classes, blocking timing helpers
(`sleep`, `timedWait`, `timedJoin`), and later Duration overloads remain outside
this bounded implementation. Pool keep-alive still accepts only canonical
`SECONDS`; the conversion work does not broaden the ThreadPoolExecutor contract.

Reference contracts: [Android TimeUnit](https://developer.android.com/reference/java/util/concurrent/TimeUnit)
and [Java 8 TimeUnit](https://docs.oracle.com/javase/8/docs/api/java/util/concurrent/TimeUnit.html).
The Rust implementation is independently written from unit ratios and the
documented truncation/saturation contract. No upstream implementation code or
external application code is copied.
