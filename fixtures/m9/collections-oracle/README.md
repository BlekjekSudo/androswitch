# Collections factory JVM observations

This host-only Java source observes the immutable factory contracts reused from
RustyDroid. It does not build another APK or change the 219-case Conformance APK.
It is original AndroSwitch test code under MIT OR Apache-2.0.

```powershell
javac -J-Xmx128m --release 8 -g:none -d local-artifacts/horizon-std/collections-oracle fixtures/m9/collections-oracle/CollectionsFactoryOracle.java
java -Xmx64m -cp local-artifacts/horizon-std/collections-oracle CollectionsFactoryOracle
```

The observations include empty/singleton values, nulls, rejected mutations,
field identity, live unmodifiable views and sorted return types. They are JVM
observations, separate from Android reference/hardware execution. The inherited
snapshot wrappers differ from the JVM's live views. The inherited empty sorted
factories use plain Set/Map marker types; the device does not expose those two
overloads until their sorted-container contract is integrated. No Android
compatibility claim follows from matching the other rows.

Product integration tests use the existing unchanged Conformance APK as their
application context:

```powershell
cargo test -p as-framework --features host --test collection_factories --locked
```
