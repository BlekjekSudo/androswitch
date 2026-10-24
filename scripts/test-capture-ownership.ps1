# Exercise the real recorder with local files only; never starts an emulator.
param([string]$OutputDirectory='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $OutputDirectory){$OutputDirectory=Join-Path $projectRoot ('local-artifacts/m9/capture-ownership/'+[guid]::NewGuid().ToString('N'))}
$testRoot=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path -LiteralPath $testRoot){throw 'Choose a fresh test directory.'}
New-Item -ItemType Directory -Path $testRoot|Out-Null
$harness=Join-Path $PSScriptRoot 'test-ryujinx-external.ps1'
$parseErrors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($harness,[ref]$null,[ref]$parseErrors)
if($parseErrors.Count){throw 'Capture harness has parse errors.'}
# Import just the production function; the CLI entry point must not execute.
$definitions=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Capture-Session'},$true))
if($definitions.Count -ne 1){throw 'Expected one production capture function.'}
. ([scriptblock]::Create($definitions[0].Extent.Text))
$results=[Collections.Generic.List[object]]::new()
function Write-TestJson([string]$Name,[object]$Value){
    $Value|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $sdRoot "logs/$Name") -Encoding utf8NoBOM
}
function New-Case([string]$Name){
    $script:sdRoot=Join-Path $testRoot "$Name/sd"
    $script:run=Join-Path $testRoot "$Name/run"
    New-Item -ItemType Directory -Path (Join-Path $sdRoot 'logs'),$run|Out-Null
    $script:session=[pscustomobject]@{schema_version=1;session_id="case-$Name";package='org.fixture.capture';revision=1;status='running'}
    $script:owner=[pscustomobject]@{schema_version=1;capture_protocol=1;session_id=$session.session_id;package=$session.package;revision=1}
    Write-TestJson 'session-report.json' $session
    Write-TestJson 'capture-owner.json' $owner
    Write-TestJson 'pending-session.json' $session
    $script:completeCapturePaths=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $script:captureMetadata=@{}
    $script:raceChange=''
}
function Write-Ppm([string]$Name,[switch]$Partial){
    $pixels=if($Partial){[byte[]]@(0,0,0)}else{[byte[]]@(0,0,0,255,0,0)}
    $path=Join-Path $sdRoot "logs/$Name"
    [IO.File]::WriteAllBytes($path,[byte[]]([Text.Encoding]::ASCII.GetBytes("P6`n2 1`n255`n")+$pixels))
    [IO.File]::SetLastWriteTimeUtc($path,[DateTime]::SpecifyKind([DateTime]'1970-01-02',[DateTimeKind]::Utc))
}
# Deterministically change ownership during an actual file copy. All other
# copies still use the normal filesystem command with its original arguments.
function Copy-Item([string]$LiteralPath,[string]$Destination){
    Microsoft.PowerShell.Management\Copy-Item -LiteralPath $LiteralPath -Destination $Destination
    if($script:raceChange -and $LiteralPath.EndsWith('.ppm',[StringComparison]::Ordinal)){
        $change=$script:raceChange;$script:raceChange=''
        if($change -eq 'owner'){$owner.session_id='next-generation';Write-TestJson 'capture-owner.json' $owner}
        if($change -eq 'pending'){[IO.File]::Delete((Join-Path $sdRoot 'logs/pending-session.json'))}
    }
}
function Assert-Case([string]$Name,[bool]$Condition){
    $results.Add(@{id=$Name;passed=$Condition})
    if(-not $Condition){throw "Capture ownership contract failed: $Name"}
}
try{
    New-Case 'old-clock';Write-Ppm 'gles-frame-2.ppm'
    $images=@(Capture-Session $session)
    Assert-Case 'matching generation accepts old-clock GPU image' ($images.Count -eq 1 -and $images[0].valid_ppm -and $images[0].sampled_nonuniform)

    New-Case 'wrong-owner';Write-Ppm 'gles-frame-2.ppm';$owner.session_id='previous';Write-TestJson 'capture-owner.json' $owner
    Assert-Case 'wrong generation rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'missing-owner';Write-Ppm 'gles-frame-2.ppm';[IO.File]::Delete((Join-Path $sdRoot 'logs/capture-owner.json'))
    Assert-Case 'missing protocol rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'unknown-protocol';Write-Ppm 'gles-frame-2.ppm';$owner.capture_protocol=2;Write-TestJson 'capture-owner.json' $owner
    Assert-Case 'unknown protocol rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'cpu-active';Write-Ppm 'last-frame.ppm'
    Assert-Case 'matching active CPU request accepted' (@(Capture-Session $session).Count -eq 1)

    New-Case 'after-return';Write-Ppm 'last-frame.ppm';Write-Ppm 'gles-frame-2.ppm';[IO.File]::Delete((Join-Path $sdRoot 'logs/pending-session.json'))
    $images=@(Capture-Session $session)
    Assert-Case 'after return accepts GPU and excludes launcher CPU image' ($images.Count -eq 1 -and [IO.Path]::GetFileName($images[0].file) -ceq 'gles-frame-2.ppm')

    New-Case 'wrong-pending';Write-Ppm 'last-frame.ppm';Write-TestJson 'pending-session.json' @{schema_version=1;session_id=$session.session_id;package='org.fixture.other';revision=1}
    Assert-Case 'different active package rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'wrong-report';Write-Ppm 'gles-frame-2.ppm';$sessionCopy=$session.PSObject.Copy();$sessionCopy.revision=2;Write-TestJson 'session-report.json' $sessionCopy
    Assert-Case 'changed current revision rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'owner-race';Write-Ppm 'gles-frame-2.ppm';$script:raceChange='owner'
    Assert-Case 'generation changed during copy rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'return-race';Write-Ppm 'last-frame.ppm';$script:raceChange='pending'
    Assert-Case 'CPU return during copy rejected' (@(Capture-Session $session).Count -eq 0)

    New-Case 'retry';Write-Ppm 'gles-frame-2.ppm' -Partial
    $images=@(Capture-Session $session)
    Assert-Case 'partial PPM remains unverified' ($images.Count -eq 1 -and -not $images[0].valid_ppm)
    Write-Ppm 'gles-frame-2.ppm';$images=@(Capture-Session $session)
    Assert-Case 'complete retry accepted' ($images.Count -eq 1 -and $images[0].valid_ppm)
    $savedHash=$images[0].sha256;Write-Ppm 'gles-frame-2.ppm' -Partial;$images=@(Capture-Session $session)
    Assert-Case 'validated session copy stays immutable' ($images.Count -eq 1 -and $images[0].sha256 -ceq $savedHash -and $images[0].valid_ppm)

    New-Case 'invalid-id';$session.session_id='case-invalid/../../escape'
    Assert-Case 'invalid session path rejected' (@(Capture-Session $session).Count -eq 0)
    $status='passed'
}catch{$status='failed';throw}finally{
    @{schema_version=1;status=$status;scope='Production recorder, real local files, guest-clock skew and deterministic copy races; no emulator or device execution.';cases=@($results.ToArray());harness_sha256=(Get-FileHash -LiteralPath $harness).Hash.ToLowerInvariant();test_sha256=(Get-FileHash -LiteralPath $PSCommandPath).Hash.ToLowerInvariant()}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $testRoot 'result.json') -Encoding utf8NoBOM
}
Write-Output "Capture ownership checks passed: $($results.Count); receipt: $testRoot/result.json"
