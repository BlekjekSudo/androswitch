param([string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',[string]$RyujinxProfile="$env:APPDATA/Ryujinx",[int]$TimeoutSeconds=180)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$id=[guid]::NewGuid().ToString('N');$run=Join-Path $projectRoot "local-artifacts/m6/runs/$id"
$root=Join-Path $RyujinxProfile 'sdcard/switch/androswitch'
New-Item -ItemType Directory -Force -Path $run,"$root/runtime","$root/logs" | Out-Null
$lock=[IO.File]::Open("$root/e2e.lock",[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process=$null;$receipt=@{schema_version=1;session_id=$id;status='running';backend='ryujinx';physical_device='not_run'}
try{
  $apk="$projectRoot/local-artifacts/m6/fixture/native-media.apk"
  $install=& "$projectRoot/target/debug/asctl.exe" install $apk --root $root --update --json
  if($LASTEXITCODE){throw 'NativeActivity fixture installation failed'}
  $record=($install | ConvertFrom-Json).package
  & "$projectRoot/target/debug/asctl.exe" clear-data $record.package --root $root --json | Out-Null
  if($LASTEXITCODE){throw 'Fixture storage preparation failed'}
  @{schema_version=1;session_id=$id;package=$record.package;revision=$record.revision;events=@(@{kind='wait';ms=10000},@{kind='touch';action=0;x=100;y=100},@{kind='touch';action=1;x=100;y=100},@{kind='wait';ms=4000})} | ConvertTo-Json -Depth 10 | Set-Content "$root/runtime/launch.json" -Encoding utf8NoBOM
  Copy-Item "$root/runtime/launch.json" "$run/launch.json"
  $nro="$root/androswitch.nro";Copy-Item "$projectRoot/local-artifacts/product/androswitch.nro" $nro
  $reportPath="$root/logs/session-report.json";if(Test-Path $reportPath){Move-Item $reportPath "$run/previous-report.json"}
  $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$run/ryujinx.stdout.log" -RedirectStandardError "$run/ryujinx.stderr.log" -PassThru
  $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds);$result=$null
  do{
    if($process.HasExited){throw "Ryujinx exited $($process.ExitCode)"}
    if(Test-Path $reportPath){try{$result=Get-Content $reportPath -Raw | ConvertFrom-Json;if($result.session_id -eq $id -or $result.status -eq 'failed'){break}}catch{}}
    Start-Sleep -Milliseconds 500
  }while([DateTime]::UtcNow -lt $deadline)
  if($null -eq $result -or $result.session_id -ne $id -or $result.status -ne 'passed_execution'){throw "No successful media execution: $($result | ConvertTo-Json -Depth 3 -Compress)"}
  Copy-Item $reportPath "$run/emulator.json"
  $proofPath="$root/userdata/$($record.package)/files/media.json"
  $proof=Get-Content $proofPath -Raw | ConvertFrom-Json
  if($proof.frames -lt 31 -or $proof.input -lt 2 -or $proof.asset_ok -ne 1 -or $proof.audio_frames -ne 12000 -or $proof.gl_ok -ne 1 -or $proof.readback_ok -ne 1 -or $proof.tls -ne 73){throw "Media proof mismatch: $($proof | ConvertTo-Json -Compress)"}
  if($result.snapshots[-1].audio_frames -ne 12000 -or $result.snapshots[-1].native_frames -lt 31){throw 'Platform media counters mismatch'}
  Copy-Item $proofPath "$run/guest-media.json"
  foreach($frame in @(2,30)){Copy-Item "$root/logs/gles-frame-$frame.ppm" "$run/gles-frame-$frame.ppm"}
  $receipt.status='passed';$receipt.proof=$proof;$receipt.nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();$receipt.apk_sha256=(Get-FileHash $apk).Hash.ToLowerInvariant();$receipt.emulator=@{version=(Get-Item $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash $Ryujinx).Hash.ToLowerInvariant()}
  Write-Output "M6 NativeActivity media E2E passed: $run/verification.json"
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
  if($process -and -not $process.HasExited){$owned=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)";if($owned.CommandLine -like '*androswitch.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)){Stop-Process -Id $process.Id -ErrorAction SilentlyContinue}}
  $receipt | ConvertTo-Json -Depth 12 | Set-Content "$run/verification.json" -Encoding utf8NoBOM
  $lock.Dispose();Remove-Item -LiteralPath "$root/e2e.lock" -ErrorAction SilentlyContinue
}
