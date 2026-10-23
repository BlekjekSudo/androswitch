# Read/validate an interrupted workflow without launching any desktop application.
param([Parameter(Mandatory)][string]$RunId,[string]$RyujinxProfile="$env:APPDATA/Ryujinx",[string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe')
$ErrorActionPreference='Stop'
if($RunId -notmatch '^[a-f0-9]{32}$'){throw 'Expected a recorded E2E run ID'}
$projectRoot=Split-Path -Parent $PSScriptRoot
$run=Join-Path $projectRoot "local-artifacts/m7/runs/$RunId"
$switchBase=[IO.Path]::GetFullPath((Join-Path $RyujinxProfile 'sdcard/switch'))
$root=[IO.Path]::GetFullPath((Join-Path $switchBase "androswitch-e2e-result-$RunId"))
if(-not $root.StartsWith($switchBase+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Evidence path escaped SD result directory'}
$receipt=Get-Content "$run/verification.json" -Raw | ConvertFrom-Json -AsHashtable
if($receipt.session_id -ne $RunId -or $receipt.scenario -ne 'workflow' -or $receipt.sd_result -ne $root){throw 'Receipt identity/path does not match the saved workflow'}
$script=Get-Content "$run/launcher-test.json" -Raw | ConvertFrom-Json
if($script.session_id -ne $RunId){throw 'Saved command script identity mismatch'}
$actualHash=(Get-FileHash "$root/androswitch.nro").Hash.ToLowerInvariant()
if($actualHash -cne $receipt.nro_sha256){throw 'Saved NRO does not match interrupted receipt'}
try{
  & "$PSScriptRoot/validate-launcher-report.ps1" -Root $root -Run $run -Receipt $receipt -CommandCount $script.commands.Count -Scenario workflow -Ryujinx $Ryujinx
  $receipt.verification_method='shared_validator_on_saved_SD_after_desktop_test_interruption'
  $receipt.emulator_relaunched_for_validation=$false
  $receipt.validation='completed'
}catch{$receipt.status='failed';$receipt.error=$_.Exception.Message;throw}finally{
  $receipt | ConvertTo-Json -Depth 12 | Set-Content "$run/verification.json" -Encoding utf8NoBOM
}
