param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../../../..").Path
$output=Join-Path $projectRoot 'target/portable-format-fixture'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& javac --release 8 -g:none -d "$output/classes" "$PSScriptRoot/FormatFixture.java"
if($LASTEXITCODE){throw 'Formatter fixture Java compilation failed'}
& java -cp "$output/classes" org.androswitch.vm.FormatFixture | ForEach-Object {
    [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($_)).Replace('\','\\').Replace("`n",'\n').Replace("`r",'\r')
} | Set-Content "$output/jvm-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Formatter fixture JVM oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$AndroidSdk/build-tools/35.0.0/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Formatter fixture DEX compilation failed'}
@{schema_version=1;api=26;sources=@(@(Get-Item "$PSScriptRoot/FormatFixture.java",$PSCommandPath) | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item "$output/dex/classes.dex","$output/jvm-oracle.txt" | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Get-Content "$output/jvm-oracle.txt"
