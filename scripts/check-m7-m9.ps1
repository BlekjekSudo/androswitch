param([string]$FromCheck='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try{
  $env:CARGO_BUILD_JOBS='1';$env:CARGO_PROFILE_TEST_DEBUG='0'
  New-Item -ItemType Directory -Force target/evidence|Out-Null
  $checks=@(
    @{id='format';args=@('fmt','--all','--','--check')},
    @{id='parser';args=@('test','-p','rd-apk','--lib','--no-default-features','--locked')},
    @{id='platform';args=@('test','-p','as-platform','--features','std','--locked')},
    @{id='packages';args=@('test','-p','as-package','--locked')},
    @{id='persistent';args=@('test','-p','as-storage','-p','as-sqlite','--locked')},
    @{id='native-loader';args=@('test','-p','as-native','--locked','--','--include-ignored')},
    @{id='launcher';args=@('test','-p','as-launcher','--features','host','--locked','--','--include-ignored')},
    @{id='framework';args=@('test','-p','as-framework','--features','host','--tests','--locked','--','--include-ignored')},
    @{id='portable-core';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_core','--locked','--','--include-ignored')},
    @{id='framework-clippy';args=@('clippy','-p','as-framework','--features','host','--all-targets','--locked','--','-D','warnings')},
    @{id='package-clippy';args=@('clippy','-p','as-launcher','-p','as-package','--features','as-launcher/host','--all-targets','--locked','--','-D','warnings')},
    @{id='storage-clippy';args=@('clippy','-p','as-storage','-p','as-sqlite','--all-targets','--locked','--','-D','warnings')},
    @{id='device-bridge-clippy';args=@('clippy','-p','as-switch-smoke','--features','host-cli,portable-tests','--all-targets','--locked','--','-D','warnings')}
  )
  $results=@()
  if($FromCheck){
    $ids=@($checks|ForEach-Object id);$offset=$ids.IndexOf($FromCheck)
    if($offset -lt 0){throw 'Unknown resume check'}
    $previous=Get-Content target/evidence/m9-checks.json -Raw|ConvertFrom-Json
    foreach($prior in @($previous.checks|Select-Object -First $offset)){
      if($prior.exit_code -ne 0 -or (Get-FileHash $prior.log).Hash.ToLowerInvariant() -cne $prior.log_sha256){throw 'Cannot resume changed/failing prior checks'}
      $results+=$prior
    }
    if($results.Count -ne $offset){throw 'Missing prior checks'}
    $checks=@($checks|Select-Object -Skip $offset)
  }
  $expectedCount=$results.Count+$checks.Count
  foreach($check in $checks){
    $log="target/evidence/m9-$($check.id).log";$arguments=$check.args;$timer=[Diagnostics.Stopwatch]::StartNew()
    & cargo @arguments *> $log
    $code=$LASTEXITCODE;$timer.Stop()
    $results+=@{id=$check.id;command='cargo '+($arguments -join ' ');exit_code=$code;seconds=$timer.Elapsed.TotalSeconds;log=$log;log_sha256=(Get-FileHash $log).Hash.ToLowerInvariant()}
    @{schema_version=1;completed=($results.Count -eq $expectedCount);checks=$results;desktop_emulator='not_started'}|ConvertTo-Json -Depth 7|Set-Content target/evidence/m9-checks.json -Encoding utf8NoBOM
    Write-Output "$($check.id): exit $code"
    if($code){Get-Content $log -Tail 45;throw "Required check failed: $($check.id)"}
  }
}finally{Pop-Location}
