param([string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',[string]$RyujinxProfile="$env:APPDATA/Ryujinx",[int]$TimeoutSeconds=300,[ValidateSet('workflow','quiescence')][string]$Scenario='workflow')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$preflightDeadline=[DateTime]::UtcNow.AddSeconds(3)
do{$existing=@(Get-CimInstance Win32_Process -Filter "Name='Ryujinx.exe'");if(-not $existing.Count){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $preflightDeadline)
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
  $commands.Add(@{kind='frame'});Key right
  for($i=0;$i -lt 6;$i++){Key accept;if($i -lt 5){Key down}}
  Key left
  $java=(Get-Content "$projectRoot/local-artifacts/m5/fixture/java-request.json" -Raw | ConvertFrom-Json).events
  $kotlin=(Get-Content "$projectRoot/local-artifacts/m5/fixture/kotlin-request.json" -Raw | ConvertFrom-Json).events
  $native=@(@{kind='wait';ms=10000},@{kind='touch';action=0;x=100;y=100},@{kind='touch';action=1;x=100;y=100},@{kind='wait';ms=4000})
  OpenApp $java
  OpenApp @(@{kind='wait';ms=1500})
  Key down;OpenApp $kotlin
  Key down;OpenApp @(@{kind='frame'})
  Key down;OpenApp $native
  Key up;Key up;OpenApp $kotlin
  Key down;Key down;OpenApp $native
  Key down;OpenApp @(@{kind='frame'})
  for($i=0;$i -lt 4;$i++){Key up}
  Key remove;Key back;Key remove;Key accept
  Key right;Key accept;Key left;OpenApp @(@{kind='wait';ms=1500})
  Key right;Key accept;Key left;OpenApp @(@{kind='wait';ms=1500})
  Key clear_data;Key accept;OpenApp @(@{kind='wait';ms=1500})
  Key details;Key back;$commands.Add(@{kind='frame'})
  if($Scenario -eq 'quiescence'){
    foreach($file in Get-ChildItem "$root/apks" -File){Remove-Item -LiteralPath $file.FullName}
    Copy-Item "$projectRoot/local-artifacts/m6/fixture/nonquiescent.apk" "$root/apks/01-nonquiescent.apk"
    $commands.Clear();Key right;Key accept;Key left;OpenApp @(@{kind='wait';ms=50})
  }
  @{schema_version=1;session_id=$id;commands=@($commands.ToArray())} | ConvertTo-Json -Depth 20 | Set-Content "$root/runtime/launcher-test.json" -Encoding utf8NoBOM
  Copy-Item "$root/runtime/launcher-test.json" "$run/launcher-test.json"
  $nro="$root/androswitch.nro";Copy-Item "$projectRoot/local-artifacts/product/androswitch.nro" $nro
  $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$run/ryujinx.stdout.log" -RedirectStandardError "$run/ryujinx.stderr.log" -PassThru
  $receipt.pid=$process.Id;$deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$result=$null
  do{
    if($Scenario -eq 'quiescence' -and (Test-Path "$root/logs/session-report.json")){
      try{$failure=Get-Content "$root/logs/session-report.json" -Raw | ConvertFrom-Json;if($failure.session_id -eq "$id-1" -and $failure.error -match 'NATIVE_NOT_QUIESCENT'){break}}catch{}
    }
    if($process.HasExited){throw "Ryujinx exited $($process.ExitCode); inspect pending session and teardown logs"}
    if(Test-Path "$root/logs/launcher-report.json"){try{$result=Get-Content "$root/logs/launcher-report.json" -Raw | ConvertFrom-Json;if($result.session_id -eq $id){break}}catch{}}
    Start-Sleep -Milliseconds 500
  }while([DateTime]::UtcNow -lt $deadline)
  if($Scenario -eq 'quiescence'){
    if($failure.session_id -ne "$id-1" -or $failure.error -notmatch 'NATIVE_NOT_QUIESCENT'){throw 'Nonquiescent worker did not fail closed with a named error'}
    if(Test-Path "$root/logs/launcher-report.json"){throw 'Unsafe worker returned to a completed launcher workflow'}
    if(Test-Path "$root/runtime/launch.json"){throw 'Launch request was not consumed; crash recovery would relaunch the same APK'}
    Copy-Item "$root/logs/session-report.json" "$run/expected-stop-failure.json"
    # Use the exact process object returned by this run's Start-Process. This
    # retains ownership even when Windows reports a different resolved path.
    if(-not $process.HasExited){$process.Kill();$null=$process.WaitForExit(15000)}
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
  if($result.snapshots[-1].shell.installed.Count -ne 5){throw 'Installed list mismatch'}
  if($result.snapshots[-1].shell.decoded_icons -ne 1){throw 'APK resource PNG icon was not decoded'}
  if(-not ($result.snapshots | Where-Object {$_.shell.message -match 'INVALID_APK'})){throw 'Broken APK install did not report its failure'}
  $failed=@($result.sessions | Where-Object status -eq 'failed');$successful=@($result.sessions | Where-Object status -eq 'passed_execution')
  if($failed.Count -ne 1 -or $failed[0].error -notmatch 'getSystemService'){throw 'Expected named framework API failure missing'}
  if($successful.Count -ne 10){throw "Expected 10 completed app sessions, got $($successful.Count)"}
  $javaRuns=@($successful | Where-Object package -eq 'org.fixture.uiproof')
  $lastTexts=@($javaRuns | ForEach-Object {($_.snapshots[-1].views | Where-Object text | ForEach-Object text) -join ' | '})
  if($javaRuns.Count -ne 5 -or $lastTexts[1] -notmatch 'Ready 1 / app 1' -or $lastTexts[2] -notmatch 'Ready 1 / app 1' -or $lastTexts[3] -notmatch 'Ready 1 / app 1' -or $lastTexts[4] -notmatch 'Ready 0 / app 1'){throw "Save/update/uninstall/restart mismatch: $lastTexts"}
  $mixed=@($successful | Where-Object package -eq 'org.fixture.mixedmedia')[0]
  if(-not ($mixed.snapshots[-1].views | Where-Object text -eq 'Mixed assets 150000 / Java PCM 4800 / native 2400 / SL 4800')){throw 'Mixed Java/JNI asset/audio mismatch'}
  if($mixed.snapshots[-1].audio_frames -ne 9600){throw 'Shared PCM backend frame count mismatch'}
  foreach($nativeRun in @($successful | Where-Object package -eq 'org.fixture.nativemedia')){if($nativeRun.snapshots[-1].native_frames -lt 31 -or $nativeRun.snapshots[-1].audio_frames -ne 12000){throw 'Repeated native session/reset mismatch'}}
  $media=Get-Content "$root/userdata/org.fixture.nativemedia/files/media.json" -Raw | ConvertFrom-Json
  if($media.gl_ok -ne 1 -or $media.readback_ok -ne 1 -or $media.asset_ok -ne 1 -or $media.tls -ne 73 -or $media.input -lt 2 -or $media.audio_frames -ne 12000){throw 'Guest rendering/full-frame readback/asset/thread/input proof mismatch'}
  Copy-Item "$root/userdata/org.fixture.nativemedia/files/media.json" "$run/guest-media.json"
  if(Test-Path "$root/logs/pending-session.json"){throw 'Clean workflow left an interrupted-session marker'}
  Copy-Item "$root/logs/launcher-report.json" "$run/emulator.json";Copy-Item "$root/logs/last-frame.ppm" "$run/launcher.ppm"
  $receipt.status='passed';$receipt.commands=$commands.Count;$receipt.completed_sessions=$successful.Count;$receipt.expected_api_errors=$failed.Count;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
  Write-Output "M7 one-NRO launcher E2E passed: $run/verification.json"
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
  if($process -and -not $process.HasExited){$process.Kill();$null=$process.WaitForExit(15000)}
  if(Test-Path $root){Move-Item -LiteralPath $root -Destination $resultRoot}
  if($moved -and (Test-Path $backup)){Move-Item -LiteralPath $backup -Destination $root}
  $receipt | ConvertTo-Json -Depth 12 | Set-Content "$run/verification.json" -Encoding utf8NoBOM
  $lock.Dispose();Remove-Item -LiteralPath "$switchRoot/androswitch-launcher-e2e.lock" -ErrorAction SilentlyContinue
}
