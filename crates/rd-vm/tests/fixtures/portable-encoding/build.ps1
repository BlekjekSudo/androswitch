param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../../../..").Path
$output=Join-Path $projectRoot 'target/portable-encoding-fixture'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& javac --release 8 -encoding UTF-8 -g:none -d "$output/classes" "$PSScriptRoot/EncodingFixture.java"
if($LASTEXITCODE){throw 'Encoding fixture Java compilation failed'}
& java '-Dfile.encoding=UTF-8' -cp "$output/classes" org.androswitch.vm.EncodingFixture | Set-Content "$output/jvm-oracle.tsv" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Encoding fixture JVM oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$AndroidSdk/build-tools/35.0.0/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Encoding fixture DEX compilation failed'}
@{schema_version=1;api=26;default_charset='UTF-8 (explicit JVM -Dfile.encoding=UTF-8)';android_reference='android-8.0.0_r1';jvm_equal_rows=31;source_derived_android_replacement_rows=@(31,32);sources=@(@(Get-Item "$PSScriptRoot/EncodingFixture.java",$PSCommandPath) | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item "$output/dex/classes.dex","$output/jvm-oracle.tsv" | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Get-Content "$output/jvm-oracle.tsv"
