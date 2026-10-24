# M7 improvements: file-backed APKs

The product now accepts single APK containers up to **512 MiB**, using positional
storage reads rather than a container-sized allocation. This is a container
budget; it does not promise that an APK's decoded code/resources fit the 256 MiB
homebrew heap or that its Android APIs are implemented.

## Shared implementation

- The existing RustyDroid ZIP/manifest/resource/DEX algorithms remain the parser
  foundation. `Zip::from_source` adds an owned `ReadAt + Send + Sync` source and
  bounded tail/directory discovery, including ZIP64. The in-memory parser stays
  available for upstream tools and fixtures.
- File-backed extraction is capped at 64 MiB per materialized entry; assets use
  incremental STORED/DEFLATE readers with CRC/size validation and bounded buffers.
  Raw DEX inputs have a separate combined 32 MiB budget before VM construction.
- Directory metadata is capped at 16 MiB/100,000 entries. The signing detector
  reads the footer/block with an independent 16 MiB cap; presence detection is
  still not cryptographic signature verification.
- Installation copies to exclusive private staging, hashes and validates the
  stored copy, then publishes an immutable revision. Copy/hash/verification use
  64 KiB buffers. An owned writer commits once before publication, avoiding a
  disk flush for every chunk. Source APKs may be removed after installation.
- Identical content reuses its verified installed revision. File imports may
  write temporary staging while verifying the request; the legacy resident
  install API still has its tested no-write fast path.
- Launcher icon reads and runtime loading use the same file-backed APK path.
  Native libraries are extracted on demand for the requested dependency graph.
  Unused native libraries are no longer all extracted during session creation.

`Zip::raw_bytes()` now returns a typed result: a file-backed ZIP cannot provide a
resident raw slice. Callers must use positional extraction or bounded copying.
Neither fake bytes nor an implicit whole-file allocation are returned.

## Evidence

The original Java UI fixture is packaged with a 128 MiB STORED asset. Host tests
install and launch it, cross an asset chunk boundary using seek/read, save data,
restart and reuse the same installation revision. This is a large-container
proof using original fixture code, not an external-application compatibility
claim. The build receipt freezes the source APK, generated APK and generator.

Additional tests cover partial source reads, incompressible DEFLATE crossing
input buffers, CRC rejection, metadata budgets, lazy native dependencies and
48 injected update mutation boundaries. The original parser, package, launcher
and Java/Kotlin lifecycle checks remain required.

```powershell
python scripts/build-large-apk-fixture.py
cargo test -p rd-apk --lib --no-default-features --locked
cargo test -p as-package --locked
cargo test -p as-native --test loader --locked -- --include-ignored
cargo test -p as-launcher --features host --test workflow --locked -- --include-ignored
cargo test -p as-framework --features host --test lifecycle --locked -- --include-ignored
./scripts/build-androswitch.ps1
```

M1–M7 hardware testing is confirmed separately by the user in the roadmap.
The user subsequently confirms installation of a Candy Crush APK larger than
65 MB on a physical Switch with the later M7–M9 candidate. Its launch is blocked
by the separate 32 MiB combined raw DEX guard, not the APK size cap. The exact
binary hash and complete large-asset/restart/update scenario results were not
supplied. See [hardware feedback](HARDWARE_FEEDBACK.md). The new candidate has
not been executed in Ryujinx; desktop emulator launch remains opt-in after the
reported window-disappearance incident.
