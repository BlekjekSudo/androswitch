# Validate saved SD evidence only. This script never launches or stops an app.
param([Parameter(Mandatory)][string]$Root,[Parameter(Mandatory)][string]$Run,[Parameter(Mandatory)][hashtable]$Receipt,[Parameter(Mandatory)][int]$CommandCount,[string]$Scenario='workflow',[int]$Failures=0,[string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe')
$ErrorActionPreference='Stop'
$id=$Receipt.session_id
$nro=Join-Path $Root 'androswitch.nro'
$result=Get-Content (Join-Path $Root 'logs/launcher-report.json') -Raw | ConvertFrom-Json
if(Test-Path (Join-Path $Root 'logs/display-error.json')){throw 'Display failure recorded in saved evidence'}
  if($null -eq $result -or $result.session_id -ne $id -or $result.status -ne 'passed_execution'){throw 'No fresh completed launcher workflow report'}
  if($result.snapshots.Count -ne $CommandCount+1){throw 'Launcher snapshot count mismatch'}
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
  $receipt.status='passed';$receipt.commands=$CommandCount;$receipt.completed_sessions=$successful.Count;$receipt.expected_api_errors=$failed.Count;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
  Write-Output "M7 one-NRO launcher E2E passed: $run/verification.json"
