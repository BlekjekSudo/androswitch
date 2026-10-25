# An external-APK first-screen candidate, never an automatic gameplay pass.
# PrepareOnly writes to local-artifacts and leaves the emulator/SD untouched.
param(
    [Parameter(Mandatory)][ValidateCount(1,16)][string[]]$ApkPaths,
    [string]$Nro='',
    [ValidateRange(1,100)][int]$LaunchesPerApk=1,
    [string]$UserDataSeed='',
    [string[]]$UserDataWatch=@(),
    [string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile="$env:APPDATA/Ryujinx",
    [ValidateRange(1000,30000)][int]$StartupWaitMilliseconds=10000,
    [ValidateRange(30,1800)][int]$TimeoutSeconds=300,
    [switch]$Headless,
    [switch]$TraceNative,
    [switch]$PrepareOnly,
    [switch]$AllowDesktopEmulator
)
$ErrorActionPreference='Stop'
if(-not $PrepareOnly -and -not $AllowDesktopEmulator){
    throw 'Desktop Ryujinx E2E requires -AllowDesktopEmulator after the reported disappearance of other app windows. Use -PrepareOnly to create the request and artifact without opening Ryujinx or changing its SD folder.'
}
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Nro){$Nro=Join-Path $projectRoot 'local-artifacts/product/androswitch.nro'}
$nroSource=(Get-Item -LiteralPath $Nro).FullName
$emulator=(Get-Item -LiteralPath $Ryujinx).FullName
$profile=[IO.Path]::GetFullPath($RyujinxProfile)
$files=@($ApkPaths | ForEach-Object {
    $file=Get-Item -LiteralPath $_
    if($file.PSIsContainer -or $file.Extension -ine '.apk'){throw "Expected an APK file: $($_)"}
    $file
} | Sort-Object FullName)
$id=[guid]::NewGuid().ToString('N')
$run=Join-Path $projectRoot "local-artifacts/m9/external-runs/$id"
$switchRoot=[IO.Path]::GetFullPath((Join-Path $profile 'sdcard/switch'))
$sdRoot=Join-Path $switchRoot 'androswitch'
$backup=Join-Path $switchRoot "androswitch-external-backup-$id"
$sdResult=Join-Path $switchRoot "androswitch-external-result-$id"
$lockPath=Join-Path $switchRoot 'androswitch-launcher-e2e.lock'
function Assert-SdChild([string]$Path){
    $full=[IO.Path]::GetFullPath($Path)
    if(-not $full.StartsWith($switchRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){
        throw "SD path escaped its designated switch directory: $full"
    }
    if(Test-Path -LiteralPath $full){
        $item=Get-Item -LiteralPath $full -Force
        if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw "Refusing to move a linked SD path: $full"}
    }
}
foreach($path in @($sdRoot,$backup,$sdResult,$lockPath)){Assert-SdChild $path}
New-Item -ItemType Directory -Path $run | Out-Null
$artifact=Join-Path $run 'androswitch.nro'
$nroHash=(Get-FileHash -LiteralPath $nroSource).Hash.ToLowerInvariant()
Copy-Item -LiteralPath $nroSource -Destination $artifact
if((Get-FileHash -LiteralPath $artifact).Hash.ToLowerInvariant() -ne $nroHash -or
   (Get-FileHash -LiteralPath $nroSource).Hash.ToLowerInvariant() -ne $nroHash){throw 'NRO changed while the candidate was being prepared; retry after its build completes.'}
$stream=[IO.File]::OpenRead($artifact)
try{
    $stream.Position=16;$magic=[byte[]]::new(4)
    if($stream.Read($magic,0,4) -ne 4 -or [Text.Encoding]::ASCII.GetString($magic) -cne 'NRO0'){
        throw 'Candidate artifact is not an NRO.'
    }
}finally{$stream.Dispose()}
$commands=[Collections.Generic.List[object]]::new()
$inputs=[Collections.Generic.List[object]]::new()
$launches=[Collections.Generic.List[object]]::new()
$seedFiles=[Collections.Generic.List[object]]::new()
foreach($relative in $UserDataWatch){
    if($relative -cnotmatch '^[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)+/[A-Za-z0-9_./-]+$' -or $relative.Split('/') -contains '..' -or $relative.Contains('//')){throw 'Invalid relative package userdata watch path'}
}

