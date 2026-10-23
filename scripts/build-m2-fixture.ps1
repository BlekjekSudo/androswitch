param([string]$AndroidSdk = "$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $projectRoot 'fixtures/m2/java/org/androswitch/fixture/StorageProbe.java'
$output = Join-Path $projectRoot 'local-artifacts/m2/fixture'
$classes = Join-Path $output 'classes'
$dex = Join-Path $output 'dex'
New-Item -ItemType Directory -Force -Path $classes,$dex | Out-Null
$tools = Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar = Join-Path $AndroidSdk 'platforms/android-35/android.jar'
& javac --release 8 -g:none -d $classes $source
if ($LASTEXITCODE) { throw 'javac failed' }
$classFiles = @(Get-ChildItem -LiteralPath $classes -Recurse -Filter '*.class' | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 24 --lib $androidJar --output $dex @classFiles
if ($LASTEXITCODE) { throw 'D8 failed' }
$receipts = @()
foreach ($spec in @(@('org.fixture.alpha',1),@('org.fixture.alpha',2),@('org.fixture.beta',1))) {
    $name = $spec[0]; $version = $spec[1]
    $manifest = Join-Path $output "$name-$version.xml"
    '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="' + $name + '" android:versionCode="' + $version + '"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="35"/><application android:label="Storage fixture"/></manifest>' | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM
    $apk = Join-Path $output "$name-$version.apk"
    & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest $manifest
    if ($LASTEXITCODE) { throw 'aapt2 failed' }
    $zip = [IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
    try { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null } finally { $zip.Dispose() }
    $receipts += [ordered]@{package=$name;version_code=$version;apk=$apk;sha256=(Get-FileHash -LiteralPath $apk).Hash.ToLowerInvariant()}
}
[ordered]@{schema_version=1;source_sha256=(Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant();java=((& java -version 2>&1 | Out-String).Trim());build_tools='35.0.0';apks=$receipts} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$output/build-receipt.json" -Encoding utf8NoBOM
Get-Item -LiteralPath "$output/build-receipt.json"
