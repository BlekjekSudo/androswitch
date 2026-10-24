# Installed package metadata contracts

Build with `./fixtures/m9/package-info/build.ps1`, then run:

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
cargo test -p as-framework --features host --test package_info --locked -- --test-threads=1
```

The normal Android SDK proof is `target/package-info-fixture/package-info.apk`.
It displays `PACKAGE INFO PASS` and writes
`/sdcard/Download/androswitch-package-info-proof.txt`. It checks its actual
installed identity/version, a readable mounted APK header, and typed unknown/null
package failures. `renamed.apk` exercises the same DEX under another manifest
identity. There are no native test hooks or runtime package profiles.

The remaining APKs provide another installed package with a 2 MiB stored asset,
missing version defaults, an update, unsupported resource-valued versionName,
resource-valued application flags, min-only SDK, empty SDK, and absent SDK cases.
The byte hashes and source hashes are recorded in `build-receipt.json`.

`PackageManager.getPackageInfo(String,int)` currently supports `flags=0`.
It reads the committed installed record and bounded ZIP index/manifest; metadata
queries do not extract DEX, resources, native libraries, or the padded asset.
PackageInfo has actual packageName, API 26 int versionCode, nullable literal
versionName, and a fresh ApplicationInfo. Missing versionCode is 0. Missing
targetSdk defaults to the effective minSdk; an absent SDK declaration uses 1.

ApplicationInfo exposes proven package/class names, private data directory,
SDK values, label/icon/theme IDs, and nonlocalized label. Its sourceDir and
publicSourceDir are present only for the current mounted installation whose
captured revision/hash matches the requested record. Other package APK mounts
are not implemented. Updating a running package must not attach its stale alias
to newer version facts. Uninstalled or unknown packages throw
NameNotFoundException. A null package argument produces that exception with a
real null Java message.

Unproven flags, UID, native library directory, install/update times, signatures,
and component arrays remain unavailable fields. Nonzero query flags and
resource-valued versionName are explicit unsupported operations. Resource-valued
application booleans do not prevent access to unrelated proven fields. The
preexisting Context.getApplicationInfo flags subset remains a separate legacy
limitation and is not copied into fresh PackageInfo results.

Application code receives mutable metadata snapshots; changing a snapshot does
not change the registry. The GC follows real PackageInfo → ApplicationInfo/string
field references and does not retain discarded snapshots in a service cache.
Metadata queries validate the file size and manifest identity/version against
the immutable record. They do not repeat the full APK hash/signature verification
performed by the package load path.

The API 26 reference contracts are
[ApplicationPackageManager](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-8.0.0_r1/core/java/android/app/ApplicationPackageManager.java),
[PackageParser](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-8.0.0_r1/core/java/android/content/pm/PackageParser.java),
and [uses-sdk defaults](https://developer.android.com/guide/topics/manifest/uses-sdk-element).
Host contract tests are separate from Horizon emulator and physical-device
verification; the host receipt makes no external-game first-screen claim.
