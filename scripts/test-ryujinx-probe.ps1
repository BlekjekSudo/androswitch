param(
    [Parameter(Mandatory=$true)][string]$Apk,
    [Parameter(Mandatory=$true)][string]$Request,
    [Parameter(Mandatory=$true)][string]$Expected,
    [string]$Milestone='M3',
    [string]$HostFeatures='host-native',
    [string]$Ryujinx='D:/Dev/Switch/RyujinX/Ryujinx.exe',
    [string]$RyujinxProfile="$env:APPDATA/Ryujinx",
    [int]$TimeoutSeconds=180,
    [switch]$SkipBuild,
    [switch]$SkipHost
)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$runId=[guid]::NewGuid().ToString('N')
$runRoot=Join-Path $projectRoot "local-artifacts/$($Milestone.ToLowerInvariant())/runs/$runId"
$sdRoot=Join-Path $RyujinxProfile 'sdcard/switch/androswitch-smoke'
New-Item -ItemType Directory -Force -Path $runRoot,$sdRoot | Out-Null
$lockPath=Join-Path $sdRoot 'e2e.lock'
$lock=[IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process=$null
$result=[ordered]@{schema_version=1;session_id=$runId;milestone=$Milestone;status='running';physical_device='not_run';started_at=[DateTime]::UtcNow.ToString('o')}
try {
    if(-not $SkipBuild){& "$PSScriptRoot/build-switch-smoke.ps1" | Out-Host}
    $nro=Join-Path $sdRoot 'androswitch-smoke.nro'
    Copy-Item -LiteralPath "$projectRoot/local-artifacts/m1/androswitch-smoke.nro" -Destination $nro
    Copy-Item -LiteralPath $Apk -Destination "$sdRoot/app.apk"
    $requestObject=Get-Content -LiteralPath $Request -Raw | ConvertFrom-Json
    $requestObject.session_id=$runId
    $requestPath=Join-Path $runRoot 'request.json'
    $requestObject | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $requestPath -Encoding utf8NoBOM
    Copy-Item -LiteralPath $requestPath -Destination "$sdRoot/request.json"
    if(-not $SkipHost){
        Push-Location -LiteralPath $projectRoot
        try {$hostJson=& cargo run --quiet -p as-switch-smoke --features $HostFeatures --bin as-probe -- $Apk $requestPath "$runRoot/host-storage";if($LASTEXITCODE){throw 'Host oracle failed'}}finally{Pop-Location}
        $hostJson | Set-Content -LiteralPath "$runRoot/host.json" -Encoding utf8NoBOM
        $hostReport=($hostJson -join "`n") | ConvertFrom-Json
    }
    $reportPath=Join-Path $sdRoot 'probe-result.json'
    if(Test-Path -LiteralPath $reportPath){Move-Item -LiteralPath $reportPath -Destination "$runRoot/previous-probe.json"}
    $process=Start-Process -FilePath $Ryujinx -ArgumentList @($nro) -WorkingDirectory (Split-Path -Parent $Ryujinx) -WindowStyle Hidden -RedirectStandardOutput "$runRoot/ryujinx.stdout.log" -RedirectStandardError "$runRoot/ryujinx.stderr.log" -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $observed=$null
    do{
        if($process.HasExited){throw "Ryujinx exited: $($process.ExitCode)"}
        if(Test-Path -LiteralPath $reportPath){try{$observed=Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json;if($observed.session_id -eq $runId -or $observed.code){break}}catch{}}
        Start-Sleep -Milliseconds 500
    }while([DateTime]::UtcNow -lt $deadline)
    if($null -eq $observed -or $observed.session_id -ne $runId){throw "No fresh report: $($observed | ConvertTo-Json -Compress)"}
    Copy-Item -LiteralPath $reportPath -Destination "$runRoot/emulator.json"
    $expectedObject=Get-Content -LiteralPath $Expected -Raw | ConvertFrom-Json
    foreach($field in @('calls','native')){
        if(@($observed.$field).Count -ne @($expectedObject.$field).Count){throw "Wrong result count: $field"}
        for($i=0;$i -lt @($expectedObject.$field).Count;$i++){
            $call=$observed.$field[$i];$wanted=$expectedObject.$field[$i]
            if($wanted -is [string]){if($null -ne $call.error -or $call.value -cne $wanted){throw "Mismatch $field[$i]: $($call | ConvertTo-Json -Compress -Depth 5)"}}
            elseif($wanted.error){if($call.error.code -cne $wanted.error -or $null -ne $call.value){throw "Missing error $field[$i]"}}
            if(-not $SkipHost -and $wanted -is [string]){if($hostReport.$field[$i].value -cne $call.value -or $hostReport.$field[$i].error){throw "Host oracle mismatch $field[$i]"}}
        }
    }
    $result.status='passed';$result.native_calls=@($observed.native).Count;$result.dex_calls=@($observed.calls).Count
    $result.inputs=@($Apk,$Request,$Expected,$nro | ForEach-Object {@{file=[IO.Path]::GetFullPath($_);sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}})
    $result.emulator=@{version=(Get-Item -LiteralPath $Ryujinx).VersionInfo.ProductVersion;sha256=(Get-FileHash -LiteralPath $Ryujinx).Hash.ToLowerInvariant()}
}catch{$result.status='failed';$result.error=$_.Exception.Message;throw}finally{
    if($process -and -not $process.HasExited){$owned=Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)";if($owned.CommandLine -like '*androswitch-smoke.nro*' -and $owned.ExecutablePath -eq [IO.Path]::GetFullPath($Ryujinx)){Stop-Process -Id $process.Id -ErrorAction SilentlyContinue}}
    $result.finished_at=[DateTime]::UtcNow.ToString('o')
    $result | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath "$runRoot/verification.json" -Encoding utf8NoBOM
    $lock.Dispose();Remove-Item -LiteralPath $lockPath
    Write-Output "$Milestone E2E $($result.status): $runRoot/verification.json"
}