$commands.Add(@{kind='frame'})
for($index=0;$index -lt $files.Count;$index++){
    $commands.Add(@{kind='key';key='right'})
    for($row=0;$row -lt $index;$row++){$commands.Add(@{kind='key';key='down'})}
    $commands.Add(@{kind='key';key='accept'})
    $installStep=$commands.Count
    # Keep the explicit application frame visible to the polling recorder
    # before the return path and next launch can clear shared capture names.
    $events=@(@{kind='wait';ms=$StartupWaitMilliseconds},@{kind='frame'},@{kind='wait';ms=2000},@{kind='return_to_launcher'})
    $input=@{
        index=$index;source=$files[$index].FullName;staged_name=('{0:D4}-external.apk' -f $index)
        bytes=$files[$index].Length;sha256=(Get-FileHash -LiteralPath $files[$index].FullName).Hash.ToLowerInvariant()
        install_snapshot_step=$installStep
    }
    $inputs.Add($input)
    for($attempt=0;$attempt -lt $LaunchesPerApk;$attempt++){
        $commands.Add(@{kind='open';events=$events})
        $commands.Add(@{kind='frames';count=60})
        $entry=@{};foreach($pair in $input.GetEnumerator()){$entry[$pair.Key]=$pair.Value}
        $entry.expected_session_id="$id-$($launches.Count+1)";$entry.attempt=$attempt
        $launches.Add($entry)
    }
}
if($commands.Count -gt 256){throw 'Request exceeds the launcher command limit.'}

if($UserDataSeed){
    $seedRoot=(Get-Item -LiteralPath $UserDataSeed).FullName
    if(-not(Test-Path -LiteralPath $seedRoot -PathType Container)){throw 'UserDataSeed must be a directory of package-owned test data'}
    $items=@(Get-Item -LiteralPath $seedRoot)+@(Get-ChildItem -LiteralPath $seedRoot -Recurse -Force)
    if(@($items|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'UserDataSeed links are forbidden'}
    $bytes=0L
    foreach($file in @($items|Where-Object {-not $_.PSIsContainer})){
        if($seedFiles.Count -ge 1024){throw 'Seed file limit exceeded'}
        $bytes+=$file.Length;if($bytes -gt 16MB){throw 'Seed size limit exceeded'}
        $relative=[IO.Path]::GetRelativePath($seedRoot,$file.FullName).Replace('\','/')
        if($relative -notmatch '^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+/' -or $relative.Split('/') -contains '..'){throw 'Seed must begin with a normal package directory'}
        $target=Join-Path $run "userdata-seed/$relative"
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target)|Out-Null
        $hash=(Get-FileHash -LiteralPath $file.FullName).Hash.ToLowerInvariant()
        Copy-Item -LiteralPath $file.FullName -Destination $target
        if((Get-FileHash -LiteralPath $target).Hash.ToLowerInvariant() -cne $hash){throw 'Seed changed during preparation'}
        $seedFiles.Add(@{source=$file.FullName;relative=$relative;frozen=$target;bytes=$file.Length;sha256=$hash})
    }
}

$request=@{schema_version=1;session_id=$id;commands=@($commands.ToArray())}
$request|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'launcher-test.json') -Encoding utf8NoBOM
$receipt=@{
    schema_version=1;session_id=$id;backend='ryujinx';physical_device='not_run'
    status='prepared_not_run';execution_attempted=$false;visual_review='not_run'
    first_screen_contract='Runtime execution, fresh guest frames and safe return; a recognizable application screen requires visual review. This does not prove gameplay.'
    artifact=$artifact;nro_sha256=$nroHash;emulator_path=$emulator;ryujinx_profile=$profile
    emulator_sha256=(Get-FileHash -LiteralPath $emulator).Hash.ToLowerInvariant()
    emulator_version=(Get-Item -LiteralPath $emulator).VersionInfo.ProductVersion
    sd_result=$sdResult;sd_backup=$backup;inputs=@($inputs.ToArray());launches=@($launches.ToArray());userdata_seed=@($seedFiles.ToArray());commands=$commands.Count
    startup_wait_ms=$StartupWaitMilliseconds;cases=@();sd_original_restore='not_modified'
    emulator_frontend=if($Headless){'headless_sdl2'}else{'desktop'}
    native_trace_requested=[bool]$TraceNative
    capture_ownership_contract='Protocol1 runtime generation identity; matching pending request for CPU frames; identity rechecked after copy. Filesystem timestamps are not ownership evidence.'
}
function Save-Receipt {
    $receipt|ConvertTo-Json -Depth 32|Set-Content (Join-Path $run 'verification.json') -Encoding utf8NoBOM
}
Save-Receipt
if($PrepareOnly){Write-Output "Prepared, not executed: $run/verification.json";return}

