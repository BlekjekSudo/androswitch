param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/gc-owners-fixture'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& javac --release 8 -g:none -cp $android -d "$output/classes" "$PSScriptRoot/java/org/androswitch/contracts/GcOwnersActivity.java"
if($LASTEXITCODE){throw 'GC owners fixture Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $android --output "$output/dex" @classes
if($LASTEXITCODE){throw 'GC owners fixture DEX compilation failed'}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -std=c11 -Wall -Wextra -Werror -fPIC -shared -O2 -fno-emulated-tls -mno-outline-atomics -nostdlib "$PSScriptRoot/native/gc_owners.c" '-Wl,-soname,libgcownersproof.so' '-Wl,--no-undefined' -lc -o "$output/libgcownersproof.so"
if($LASTEXITCODE){throw 'GC owners fixture ARM64 JNI compilation failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.gcownersproof" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="GC native suspension proof"><activity android:name="org.androswitch.contracts.GcOwnersActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/AndroidManifest.xml" -Encoding utf8NoBOM
$apk="$output/gc-owners-proof.apk"
& "$tools/aapt2.exe" link -o $apk -I $android --manifest "$output/AndroidManifest.xml"
if($LASTEXITCODE){throw 'GC owners fixture APK packaging failed'}
$zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
try { foreach($entry in @(@('dex/classes.dex','classes.dex'),@('libgcownersproof.so','lib/arm64-v8a/libgcownersproof.so'))) {
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/$($entry[0])",$entry[1],[IO.Compression.CompressionLevel]::Optimal) | Out-Null
}} finally { $zip.Dispose() }
@{schema_version=1;api=26;ndk='28.2.13676358';build_tools='35.0.0';sources=@(Get-ChildItem $PSScriptRoot -Recurse -File | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item $apk,"$output/libgcownersproof.so","$output/dex/classes.dex" | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});proof='Two real Java workers enter native barriers; A resumes, forces GC and fully unwinds its local-only-object frame before releasing B. B forces GC and validates its suspended local. Requires actual execution and clean teardown; building alone is not proof.'} | ConvertTo-Json -Depth 10 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output $apk
