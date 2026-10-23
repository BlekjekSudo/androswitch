$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    New-Item -ItemType Directory -Force -Path target/evidence | Out-Null
    $null = & (Join-Path $PSScriptRoot 'fetch-fixtures.ps1') -ExtractDex
    $checks = @(
        @{id='androswitch-tests';args=@('test','-p','as-platform','-p','as-runtime','-p','as-cli','-p','as-switch-smoke','--locked')},
        @{id='real-apk-test';args=@('test','-p','as-runtime','--test','inspection','--locked','--','--ignored')},
        @{id='parser-regressions';args=@('test','-p','rd-apk','-p','rd-dex','--lib','--locked','--','--test-threads=1','--skip','paged_open_handles_sparse_multigigabyte_container')},
        @{id='dex-real-std';args=@('test','-p','rd-dex','--test','portable_index','--locked','--','--ignored')},
        @{id='dex-real-no-std';args=@('test','-p','rd-dex','--no-default-features','--test','portable_index','--locked','--','--ignored')},
        @{id='platform-no-std';args=@('check','-p','as-platform','--no-default-features','--locked')},
        @{id='dex-no-std';args=@('clippy','-p','rd-dex','--no-default-features','--lib','--locked','--','-D','warnings')},
        @{id='clippy';args=@('clippy','-p','as-platform','-p','as-runtime','-p','as-cli','-p','as-switch-smoke','--all-targets','--locked','--','-D','warnings')},
        @{id='format';args=@('fmt','--all','--','--check')}
    )
    $results = @()
    foreach ($check in $checks) {
        $arguments = $check.args
        $logPath = Join-Path $projectRoot "target/evidence/$($check.id).log"
        & cargo @arguments *> $logPath
        $code = $LASTEXITCODE
        $results += [ordered]@{id=$check.id;command=('cargo ' + ($arguments -join ' '));exit_code=$code;log=$logPath}
        $receipt = [ordered]@{schema_version=1;upstream_revision='6388281f66d254c8e01a0fdc7a5a9a30239bc762';checks=$results;completed=($results.Count -eq $checks.Count);horizon_emulator_verified=$false;physical_device_verified=$false}
        $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath target/evidence/m0-checks.json -Encoding utf8
        Write-Output "$($check.id): exit $code"
        if ($code -ne 0) { Get-Content -LiteralPath $logPath -Tail 35; throw "Check failed: $($check.id)" }
    }
} finally { Pop-Location }
