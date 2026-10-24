param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m9/portrait'
$output=Join-Path $projectRoot 'local-artifacts/m9/portrait-fixture'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& javac --release 8 -g:none -cp $android -d "$output/classes" "$source/java/org/androswitch/graphics/portrait/PortraitActivity.java"
if($LASTEXITCODE){throw 'Portrait fixture Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $android --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Portrait fixture DEX compilation failed'}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -std=c11 -Wall -Wextra -Werror -fPIC -shared -O2 -fno-emulated-tls -mno-outline-atomics -nostdlib "$source/native/portrait.c" '-Wl,-soname,libportraitproof.so' '-Wl,--no-undefined' -lEGL -lGLESv2 -lc -o "$output/libportraitproof.so"
if($LASTEXITCODE){throw 'Portrait fixture ARM64 JNI/GLES compilation failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.portraitproof" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Portrait GL and Touch"><activity android:name="org.androswitch.graphics.portrait.PortraitActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/AndroidManifest.xml" -Encoding utf8NoBOM
$apk="$output/portrait-proof.apk"
& "$tools/aapt2.exe" link -o $apk -I $android --manifest "$output/AndroidManifest.xml"
if($LASTEXITCODE){throw 'Portrait fixture APK packaging failed'}
$zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
try { foreach($entry in @(@('dex/classes.dex','classes.dex'),@('libportraitproof.so','lib/arm64-v8a/libportraitproof.so'))) {
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/$($entry[0])",$entry[1],[IO.Compression.CompressionLevel]::Optimal) | Out-Null
}} finally { $zip.Dispose() }
@{schema_version=1;api=26;ndk='28.2.13676358';build_tools='35.0.0';sources=@(@(Get-ChildItem $source -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item $apk,"$output/libportraitproof.so","$output/dex/classes.dex" | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});proof='Requires real EGL context/surface720x1280 matching Java callbacks, four GPU readbacks, animated frames, logical touch and persistent report. No host execution claim.'} | ConvertTo-Json -Depth 10 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output $apk
