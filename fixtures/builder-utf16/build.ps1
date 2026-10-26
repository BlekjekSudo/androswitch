param([string]$OutputDirectory='local-artifacts/builder-utf16')
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$output=[IO.Path]::GetFullPath($OutputDirectory,$repo)
New-Item -ItemType Directory -Force "$output/classes"|Out-Null
& javac --release 8 -g:none -d "$output/classes" "$PSScriptRoot/BuilderUtf16Oracle.java"
if($LASTEXITCODE){throw 'Builder JVM fixture compilation failed'}
& java -cp "$output/classes" BuilderUtf16Oracle | Set-Content "$output/jvm.tsv" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Builder JVM fixture execution failed'}
@{source_sha256=(Get-FileHash "$PSScriptRoot/BuilderUtf16Oracle.java").Hash.ToLowerInvariant();oracle_sha256=(Get-FileHash "$output/jvm.tsv").Hash.ToLowerInvariant();scope='Independent JVM raw UTF-16 buffer contracts; no additional device APK';java=@(& java -version 2>&1|ForEach-Object ToString)}|ConvertTo-Json -Depth 4|Set-Content "$output/receipt.json" -Encoding utf8NoBOM
