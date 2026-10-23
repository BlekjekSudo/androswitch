param(
    [string]$Ryujinx = 'D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile = "$env:APPDATA/Ryujinx",
    [int]$TimeoutSeconds = 180,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$runId = [guid]::NewGuid().ToString('N')
$runRoot = Join-Path $projectRoot "local-artifacts/m2/runs/$runId"
$fixture = Join-Path $projectRoot 'local-artifacts/m2/fixture'
$sdProbe = Join-Path $RyujinxProfile 'sdcard/switch/androswitch-smoke'
$sdStore = Join-Path $RyujinxProfile 'sdcard/switch/androswitch'
$hostStore = Join-Path $runRoot 'host-store'
New-Item -ItemType Directory -Force -Path $runRoot,$sdProbe,$hostStore | Out-Null
$lockPath = Join-Path $sdProbe 'e2e.lock'
$lock = [IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$result = [ordered]@{schema_version=1;session_id=$runId;milestone='M2';status='running';physical_device='not_run';steps=@();started_at=[DateTime]::UtcNow.ToString('o')}
$process = $null
try {
    & "$PSScriptRoot/build-m2-fixture.ps1" | Out-Host
    if (-not $SkipBuild) { & "$PSScriptRoot/build-switch-smoke.ps1" | Out-Host }
    Push-Location -LiteralPath $projectRoot
    try { & cargo build -p as-switch-smoke --features host-cli --bin as-probe; if ($LASTEXITCODE) { throw 'host build failed' } } finally { Pop-Location }
    $nro = Join-Path $sdProbe 'androswitch-smoke.nro'
    Copy-Item -LiteralPath "$projectRoot/local-artifacts/m1/androswitch-smoke.nro" -Destination $nro
    Copy-Item -LiteralPath "$fixture/dex/classes.dex" -Destination "$sdProbe/classes.dex"
    # Preserve previous test state as an inspectable backup, never remove a user installation.
    foreach ($name in @('org.fixture.alpha','org.fixture.beta')) {
        foreach ($sub in @('installed','userdata','Android/data','Android/obb')) {
            $old = Join-Path $sdStore "$sub/$name"
            if (Test-Path -LiteralPath $old) {
                $backup = Join-Path $runRoot "previous/$sub"
                New-Item -ItemType Directory -Force -Path $backup | Out-Null
                Move-Item -LiteralPath $old -Destination (Join-Path $backup $name)
            }
        }
    }
    foreach ($root in @($hostStore,$sdStore)) {
        $obbDir = Join-Path $root 'Android/obb/org.fixture.alpha'
        New-Item -ItemType Directory -Force -Path $obbDir | Out-Null
        $bytes = New-Object byte[] 65544
        for ($i=0; $i -lt $bytes.Length; $i++) { $bytes[$i] = $i % 251 }
        [IO.File]::WriteAllBytes((Join-Path $obbDir 'main.obb'),$bytes)
    }
    $steps = @(
        @('install-a','install','org.fixture.alpha',1,'store',8),
        @('install-b','install','org.fixture.beta',1,'store',20),
        @('restart-a','launch','org.fixture.alpha',1,'read',$null),
        @('restart-b','launch','org.fixture.beta',1,'read',$null),
        @('update-a','update','org.fixture.alpha',2,'read',$null),
        @('obb-stream','launch','org.fixture.alpha',2,'obb',$null),
        @('mkdir-contract','launch','org.fixture.alpha',2,'mkdir',$null)
    )
    foreach ($step in $steps) {
        $id,$operation,$package,$version,$method,$value = $step
        $stepRoot = Join-Path $runRoot $id
        New-Item -ItemType Directory -Force -Path $stepRoot | Out-Null
        $request = [ordered]@{schema_version=1;session_id="$runId-$id";storage=@{operation=$operation;package=$package};calls=@(@{id=$id;class='Lorg/androswitch/fixture/StorageProbe;';method=$method;descriptor=$(if ($method -eq 'store') {'(I)I'} else {'()I'});args=@($(if ($null -ne $value) {$value.ToString()}))})}
        $requestPath = Join-Path $stepRoot 'request.json'
        $request | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $requestPath -Encoding utf8NoBOM
        $apk = if ($operation -eq 'launch') { '-' } else { Join-Path $fixture "$package-$version.apk" }
        $hostJson = & "$projectRoot/target/debug/as-probe.exe" $apk $requestPath $hostStore
        if ($LASTEXITCODE) { throw 'host probe failed' }
        $hostJson | Set-Content -LiteralPath "$stepRoot/host.json" -Encoding utf8NoBOM
        $hostReport = ($hostJson -join "`n") | ConvertFrom-Json
        if ($null -eq $hostReport.calls -or $hostReport.calls[0].error) { throw "Host failed $id : $hostJson" }
        if ($operation -eq 'launch') {
            $inputApk = Join-Path $sdProbe 'app.apk'
            if (Test-Path -LiteralPath $inputApk) { Remove-Item -LiteralPath $inputApk }
        } else { Copy-Item -LiteralPath $apk -Destination "$sdProbe/app.apk" }
        Copy-Item -LiteralPath $requestPath -Destination "$sdProbe/request.json"
        $reportPath = Join-Path $sdProbe 'probe-result.json'
        if (Test-Path -LiteralPath $reportPath) { Move-Item -LiteralPath $reportPath -Destination "$stepRoot/previous-probe.json" }
        $process = Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$stepRoot/ryujinx.stdout.log" -RedirectStandardError "$stepRoot/ryujinx.stderr.log" -PassThru
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        $observed = $null
        do {
            if ($process.HasExited) { throw "Ryujinx exited during $id" }
            if (Test-Path -LiteralPath $reportPath) {
                try { $observed = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json; if ($observed.session_id -eq $request.session_id -or $observed.code) { break } } catch { }
            }
            Start-Sleep -Milliseconds 500
        } while ([DateTime]::UtcNow -lt $deadline)
        if ($null -eq $observed -or $observed.session_id -ne $request.session_id) { throw "No successful fresh emulator report for $id : $($observed | ConvertTo-Json -Compress)" }
        Copy-Item -LiteralPath $reportPath -Destination "$stepRoot/emulator.json"
        $owned = Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
        if ($owned.CommandLine -like '*androswitch-smoke.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)) { Stop-Process -Id $process.Id }
        $process.WaitForExit(10000) | Out-Null
        $process = $null
        $expected = switch ($method) {
            'store' { $start = [int]$value; (($start*257+($start+1))*257+($start+2))*257+($start+3) }
            'read' { $start = if ($package -eq 'org.fixture.alpha') {8} else {20}; (($start*257+($start+1))*257+($start+2))*257+($start+3) }
            'obb' { (65530 % 251)*256+(65537 % 251) }
            'mkdir' { 1 }
        }
        if ($observed.calls[0].error -or $observed.calls[0].value -cne $expected.ToString() -or $observed.calls[0].value -cne $hostReport.calls[0].value) { throw "Result mismatch $id : $($observed | ConvertTo-Json -Depth 8 -Compress)" }
        foreach ($field in @('package','revision','version_code','apk_sha256')) { if ($observed.installation.$field -cne $hostReport.installation.$field) { throw "Registry mismatch $field during $id" } }
        $result.steps += [ordered]@{id=$id;status='passed';value=$observed.calls[0].value;revision=$observed.installation.revision}
        Write-Output "M2 $id passed"
    }
    $result.status = 'passed'
    $result.nro_sha256 = (Get-FileHash -LiteralPath $nro).Hash.ToLowerInvariant()
    $result.fixture_build = Get-Content -LiteralPath "$fixture/build-receipt.json" -Raw | ConvertFrom-Json
} catch { $result.status = 'failed'; $result.error = $_.Exception.Message; throw } finally {
    if ($null -ne $process -and -not $process.HasExited) {
        $owned = Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
        if ($owned.CommandLine -like '*androswitch-smoke.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)) { Stop-Process -Id $process.Id -ErrorAction SilentlyContinue }
    }
    $result.finished_at = [DateTime]::UtcNow.ToString('o')
    $result | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath "$runRoot/verification.json" -Encoding utf8NoBOM
    $lock.Dispose()
    Remove-Item -LiteralPath $lockPath
    Write-Output "M2 E2E $($result.status): $runRoot/verification.json"
}
