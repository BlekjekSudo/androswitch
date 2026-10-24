param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk",[string]$HostGcc='C:/w64devkit/bin/gcc.exe')
$ErrorActionPreference='Stop'
$root=(Resolve-Path "$PSScriptRoot/../../..").Path
$out=Join-Path $root 'target/jni-constructor-fixture'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$out/classes","$out/dex","$out/host"|Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java'|ForEach-Object FullName)
& javac --release 8 -g:none -cp $android -d "$out/classes" @sources
if($LASTEXITCODE){throw 'Constructor fixture Java compilation failed'}
& $HostGcc -std=c11 -Wall -Wextra -Werror -O1 -shared "-I$root/third_party/android" "$PSScriptRoot/native/construct.c" -lpthread -o "$out/host/constructproof.dll"
if($LASTEXITCODE){throw 'Constructor JVM JNI library compilation failed'}
$oracle=@(& java "-Djava.library.path=$out/host" -cp "$out/classes" org.androswitch.contracts.ConstructorCases)
if($LASTEXITCODE){throw 'Constructor JVM JNI oracle failed'}
foreach($form in 0..2){foreach($expected in @("value`t$form`t-1985187208","reentry`t$form`t-1985187171","exception`t$form`tconstructor failure","nativeCatch`t$form`t71")){
  if($expected -notin $oracle){throw "Visible constructor fixture expectation disagrees with actual JVM oracle: $expected"}
}}
foreach($expected in @("threads`tidentity`t89","threads`tattached`t97","threads`tcontext`t103")){if($expected -notin $oracle){throw "Thread reference JNI oracle failed: $expected"}}
if("loader`tinheritance`t107" -notin $oracle){throw 'Thread context inheritance JVM oracle failed'}
$oracle|Set-Content "$out/jvm-oracle.txt" -Encoding utf8NoBOM
$classes=@(Get-ChildItem "$out/classes" -Recurse -Filter '*.class'|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $android --output "$out/dex" @classes
if($LASTEXITCODE){throw 'Constructor fixture DEX compilation failed'}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -std=c11 -Wall -Wextra -Werror -O1 -fPIC -shared -fno-emulated-tls -mno-outline-atomics -nostdlib "$PSScriptRoot/native/construct.c" '-Wl,-soname,libconstructproof.so' '-Wl,--no-undefined' -lc -o "$out/libconstructproof.so"
if($LASTEXITCODE){throw 'Constructor fixture ARM64 JNI library compilation failed'}
foreach($spec in @(@('constructor-proof','org.fixture.constructorproof'),@('renamed','net.example.construct.contract'))){
  '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="'+$spec[1]+'" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="JNI constructors"><activity android:name="org.androswitch.contracts.ConstructorActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>'|Set-Content "$out/manifest.xml" -Encoding utf8NoBOM
  $apk="$out/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $android --manifest "$out/manifest.xml"
  if($LASTEXITCODE){throw 'Constructor fixture APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{foreach($entry in @(@('dex/classes.dex','classes.dex'),@('libconstructproof.so','lib/arm64-v8a/libconstructproof.so'))){
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$out/$($entry[0])",$entry[1],[IO.Compression.CompressionLevel]::Optimal)|Out-Null
  }}finally{$zip.Dispose()}
}
@{schema_version=1;ndk='28.2.13676358';api=26;build_tools='35.0.0';jvm=((& java -version 2>&1|Out-String).Trim());oracle=@{path="$out/jvm-oracle.txt";sha256=(Get-FileHash "$out/jvm-oracle.txt").Hash.ToLowerInvariant();cases=$oracle.Count};sources=@(Get-ChildItem $PSScriptRoot -Recurse -File|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item "$out/constructor-proof.apk","$out/renamed.apk","$out/libconstructproof.so"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 10|Set-Content "$out/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$out/constructor-proof.apk"
