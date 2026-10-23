param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
& python "$PSScriptRoot/fetch-m9-corpus.py"
if($LASTEXITCODE){throw 'Pinned independent source preparation failed'}
$source="$projectRoot/local-artifacts/m9/aosp-source"
$output="$projectRoot/local-artifacts/m9/aosp-hello"
$tools="$AndroidSdk/build-tools/35.0.0";$android="$AndroidSdk/platforms/android-35/android.jar"
New-Item -ItemType Directory -Force "$output/generated","$output/classes","$output/dex"|Out-Null
& "$tools/aapt2.exe" compile --dir "$source/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'AOSP resource compilation failed'}
& "$tools/aapt2.exe" link -o "$output/hello-activity.apk" -I $android --manifest "$source/AndroidManifest.xml" --java "$output/generated" "$output/resources.zip"
if($LASTEXITCODE){throw 'AOSP original manifest/resources link failed'}
$java=@(Get-ChildItem "$source/src","$output/generated" -Recurse -Filter '*.java' -File|ForEach-Object FullName)
& javac --release 8 -g:none -cp $android -d "$output/classes" @java
if($LASTEXITCODE){throw 'AOSP unmodified Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $android --output "$output/dex" @classes
if($LASTEXITCODE){throw 'AOSP DEX compilation failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/hello-activity.apk",[IO.Compression.ZipArchiveMode]::Update)
try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
@{schema_version=1;source_lock='fixtures/m9/external.lock.json';source_modifications=$false;build_tools='35.0.0';compile_sdk=35;d8_min_api=26;manifest_min_sdk_modified=$false;java_release=8;apk_sha256=(Get-FileHash "$output/hello-activity.apk").Hash.ToLowerInvariant();build_script_sha256=(Get-FileHash $PSCommandPath).Hash.ToLowerInvariant()}|ConvertTo-Json -Depth 6|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
