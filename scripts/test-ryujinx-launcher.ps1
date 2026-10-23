param([string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',[string]$RyujinxProfile="$env:APPDATA/Ryujinx",[int]$TimeoutSeconds=300,[ValidateSet('workflow','quiescence','startup-failure','startup-quiescence')][string]$Scenario='workflow',[string]$ExternalApk='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$runStarted=[DateTime]::UtcNow
function Stop-TestProcess([Diagnostics.Process]$OwnedProcess){
  if(-not $OwnedProcess){return}
  # Ryujinx can restart itself in a child with the same NRO. Track ancestry,
  # creation time and executable/request identity; never stop unrelated sessions.
  $ownedIds=[Collections.Generic.HashSet[int]]::new();$null=$ownedIds.Add($OwnedProcess.Id)
  for($attempt=0;$attempt -lt 3;$attempt++){
    $candidates=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'" | Where-Object {$testProcess=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue;$testProcess -and -not $testProcess.HasExited})
    do{
      $added=$false
      foreach($candidate in $candidates){
        if($ownedIds.Contains([int]$candidate.ParentProcessId) -and -not $ownedIds.Contains([int]$candidate.ProcessId) -and $candidate.CreationDate.ToUniversalTime() -ge $runStarted -and $candidate.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx) -and $candidate.CommandLine.Replace('/','\').Contains($nro.Replace('/','\'))){$null=$ownedIds.Add([int]$candidate.ProcessId);$added=$true}
      }
    }while($added)
    foreach($candidate in $candidates){if($candidate.ProcessId -ne $OwnedProcess.Id -and $ownedIds.Contains([int]$candidate.ProcessId)){Stop-Process -Id $candidate.ProcessId -ErrorAction SilentlyContinue}}
    if(-not $OwnedProcess.HasExited){$OwnedProcess.Kill();$null=$OwnedProcess.WaitForExit(15000)}
    Start-Sleep -Milliseconds 100
  }
}
$preflightDeadline=[DateTime]::UtcNow.AddSeconds(3)
# CIM can retain a terminated updater child after the PID has disappeared.
do{$existing=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'" | Where-Object {$testProcess=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue;$testProcess -and -not $testProcess.HasExited});if(-not $existing.Count){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $preflightDeadline)
if($existing.Count){throw 'Close the existing Ryujinx session before this isolated E2E run.'}
$id=[guid]::NewGuid().ToString('N');$run=Join-Path $projectRoot "local-artifacts/m7/runs/$id"
$switchRoot=[IO.Path]::GetFullPath((Join-Path $RyujinxProfile 'sdcard/switch'))
$root=Join-Path $switchRoot 'androswitch';$backup=Join-Path $switchRoot "androswitch-e2e-backup-$id";$resultRoot=Join-Path $switchRoot "androswitch-e2e-result-$id"
foreach($path in @($root,$backup,$resultRoot)){if(-not [IO.Path]::GetFullPath($path).StartsWith($switchRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Resolved SD test path escaped its designated directory'}}
New-Item -ItemType Directory -Force -Path $run,$switchRoot | Out-Null
$lock=[IO.File]::Open("$switchRoot/androswitch-launcher-e2e.lock",[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process=$null;$moved=$false;$receipt=@{schema_version=1;session_id=$id;status='running';backend='ryujinx';physical_device='not_run';sd_result=$resultRoot;scenario=$Scenario}
try{
  if(Test-Path $root){if(Test-Path "$root/e2e.lock"){throw 'Another AndroSwitch E2E test holds the SD folder'};Move-Item -LiteralPath $root -Destination $backup;$moved=$true}
  New-Item -ItemType Directory -Force -Path "$root/apks","$root/runtime","$root/logs" | Out-Null
  $files=@(@('m5/fixture/java-ui.apk','01-java.apk'),@('m5/fixture/kotlin-ui.apk','02-kotlin.apk'),@('m6/fixture/mixed-media.apk','03-mixed.apk'),@('m6/fixture/native-media.apk','04-native.apk'),@('m6/fixture/unavailable.apk','05-unavailable.apk'))
  foreach($item in $files){Copy-Item "$projectRoot/local-artifacts/$($item[0])" "$root/apks/$($item[1])"}
  [IO.File]::WriteAllText("$root/apks/06-broken.apk",'invalid ZIP proof')
  $commands=[Collections.Generic.List[object]]::new()
  function Key([string]$Value){$commands.Add(@{kind='key';key=$Value})}
  function OpenApp([object[]]$Events){$commands.Add(@{kind='open';events=$Events})}
  $commands.Add(@{kind='frame'})
  for($i=0;$i -lt 6;$i++){Key right;for($row=0;$row -lt $i;$row++){Key down};Key accept}
  Key left
  $java=(Get-Content "$projectRoot/local-artifacts/m5/fixture/java-request.json" -Raw | ConvertFrom-Json).events
  $kotlin=(Get-Content "$projectRoot/local-artifacts/m5/fixture/kotlin-request.json" -Raw | ConvertFrom-Json).events
  $native=@(@{kind='wait';ms=10000},@{kind='touch';action=0;x=100;y=100},@{kind='touch';action=1;x=100;y=100},@{kind='wait';ms=4000},@{kind='return_to_launcher'})
  OpenApp $java
  OpenApp @(@{kind='wait';ms=1500})
  Key down;OpenApp $kotlin
  Key down;OpenApp @(@{kind='frame'})
  Key down;OpenApp $native
  $commands.Add(@{kind='frames';count=60})
  Key up;Key up;OpenApp $kotlin
  Key down;Key down;OpenApp $native
  $commands.Add(@{kind='frames';count=60})
  Key down;OpenApp @(@{kind='frame'})
  for($i=0;$i -lt 4;$i++){Key up}
  Key remove;Key back;Key remove;Key accept
  Key right;Key accept;OpenApp @(@{kind='wait';ms=1500})
  Key right;Key accept;OpenApp @(@{kind='wait';ms=1500})
  Key clear_data;Key accept;OpenApp @(@{kind='wait';ms=1500})
  Key details;Key back;$commands.Add(@{kind='frame'})
  if($Scenario -eq 'startup-failure'){
    foreach($file in Get-ChildItem "$root/apks" -File){Remove-Item -LiteralPath $file.FullName}
    $startupFiles=@(@('startup-failure/fixture/java.apk','01-failure-java.apk'),@('startup-failure/fixture/native.apk','02-failure-native.apk'),@('startup-failure/fixture/renamed.apk','03-failure-renamed.apk'))
    foreach($item in $startupFiles){Copy-Item "$projectRoot/local-artifacts/$($item[0])" "$root/apks/$($item[1])"}
    $failures=3
    if($ExternalApk){Copy-Item -LiteralPath $ExternalApk -Destination "$root/apks/04-external.apk";$failures++;$receipt.external_apk_sha256=(Get-FileHash -LiteralPath $ExternalApk).Hash.ToLowerInvariant()}
    Copy-Item "$projectRoot/local-artifacts/m5/fixture/java-ui.apk" "$root/apks/05-java.apk"
    Copy-Item "$projectRoot/local-artifacts/m6/fixture/native-media.apk" "$root/apks/06-native.apk"
    $commands.Clear()
    for($i=0;$i -lt $failures+2;$i++){
      Key right;for($row=0;$row -lt $i;$row++){Key down};Key accept
      if($i -lt $failures){OpenApp @(@{kind='frame'})}elseif($i -eq $failures){OpenApp $java}else{OpenApp $native}
      $commands.Add(@{kind='frames';count=60})
    }
    for($i=0;$i -lt $failures+2;$i++){Key up}
    OpenApp @(@{kind='wait';ms=1500});$commands.Add(@{kind='frames';count=60})
  }
  if($Scenario -in @('quiescence','startup-quiescence')){
    foreach($file in Get-ChildItem "$root/apks" -File){Remove-Item -LiteralPath $file.FullName}
    $boundaryFixture=if($Scenario -eq 'startup-quiescence'){'startup-failure/fixture/worker.apk'}else{'m6/fixture/nonquiescent.apk'}
    Copy-Item "$projectRoot/local-artifacts/$boundaryFixture" "$root/apks/01-nonquiescent.apk"
    $commands.Clear();Key right;Key accept;OpenApp @(@{kind='wait';ms=50})
  }
  @{schema_version=1;session_id=$id;commands=@($commands.ToArray())} | ConvertTo-Json -Depth 20 | Set-Content "$root/runtime/launcher-test.json" -Encoding utf8NoBOM
  Copy-Item "$root/runtime/launcher-test.json" "$run/launcher-test.json"
  $nro="$root/androswitch.nro";Copy-Item "$projectRoot/local-artifacts/product/androswitch.nro" $nro
  $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$run/ryujinx.stdout.log" -RedirectStandardError "$run/ryujinx.stderr.log" -PassThru
  $receipt.pid=$process.Id;$deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$result=$null
  do{
    if(Test-Path "$root/logs/display-error.json"){throw "Display failure: $(Get-Content "$root/logs/display-error.json" -Raw)"}
    if($Scenario -in @('quiescence','startup-quiescence') -and (Test-Path "$root/logs/session-report.json")){
      try{$failure=Get-Content "$root/logs/session-report.json" -Raw | ConvertFrom-Json;if($failure.session_id -eq "$id-1" -and $failure.error -match 'NATIVE_NOT_QUIESCENT'){break}}catch{}
    }
    if($process.HasExited){throw "Ryujinx exited $($process.ExitCode); inspect pending session and teardown logs"}
    if(Test-Path "$root/logs/launcher-report.json"){try{$result=Get-Content "$root/logs/launcher-report.json" -Raw | ConvertFrom-Json;if($result.session_id -eq $id){break}}catch{}}
    Start-Sleep -Milliseconds 500
  }while([DateTime]::UtcNow -lt $deadline)
  if($Scenario -in @('quiescence','startup-quiescence')){
    if($failure.session_id -ne "$id-1" -or $failure.error -notmatch 'NATIVE_NOT_QUIESCENT'){throw 'Nonquiescent worker did not fail closed with a named error'}
    if($Scenario -eq 'startup-quiescence' -and $failure.error -notmatch 'JNI_VERSION_UNSUPPORTED'){throw 'Startup cleanup erased the original JNI_OnLoad failure'}
    if(Test-Path "$root/logs/launcher-report.json"){throw 'Unsafe worker returned to a completed launcher workflow'}
    if(Test-Path "$root/runtime/launch.json"){throw 'Launch request was not consumed; crash recovery would relaunch the same APK'}
    Copy-Item "$root/logs/session-report.json" "$run/expected-stop-failure.json"
    Stop-TestProcess $process
    @{schema_version=1;session_id="$id-recovery";commands=@(@{kind='frame'})} | ConvertTo-Json -Depth 8 | Set-Content "$root/runtime/launcher-test.json" -Encoding utf8NoBOM
    $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$run/recovery.stdout.log" -RedirectStandardError "$run/recovery.stderr.log" -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$recovery=$null
    do{if(Test-Path "$root/logs/launcher-report.json"){try{$recovery=Get-Content "$root/logs/launcher-report.json" -Raw | ConvertFrom-Json;if($recovery.session_id -eq "$id-recovery"){break}}catch{}};Start-Sleep -Milliseconds 500}while([DateTime]::UtcNow -lt $deadline)
    if($recovery.session_id -ne "$id-recovery" -or $recovery.snapshots[0].shell.message -notmatch 'Previous session was interrupted'){throw 'Next boot did not present interruption details in the console'}
    $interrupted=Get-Content "$root/logs/last-interrupted.json" -Raw | ConvertFrom-Json
    if($interrupted.session_id -ne "$id-1"){throw 'Wrong interruption record'}
    Copy-Item "$root/logs/launcher-report.json" "$run/recovery.json"
    $receipt.status='passed';$receipt.boundary='process_exit_required';$receipt.next_boot_recovery='passed';$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant()
    Write-Output "M7 nonquiescent worker/recovery E2E passed: $run/verification.json"
    return
  }
  if($null -eq $result -or $result.session_id -ne $id -or $result.status -ne 'passed_execution'){throw 'No fresh completed launcher workflow report'}
  if($result.snapshots.Count -ne $commands.Count+1){throw 'Launcher snapshot count mismatch'}
  foreach($snapshot in $result.snapshots){if($snapshot.presented_hash -cne $snapshot.shell.frame_hash){throw "Launcher framebuffer mismatch at step $($snapshot.step)"}}
  if($Scenario -eq 'startup-failure'){
    $failed=@($result.sessions | Where-Object status -eq 'failed');$successful=@($result.sessions | Where-Object status -eq 'passed_execution')
    if($failed.Count -ne $failures -or $successful.Count -ne 3){throw 'Startup failures did not recover into three successful sessions'}
    foreach($session in $failed){
      $dependency=if($session.package -eq 'com.martinmagni.mekorama'){'libstdc\+\+\.so'}else{'libunavailable-contract\.so'}
      if($session.error -notmatch $dependency){throw "Wrong native dependency error: $($session.error)"}
      foreach($phase in @('java_startup_abort','native_resources_released','runtime_closed')){if($phase -notin $session.teardown.phase){throw "Failed startup skipped $phase"}}
    }
    if(@($failed | Where-Object package -eq 'org.example.renamedstartup').Count -ne 1){throw 'Renamed package did not use the same error/recovery path'}
    if($successful[-1].package -ne 'org.fixture.uiproof' -or $successful[-2].package -ne 'org.fixture.nativemedia'){throw 'Java/native/Java recovery order mismatch'}
    if($successful[-2].snapshots[-1].native_frames -lt 31 -or $successful[-2].snapshots[-1].audio_frames -ne 12000){throw 'Post-failure native graphics/audio did not execute'}
    if(Test-Path "$root/logs/pending-session.json"){throw 'Recovered startup left an interruption marker'}
    Copy-Item "$root/logs/launcher-report.json" "$run/emulator.json"
    Copy-Item "$root/logs/display-handoffs.log" "$run/display-handoffs.log"
    $receipt.status='passed';$receipt.completed_sessions=$successful.Count;$receipt.expected_startup_errors=$failed.Count;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
    Write-Output "Native startup recovery E2E passed: $run/verification.json"
    return
  }
  if($result.snapshots[-1].shell.installed.Count -ne 5){throw 'Installed list mismatch'}
  if($result.snapshots[-1].shell.decoded_icons -ne 1){throw 'APK resource PNG icon was not decoded'}
  if(-not ($result.snapshots | Where-Object {$_.shell.message -match 'INVALID_APK'})){throw 'Broken APK install did not report its failure'}
  $failed=@($result.sessions | Where-Object status -eq 'failed');$successful=@($result.sessions | Where-Object status -eq 'passed_execution')
  if($failed.Count -ne 1 -or $failed[0].error -notmatch 'getSystemService'){throw 'Expected named framework API failure missing'}
  if($successful.Count -ne 10){throw "Expected 10 completed app sessions, got $($successful.Count)"}
  $javaRuns=@($successful | Where-Object package -eq 'org.fixture.uiproof')
  $lastTexts=@($javaRuns | ForEach-Object {($_.snapshots[-1].views | Where-Object text | ForEach-Object text) -join ' | '})
  if($javaRuns.Count -ne 5 -or $lastTexts[1] -notmatch 'Ready 1 / app 1' -or $lastTexts[2] -notmatch 'Ready 1 / app 1' -or $lastTexts[3] -notmatch 'Ready 1 / app 1' -or $lastTexts[4] -notmatch 'Ready 0 / app 1'){throw "Save/update/uninstall/restart mismatch: $lastTexts"}
  if($javaRuns[2].revision -ne $javaRuns[3].revision){throw 'Identical APK reinstall created a new revision'}
  if(-not ($result.snapshots | Where-Object {$_.shell.tab -eq 'installed' -and $_.shell.message -match 'Already installed'})){throw 'Identical install did not immediately select the installed app'}
  $mixed=@($successful | Where-Object package -eq 'org.fixture.mixedmedia')[0]
  if(-not ($mixed.snapshots[-1].views | Where-Object text -eq 'Mixed assets 150000 / Java PCM 4800 / native 2400 / SL 4800')){throw 'Mixed Java/JNI asset/audio mismatch'}
  if($mixed.snapshots[-1].audio_frames -ne 9600){throw 'Shared PCM backend frame count mismatch'}
  foreach($nativeRun in @($successful | Where-Object package -eq 'org.fixture.nativemedia')){
    if($nativeRun.snapshots[-1].native_frames -lt 31 -or $nativeRun.snapshots[-1].audio_frames -ne 12000){throw 'Repeated native session/reset mismatch'}
    foreach($phase in @('egl_wait_gpu','egl_terminated','guest_thread_joined','native_resources_released','runtime_closed')){if($phase -notin $nativeRun.teardown.phase){throw "Native Plus shutdown did not complete $phase"}}
  }
  $media=Get-Content "$root/userdata/org.fixture.nativemedia/files/media.json" -Raw | ConvertFrom-Json
  if($media.gl_ok -ne 1 -or $media.readback_ok -ne 1 -or $media.asset_ok -ne 1 -or $media.tls -ne 73 -or $media.input -lt 2 -or $media.audio_frames -ne 12000){throw 'Guest rendering/full-frame readback/asset/thread/input proof mismatch'}
  Copy-Item "$root/userdata/org.fixture.nativemedia/files/media.json" "$run/guest-media.json"
  if(Test-Path "$root/logs/pending-session.json"){throw 'Clean workflow left an interrupted-session marker'}
  $trace=Get-Content "$root/logs/teardown.log" | ForEach-Object {$_ | ConvertFrom-Json}
  if($trace[-1].phase -ne 'launcher_ready'){throw 'Teardown did not reach a presented launcher framebuffer'}
  Copy-Item "$root/logs/teardown.log" "$run/teardown.log"
  if(Test-Path "$root/logs/display-error.json"){throw 'Display backend recorded a checked failure'}
  $handoffs=@(Get-Content "$root/logs/display-handoffs.log" | ForEach-Object {$_ | ConvertFrom-Json})
  if($handoffs.Count -ne 16){throw 'Both native returns must survive eight CPU frames'}
  foreach($generation in @($handoffs | Group-Object generation)){
    if(($generation.Group.frame -join ',') -ne '1,2,3,4,5,6,7,8'){throw 'Incomplete display handoff sequence'}
  }
  foreach($frame in $handoffs){if($frame.buffers -ne 3 -or $frame.slot -lt 0 -or $frame.slot -ge $frame.buffers -or $frame.result -ne 0){throw 'Invalid display queue slot/result'}}
  if(@($handoffs.map_fd | Sort-Object -Unique).Count -ne 1 -or @($handoffs.fence_fd | Sort-Object -Unique).Count -ne 1){throw 'Display service handles changed between renderer sessions'}
  Copy-Item "$root/logs/display-handoffs.log" "$run/display-handoffs.log"
  Copy-Item "$root/logs/launcher-report.json" "$run/emulator.json";Copy-Item "$root/logs/last-frame.ppm" "$run/launcher.ppm"
  $receipt.status='passed';$receipt.commands=$commands.Count;$receipt.completed_sessions=$successful.Count;$receipt.expected_api_errors=$failed.Count;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
  Write-Output "M7 one-NRO launcher E2E passed: $run/verification.json"
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
  Stop-TestProcess $process
  if(Test-Path $root){Move-Item -LiteralPath $root -Destination $resultRoot}
  if($moved -and (Test-Path $backup)){Move-Item -LiteralPath $backup -Destination $root}
  $receipt | ConvertTo-Json -Depth 12 | Set-Content "$run/verification.json" -Encoding utf8NoBOM
  $lock.Dispose();Remove-Item -LiteralPath "$switchRoot/androswitch-launcher-e2e.lock" -ErrorAction SilentlyContinue
}
