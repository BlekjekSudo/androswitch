# Ryujinx guarded system-register diagnostic

This optional host tool investigates a Ryujinx translator defect. It is separate
from the AndroSwitch runtime and release package. It does not modify an APK,
Android library, CPU capability report, installed emulator, or emulator profile.

Ryujinx at `e2143d43bcb6762340d8a01f20e7b5fdf104f02f` throws a host
`NotImplementedException` while translating any unknown `MRS`, including an
instruction behind an untaken capability guard. The patch uses Ryujinx's existing
runtime undefined-instruction path. An executed unsupported read still faults;
it never receives an invented register value. The patch covers unknown `MRS`
only; unknown `MSR` and other translator limitations remain unchanged.

## Source and credit

Ryujinx and ARMeilleure are work of the Ryujinx contributors, distributed under
the [MIT license](Ryujinx.LICENSE.txt). The source revision above is preserved in this
[GitHub fork](https://github.com/wangxiaoming1988/Ryujinx-fork/tree/e2143d43bcb6762340d8a01f20e7b5fdf104f02f).
The original Ryubing source host was unreachable during this investigation.
`mrs-runtime-undefined.patch` contains the small local change; no emulator binary
is stored in this repository. Preserve Ryujinx's `LICENSE.txt` and `THIRDPARTY.md`
with any local diagnostic build.

The bundle helper follows the .NET Foundation's published
[manifest](https://github.com/dotnet/runtime/blob/main/src/installer/managed/Microsoft.NET.HostModel/Bundle/Manifest.cs)
and [file-entry](https://github.com/dotnet/runtime/blob/main/src/installer/managed/Microsoft.NET.HostModel/Bundle/FileEntry.cs)
formats. Those sources are MIT licensed. AndroSwitch's Android VM remains based
on [RustyDroid](https://github.com/nadabgfccvd/RustyDroid); this emulator diagnostic
does not replace the upstream attribution in the main repository.

## Reproduce the translator test

Use an isolated source checkout inside `local-artifacts/`, an installed .NET SDK
capable of targeting `net9.0`, and the matching .NET runtime. SDK 10.0.100 targeting
.NET 9 was used for the recorded probe. The test does not launch the emulator UI,
use a profile, or require firmware, keys, or an APK.

From the repository root, in PowerShell (set `$dotnetTool` to your SDK executable):

```powershell
$repoRoot = (Get-Location).Path
$sourcePath = Join-Path $repoRoot 'local-artifacts/m9/ryujinx-cfg-fix/source'
New-Item -ItemType Directory -Path $sourcePath -Force | Out-Null
git -C $sourcePath init
git -C $sourcePath remote add origin https://github.com/wangxiaoming1988/Ryujinx-fork.git
git -C $sourcePath fetch --depth=1 origin e2143d43bcb6762340d8a01f20e7b5fdf104f02f
git -C $sourcePath checkout --detach FETCH_HEAD
$dotnetTool = 'path/to/SDK/dotnet.exe'
$runtimeTool = 'path/to/dotnet-with-NET9-runtime/dotnet.exe'
& $dotnetTool build tools/ryujinx-cfg/CfgProbe.csproj -c Release "-p:RyujinxSource=$sourcePath" -p:Version=1.3.3 -p:AssemblyVersion=1.3.3.0 -p:FileVersion=1.3.3.0 -p:InformationalVersion=1.3.3+e2143d4.androswitch-mrs -p:IncludeSourceRevisionInInformationalVersion=false --source https://api.nuget.org/v3/index.json -p:NuGetAudit=false
```

Before applying the patch, running the probe with `guarded d5380429` reproduces
the emit-time exception even though bit 11 is clear in the supplied capability
value. Apply the patch and rebuild the same project:

```powershell
git -C $sourcePath apply "$repoRoot/tools/ryujinx-cfg/mrs-runtime-undefined.patch"
& $dotnetTool build tools/ryujinx-cfg/CfgProbe.csproj -c Release "-p:RyujinxSource=$sourcePath" -p:Version=1.3.3 -p:AssemblyVersion=1.3.3.0 -p:FileVersion=1.3.3.0 -p:InformationalVersion=1.3.3+e2143d4.androswitch-mrs -p:IncludeSourceRevisionInInformationalVersion=false --no-restore
$probePath = Join-Path $repoRoot 'local-artifacts/m9/ryujinx-cfg-fix/probe-build/bin/Release/net9.0/CfgProbe.dll'
foreach ($word in @('d5380429','d5380409','d5380489','d5380609','d5380629')) {
    foreach ($mode in @('guarded','execute')) {
        & $runtimeTool $probePath $mode $word
        & $runtimeTool $probePath $mode $word unmanaged
    }
}
```

Before creating a bundle, verify the assembly identity:

```powershell
& $runtimeTool $probePath identity "$sourcePath/src/ARMeilleure/bin/Release/net9.0/ARMeilleure.dll"
```

The stock assembly and its Ryujinx dependencies use version `1.3.3.0`. The
build flags above preserve those identities; a default `1.0.0.0` build cannot
be substituted into the stock bundle.

Each CPU case reports JSON. Guarded cases must complete with no undefined callback.
Executed cases must report the exact fault address and instruction, leave the
read destination unchanged, and skip the following marker instruction. Both
managed and unmanaged dispatch loops are checked. These are synthetic CPU tests,
not evidence of complete Android application startup or physical Switch support.

## Optional isolated emulator bundle

The exact full source publish needs `Ryujinx.LibHac` version
`0.21.0-alpha.116`, which was unavailable from the reachable public feed. The
recorded diagnostic therefore preserves the stock 1.3.3 single-file executable
and replaces only `ARMeilleure.dll` with the exact-source assembly built above.
It is a patched stock bundle, not a full clean emulator rebuild.

`rebundle.py` accepts a v6 .NET single-file bundle, a replacement assembly, a **new**
output path, and the expected original SHA-256. It refuses existing output paths
and leaves the original untouched. It preserves the other payloads and gives the
new bundle a different extraction identity. No profile, keys, firmware, or cache
is copied. For the recorded stock executable:

```powershell
python tools/ryujinx-cfg/rebundle.py `
    'path/to/original/Ryujinx.exe' `
    "$sourcePath/src/ARMeilleure/bin/Release/net9.0/ARMeilleure.dll" `
    'local-artifacts/m9/ryujinx-cfg-fix/publish/Ryujinx.exe' `
    --expected-sha cc8826b9ed4a72299c4db48fae077f5a3e659bb03671b7c40cb094d8e727cde0
```

Copy only the original emulator's native support DLLs, `LICENSE.txt`,
`THIRDPARTY.md`, and optional `alsoft.ini` beside this new executable. Use a new,
explicit `--root-data-dir` for an emulator run. Keep evidence from this diagnostic
separate from stock Ryujinx results. Do not replace the user's emulator or reuse
a profile implicitly. `--no-gui --help` reaches the command-line help parser
before configuration loading. Help alone does not resolve every emulator subsystem;
a real NRO run is a separate validation step.
