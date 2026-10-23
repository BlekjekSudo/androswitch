# AndroSwitch

A general Android application runtime for Nintendo Switch HorizonOS, derived
from RustyDroid. Implement shared Android API/ABI behavior without package-specific
hooks or game profiles.

**M0 and M1 complete for the host/emulator gates.** The shared no_std APK/DEX
parsers and upstream Dalvik interpreter execute a Java APK method suite in
Ryujinx: 12 results match the JVM, and six negative cases report structured
errors. Installation, Android lifecycle and JNI are not implemented yet. Inherited
RustyDroid features do not imply compatibility on Switch.

## Quick start (Windows host)

```powershell
cargo run -p as-cli -- capabilities --json
.\scripts\fetch-fixtures.ps1
cargo run -p as-cli -- inspect fixtures/cache/org.fdroid.fdroid_2000052.apk --json
cargo test -p as-platform -p as-runtime -p as-cli
cargo test -p as-runtime --test inspection -- --ignored
```

Inspection reports the manifest, DEX/native/resource inventory, launchers,
detected signing schemes, and static SDK/ABI/split checks. Static checks do not
prove execution. Signing detection is not cryptographic verification. Lower
minSdk remains eligible for the API 26 reference; targetSdk is not a minimum.

## Target and progress

- ARM64 native execution, DEX interpreter, bidirectional JNI.
- Offline v0.1, one foreground app, console launcher.
- Persistent Java/native storage under `switch/androswitch/`.
- Device core: `no_std + alloc` and libnx; no Python/Unicorn/guest Linux.
- Separate implementation, host, emulator and physical-device evidence.

See the [roadmap](docs/ANDROSWITCH_ROADMAP.md),
[architecture](docs/ARCHITECTURE.md),
[capability inventory](docs/ANDROSWITCH_COMPATIBILITY.md), and
[frozen fixtures](fixtures/corpus.lock.json).

Run the local Ryujinx E2E gate with `./scripts/test-ryujinx-m1.ps1` (Windows,
WSL/libnx, pinned nightly, Android SDK 35 build tools and a JDK). It builds the
fixture and NRO, runs a fresh session, compares actual SD reports and closes
only its own emulator process. Host checks: `./scripts/check-m1.ps1`.

Verification: [M0 baseline](docs/M0_BASELINE.md), [M0 results](reports/m0.json),
[M1 E2E procedure](docs/M1_E2E.md), and [M1 observed results](reports/m1-core.json).
The AndroSwitch gates pass; the pinned, unmodified upstream baseline has five
documented MCP failures. Physical Switch verification is pending.

## Upstream and licensing

History-preserving fork of [RustyDroid](https://github.com/nadabgfccvd/RustyDroid)
at `6388281f66d254c8e01a0fdc7a5a9a30239bc762`. Upstream attribution is retained.
AndroSwitch additions use the same MIT OR Apache-2.0 license. See
[notices](NOTICE.md), [MIT](LICENSE-MIT), and [Apache-2.0](LICENSE-APACHE).

[Preserved upstream README](README.rustydroid.md), `RustyDroid.md`, the original
roadmap and `rd` tools describe upstream. Use `asctl` and the AndroSwitch docs
for this project's actual status.