# Only the explicit execution branch can create an SD lock, rename the existing
# application folder or start a process. It never clones the emulator profile,
# changes its configuration, or manipulates any desktop windows.
$owned=[Collections.Generic.Dictionary[int,DateTime]]::new()
$process=$null;$lock=$null;$backupMoved=$false;$stageCreated=$false
$runStarted=[DateTime]::UtcNow
$sdNro=Join-Path $sdRoot 'androswitch.nro'
$exeName=[IO.Path]::GetFileName($emulator)
function Get-EmulatorProcesses {
    @(Get-CimInstance Win32_Process -Filter "Name='$($exeName.Replace("'","''"))'" | Where-Object {
        $_.ExecutablePath -and [IO.Path]::GetFullPath($_.ExecutablePath) -ieq $emulator -and
        (Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue)
    })
}
function Update-OwnedProcesses {
    $candidates=@(Get-EmulatorProcesses)
    do{
        $added=$false
        foreach($candidate in $candidates){
            $pidValue=[int]$candidate.ProcessId
            if(-not $owned.ContainsKey($pidValue) -and $owned.ContainsKey([int]$candidate.ParentProcessId) -and
               $candidate.CreationDate.ToUniversalTime() -ge $runStarted -and $candidate.CommandLine -and
               $candidate.CommandLine.Replace('/','\').Contains($sdNro.Replace('/','\'))){
                $owned.Add($pidValue,$candidate.CreationDate.ToUniversalTime());$added=$true
            }
        }
    }while($added)
    @($candidates|Where-Object {
        $owned.ContainsKey([int]$_.ProcessId) -and
        $_.CreationDate.ToUniversalTime() -eq $owned[[int]$_.ProcessId]
    })
}
function Stop-OwnedProcesses {
    # Revalidate identity and creation time on every stop; PID reuse and another
    # already-open emulator can never become cleanup targets.
    if(-not $owned.Count){return}
    # The Process object is returned by our own Start-Process. Pin its kernel
    # handle at creation so a delayed/incomplete CIM inventory is not confused
    # with process exit, and PID reuse cannot select another application's PID.
    if($process -and $receipt.root_handle_pinned){
        try{
            if(-not $process.HasExited){
                $process.Kill()
                $stopTimer=[Diagnostics.Stopwatch]::StartNew()
                $stopped=$false;$waitAttempts=0
                # GPU/process teardown can outlast one ten-second wait even
                # after TerminateProcess. Keep waiting on our pinned handle;
                # never turn an incomplete inventory into permission to move SD.
                while(-not $stopped -and $waitAttempts -lt 3){
                    $waitAttempts++
                    $stopped=$process.WaitForExit(10000)
                }
                $stopTimer.Stop()
                $receipt.root_stop_wait_ms=$stopTimer.ElapsedMilliseconds
                $receipt.root_stop_wait_attempts=$waitAttempts
                if(-not $stopped){throw 'The owned root process did not stop; preserve its SD backup.'}
            }
        }catch{
            $receipt.root_stop_error=$_.Exception.Message
            throw
        }
    }
    for($attempt=0;$attempt -lt 3;$attempt++){
        $live=@(Update-OwnedProcesses)
        if(-not $live.Count){return}
        foreach($candidate in ($live|Sort-Object {if($process -and $_.ProcessId -eq $process.Id){1}else{0}})){
            $target=Get-Process -Id $candidate.ProcessId -ErrorAction SilentlyContinue
            if($target){$target.Kill();$null=$target.WaitForExit(10000)}
        }
    }
    if(@(Update-OwnedProcesses).Count){throw 'An owned emulator process did not stop; SD backup must remain preserved.'}
}
$completeCapturePaths=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$captureMetadata=@{}
function Capture-Session([object]$Session){
    if($Session.session_id -isnot [string] -or $Session.session_id -cnotmatch '^[A-Za-z0-9_.-]{1,128}$'){return @()}
    $captureRoot=$sdRoot
    $ownsCapture={
        param([bool]$NeedsPending)
        $names=@('capture-owner.json','session-report.json')
        if($NeedsPending){$names+= 'pending-session.json'}
        foreach($name in $names){
            try{
                $bound=Get-Content -LiteralPath (Join-Path $captureRoot "logs/$name") -Raw -ErrorAction Stop|ConvertFrom-Json -ErrorAction Stop
                if($bound.schema_version -ne 1 -or $bound.session_id -cne $Session.session_id -or
                   $bound.package -cne $Session.package -or $bound.revision -ne $Session.revision){return $false}
                if($name -eq 'capture-owner.json' -and $bound.capture_protocol -ne 1){return $false}
            }catch{return $false}
        }
        return $true
    }.GetNewClosure()
    $destination=Join-Path $run "sessions/$($Session.session_id)"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $Session|ConvertTo-Json -Depth 28|Set-Content (Join-Path $destination 'session-report.json') -Encoding utf8NoBOM
    if($Session.status -ne 'running' -and (& $ownsCapture $false)){
        foreach($relative in $UserDataWatch){
            $source=Join-Path $sdRoot "userdata/$relative"
            if(-not(Test-Path -LiteralPath $source -PathType Leaf)){continue}
            $check=Get-Item -LiteralPath $source -Force;$linked=$false
            while($check -and $check.FullName.StartsWith($sdRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){
                if($check.Attributes -band [IO.FileAttributes]::ReparsePoint){$linked=$true;break};$check=if($check.PSIsContainer){$check.Parent}else{$check.Directory}
            }
            if($linked -or (Get-Item -LiteralPath $source).Length -gt 2MB){continue}
            $target=Join-Path $destination "userdata/$relative";New-Item -ItemType Directory -Force (Split-Path $target)|Out-Null
            $hash=(Get-FileHash -LiteralPath $source).Hash
            Copy-Item -LiteralPath $source -Destination $target
            if(-not(& $ownsCapture $false) -or (Get-FileHash -LiteralPath $target).Hash -cne $hash -or (Get-FileHash -LiteralPath $source).Hash -cne $hash){Remove-Item -LiteralPath $target}
        }
    }
    foreach($name in @('memory-admission.json','native-compatibility.json','capture-owner.json','uncaught-exceptions.json')){
        $source=Join-Path $sdRoot "logs/$name"
        if(-not (Test-Path -LiteralPath $source)){continue}
        try{
            $bound=Get-Content -LiteralPath $source -Raw|ConvertFrom-Json
            if($bound.session_id -eq $Session.session_id -and $bound.package -eq $Session.package -and
               $bound.revision -eq $Session.revision){
                $bound|ConvertTo-Json -Depth 28|Set-Content (Join-Path $destination $name) -Encoding utf8NoBOM
            }
        }catch{# Optional diagnostic may still be writing; never bind stale data.
        }
    }
    # The device truncates this trace for every new launch. Retain its raw
    # operation/result/tick records alongside the already matched session.
    $stopTrace=Join-Path $sdRoot 'logs/media-stop.log'
    if(Test-Path -LiteralPath $stopTrace){
        Copy-Item -LiteralPath $stopTrace -Destination (Join-Path $destination 'media-stop.log')
    }
    # Raw process traces aid diagnosis; they do not establish a session's
    # completion or replace the identity-bound final launcher report.
    foreach($name in @('teardown.log','memory.log','native-modules.log','native-calls.log','jni-failures.log')){
        $trace=Join-Path $sdRoot "logs/$name"
        if(Test-Path -LiteralPath $trace){
            Copy-Item -LiteralPath $trace -Destination (Join-Path $destination $name)
        }
    }
    $images=[Collections.Generic.List[object]]::new()
    foreach($imageFile in @(Get-ChildItem -LiteralPath (Join-Path $sdRoot 'logs') -File -Filter '*.ppm')){
        if($imageFile.Name -cnotin @('last-frame.ppm','gles-frame-2.ppm','gles-frame-30.ppm')){continue}
        $target=Join-Path $destination $imageFile.Name
        if($completeCapturePaths.Contains($target)){
            $images.Add($captureMetadata[$target]);continue
        }
        # The runtime clears old images before publishing its generation owner.
        # CPU launcher captures share last-frame.ppm, so require an active matching
        # request too. Filesystem time may use a different guest clock offset.
        $isCpu=$imageFile.Name -ceq 'last-frame.ppm'
        if(-not (& $ownsCapture $isCpu)){continue}
        # A completed capture is immutable for this session. An early partial
        # copy can be retried while its writer finishes, without replacing a
        # valid image during the next application's filename cleanup.
        if(-not $completeCapturePaths.Contains($target)){
            Copy-Item -LiteralPath $imageFile.FullName -Destination $target
        }
        # A later launch/return may have changed ownership during the copy.
        if(-not (& $ownsCapture $isCpu)){continue}
        $data=[IO.File]::ReadAllBytes($target)
        $header=[Text.Encoding]::ASCII.GetString($data,0,[Math]::Min(128,$data.Length))
        $match=[regex]::Match($header,'\AP6\s+(\d+)\s+(\d+)\s+255\s')
        $valid=$false;$nonuniform=$false;$width=0;$height=0
        if($match.Success){
            $width=[int]$match.Groups[1].Value;$height=[int]$match.Groups[2].Value
            $pixelCount=[long]$width*$height;$offset=$match.Length
            $valid=$width -gt 0 -and $height -gt 0 -and $pixelCount -le 4000000 -and
                   $data.Length -eq $offset+3*$pixelCount
            if($valid){
                for($sample=1;$sample -lt 4096;$sample++){
                    $at=$offset+3*[long]([Math]::Floor(($pixelCount-1)*($sample/4095.0)))
                    if($data[$at] -ne $data[$offset] -or $data[$at+1] -ne $data[$offset+1] -or $data[$at+2] -ne $data[$offset+2]){
                        $nonuniform=$true;break
                    }
                }
            }
        }
        if($valid){$null=$completeCapturePaths.Add($target)}
        $metadata=@{file=$target;sha256=(Get-FileHash -LiteralPath $target).Hash.ToLowerInvariant();valid_ppm=$valid;width=$width;height=$height;sampled_nonuniform=$nonuniform;recognizable_app_screen='requires_visual_review'}
        if($valid){$captureMetadata[$target]=$metadata}
        $images.Add($metadata)
    }
    return @($images.ToArray())
}
try{
    if(@(Get-EmulatorProcesses).Count){throw 'Close the existing Ryujinx session before executing this isolated test.'}
    New-Item -ItemType Directory -Path $switchRoot -Force | Out-Null
    if((Get-Item -LiteralPath $switchRoot -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Refusing an SD switch directory that is a link.'}
    $lock=[IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    foreach($path in @($sdRoot,$backup,$sdResult)){Assert-SdChild $path}
    if(Test-Path -LiteralPath $backup){throw 'Backup destination already exists.'}
    if(Test-Path -LiteralPath $sdResult){throw 'Result destination already exists.'}
    if(Test-Path -LiteralPath $sdRoot){
        if(Test-Path -LiteralPath (Join-Path $sdRoot 'e2e.lock')){throw 'Another test holds the AndroSwitch SD folder.'}
        Move-Item -LiteralPath $sdRoot -Destination $backup;$backupMoved=$true
    }
    New-Item -ItemType Directory -Path $sdRoot | Out-Null;$stageCreated=$true
    foreach($name in @('apks','runtime','logs')){New-Item -ItemType Directory -Path (Join-Path $sdRoot $name)|Out-Null}
    if($TraceNative){
        [IO.File]::WriteAllText((Join-Path $sdRoot 'logs/native-trace.enabled'),"1`n")
    }
    foreach($inputFile in $inputs){
        if((Get-FileHash -LiteralPath $inputFile.source).Hash.ToLowerInvariant() -ne $inputFile.sha256){throw 'An input APK changed after preparation.'}
        Copy-Item -LiteralPath $inputFile.source -Destination (Join-Path $sdRoot "apks/$($inputFile.staged_name)")
    }
    foreach($seed in $seedFiles){
        if((Get-FileHash -LiteralPath $seed.frozen).Hash.ToLowerInvariant() -cne $seed.sha256){throw 'Frozen seed changed'}
        $destination=[IO.Path]::GetFullPath((Join-Path $sdRoot "userdata/$($seed.relative)"))
        if(-not $destination.StartsWith([IO.Path]::GetFullPath((Join-Path $sdRoot 'userdata'))+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Seed destination escaped test userdata'}
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination)|Out-Null
        Copy-Item -LiteralPath $seed.frozen -Destination $destination
    }
    Copy-Item -LiteralPath $artifact -Destination $sdNro
    Copy-Item -LiteralPath (Join-Path $run 'launcher-test.json') -Destination (Join-Path $sdRoot 'runtime/launcher-test.json')
    $receipt.execution_attempted=$true;$receipt.status='attempted';Save-Receipt
    # Explicit profile selection prevents reading one profile's SD while the
    # emulator runs another. No fullscreen, monitor or window-position flags.
    $arguments=@('--root-data-dir',('"'+$profile+'"'),('"'+$sdNro+'"'))
    if($Headless){$arguments=@('--no-gui')+$arguments}
    $process=Start-Process -FilePath $emulator -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $emulator) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $run 'ryujinx.stdout.log') -RedirectStandardError (Join-Path $run 'ryujinx.stderr.log') -PassThru
    try{$null=$process.Handle;$receipt.root_handle_pinned=$true}catch{$receipt.root_handle_pinned=$false}
    $owned.Add($process.Id,$runStarted)
    $rootIdentity=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)"
    # Retain the root PID as an ancestry anchor even if Ryujinx restarts before
    # CIM can read it. A stale PID alone is never sufficient to stop a process.
    $rootCreation=if($rootIdentity){$rootIdentity.CreationDate.ToUniversalTime()}else{$runStarted}
    $owned[$process.Id]=$rootCreation
    $receipt.pid=$process.Id
    Save-Receipt
    $captured=@{};$settledCaptures=@{};$deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$result=$null
    do{
        $terminalRefusal=$null
        foreach($logName in @('ryujinx.stdout.log','ryujinx.stderr.log')){
            $logPath=Join-Path $run $logName
            if(Test-Path -LiteralPath $logPath){
                $terminal=@(Get-Content -LiteralPath $logPath -Tail 80|Where-Object {$_ -match 'Unhandled exception caught:.*ServiceNotImplementedException'})
                if($terminal.Count){$receipt.terminal_emulator_error=$terminal[-1];throw 'The owned emulator encountered an unimplemented Horizon service; preserve the partial journal and stop this run.'}
            }
        }
        $sessionFile=Join-Path $sdRoot 'logs/session-report.json'
        if(Test-Path -LiteralPath $sessionFile){
            try{
                $session=Get-Content -LiteralPath $sessionFile -Raw|ConvertFrom-Json
                if($session.session_id -like "$id-*"){
                    if($session.status -in @('running','passed_execution','failed') -and -not $settledCaptures.ContainsKey($session.session_id)){
                        $images=@(Capture-Session $session)
                        if($images.Count -or -not $captured.ContainsKey($session.session_id)){
                            $captured[$session.session_id]=$images
                        }
                        $complete=$images.Count -gt 0 -and @($images|Where-Object {-not $_.valid_ppm}).Count -eq 0
                        $pending=Test-Path -LiteralPath (Join-Path $sdRoot 'logs/pending-session.json')
                        if($session.status -ne 'running' -and -not $pending -and ($complete -or ($session.status -eq 'failed' -and -not $images.Count))){
                            $settledCaptures[$session.session_id]=$true
                        }
                    }
                    if($session.status -ceq 'failed' -and $session.error -match '; reopen homebrew$'){
                        $phaseFile=Join-Path $sdRoot 'logs/teardown.log'
                        if(Test-Path -LiteralPath $phaseFile){
                            $lastPhase=Get-Content -LiteralPath $phaseFile -Tail 1|ConvertFrom-Json
                            if($lastPhase.phase -ceq 'stop_failed'){$terminalRefusal=$session}
                        }
                    }
                }
            }catch{# Retry partial in-progress JSON writes; final evidence validation is strict.
            }
        }
        if($terminalRefusal){
            # The runtime explicitly requires process exit. Waiting cannot
            # produce a completed launcher workflow; preserve it as a failure.
            $receipt.terminal_runtime_refusal=$terminalRefusal
            throw "Runtime cleanup refused; process exit required: $($terminalRefusal.error)"
        }
        $launcherFile=Join-Path $sdRoot 'logs/launcher-report.json'
        if(Test-Path -LiteralPath $launcherFile){
            try{$candidate=Get-Content -LiteralPath $launcherFile -Raw|ConvertFrom-Json;if($candidate.session_id -eq $id){$result=$candidate;break}}catch{}
        }
        if(-not @(Update-OwnedProcesses).Count){
            $exited=$false
            try{$exited=$process.HasExited}catch{# Unknown identity waits for the bounded deadline.
            }
            if($exited){throw 'The owned emulator exited before the launcher workflow completed.'}
            $receipt.root_alive_without_cim_match=$true
        }
        Start-Sleep -Milliseconds 500
    }while([DateTime]::UtcNow -lt $deadline)
    if(-not $result -or $result.status -ne 'passed_execution'){throw 'No fresh completed launcher report before the timeout.'}
    Copy-Item -LiteralPath (Join-Path $sdRoot 'logs/launcher-report.json') -Destination (Join-Path $run 'emulator.json')
    if($result.snapshots.Count -ne $commands.Count+1){throw 'Launcher snapshot count does not match the prepared command count.'}
    foreach($snapshot in $result.snapshots){if($snapshot.presented_hash -cne $snapshot.shell.frame_hash){throw "Launcher submission mismatch at step $($snapshot.step)."}}
    $cases=[Collections.Generic.List[object]]::new()
    foreach($inputFile in $launches){
        $session=@($result.sessions|Where-Object session_id -eq $inputFile.expected_session_id)|Select-Object -First 1
        $installed=@($result.snapshots|Where-Object step -eq $inputFile.install_snapshot_step)|Select-Object -First 1
        $selected=if($installed){$installed.shell.installed[$installed.shell.selected]}else{$null}
        $identity=$selected -and $selected.apk_sha256 -eq $inputFile.sha256 -and $session -and
                  $selected.package -eq $session.package -and $selected.revision -eq $session.revision
        $frames=@($session.snapshots|Where-Object {$_.native_frames -gt 0 -or ($_.presented_hash -and $_.presented_hash -ceq $_.frame_hash)})
        $images=@(if($captured.ContainsKey($inputFile.expected_session_id)){$captured[$inputFile.expected_session_id]})
        $freshNonblank=@($images|Where-Object {$_.valid_ppm -and $_.sampled_nonuniform})
        $phases=@($session.teardown|ForEach-Object phase)
        $returned='runtime_closed' -in $phases
        $execution=$identity -and $session.status -eq 'passed_execution' -and -not $session.error -and
                   $session.presented_frames -gt 0 -and $frames.Count -gt 0 -and $returned
        $state=if(-not $execution){'blocked_or_unreported'}elseif(-not $freshNonblank.Count){'execution_observed_render_unconfirmed'}else{'execution_verified_visual_review_required'}
        $cases.Add(@{input=$inputFile;status=$state;installed_identity_matches=[bool]$identity;runtime_status=$session.status;error=$session.error;teardown_phases=$phases;returned_to_launcher=$returned;presented_frames=$session.presented_frames;snapshots=@($session.snapshots|Where-Object {$null -ne $_});captures=$images;first_screen='not_visually_verified'})
    }
    $receipt.cases=@($cases.ToArray())
    $receipt.visual_review='required'
    $receipt.status=if(@($cases|Where-Object status -eq 'blocked_or_unreported').Count){'blocked_runtime'}elseif(@($cases|Where-Object status -eq 'execution_observed_render_unconfirmed').Count){'attempted_render_unconfirmed'}else{'execution_verified_visual_review_required'}
    if(Test-Path -LiteralPath (Join-Path $sdRoot 'logs/display-error.json')){$receipt.status='failed_display';$receipt.display_error=Get-Content -LiteralPath (Join-Path $sdRoot 'logs/display-error.json') -Raw|ConvertFrom-Json}
    $receipt.completed_sessions=@($result.sessions).Count
}catch{
    $receipt.status=if($receipt.execution_attempted){'attempted_failed'}else{'preflight_failed'}
    $receipt.error=$_.Exception.Message
}finally{
    if($process){
        try{
            $process.Refresh()
            if($process.HasExited){$receipt.emulator_exit_code_before_cleanup=$process.ExitCode}
        }catch{# Process identity can disappear during a launcher/bootstrap exit.
        }
    }
    # A native process fault can prevent the final launcher report entirely.
    # Preserve the actual last status/error/phases, including a still-running
    # marker, instead of describing that attempt as a completed app session.
    if($stageCreated){
        try{
            $sessionPath=Join-Path $sdRoot 'logs/session-report.json'
            if(Test-Path -LiteralPath $sessionPath){
                $last=Get-Content -LiteralPath $sessionPath -Raw|ConvertFrom-Json
                if($last.session_id -like "$id-*"){
                    $receipt.latest_session_report=$last
                    $lastImages=@(Capture-Session $last)
                    if($lastImages.Count -or -not $captured.ContainsKey($last.session_id)){$captured[$last.session_id]=$lastImages}
                }
            }
            $phasePath=Join-Path $sdRoot 'logs/teardown.log'
            if(Test-Path -LiteralPath $phasePath){$receipt.latest_teardown=@(Get-Content -LiteralPath $phasePath|Where-Object {$_}|ForEach-Object {$_|ConvertFrom-Json})}
            $stopPath=Join-Path $sdRoot 'logs/media-stop.log'
            if(Test-Path -LiteralPath $stopPath){$receipt.latest_media_stop=@(Get-Content -LiteralPath $stopPath|Where-Object {$_}|ForEach-Object {$_|ConvertFrom-Json})}
            $admissionPath=Join-Path $sdRoot 'logs/memory-admission.json'
            if(Test-Path -LiteralPath $admissionPath){$receipt.latest_memory_admission=Get-Content -LiteralPath $admissionPath -Raw|ConvertFrom-Json}
        }catch{$receipt.evidence_capture_error=$_.Exception.Message}
    }
    try{
        Stop-OwnedProcesses
        if($stageCreated -and (Test-Path -LiteralPath $sdRoot)){
            foreach($path in @($sdRoot,$sdResult)){Assert-SdChild $path}
            Move-Item -LiteralPath $sdRoot -Destination $sdResult
        }
        if($backupMoved -and (Test-Path -LiteralPath $backup)){
            foreach($path in @($backup,$sdRoot)){Assert-SdChild $path}
            if(Test-Path -LiteralPath $sdRoot){throw "SD restore destination is occupied. Original data remains at $backup"}
            Move-Item -LiteralPath $backup -Destination $sdRoot
        }
        $receipt.sd_original_restore=if($stageCreated -or $backupMoved){'restored'}else{'not_modified'}
    }catch{$receipt.restore_error=$_.Exception.Message;$receipt.sd_original_restore='manual_recovery_required';$receipt.status='cleanup_failed'}
    if($lock){$lock.Dispose();Remove-Item -LiteralPath $lockPath -ErrorAction SilentlyContinue}
    # A later native fault must not erase an earlier named rejection. These
    # are observations, never invented completed cases or install identities.
    $observations=@(foreach($inputFile in $launches){
        $path=Join-Path $run "sessions/$($inputFile.expected_session_id)/session-report.json"
        if(Test-Path -LiteralPath $path){
            try{
                $observed=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($observed.session_id -ceq $inputFile.expected_session_id){
                    @{session_id=$observed.session_id;package=$observed.package;revision=$observed.revision;
                      runtime_status=$observed.status;error=$observed.error;report=$path;
                      completed_case_verified=$false;scope='Diagnostic observation; final launcher result is required to verify completion and installation identity.'}
                }
            }catch{# Keep raw files; malformed evidence does not become a case.
            }
        }
    })
    $receipt.observed_sessions=$observations
    Save-Receipt
}
Write-Output "External-APK evidence ($($receipt.status)): $run/verification.json"
if($receipt.status -ne 'execution_verified_visual_review_required'){throw "External launch was not proved: $($receipt.status). See the verification report and preserved SD result."}
