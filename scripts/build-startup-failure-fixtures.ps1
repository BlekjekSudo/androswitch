param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$output=Join-Path $projectRoot 'local-artifacts/startup-failure/fixture'
$source=Join-Path $projectRoot 'fixtures/startup-failure'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -nostdlib "$source/dependency.c" '-Wl,-soname,libunavailable-contract.so' -o "$output/libunavailable-contract.so"
if($LASTEXITCODE){throw 'Link-time dependency build failed'}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -nostdlib "$source/missing.c" '-Wl,-soname,libload-failure.so' '-Wl,--no-undefined' "-L$output" -lunavailable-contract -o "$output/missing.so"
if($LASTEXITCODE){throw 'Missing-dependency fixture build failed'}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -O2 -nostdlib "$source/worker.c" '-Wl,-soname,libload-failure.so' '-Wl,--no-undefined' -lc -o "$output/worker.so"
if($LASTEXITCODE){throw 'Failed-onload worker fixture build failed'}
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" "$source/MainActivity.java"
if($LASTEXITCODE){throw 'Startup-failure Java compilation failed'}
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" "$output/classes/org/androswitch/startup/MainActivity.class"
if($LASTEXITCODE){throw 'Startup-failure DEX compilation failed'}
foreach($spec in @(@('java','org.fixture.startupjava','Startup Java','missing.so'),@('renamed','org.example.renamedstartup','Startup Renamed','missing.so'),@('native','org.fixture.startupnative','Startup Native','missing.so'),@('worker','org.fixture.startupworker','Startup Worker','worker.so'))){
  $native=$spec[0] -eq 'native'
  $activity=if($native){'android.app.NativeActivity'}else{'org.androswitch.startup.MainActivity'}
  $metadata=if($native){'<meta-data android:name="android.app.lib_name" android:value="load-failure"/>'}else{''}
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"$($spec[2])`"><activity android:name=`"$activity`" android:exported=`"true`">$metadata<intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  $apk="$output/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'Startup-failure APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/$($spec[3])",'lib/arm64-v8a/libload-failure.so',[IO.Compression.CompressionLevel]::Optimal) | Out-Null
    if(-not $native){[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}
  }finally{$zip.Dispose()}
}
@{schema_version=1;ndk='28.2.13676358';build_tools='35.0.0';api=26;sources=@(@(Get-ChildItem $source -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*.apk" -File | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
