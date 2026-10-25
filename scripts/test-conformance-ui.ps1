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
& "$PSScriptRoot/test-ryujinx-external.ps1" -ApkPaths "$PSScriptRoot/../local-artifacts/conformance/AndroSwitchConformance.apk" -SessionEvents $events.ToArray() -Headless -Ryujinx $Ryujinx -RyujinxProfile $RyujinxProfile -PrepareOnly:$PrepareOnly -AllowDesktopEmulator:$AllowDesktopEmulator
