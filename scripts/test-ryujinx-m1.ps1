param(
    [string]$Ryujinx = 'D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile = "$env:APPDATA/Ryujinx",
    [int]$TimeoutSeconds = 180,
    [switch]$SkipBuild,
    [switch]$SkipFixtureBuild
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$artifactRoot = Join-Path $projectRoot 'local-artifacts/m1'
$fixtureRoot = Join-Path $artifactRoot 'fixture'
$runId = [guid]::NewGuid().ToString('N')
$runRoot = Join-Path $artifactRoot "runs/$runId"
$sdRoot = Join-Path $RyujinxProfile 'sdcard/switch/androswitch-smoke'
if (-not (Test-Path -LiteralPath $Ryujinx -PathType Leaf)) { throw "Ryujinx missing: $Ryujinx" }
New-Item -ItemType Directory -Force -Path $runRoot,$sdRoot | Out-Null
# Exclusive ownership of this test directory; never accept a concurrent/stale run.
$lockPath = Join-Path $sdRoot 'e2e.lock'
$lock = [IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process = $null
$result = [ordered]@{schema_version=1;session_id=$runId;status='running';backend='horizon_emulator';physical_device='not_run';started_at=[DateTime]::UtcNow.ToString('o')}
$receipt = Join-Path $runRoot 'verification.json'
try {
    if (-not $SkipFixtureBuild) { & (Join-Path $PSScriptRoot 'build-m1-fixture.ps1') | Out-Host }
    if (-not $SkipBuild) { & (Join-Path $PSScriptRoot 'build-switch-smoke.ps1') | Out-Host }
    & (Join-Path $PSScriptRoot 'prepare-switch-smoke.ps1') | Out-Host
    $request = Get-Content -LiteralPath (Join-Path $fixtureRoot 'request.json') -Raw | ConvertFrom-Json
    $request.session_id = $runId
    $requestPath = Join-Path $runRoot 'request.json'
    $request | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $requestPath -Encoding utf8NoBOM
    $apkPath = Join-Path $fixtureRoot 'vm-semantics.apk'
    $nroPath = Join-Path $sdRoot 'androswitch-smoke.nro'
    Copy-Item -LiteralPath (Join-Path $artifactRoot 'androswitch-smoke.nro') -Destination $nroPath
    Copy-Item -LiteralPath (Join-Path $artifactRoot 'package/switch/androswitch-smoke/classes.dex') -Destination (Join-Path $sdRoot 'classes.dex')
    Copy-Item -LiteralPath $apkPath -Destination (Join-Path $sdRoot 'app.apk')
    Copy-Item -LiteralPath $requestPath -Destination (Join-Path $sdRoot 'request.json')
    $packageDir = Join-Path $artifactRoot 'package/switch/androswitch-smoke'
    Copy-Item -LiteralPath $apkPath -Destination (Join-Path $packageDir 'app.apk')
    Copy-Item -LiteralPath $requestPath -Destination (Join-Path $packageDir 'request.json')
    foreach ($name in @('smoke.json','probe-result.json')) {
        $old = Join-Path $sdRoot $name
        if (Test-Path -LiteralPath $old) { Move-Item -LiteralPath $old -Destination (Join-Path $runRoot "previous-$name") }
    }
    Push-Location -LiteralPath $projectRoot
    try {
        $hostJson = & cargo run --quiet -p as-switch-smoke --features host-cli --bin as-probe -- $apkPath $requestPath
        if ($LASTEXITCODE -ne 0) { throw 'Portable host interpreter failed.' }
    } finally { Pop-Location }
    $hostJson | Set-Content -LiteralPath (Join-Path $runRoot 'host.json') -Encoding utf8NoBOM
    $hostReport = ($hostJson -join "`n") | ConvertFrom-Json
    if (-not $hostReport.portable_vm) { throw 'Host reference linked desktop VM; portable proof is invalid.' }
    $process = Start-Process -FilePath $Ryujinx -ArgumentList @($nroPath) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'ryujinx.stdout.log') -RedirectStandardError (Join-Path $runRoot 'ryujinx.stderr.log') -PassThru
    $result.pid = $process.Id
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $receipt -Encoding utf8NoBOM
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $reportPath = Join-Path $sdRoot 'probe-result.json'
    $observed = $null
    do {
        if ($process.HasExited) { throw "Ryujinx exited before reporting (exit $($process.ExitCode))." }
        if (Test-Path -LiteralPath $reportPath) {
            try { $observed = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json; if ($observed.session_id -eq $runId) { break } } catch { }
        }
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($null -eq $observed -or $observed.session_id -ne $runId) { throw 'Timed out waiting for a fresh emulator report.' }
    Copy-Item -LiteralPath $reportPath -Destination (Join-Path $runRoot 'emulator.json')
    Copy-Item -LiteralPath (Join-Path $sdRoot 'smoke.json') -Destination (Join-Path $runRoot 'dex-parser.json')
    & (Join-Path $PSScriptRoot 'verify-switch-smoke.ps1') -ReportPath (Join-Path $runRoot 'dex-parser.json') -Backend horizon_emulator | Out-Null
    foreach ($field in @('kind','package','min_sdk','target_sdk','dex_files','portable_vm','application_lifecycle','jni')) {
        if ($observed.$field -cne $hostReport.$field) { throw "Host/emulator metadata differs: $field" }
    }
    $jvm = Get-Content -LiteralPath (Join-Path $fixtureRoot 'jvm-expected.json') -Raw | ConvertFrom-Json
    $expectedErrors = @{'missing-api'='NOT_FOUND';'missing-field'='API_UNAVAILABLE';'missing-monitor'='API_UNAVAILABLE';'missing-native'='NOT_IMPLEMENTED';'fuel-limit'='VM_FUEL';'stack-limit'='VM_STACK_OVERFLOW'}
    if ($observed.calls.Count -ne $request.calls.Count -or $hostReport.calls.Count -ne $request.calls.Count) { throw 'Missing call results.' }
    for ($i = 0; $i -lt $request.calls.Count; $i++) {
        $actual = $observed.calls[$i]; $hostCall = $hostReport.calls[$i]; $id = $request.calls[$i].id
        if ($actual.id -cne $id -or $hostCall.id -cne $id -or $actual.value -cne $hostCall.value -or $actual.instructions -ne $hostCall.instructions -or ($actual.error | ConvertTo-Json -Compress) -cne ($hostCall.error | ConvertTo-Json -Compress)) { throw "Host/emulator call differs: $id" }
        if ($expectedErrors.ContainsKey($id)) {
            if ($actual.error.code -cne $expectedErrors[$id] -or $null -ne $actual.value -or [string]::IsNullOrEmpty($actual.error.cause)) { throw "Missing expected structured error: $id" }
        } elseif ($null -ne $actual.error -or $actual.value -cne $jvm.$id -or $actual.instructions -le 0) { throw "JVM semantic mismatch: $id" }
    }
    $result.status = 'passed'
    $result.calls = $observed.calls.Count
    $result.jvm_comparisons = @($jvm.PSObject.Properties).Count
    $result.negative_cases = $expectedErrors.Count
    $result.portable_vm = $true
    $result.emulator = [ordered]@{file=[IO.Path]::GetFullPath($Ryujinx);sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $Ryujinx).Hash.ToLowerInvariant();version=(Get-Item -LiteralPath $Ryujinx).VersionInfo.ProductVersion}
    $result.application_lifecycle = $false
    $result.jni = $false
    $result.inputs = @($apkPath,$requestPath,$nroPath | ForEach-Object { [ordered]@{file=$_;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $_).Hash.ToLowerInvariant()} })
    $result.fixture_build = Get-Content -LiteralPath (Join-Path $fixtureRoot 'build-receipt.json') -Raw | ConvertFrom-Json
} catch {
    $result.status = 'failed'; $result.error = $_.Exception.Message
    throw
} finally {
    if ($null -ne $process -and -not $process.HasExited) {
        $owned = Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
        if ($owned.CommandLine -like '*androswitch-smoke.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)) { Stop-Process -Id $process.Id -ErrorAction SilentlyContinue }
    }
    $result.finished_at = [DateTime]::UtcNow.ToString('o')
    $result | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath $receipt -Encoding utf8NoBOM
    $lock.Dispose()
    Remove-Item -LiteralPath $lockPath
    Write-Output "M1 E2E $($result.status): $receipt"
}
