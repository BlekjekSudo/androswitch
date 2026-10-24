param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$repo=(Resolve-Path "$PSScriptRoot/../../..").Path
$out=Join-Path $repo 'target/time-unit-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$out/classes","$out/dex"|Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File|ForEach-Object FullName)
& javac --release 8 -g:none -cp $android -d "$out/classes" @sources
if($LASTEXITCODE){throw 'TimeUnit fixture Java compilation failed'}
$oracle=@(& java -cp "$out/classes" org.androswitch.timeunits.TimeUnitCases)
if($LASTEXITCODE){throw 'TimeUnit actual JVM oracle failed'}
if($oracle.Count -ne 1995){throw "Unexpected TimeUnit JVM oracle row count $($oracle.Count)"}
$oracle|Set-Content "$out/oracle.tsv" -Encoding utf8NoBOM
$proof=@(& java -cp "$out/classes" org.androswitch.timeunits.TimeUnitCases proof)
if($LASTEXITCODE){throw 'TimeUnit JVM proof failed'}
($proof -join "`n")+"`n"|Set-Content "$out/proof.txt" -NoNewline -Encoding utf8NoBOM
$classes=@(Get-ChildItem "$out/classes" -Recurse -Filter '*.class' -File|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $android --output "$out/dex" @classes
if($LASTEXITCODE){throw 'TimeUnit fixture DEX compilation failed'}
foreach($spec in @(@('time-unit','org.fixture.timeunit'),@('renamed','net.example.timeunit.contract'))){
  '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="'+$spec[1]+'" android:versionCode="1"><uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/><uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE"/><application android:label="TimeUnit proof"><activity android:name="org.androswitch.timeunits.TimeUnitActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>'|Set-Content "$out/manifest.xml" -Encoding utf8NoBOM
  $apk="$out/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $android --manifest "$out/manifest.xml"
  if($LASTEXITCODE){throw 'TimeUnit fixture APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$out/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';jvm=((& java -version 2>&1|Out-String).Trim());oracle=@{path="$out/oracle.tsv";sha256=(Get-FileHash "$out/oracle.tsv").Hash.ToLowerInvariant();rows=$oracle.Count};proof=@{path="$out/proof.txt";sha256=(Get-FileHash "$out/proof.txt").Hash.ToLowerInvariant();vfs='/sdcard/Download/androswitch-timeunit-proof.txt'};sources=@(Get-ChildItem $PSScriptRoot -Recurse -File|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item "$out/time-unit.apk","$out/renamed.apk","$out/dex/classes.dex"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 10|Set-Content "$out/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$out/build-receipt.json"
