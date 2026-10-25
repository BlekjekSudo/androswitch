param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../../../..").Path
$output=Join-Path $projectRoot 'target/portable-atomic-fixture'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
& javac --release 8 -g:none -d "$output/classes" "$PSScriptRoot/AtomicFixture.java" "$PSScriptRoot/ReferenceFixture.java"
if($LASTEXITCODE){throw 'Atomic fixture Java compilation failed'}
& java -cp "$output/classes" org.androswitch.vm.AtomicFixture | Set-Content "$output/jvm-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Atomic fixture JVM oracle failed'}
& java -cp "$output/classes" org.androswitch.vm.ReferenceFixture | Set-Content "$output/reference-oracle.tsv" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'AtomicReference fixture JVM oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$AndroidSdk/build-tools/35.0.0/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Atomic fixture DEX compilation failed'}
@{schema_version=1;api=26;sources=@(@(Get-Item "$PSScriptRoot/AtomicFixture.java","$PSScriptRoot/ReferenceFixture.java",$PSCommandPath) | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-Item "$output/dex/classes.dex","$output/jvm-oracle.txt","$output/reference-oracle.tsv" | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Get-Content "$output/jvm-oracle.txt"
