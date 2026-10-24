param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/resource-names-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex","$output/generated" | Out-Null
& "$tools/aapt2.exe" compile --dir "$PSScriptRoot/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'Resource names compile failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.resources" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><application android:label="Resource metadata"><activity android:name="org.androswitch.contracts.ResourceNamesActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/manifest.xml" -Encoding utf8NoBOM
foreach($spec in @(@('resources','org.fixture.resources','0x7f'),@('renamed','net.example.manifestonly','0x7f'),@('high-package','net.example.highresourceid','0x80'))){
  & "$tools/aapt2.exe" link -o "$output/$($spec[0]).apk" -I $androidJar --manifest "$output/manifest.xml" --rename-manifest-package $spec[1] --package-id $spec[2] --custom-package org.androswitch.contracts --java "$output/generated" "$output/resources.zip"
  if($LASTEXITCODE){throw 'Resource names APK link failed'}
  # R constants are rebuilt per package ID; this preserves the exact SDK output.
  $sources=@(Get-ChildItem "$PSScriptRoot/java","$output/generated" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
  & javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
  if($LASTEXITCODE){throw 'Resource names Java compilation failed'}
  & java -cp "$output/classes" org.androswitch.contracts.IdentifierNumbers | Set-Content "$output/identifier-jvm-oracle.txt" -Encoding utf8NoBOM
  if($LASTEXITCODE){throw 'Resource identifier JVM oracle failed'}
  $classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
  & "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
  if($LASTEXITCODE){throw 'Resource names DEX compilation failed'}
  $zip=[IO.Compression.ZipFile]::Open("$output/$($spec[0]).apk",[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
  & "$tools/aapt2.exe" dump resources "$output/$($spec[0]).apk" | Set-Content "$output/$($spec[0])-resources.txt" -Encoding utf8NoBOM
  if($LASTEXITCODE){throw 'Resource names table dump failed'}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$PSScriptRoot/java","$PSScriptRoot/res" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-Item "$output/resources.apk","$output/renamed.apk","$output/high-package.apk"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});oracle=@{path="$output/identifier-jvm-oracle.txt";sha256=(Get-FileHash "$output/identifier-jvm-oracle.txt").Hash.ToLowerInvariant()}}|ConvertTo-Json -Depth 8|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
