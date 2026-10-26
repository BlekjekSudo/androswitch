# Normal product C main/atexit after an unmodified graphics/audio APK.
param(
    [Parameter(Mandatory)][string]$Nro,
    [string]$Asctl='',
    [string]$Apk='',
    [string]$Ryujinx='C:/Users/Administrator/.cache/androswitch/ryujinx-hle-fixed-20261025/Ryujinx.exe',
    [string]$RyujinxProfile='C:/Users/Administrator/.cache/androswitch/ryujinx-cfg-e2e',
    [ValidateRange(30,600)][int]$TimeoutSeconds=300
)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
if(-not $Asctl){$Asctl=Join-Path $repo 'target/debug/asctl.exe'}
if(-not $Apk){$Apk=Join-Path $repo 'local-artifacts/m6/fixture/native-media.apk'}
$emulator=(Get-Item -LiteralPath $Ryujinx).FullName
if((Get-FileHash $emulator).Hash.ToLowerInvariant() -cne 'cf03a8da21588166082735c0427413fe791032b84cba77919f38f5652218997c'){throw 'Pinned headless emulator required; no GUI fallback'}
$testProfile=[IO.Path]::GetFullPath($RyujinxProfile)
$switchRoot=[IO.Path]::GetFullPath((Join-Path $testProfile 'sdcard/switch'))
$id=[guid]::NewGuid().ToString('N');$run=Join-Path $repo "local-artifacts/horizon-std/product-exit-runs/$id"
$sd=Join-Path $switchRoot 'androswitch';$backup=Join-Path $switchRoot "androswitch-std-exit-backup-$id"
$saved=Join-Path $run 'sd-result';$lockPath=Join-Path $switchRoot 'androswitch-launcher-e2e.lock'
function Assert-SdChild([string]$path){
    $full=[IO.Path]::GetFullPath($path)
    if(-not $full.StartsWith($switchRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'SD path escaped the designated test directory'}
    if((Test-Path -LiteralPath $full) -and ((Get-Item -LiteralPath $full -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Linked SD paths are forbidden'}
}
Assert-SdChild $sd;Assert-SdChild $backup
New-Item -ItemType Directory -Path $run|Out-Null
Copy-Item -LiteralPath $Nro -Destination "$run/androswitch.nro"
$hash=(Get-FileHash "$run/androswitch.nro").Hash.ToLowerInvariant()
if((Get-FileHash $Nro).Hash.ToLowerInvariant() -cne $hash){throw 'NRO changed during freeze'}
Copy-Item -LiteralPath $Apk -Destination "$run/app.apk"
$receipt=[ordered]@{schema_version=1;session_id=$id;status='prepared';nro_sha256=$hash;apk_sha256=(Get-FileHash "$run/app.apk").Hash.ToLowerInvariant();asctl_sha256=(Get-FileHash $Asctl).Hash.ToLowerInvariant();emulator_sha256=(Get-FileHash $emulator).Hash.ToLowerInvariant();backend='pinned Ryujinx headless; direct product request';physical_device='not_run';sd_restore='not_modified';sd_result=$saved;sd_backup=$backup}
$process=$null;$owned=@{};$moved=$false;$staged=$false;$lock=$null
function Save-Receipt {$receipt|ConvertTo-Json -Depth 12|Set-Content "$run/verification.json" -Encoding utf8NoBOM}
function Update-Owned {
    $all=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'")
    foreach($p in $all){
        if($owned.ContainsKey([int]$p.ParentProcessId) -and $p.ExecutablePath -eq $emulator -and $p.CommandLine -is [string] -and $p.CommandLine.Replace('/','\').Contains($sd.Replace('/','\')) -and $p.CreationDate.ToUniversalTime() -ge $owned[[int]$p.ParentProcessId]){$owned[[int]$p.ProcessId]=$p.CreationDate.ToUniversalTime()}
    }
    @($all|Where-Object {$owned.ContainsKey([int]$_.ProcessId) -and $_.ExecutablePath -eq $emulator -and $_.CreationDate.ToUniversalTime() -eq $owned[[int]$_.ProcessId]})
}
function Stop-Owned {
    $live=@(Update-Owned)
    if($process -and -not $process.HasExited){$process.Kill();if(-not $process.WaitForExit(10000)){throw 'Owned emulator did not stop'}}
    foreach($p in $live){
        if($process -and $p.ProcessId -eq $process.Id){continue}
        $fresh=Get-CimInstance Win32_Process -Filter "ProcessId=$($p.ProcessId)"
        if($fresh -and $fresh.ExecutablePath -eq $emulator -and $fresh.CreationDate -eq $p.CreationDate){$child=Get-Process -Id $p.ProcessId;if(-not $child.HasExited){$child.Kill();if(-not $child.WaitForExit(10000)){throw 'Owned child did not stop'}}}
    }
    if(@(Update-Owned).Count){throw 'Owned emulator still holds test SD; keep backup'}
}
try {
    $existing=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'"|Where-Object {
        $_.ExecutablePath -and [IO.Path]::GetFullPath($_.ExecutablePath) -ieq $emulator -and
        ($p=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue) -and -not $p.HasExited
    })
    if($existing.Count){throw 'An emulator is already running'}
    New-Item -ItemType Directory -Force $switchRoot|Out-Null
    if((Get-Item $switchRoot -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked switch root forbidden'}
    $lock=[IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    if(Test-Path -LiteralPath $backup){throw 'Backup destination already exists'}
    if(Test-Path $sd){Move-Item -LiteralPath $sd -Destination $backup;$moved=$true}
    New-Item -ItemType Directory -Path $sd|Out-Null;$staged=$true
    $installation=& $Asctl install "$run/app.apk" --root $sd --json
    if($LASTEXITCODE){throw 'Direct test installation failed'}
    $package=($installation|ConvertFrom-Json).package
    $receipt.package=$package.package;$receipt.revision=$package.revision
    New-Item -ItemType Directory -Force "$sd/runtime","$sd/logs"|Out-Null
    @{schema_version=1;session_id=$id;package=$package.package;revision=$package.revision;events=@(@{kind='wait';ms=10000},@{kind='touch';action=0;x=100;y=100},@{kind='touch';action=1;x=100;y=100},@{kind='wait';ms=4000},@{kind='frame'},@{kind='return_to_launcher'})}|ConvertTo-Json -Depth 10|Set-Content "$sd/runtime/launch.json" -Encoding utf8NoBOM
    Copy-Item "$sd/runtime/launch.json" "$run/launch.json"
    Copy-Item "$run/androswitch.nro" "$sd/androswitch.nro"
    $launchArguments=@('--no-gui','--root-data-dir',('"'+$testProfile+'"'),('"'+"$sd/androswitch.nro"+'"'))
    $process=Start-Process -FilePath $emulator -ArgumentList $launchArguments -WorkingDirectory (Split-Path -Parent $emulator) -WindowStyle Hidden -RedirectStandardOutput "$run/ryujinx.stdout.log" -RedirectStandardError "$run/ryujinx.stderr.log" -PassThru
    $null=$process.Handle;$identity=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
    if(-not $identity){throw 'Could not pin owned process identity'}
    $owned[$process.Id]=$identity.CreationDate.ToUniversalTime();$receipt.status='running';Save-Receipt
    $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$completed=$false
    do {
        $null=Update-Owned
        $marker="$sd/logs/rust-std-shutdown.txt"
        if(Test-Path $marker){
            if((Get-Content $marker -Raw).Trim() -cne '0'){throw 'Product std shutdown reported unreclaimed threads'}
            if((Get-Content "$run/ryujinx.stdout.log" -Raw) -match 'ServiceAm Exit:'){$completed=$true;break}
        }
        Start-Sleep -Milliseconds 300
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $completed){throw 'No actual product return/atexit and guest OS exit before deadline'}
    $session=Get-Content "$sd/logs/session-report.json" -Raw|ConvertFrom-Json
    if($session.session_id -cne $id -or $session.status -cne 'passed_execution' -or $session.error){throw 'Media session did not complete successfully'}
    $proof=Get-Content "$sd/userdata/$($package.package)/files/media.json" -Raw|ConvertFrom-Json
    if($proof.frames -lt 31 -or $proof.input -lt 2 -or $proof.audio_frames -ne 12000 -or $proof.gl_ok -ne 1 -or $proof.readback_ok -ne 1 -or $proof.tls -ne 73){throw 'Graphics/audio/input/TLS proof mismatch'}
    if((Get-Content "$sd/rust-build-profile.txt" -Raw) -cne "std=1`nbackend=horizon`n"){throw 'Runner is not the std Horizon profile'}
    $platform=Get-Content "$sd/logs/rust-std-platform.json" -Raw|ConvertFrom-Json
    if($platform.passed -ne $true -or $platform.std_unix_ms -lt 1600000000000 -or [Math]::Abs($platform.std_unix_ms-$platform.native_unix_ms) -ge 1000 -or $platform.std_cores -le 0 -or $platform.std_cores -ne $platform.native_cores){throw 'Product std clock/core observations differ from native Horizon'}
    $receipt.status='passed';$receipt.proof=$proof;$receipt.std_platform=$platform;$receipt.std_shutdown=0;$receipt.guest_os_exit_observed=$true
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
    try {
        Stop-Owned
        Assert-SdChild $sd;Assert-SdChild $backup
        $outputRoot=[IO.Path]::GetFullPath((Join-Path $repo 'local-artifacts/horizon-std/product-exit-runs'))
        if(-not [IO.Path]::GetFullPath($saved).StartsWith($outputRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $saved)){throw 'Result destination escaped the designated fresh output directory'}
        if($staged -and (Test-Path $sd)){Move-Item -LiteralPath $sd -Destination $saved}
        if($moved){Move-Item -LiteralPath $backup -Destination $sd}
        $receipt.sd_restore=if($staged -or $moved){'restored'}else{'not_modified'}
    }catch{
        $receipt.sd_restore='manual_recovery_required';$receipt.restore_error=$_.Exception.Message;throw
    }finally{
        Save-Receipt
        if($lock){$lock.Dispose();Remove-Item -LiteralPath $lockPath}
        Write-Output "Product normal-exit evidence: $run"
    }
}
