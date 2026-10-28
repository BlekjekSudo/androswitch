$ErrorActionPreference='Stop'
$repo=(Resolve-Path "$PSScriptRoot/../..").Path
$out="$repo/local-artifacts/numeric-oracle"
New-Item -ItemType Directory -Force "$out/classes"|Out-Null
& javac --release 8 -g:none -d "$out/classes" "$PSScriptRoot/StaticOracle.java"
if($LASTEXITCODE){throw 'Numeric JVM oracle compilation failed'}
& java -cp "$out/classes" StaticOracle "$PSScriptRoot/catalog.tsv"
if($LASTEXITCODE){throw 'Independent JVM numeric execution failed'}
$catalog=[IO.File]::ReadAllText("$PSScriptRoot/catalog.tsv").Replace("`r`n","`n")
[IO.File]::WriteAllText("$PSScriptRoot/catalog.tsv",$catalog,[Text.UTF8Encoding]::new($false))
@{schema=1;scope='Frozen independent JVM inputs/results; only exact inherited numeric metadata rows are compared by the VM test';source_sha256=(Get-FileHash "$PSScriptRoot/StaticOracle.java").Hash.ToLowerInvariant();catalog_sha256=(Get-FileHash "$PSScriptRoot/catalog.tsv").Hash.ToLowerInvariant();rows=@(Get-Content "$PSScriptRoot/catalog.tsv").Count;java=(& java -version 2>&1|Out-String).Trim()}|ConvertTo-Json -Depth 5|Set-Content "$PSScriptRoot/receipt.json" -Encoding utf8NoBOM
