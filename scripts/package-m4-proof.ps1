param([string]$OutputRoot='local-artifacts/m4/hardware')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$output=if([IO.Path]::IsPathRooted($OutputRoot)){$OutputRoot}else{Join-Path $projectRoot $OutputRoot}
$session=[guid]::NewGuid().ToString('N')
$runRoot=Join-Path $output $session
$fixture=Join-Path $projectRoot 'local-artifacts/m4/fixture'
$nro=Join-Path $projectRoot 'local-artifacts/m1/androswitch-smoke.nro'
if(-not(Test-Path -LiteralPath $nro)){throw 'Build and verify the NRO first.'}
$packages=@()
foreach($spec in @(@('java','java-jni.apk','java-request.json','java-expected.json'),@('kotlin','kotlin-jni.apk','kotlin-request.json','kotlin-expected.json'),@('emutls','java-jni-emutls.apk','emutls-request.json','java-expected.json'))){
    $name,$apkName,$requestName,$expectedName=$spec
    $packageRoot=Join-Path $runRoot $name
    $sd=Join-Path $packageRoot 'switch/androswitch-smoke'
    New-Item -ItemType Directory -Force -Path $sd | Out-Null
    Copy-Item -LiteralPath $nro -Destination "$sd/androswitch-smoke.nro"
    Copy-Item -LiteralPath "$fixture/$apkName" -Destination "$sd/app.apk"
    $zip=[IO.Compression.ZipFile]::OpenRead("$fixture/$apkName")
    try{[IO.Compression.ZipFileExtensions]::ExtractToFile($zip.GetEntry('classes.dex'),"$sd/classes.dex")}finally{$zip.Dispose()}
    $request=Get-Content -LiteralPath "$fixture/$requestName" -Raw | ConvertFrom-Json
    $request.session_id="hardware-$name-$session"
    $request | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath "$sd/request.json" -Encoding utf8NoBOM
    Copy-Item -LiteralPath "$fixture/$expectedName" -Destination "$packageRoot/expected.json"
    @"
AndroSwitch M4 developer proof ($name)

Extract switch/ to the SD card, then start androswitch-smoke.nro using
homebrew application/title override. This runs selected APK DEX/JNI methods;
it is not an Activity launcher. Press + after the report has been written.

Collect switch/androswitch-smoke/probe-result.json and smoke.json.
The session_id must be $($request.session_id).
Compare individual calls with expected.json. The Java/emutls fixture's
MonitorEnter error is intentional and must say JNI_API_UNAVAILABLE.

Installed proof packages and save files are created under switch/androswitch/.
Updates retain data. Do not delete an existing AndroSwitch user-data directory.
Install/run one proof ZIP at a time; each uses the same developer input folder.

Hardware status before this package: NOT RUN. A generated ZIP is not evidence
that this has passed on a physical Switch.

Based on RustyDroid by nadabgfccvd and contributors. See CREDITS.md and NOTICE.md.
"@ | Set-Content -LiteralPath "$packageRoot/READ-ME.txt" -Encoding utf8NoBOM
    foreach($file in @('CREDITS.md','NOTICE.md','LICENSE-MIT','LICENSE-APACHE')){Copy-Item -LiteralPath "$projectRoot/$file" -Destination "$packageRoot/$file"}
    New-Item -ItemType Directory -Force -Path "$packageRoot/third_party/android" | Out-Null
    Copy-Item -LiteralPath "$projectRoot/third_party/so_util.LICENSE" -Destination "$packageRoot/third_party/so_util.LICENSE"
    Copy-Item -LiteralPath "$projectRoot/third_party/README.md" -Destination "$packageRoot/third_party/README.md"
    Copy-Item -LiteralPath "$projectRoot/third_party/android/jni.h" -Destination "$packageRoot/third_party/android/jni.h"
    $archive=Join-Path $runRoot "androswitch-m4-$name-proof.zip"
    [IO.Compression.ZipFile]::CreateFromDirectory($packageRoot,$archive)
    $packages+=@{name=$name;session_id=$request.session_id;file=$archive;sha256=(Get-FileHash -LiteralPath $archive).Hash.ToLowerInvariant();nro_sha256=(Get-FileHash -LiteralPath $nro).Hash.ToLowerInvariant();physical_device='not_run'}
}
@{schema_version=1;created_at=[DateTime]::UtcNow.ToString('o');packages=$packages} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$runRoot/packages.json" -Encoding utf8NoBOM
Write-Output "$runRoot/packages.json"
