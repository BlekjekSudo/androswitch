param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/../../../../..").Path
$output = Join-Path $projectRoot 'target/portable-execution-fixture'
$source = Join-Path $PSScriptRoot 'ExecutionFixture.java'
$artifacts = @()
foreach ($variant in @('original', 'renamed')) {
    $variantOutput = Join-Path $output $variant
    New-Item -ItemType Directory -Force -Path "$variantOutput/classes", "$variantOutput/dex", "$variantOutput/source" | Out-Null
    $package = if ($variant -eq 'original') { 'org.androswitch.vm.execution' } else { 'org.compat.renamed.execution' }
    $javaSource = Join-Path $variantOutput 'source/ExecutionFixture.java'
    (Get-Content $source -Raw).Replace('org.androswitch.vm.execution', $package) | Set-Content $javaSource -Encoding utf8NoBOM
    & javac --release 8 -g:none -d "$variantOutput/classes" $javaSource
    if ($LASTEXITCODE) { throw "Execution $variant Java compilation failed" }
    & java -cp "$variantOutput/classes" "$package.ExecutionFixture" | Set-Content "$variantOutput/jvm-oracle.txt" -Encoding utf8NoBOM
    if ($LASTEXITCODE) { throw "Execution $variant JVM oracle failed" }
    $classes = @(Get-ChildItem "$variantOutput/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
    & "$AndroidSdk/build-tools/35.0.0/d8.bat" --min-api 26 --lib "$AndroidSdk/platforms/android-35/android.jar" --output "$variantOutput/dex" @classes
    if ($LASTEXITCODE) { throw "Execution $variant DEX compilation failed" }
    $artifacts += @(Get-Item "$variantOutput/dex/classes.dex", "$variantOutput/jvm-oracle.txt" | ForEach-Object {
        @{path=$_.FullName; sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}
    })
    Get-Content "$variantOutput/jvm-oracle.txt"
}
@{
    schema_version=1; api=26
    sources=@(Get-Item $source, $PSCommandPath | ForEach-Object {
        @{path=$_.FullName; sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}
    })
    artifacts=$artifacts
} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
