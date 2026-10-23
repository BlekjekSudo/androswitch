param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$output=Join-Path $projectRoot 'local-artifacts/m6/fixture'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
New-Item -ItemType Directory -Force -Path "$output/assets" | Out-Null
$bytes=[byte[]]::new(150000);for($i=0;$i -lt $bytes.Length;$i++){$bytes[$i]=$i%251};[IO.File]::WriteAllBytes("$output/assets/stream.bin",$bytes)
& python "$PSScriptRoot/make-fixture-icon.py" "$output/res/drawable/fixture_icon.png"
if($LASTEXITCODE){throw 'PNG fixture generation failed'}
& "$tools/aapt2.exe" compile --no-crunch --dir "$output/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'PNG resource compilation failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.nativemedia" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Native Graphics and Audio" android:icon="@drawable/fixture_icon" android:hasCode="false"><activity android:name="android.app.NativeActivity" android:exported="true"><meta-data android:name="android.app.lib_name" android:value="media-proof"/><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/AndroidManifest.xml" -Encoding utf8NoBOM
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -O2 -fno-emulated-tls -mno-outline-atomics -nostdlib "$projectRoot/fixtures/m6/native/activity.c" '-Wl,-soname,libmedia-proof.so' '-Wl,--no-undefined' -landroid -lEGL -lGLESv2 -laaudio -lc -o "$output/libmedia-proof.so"
if($LASTEXITCODE){throw 'NativeActivity fixture compilation failed'}
& "$tools/aapt2.exe" link -o "$output/native-media.apk" -I "$AndroidSdk/platforms/android-35/android.jar" --manifest "$output/AndroidManifest.xml" -A "$output/assets" "$output/resources.zip"
if($LASTEXITCODE){throw 'NativeActivity packaging failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/native-media.apk",[IO.Compression.ZipArchiveMode]::Update)
try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/libmedia-proof.so",'lib/arm64-v8a/libmedia-proof.so',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
New-Item -ItemType Directory -Force -Path "$output/mixed-classes","$output/mixed-dex" | Out-Null
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -O2 -fno-emulated-tls -mno-outline-atomics -nostdlib "$projectRoot/fixtures/m6/native/mixed.c" '-Wl,-soname,libmixed-media.so' '-Wl,--no-undefined' -landroid -laaudio -lOpenSLES -lc -o "$output/libmixed-media.so"
if($LASTEXITCODE){throw 'Mixed JNI compilation failed'}
& javac --release 8 -g:none -cp "$AndroidSdk/platforms/android-35/android.jar" -d "$output/mixed-classes" "$projectRoot/fixtures/m6/java/org/androswitch/media/MainActivity.java"
if($LASTEXITCODE){throw 'Mixed Java compilation failed'}
$classes=@(Get-ChildItem "$output/mixed-classes" -Recurse -File | Where-Object Extension -eq '.class' | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$output/mixed-dex" @classes
if($LASTEXITCODE){throw 'Mixed DEX compilation failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.mixedmedia" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Mixed Java and Native Audio"><activity android:name="org.androswitch.media.MainActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/mixed-manifest.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" link -o "$output/mixed-media.apk" -I "$AndroidSdk/platforms/android-35/android.jar" --manifest "$output/mixed-manifest.xml" -A "$output/assets"
if($LASTEXITCODE){throw 'Mixed APK link failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/mixed-media.apk",[IO.Compression.ZipArchiveMode]::Update)
try{foreach($item in @(@('libmixed-media.so','lib/arm64-v8a/libmixed-media.so'),@('mixed-dex/classes.dex','classes.dex'))){[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/$($item[0])",$item[1],[IO.Compression.CompressionLevel]::Optimal) | Out-Null}}finally{$zip.Dispose()}
New-Item -ItemType Directory -Force -Path "$output/unavailable-classes","$output/unavailable-dex" | Out-Null
& javac --release 8 -g:none -cp "$AndroidSdk/platforms/android-35/android.jar" -d "$output/unavailable-classes" "$projectRoot/fixtures/m6/java/org/androswitch/media/UnavailableActivity.java"
if($LASTEXITCODE){throw 'Unavailable API Java fixture failed'}
& "$tools/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$output/unavailable-dex" "$output/unavailable-classes/org/androswitch/media/UnavailableActivity.class"
if($LASTEXITCODE){throw 'Unavailable API DEX fixture failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.unavailable" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Unavailable API proof"><activity android:name="org.androswitch.media.UnavailableActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/unavailable-manifest.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" link -o "$output/unavailable.apk" -I "$AndroidSdk/platforms/android-35/android.jar" --manifest "$output/unavailable-manifest.xml"
if($LASTEXITCODE){throw 'Unavailable API APK fixture failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/unavailable.apk",[IO.Compression.ZipArchiveMode]::Update)
try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/unavailable-dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -fPIC -shared -O2 -nostdlib "$projectRoot/fixtures/m6/native/nonquiescent.c" '-Wl,-soname,libworker-proof.so' '-Wl,--no-undefined' -lc -o "$output/libworker-proof.so"
if($LASTEXITCODE){throw 'Nonquiescent worker fixture compilation failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.nonquiescent" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Nonquiescent worker proof" android:hasCode="false"><activity android:name="android.app.NativeActivity" android:exported="true"><meta-data android:name="android.app.lib_name" android:value="worker-proof"/><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/nonquiescent-manifest.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" link -o "$output/nonquiescent.apk" -I "$AndroidSdk/platforms/android-35/android.jar" --manifest "$output/nonquiescent-manifest.xml"
if($LASTEXITCODE){throw 'Nonquiescent APK packaging failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/nonquiescent.apk",[IO.Compression.ZipArchiveMode]::Update)
try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/libworker-proof.so",'lib/arm64-v8a/libworker-proof.so',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
@{schema_version=1;ndk='28.2.13676358';build_tools='35.0.0';api=26;sources=@(@(Get-ChildItem "$projectRoot/fixtures/m6" -Recurse -File)+@(Get-Item "$PSScriptRoot/build-m6-fixture.ps1","$PSScriptRoot/make-fixture-icon.py") | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*.apk" -File | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
