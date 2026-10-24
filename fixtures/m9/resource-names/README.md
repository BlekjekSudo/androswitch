# Resource identity contract

This original SDK fixture exercises the four `Resources.getResource*Name` APIs.
It uses the normal Android SDK and contains no AndroSwitch imports or application
profiles. Build it with:

```powershell
pwsh -File fixtures/m9/resource-names/build.ps1
```

The builder saves three ignored APKs, their `aapt2 dump resources` output, and a
source/APK hash receipt under `target/resource-names-fixture/`:

| APK | Manifest application ID | ARSC owner | Resource package ID |
| --- | --- | --- | --- |
| resources | `org.fixture.resources` | `org.fixture.resources` | `0x7f` |
| renamed | `net.example.manifestonly` | `org.fixture.resources` | `0x7f` |
| high-package | `net.example.highresourceid` | `org.fixture.resources` | `0x80` |

`--rename-manifest-package` changes the manifest alone. Resource identity must
continue to report the actual table owner, including when resource integers are
negative Java `int` values. The fixture also covers aliases, complex style entries,
localized variants, and an entry whose only value requires API 27. Names identify
the entry itself and do not depend on following its value or selecting its config.

Host DEX tests compare the parsed identity with the SDK-generated table dump and
check typed missing-ID exceptions. They are not execution on an Android reference
device. Binary parser tests separately cover legacy package headers, nonzero
`typeIdOffset`, checked ID overflow, invalid keys, and conflicting table fragments.

The runtime's explicit framework registry currently names only `android:id/content`.
Other nonzero-type Android framework IDs report unavailable framework-table
metadata. Their absence cannot be inferred without the full framework table.
