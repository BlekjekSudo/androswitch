param(
    [Parameter(Mandatory)][string]$Nro,
    [string]$Ryujinx='C:/Users/Administrator/.cache/androswitch/ryujinx-hle-fixed-20261025/Ryujinx.exe',
    [string]$RyujinxProfile='C:/Users/Administrator/.cache/androswitch/ryujinx-cfg-e2e',
    [ValidateRange(10,300)][int]$TimeoutSeconds=90
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$emulator=(Get-Item -LiteralPath $Ryujinx).FullName
if((Get-FileHash -LiteralPath $emulator).Hash.ToLowerInvariant() -cne 'cf03a8da21588166082735c0427413fe791032b84cba77919f38f5652218997c'){throw 'Use the pinned headless-capable emulator; GUI fallback is forbidden.'}
$profile=[IO.Path]::GetFullPath($RyujinxProfile)
$switchRoot=[IO.Path]::GetFullPath((Join-Path $profile 'sdcard/switch'))
$session=[guid]::NewGuid().ToString('N')
$run=Join-Path $root "local-artifacts/horizon-std/runs/$session"
$sd=Join-Path $switchRoot 'androswitch-std-probe'
$backup=Join-Path $switchRoot "androswitch-std-probe-backup-$session"
function Assert-Child([string]$path){
    $full=[IO.Path]::GetFullPath($path)
    if(-not $full.StartsWith($switchRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'SD path escaped the designated switch directory.'}
    if((Test-Path -LiteralPath $full) -and ((Get-Item -LiteralPath $full -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Linked SD paths are forbidden.'}
}
Assert-Child $sd; Assert-Child $backup
New-Item -ItemType Directory -Path $run | Out-Null
$artifact=Join-Path $run 'androswitch-std-probe.nro'
Copy-Item -LiteralPath $Nro -Destination $artifact
$hash=(Get-FileHash -LiteralPath $artifact).Hash.ToLowerInvariant()
if((Get-FileHash -LiteralPath $Nro).Hash.ToLowerInvariant() -cne $hash){throw 'Candidate changed during freeze.'}
$stream=[IO.File]::OpenRead($artifact)
try {
    $stream.Position=16; $magic=[byte[]]::new(4)
    if($stream.Read($magic,0,4) -ne 4 -or [Text.Encoding]::ASCII.GetString($magic) -cne 'NRO0'){throw 'Candidate is not an NRO.'}
}finally{$stream.Dispose()}
$lockPath=Join-Path $switchRoot 'androswitch-launcher-e2e.lock'
$lock=[IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$receipt=[ordered]@{schema_version=1;session_id=$session;status='running';backend='ryujinx_headless_sdl2';physical_device='not_run';nro_sha256=$hash;emulator_sha256=(Get-FileHash -LiteralPath $emulator).Hash.ToLowerInvariant();emulator_version=(Get-Item -LiteralPath $emulator).VersionInfo.ProductVersion;phases=@();source_commit=(& 'C:/Program Files/Git/cmd/git.exe' -c core.fsmonitor=false -C $root rev-parse HEAD|Out-String).Trim();sd_restore='pending'}
$process=$null; $owned=@{}; $moved=$false; $staged=$false
function Save-Receipt {$receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $run 'verification.json') -Encoding utf8NoBOM}
function Update-Owned {
    $all=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'")
    foreach($candidate in $all){
        if($owned.ContainsKey([int]$candidate.ParentProcessId) -and
           $candidate.ExecutablePath -eq $emulator -and
           $candidate.CommandLine -is [string] -and $candidate.CommandLine.Contains('androswitch-std-probe.nro') -and
           $candidate.CreationDate.ToUniversalTime() -ge $owned[[int]$candidate.ParentProcessId]){
            $owned[[int]$candidate.ProcessId]=$candidate.CreationDate.ToUniversalTime()
        }
    }
    @($all|Where-Object {$owned.ContainsKey([int]$_.ProcessId) -and $_.ExecutablePath -eq $emulator -and $_.CreationDate.ToUniversalTime() -eq $owned[[int]$_.ProcessId]})
}
function Stop-Owned {
    $live=@(Update-Owned)
    if($process -and -not $process.HasExited){$process.Kill(); if(-not $process.WaitForExit(10000)){throw 'Owned emulator did not stop.'}}
    foreach($candidate in $live){
        if($process -and $candidate.ProcessId -eq $process.Id){continue}
        $fresh=Get-CimInstance Win32_Process -Filter "ProcessId=$($candidate.ProcessId)"
        if($fresh -and $fresh.ExecutablePath -eq $emulator -and $fresh.CreationDate -eq $candidate.CreationDate){
            $child=Get-Process -Id $candidate.ProcessId; $child.Kill(); if(-not $child.WaitForExit(10000)){throw 'Owned headless child did not stop.'}
        }
    }
    if(@(Update-Owned).Count){throw 'Emulator still owns test SD; preserve backup.'}
    $process=$null; $owned.Clear()
}
try {
    if(Test-Path -LiteralPath $sd){Move-Item -LiteralPath $sd -Destination $backup; $moved=$true}
    New-Item -ItemType Directory -Path $sd|Out-Null; $staged=$true
    Copy-Item -LiteralPath $artifact -Destination (Join-Path $sd 'androswitch-std-probe.nro')
    foreach($mode in @('suite','restart','panic')){
        [IO.File]::WriteAllText((Join-Path $sd 'request.txt'),"$session`n$mode`n",[Text.UTF8Encoding]::new($false))
        $phase=[ordered]@{mode=$mode;status='started';started_utc=[DateTime]::UtcNow.ToString('o')}
        $receipt.phases+= $phase; Save-Receipt
        $arguments=@('--no-gui','--root-data-dir',('"'+$profile+'"'),('"'+(Join-Path $sd 'androswitch-std-probe.nro')+'"'))
        $process=Start-Process -FilePath $emulator -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $emulator) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $run "$mode.stdout.log") -RedirectStandardError (Join-Path $run "$mode.stderr.log") -PassThru
        $null=$process.Handle
        $identity=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
        if(-not $identity){throw 'Cannot pin owned emulator identity.'}
        $owned[$process.Id]=$identity.CreationDate.ToUniversalTime(); $phase.pid=$process.Id; Save-Receipt
        $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds); $observed=$false
        do {
            $null=Update-Owned
            $marker=Join-Path $sd "$session/$(if($mode -eq 'panic'){'panic.txt'}else{"complete-$mode.txt"})"
            if(Test-Path -LiteralPath $marker){$observed=$true; break}
            if(Test-Path -LiteralPath (Join-Path $sd 'fatal.txt')){break}
            if(-not @(Update-Owned).Count){break}
            Start-Sleep -Milliseconds 300
        }while([DateTime]::UtcNow -lt $deadline)
        # Let C main/atexit or the intentional panic reach the OS termination path.
        if($observed){Start-Sleep -Seconds 3}
        $phase.marker_observed=$observed
        $phase.live_after_grace=@(Update-Owned).Count
        $phase.status=if($observed){'observed'}else{'incomplete'}
        Stop-Owned; Save-Receipt
        if(-not $observed){throw "Phase $mode did not produce its fresh completion marker."}
    }
    $receipt.status='executed_requires_contract_review'
}catch{$receipt.status='failed'; $receipt.error=$_.Exception.Message; throw}finally{
    try {
        Stop-Owned
        Assert-Child $sd; Assert-Child $backup
        if($staged -and (Test-Path -LiteralPath $sd)){Move-Item -LiteralPath $sd -Destination (Join-Path $run 'sd-result')}
        if($moved){Move-Item -LiteralPath $backup -Destination $sd}
        $receipt.sd_restore='restored'
    }finally{
        Save-Receipt; $lock.Dispose(); Remove-Item -LiteralPath $lockPath
        Write-Output "std probe evidence: $run"
    }
}
