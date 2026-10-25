# One installation, repeated ordinary app launches; never promotes missing APIs to PASS.
param(
    [ValidateRange(1,100)][int]$Launches=60,
    [ValidateRange(1000,30000)][int]$WaitMilliseconds=1000,
    [ValidateRange(30,1800)][int]$TimeoutSeconds=900,
    [string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile="$env:APPDATA/Ryujinx",
    [string]$Nro='',
    [switch]$PrepareOnly,
    [switch]$AllowDesktopEmulator
)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$output=Join-Path $repo 'local-artifacts/conformance'
$package='org.androswitch.conformance'
$catalogPath=Join-Path $output 'catalog.json'
$digest=(Get-FileHash -LiteralPath $catalogPath).Hash.ToLowerInvariant()
$seed=Join-Path $output ('seeds-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$seed/$package/files"|Out-Null
[IO.File]::WriteAllText("$seed/$package/files/autorun.txt",'1')
$watch=@("$package/files/conformance/$($digest.Substring(0,12))/pending.txt")
$parameters=@{
    ApkPaths=@((Join-Path $output 'AndroSwitchConformance.apk'));LaunchesPerApk=$Launches
    UserDataSeed=$seed;UserDataWatch=$watch;Headless=$true;Ryujinx=$Ryujinx;RyujinxProfile=$RyujinxProfile
    StartupWaitMilliseconds=$WaitMilliseconds;TimeoutSeconds=$TimeoutSeconds
    PrepareOnly=$PrepareOnly;AllowDesktopEmulator=$AllowDesktopEmulator
}
if($Nro){$parameters.Nro=$Nro}
# Locate the fresh receipt by its unique seed source, not by an untrusted newest file.
$failure=$null
try {& "$PSScriptRoot/test-ryujinx-external.ps1" @parameters} catch {$failure=$_.Exception.Message}
$matches=@(Get-ChildItem "$repo/local-artifacts/m9/external-runs" -Directory|ForEach-Object {Get-Item -LiteralPath (Join-Path $_.FullName 'verification.json') -ErrorAction SilentlyContinue}|Where-Object {
    $record=Get-Content $_.FullName -Raw|ConvertFrom-Json
    @($record.userdata_seed|Where-Object {$_.source -and $_.source.StartsWith($seed+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)}).Count -gt 0
})
if($matches.Count -ne 1){throw "Missing unambiguous external receipt; $failure"}
$receiptPath=$matches[0].FullName;$run=Split-Path -Parent $receiptPath
if($PrepareOnly){Write-Output "Prepared unified suite: $receiptPath";if($failure){throw $failure};return}
& python -X utf8 "$PSScriptRoot/collect-conformance.py" --receipt $receiptPath --catalog $catalogPath --package $package
if($LASTEXITCODE){throw "Conformance evidence collection failed; $failure"}
Write-Output "Conformance journal and actual session errors: $run/conformance-summary.json"
if($failure){Write-Warning "External renderer/session gate remains: $failure"}
