param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    if(-not $SkipBuild){& ./fixtures/conformance/build.ps1;if($LASTEXITCODE){throw 'APK build failed'}}
    $output=Join-Path $repo ('local-artifacts/conformance/host-'+[guid]::NewGuid().ToString('N').Substring(0,8))
    New-Item -ItemType Directory -Path $output|Out-Null
    $classes=Join-Path $output 'journal-classes';New-Item -ItemType Directory -Path $classes|Out-Null
    & javac --release 8 -d $classes fixtures/conformance/java/org/androswitch/conformance/Files.java fixtures/conformance/java/org/androswitch/conformance/Journal.java local-artifacts/conformance/generated/Catalog.java fixtures/conformance/selftest/JournalSelfTest.java
    if($LASTEXITCODE){throw 'Journal self-test compilation failed'}
    & java -cp $classes org.androswitch.conformance.JournalSelfTest (Join-Path $output 'journal') *> (Join-Path $output 'journal.log')
    if($LASTEXITCODE){throw 'Journal recovery/reset/escaping/status self-test failed'}
    & cargo run -p as-framework --features host --bin as-conformance --locked -- local-artifacts/conformance/AndroSwitchConformance.apk local-artifacts/conformance/catalog.json (Join-Path $output 'storage') (Join-Path $output 'report.json') *> (Join-Path $output 'host.log')
    if($LASTEXITCODE){Get-Content (Join-Path $output 'host.log') -Tail 15;throw 'Host catalog execution failed'}
    $report=Get-Content (Join-Path $output 'report.json') -Raw|ConvertFrom-Json
    $catalog=Get-Content local-artifacts/conformance/catalog.json -Raw|ConvertFrom-Json
    if($report.completed -ne $catalog.cases.Count -or @($report.cases|Group-Object id|Where-Object Count -ne 1).Count){throw 'Incomplete or duplicate host cases'}
    $journal=Get-Content (Join-Path $output 'journal/conformance/report.json') -Raw|ConvertFrom-Json
    if($journal.cases[0].detail -cne "quote`" slash\ newline`n tab`t euro$([char]0x20ac) emoji$([char]::ConvertFromUtf32(0x1f600))" -or $journal.cases[1].status -cne 'FAIL' -or $journal.cases[3].status -cne 'INTERRUPTED'){throw 'Journal JSON round-trip failed'}
    @{
        schema_version=1;status='completed_catalog_not_compatibility_pass';backend='portable_host'
        counts=@($report.cases|Group-Object status|ForEach-Object {@{status=$_.Name;count=$_.Count}})
        total=$report.completed;report=(Join-Path $output 'report.json');android_reference='not_run';hardware='not_run'
        inputs=@(Get-Item local-artifacts/conformance/AndroSwitchConformance.apk,local-artifacts/conformance/catalog.json,local-artifacts/conformance/jvm-oracle.tsv|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
        outputs=@(Get-ChildItem $output -File|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
    }|ConvertTo-Json -Depth 12|Set-Content (Join-Path $output 'verification.json') -Encoding utf8NoBOM
    Write-Output "Host catalog and journal verification: $output/verification.json"
} finally {Pop-Location}
