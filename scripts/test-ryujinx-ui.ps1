param([ValidateSet('java','kotlin')][string]$Language='java',[string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',[string]$RyujinxProfile="$env:APPDATA/Ryujinx",[int]$TimeoutSeconds=180)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$id=[guid]::NewGuid().ToString('N')
$run=Join-Path $projectRoot "local-artifacts/m5/runs/$id"
$root=Join-Path $RyujinxProfile 'sdcard/switch/androswitch'
New-Item -ItemType Directory -Force -Path $run,"$root/runtime","$root/logs" | Out-Null
$lock=[IO.File]::Open("$root/e2e.lock",[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process=$null
$receipt=@{schema_version=1;session_id=$id;status='running';backend='ryujinx';physical_device='not_run';language=$Language}
try {
    $apk=Join-Path $projectRoot "local-artifacts/m5/fixture/$Language-ui.apk"
    $fixture=Get-Content "$projectRoot/local-artifacts/m5/fixture/$Language-request.json" -Raw | ConvertFrom-Json
    $hostReport=& "$projectRoot/target/debug/as-ui-probe.exe" $apk "$projectRoot/local-artifacts/m5/fixture/$Language-request.json" "$run/host-storage"
    if($LASTEXITCODE){throw 'Host UI execution failed'}
    $hostReport | Set-Content "$run/host.json" -Encoding utf8NoBOM
    $oracle=($hostReport -join "`n") | ConvertFrom-Json
    $installJson=& "$projectRoot/target/debug/asctl.exe" install $apk --root $root --update --json
    if($LASTEXITCODE){throw 'Test package installation failed'}
    $record=($installJson | ConvertFrom-Json).package
    & "$projectRoot/target/debug/asctl.exe" clear-data $record.package --root $root --json | Out-Null
    if($LASTEXITCODE){throw 'Fixture data preparation failed'}
    @{schema_version=1;session_id=$id;package=$record.package;revision=$record.revision;events=$fixture.events} | ConvertTo-Json -Depth 12 | Set-Content "$root/runtime/launch.json" -Encoding utf8NoBOM
    Copy-Item "$root/runtime/launch.json" "$run/launch.json"
    $nro=Join-Path $root 'androswitch.nro'
    Copy-Item "$projectRoot/local-artifacts/product/androswitch.nro" $nro
    $reportPath=Join-Path $root 'logs/session-report.json'
    if(Test-Path $reportPath){Move-Item $reportPath "$run/previous-report.json"}
    $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$run/ryujinx.stdout.log" -RedirectStandardError "$run/ryujinx.stderr.log" -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $result=$null
    do {
        if($process.HasExited){throw "Ryujinx exited $($process.ExitCode)"}
        if(Test-Path $reportPath){try{$result=Get-Content $reportPath -Raw | ConvertFrom-Json;if($result.session_id -eq $id -or $result.status -eq 'failed'){break}}catch{}}
        Start-Sleep -Milliseconds 500
    }while([DateTime]::UtcNow -lt $deadline)
    if($null -eq $result -or $result.session_id -ne $id -or $result.status -ne 'passed_execution'){throw "No fresh successful execution report: $($result | ConvertTo-Json -Depth 4 -Compress)"}
    if($result.snapshots.Count -ne $oracle.snapshots.Count){throw 'Snapshot count mismatch'}
    for($i=0;$i -lt $result.snapshots.Count;$i++){
        $got=$result.snapshots[$i];$wanted=$oracle.snapshots[$i]
        if($got.frame_hash -cne $wanted.frame_hash){throw "Raster mismatch at frame $i"}
        if($got.presented_hash -cne $got.frame_hash){throw "Horizon framebuffer mismatch at frame $i"}
        $gotEvents=@($got.lifecycle | ForEach-Object {"$($_.activity):$($_.event)"}) -join ','
        $wantedEvents=@($wanted.lifecycle | ForEach-Object {"$($_.activity):$($_.event)"}) -join ','
        if($gotEvents -cne $wantedEvents){throw "Lifecycle mismatch at frame $i"}
    }
    $texts=@($result.snapshots[-1].views | Where-Object text | ForEach-Object text)
    $expected=if($Language -eq 'java'){'Ready 1 / app 1'}else{'Kotlin tapped 2'}
    if($texts -cnotcontains $expected){throw 'DEX callback/save result missing'}
    Copy-Item $reportPath "$run/emulator.json"
    Copy-Item "$root/logs/last-frame.ppm" "$run/framebuffer.ppm"
    $receipt.status='passed';$receipt.snapshots=$result.snapshots.Count;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.apk_sha256=(Get-FileHash $apk).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
    Write-Output "M5 $Language UI E2E passed: $run/verification.json"
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
    if($process -and -not $process.HasExited){$owned=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)";if($owned.CommandLine -like '*androswitch.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)){Stop-Process -Id $process.Id -ErrorAction SilentlyContinue}}
    $receipt | ConvertTo-Json -Depth 12 | Set-Content "$run/verification.json" -Encoding utf8NoBOM
    $lock.Dispose();Remove-Item -LiteralPath "$root/e2e.lock" -ErrorAction SilentlyContinue
}
