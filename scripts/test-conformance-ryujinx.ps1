# One installation, repeated ordinary app launches; never promotes missing APIs to PASS.
param(
    [ValidateRange(1,100)][int]$Launches=60,
    [ValidateRange(1000,30000)][int]$WaitMilliseconds=1000,
    [ValidateRange(1,48)][int]$WaitSegments=1,
    [ValidateRange(30,1800)][int]$TimeoutSeconds=900,
    [string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile="$env:APPDATA/Ryujinx",
    [string]$Nro='',
    [string]$FixtureDirectory='local-artifacts/conformance',
    [string]$FilterGroup='All',
    [string[]]$ExcludeCases=@(),
    [string]$ResumeEvidenceDirectory='',
    [string]$RerunCategory='',
    [switch]$Baseline,
    [switch]$PrepareOnly,
    [switch]$AllowDesktopEmulator
)
$ErrorActionPreference='Stop'
if($RerunCategory -and (-not $ResumeEvidenceDirectory -or $Launches -ne 1)){throw 'UI category rerun requires one launch and a verified resume journal'}
if($RerunCategory -and $FilterGroup -ne 'All'){throw 'Choose category rerun or an ordinary autorun filter'}
if($FilterGroup -ne 'All' -and -not $Baseline){throw 'Selected-group runs require -Baseline; full catalog acceptance is a separate gate'}
$repo=Split-Path -Parent $PSScriptRoot
if($ExcludeCases.Count -and -not $Baseline){throw 'Case exclusions are allowed only with -Baseline; full acceptance requires every catalog case to PASS'}
$output=[IO.Path]::GetFullPath($FixtureDirectory,$repo)
$package='org.androswitch.conformance'
$catalogPath=Join-Path $output 'catalog.json'
$digest=(Get-FileHash -LiteralPath $catalogPath).Hash.ToLowerInvariant()
$seed=Join-Path $output ('seeds-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force "$seed/$package/files"|Out-Null
if($ResumeEvidenceDirectory){
    $prior=(Get-Item -LiteralPath $ResumeEvidenceDirectory).FullName
    $receiptPath=Join-Path $prior 'verification.json'
    $priorReceipt=Get-Content $receiptPath -Raw|ConvertFrom-Json
    $candidate=if($Nro){$Nro}else{"$repo/local-artifacts/product/androswitch.nro"}
    if($priorReceipt.sd_original_restore -cne 'restored' -or $priorReceipt.inputs.Count -ne 1 -or
       $priorReceipt.inputs[0].sha256 -cne (Get-FileHash "$output/AndroSwitchConformance.apk").Hash.ToLowerInvariant() -or
       $priorReceipt.nro_sha256 -cne (Get-FileHash $candidate).Hash.ToLowerInvariant()){throw 'Resume requires restored SD and exactly the same APK/NRO'}
    $source=Join-Path $priorReceipt.sd_result "userdata/$package"
    $items=@(Get-ChildItem -LiteralPath $source -Recurse -Force)
    if(@($items|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'Linked resume files are not allowed'}
    $files=@($items|Where-Object {-not $_.PSIsContainer})
    if($files.Count -gt 1023 -or ($files|Measure-Object Length -Sum).Sum -gt 15MB){throw 'Resume files exceed journal seed budget'}
    foreach($file in $files){
        $relative=[IO.Path]::GetRelativePath($source,$file.FullName)
        $destination=Join-Path "$seed/$package" $relative
        New-Item -ItemType Directory -Force (Split-Path -Parent $destination)|Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $destination
    }
    @{schema_version=1;receipt=$receiptPath;receipt_sha256=(Get-FileHash $receiptPath).Hash.ToLowerInvariant();nro_sha256=$priorReceipt.nro_sha256;apk_sha256=$priorReceipt.inputs[0].sha256}|ConvertTo-Json|Set-Content "$seed/$package/files/resume-evidence.json" -Encoding utf8NoBOM
}
[IO.File]::WriteAllText("$seed/$package/files/autorun.txt",$(if($RerunCategory){'0'}else{'1'}))
if($FilterGroup -ne 'All'){
    $declaredGroups=@((Get-Content $catalogPath -Raw|ConvertFrom-Json).cases|ForEach-Object group|Sort-Object -Unique)
    if($FilterGroup -cnotin $declaredGroups){throw 'Choose a group declared by the frozen APK catalog'}
    [IO.File]::WriteAllText("$seed/$package/files/filter.txt",$FilterGroup)
}
$known=@((Get-Content $catalogPath -Raw|ConvertFrom-Json).cases|ForEach-Object id)
foreach($caseId in $ExcludeCases){if($caseId -cnotin $known){throw "Unknown excluded case: $caseId"}}
if($ExcludeCases.Count){[IO.File]::WriteAllText("$seed/$package/files/exclude.txt",($ExcludeCases -join "`n"))}

$watch=@("$package/files/conformance/$($digest.Substring(0,12))/pending.txt")
$parameters=@{
    ApkPaths=@((Join-Path $output 'AndroSwitchConformance.apk'));LaunchesPerApk=$Launches
    UserDataSeed=$seed;UserDataWatch=$watch;Headless=$true;Ryujinx=$Ryujinx;RyujinxProfile=$RyujinxProfile
    StartupWaitMilliseconds=$WaitMilliseconds;TimeoutSeconds=$TimeoutSeconds
    PrepareOnly=$PrepareOnly;AllowDesktopEmulator=$AllowDesktopEmulator
}
if($Nro){$parameters.Nro=$Nro}
if($WaitSegments -gt 1 -or $RerunCategory){
    # Keep asynchronous lifecycle/Handler cases in the same ordinary session.
    # Each protocol wait remains bounded; do not interrupt a case just to split
    # a long suite into many short launches. APK and acceptance stay unchanged.
    $events=[Collections.Generic.List[object]]::new()
    if($RerunCategory){
        # Use the unchanged APK's ordinary category/reset/run controls. Never
        # edit case result files or turn an interrupted observation into PASS.
        $activity=Get-Content "$repo/fixtures/conformance/java/org/androswitch/conformance/MainActivity.java" -Raw
        $definition=[regex]::Match($activity,'(?s)GROUPS\s*=\s*\{(.*?)\};')
        $groups=@([regex]::Matches($definition.Groups[1].Value,'"([^"]+)"')|ForEach-Object {$_.Groups[1].Value})
        $index=$groups.IndexOf($RerunCategory)
        if($index -le 0){throw 'Choose a declared non-All category'}
        $oldLauncher=Get-Content (Join-Path $prior 'emulator.json') -Raw|ConvertFrom-Json
        $views=@($oldLauncher.sessions|ForEach-Object snapshots|ForEach-Object views)
        $back=@($views|Where-Object {$_.id -eq 2 -and $_.text -ceq 'Previous category'})|Select-Object -Last 1
        $start=@($views|Where-Object {$_.id -eq 4 -and $_.text -ceq 'Run category'})|Select-Object -Last 1
        if(-not $back -or -not $start){throw 'Verified APK UI control bounds unavailable'}
        foreach($control in @(@($back)*($groups.Count-$index))+@($start)){
            $x=[int]($control.bounds[0]+$control.bounds[2]/2);$y=[int]($control.bounds[1]+$control.bounds[3]/2)
            $events.Add(@{kind='touch';action=0;x=$x;y=$y});$events.Add(@{kind='touch';action=1;x=$x;y=$y})
        }
    }
    for($segment=0;$segment -lt $WaitSegments;$segment++){$events.Add(@{kind='wait';ms=$WaitMilliseconds})}
    $events.Add(@{kind='frame'});$events.Add(@{kind='wait';ms=2000});$events.Add(@{kind='return_to_launcher'})
    $parameters.SessionEvents=@($events.ToArray())
}
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
$acceptanceArgs=if($Baseline){@()}else{@('--require-pass')}
if($FilterGroup -ne 'All'){$acceptanceArgs+=@('--group',$FilterGroup)}
& python -X utf8 "$PSScriptRoot/collect-conformance.py" --receipt $receiptPath --catalog $catalogPath --package $package @acceptanceArgs
if($LASTEXITCODE){throw "Conformance evidence collection failed; $failure"}
Write-Output "Conformance journal and actual session errors: $run/conformance-summary.json"
if($failure){Write-Warning "External renderer/session gate remains: $failure"}
