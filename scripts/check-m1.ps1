param([switch]$SkipFixtureBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $SkipFixtureBuild) { & (Join-Path $PSScriptRoot 'build-m1-fixture.ps1') | Out-Host }
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path target/evidence | Out-Null
    $checks = @(
        @{id='m1-portable-tests';args=@('test','-p','as-switch-smoke','--features','portable-tests','--test','portable_core','--locked','--','--include-ignored')},
        @{id='m1-core-clippy';args=@('clippy','-p','rd-apk','-p','rd-vm','--no-default-features','--lib','--locked','--','-D','warnings')},
        @{id='m1-smoke-clippy';args=@('clippy','-p','as-switch-smoke','--features','host-cli,portable-tests','--all-targets','--locked','--','-D','warnings')}
    )
    $results = @()
    foreach ($check in $checks) {
        $arguments = $check.args
        $logPath = Join-Path $projectRoot "target/evidence/$($check.id).log"
        & cargo @arguments *> $logPath
        $code = $LASTEXITCODE
        $results += [ordered]@{id=$check.id;command=('cargo ' + ($arguments -join ' '));exit_code=$code;log=$logPath}
        [ordered]@{schema_version=1;checks=$results;completed=($results.Count -eq $checks.Count);physical_device='not_run'} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath target/evidence/m1-checks.json -Encoding utf8NoBOM
        Write-Output "$($check.id): exit $code"
        if ($code -ne 0) { Get-Content -LiteralPath $logPath -Tail 40; throw "M1 check failed: $($check.id)" }
    }
} finally { Pop-Location }
