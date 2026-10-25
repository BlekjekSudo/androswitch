param(
    [string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile="$env:APPDATA/Ryujinx",
    [switch]$PrepareOnly,
    [switch]$AllowDesktopEmulator
)
$ErrorActionPreference='Stop'
$events=[Collections.Generic.List[object]]::new()
function Tap([int]$X,[int]$Y){
    $events.Add(@{kind='touch';action=0;x=$X;y=$Y})
    $events.Add(@{kind='touch';action=1;x=$X;y=$Y})
    $events.Add(@{kind='frame'})
}
$events.Add(@{kind='wait';ms=1500});$events.Add(@{kind='frame'})
Tap 250 180 # Next category, in the ordinary 640x360 logical View tree.
Tap 250 320 # Next result, including the formerly clipped last row.
Tap 230 275 # Export full NOT_RUN report.
$events.Add(@{kind='wait';ms=1000})
Tap 50 275  # Reset: preserve an archived report.
Tap 240 130 # Stop/save, writing the ordinary autorun control to zero.
$events.Add(@{kind='return_to_launcher'})
$result=@(& "$PSScriptRoot/test-ryujinx-external.ps1" -ApkPaths "$PSScriptRoot/../local-artifacts/conformance/AndroSwitchConformance.apk" -SessionEvents $events.ToArray() -Headless -Ryujinx $Ryujinx -RyujinxProfile $RyujinxProfile -PrepareOnly:$PrepareOnly -AllowDesktopEmulator:$AllowDesktopEmulator)
$result|Write-Output
if(-not $PrepareOnly){
    $receipts=@($result|ForEach-Object {if("$_" -match '^External-APK evidence \([^)]*\): (.+verification\.json)$'){$Matches[1]}})
    if($receipts.Count -ne 1){throw 'Missing unambiguous UI workflow receipt'}
    & python -X utf8 "$PSScriptRoot/verify-conformance-ui.py" --receipt $receipts[0]
    if($LASTEXITCODE){throw 'Conformance UI callback/export/history/stop verification failed'}
}
