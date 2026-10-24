param(
  [string]$FromCheck='',
  [switch]$ContinueAfterFailure,
  [ValidateSet('Wsl','Msys')][string]$NativeHostBackend='Wsl'
)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try{
  $env:CARGO_BUILD_JOBS='1';$env:CARGO_PROFILE_TEST_DEBUG='0';$env:CARGO_INCREMENTAL='0'
  New-Item -ItemType Directory -Force target/evidence|Out-Null
  $checks=@(
    @{id='format';args=@('fmt','--all','--','--check')},
    @{id='parser';args=@('test','-p','rd-apk','--lib','--no-default-features','--locked')},
    @{id='resource-name-parser';args=@('test','-p','rd-apk','--test','arsc_resource_names','--locked')},
    @{id='portable-execution-fixture';script='crates/rd-vm/tests/fixtures/portable-execution/build.ps1'},
    @{id='portable-execution-owners';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_execution','--locked')},
    @{id='dex-memory';args=@('test','-p','rd-dex','--test','memory_budget','--no-default-features','--locked')},
    @{id='enum-fixture';script='crates/rd-vm/tests/fixtures/portable-enum/build.ps1'},
    @{id='portable-enum';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_enum','--locked','--','--include-ignored')},
    @{id='format-fixture';script='crates/rd-vm/tests/fixtures/portable-format/build.ps1'},
    @{id='portable-format';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_format','--locked','--','--include-ignored')},
    @{id='encoding-fixture';script='crates/rd-vm/tests/fixtures/portable-encoding/build.ps1'},
    @{id='portable-encoding';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_encoding','--locked')},
    @{id='class-names-fixture';script='crates/rd-vm/tests/fixtures/portable-class-names/build.ps1'},
    @{id='portable-class-names';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_class_names','--locked','--','--include-ignored')},
    @{id='atomic-fixture';script='crates/rd-vm/tests/fixtures/portable-atomic/build.ps1'},
    @{id='portable-atomic';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_atomic','--locked','--','--include-ignored')},
    @{id='monitor-fixture';script='crates/rd-vm/tests/fixtures/portable-monitor/build.ps1'},
    @{id='portable-monitor';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_monitor','--locked','--','--include-ignored')},
    @{id='throwable-fixture';script='crates/rd-vm/tests/fixtures/portable-throwable/build.ps1'},
    @{id='portable-throwable';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_throwable','--locked','--','--include-ignored')},
    @{id='legacy-enum';args=@('test','-p','rd-vm','--test','vm_exec','--locked','enum_','--','--include-ignored')},
    @{id='platform';args=@('test','-p','as-platform','--features','std','--locked')},
    @{id='runtime-resources-c';script='scripts/test-runtime-resources.ps1'},
    @{id='packages';args=@('test','-p','as-package','--locked')},
    @{id='persistent';args=@('test','-p','as-storage','-p','as-sqlite','--locked')},
    @{id='native-abi';script='scripts/test-native-abi.ps1';script_args=@{HostBackend=$NativeHostBackend}},
    @{id='native-time';script='scripts/test-native-time.ps1';script_args=@{HostBackend=$NativeHostBackend}},
    @{id='launcher';args=@('test','-p','as-launcher','--features','host','--locked','--','--include-ignored')},
    @{id='communication-fixture';script='scripts/build-communication-fixture.ps1'},
    @{id='bundle-fixture';script='fixtures/m9/bundle/build.ps1'},
    @{id='timer-fixture';script='fixtures/m9/timer/build.ps1'},
    @{id='inherited-dispatch-fixture';script='fixtures/m9/inherited-dispatch/build.ps1'},
    @{id='executor-fixture';script='fixtures/m9/executors/build.ps1'},
    @{id='bitmap-fixture';script='fixtures/m9/bitmap/build.ps1'},
    @{id='resource-names-fixture';script='fixtures/m9/resource-names/build.ps1'},
    @{id='class-forname-fixture';script='fixtures/m9/class-forname/build.ps1'},
    @{id='text-utils-fixture';script='fixtures/m9/text-utils/build.ps1'},
    @{id='gc-owners-fixture';script='fixtures/m9/gc-owners/build.ps1'},
    @{id='jni-constructor-fixture';script='fixtures/m9/jni-construct/build.ps1'},
    @{id='jni-constructor-abi';script='scripts/test-jni-constructor-abi.ps1'},
    @{id='jni-objects-fixture';script='fixtures/m9/jni-objects/build.ps1'},
    @{id='jni-objects';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','jni_objects','--locked','--','--test-threads=1')},
    @{id='jni-constructor';args=@('test','-p','as-switch-smoke','--features','portable-tests','--test','jni_constructor','--locked','--','--test-threads=1')},
    @{id='jni-method-metadata';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','jni_method_metadata','--locked','--','--test-threads=1')},
    @{id='glsurface-fixture';script='scripts/build-glsurface-fixture.ps1'},
    @{id='application-lifecycle-fixture';script='fixtures/m9/application-lifecycle/build.ps1'},
    @{id='uncaught-fixture';script='fixtures/m9/uncaught/build.ps1'},
    @{id='thread-local-fixture';script='fixtures/m9/thread-local/build.ps1'},
    @{id='package-info-fixture';script='fixtures/m9/package-info/build.ps1'},
    @{id='copy-on-write-fixture';script='fixtures/m9/copy-on-write/build.ps1'},
    @{id='reflection-fields-fixture';script='fixtures/m9/reflection-fields/build.ps1'},
    @{id='time-unit-fixture';script='fixtures/m9/time-unit/build.ps1'},
    @{id='runtime-fixture';script='fixtures/m9/runtime/build.ps1'},
    @{id='framework';args=@('test','-p','as-framework','--features','host','--lib','--tests','--locked','--','--include-ignored','--test-threads=1')},
    @{id='m1-fixture';script='scripts/build-m1-fixture.ps1'},
    @{id='portable-core';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--test','portable_core','--locked','--','--include-ignored')},
    @{id='session-reports';args=@('test','-p','as-switch-smoke','--features','host-cli,portable-tests','--lib','session_report::tests','--locked')},
    @{id='capture-ownership';script='scripts/test-capture-ownership.ps1'},
    @{id='horizon-display';script='scripts/test-horizon-display.ps1'},
    @{id='background-workers';script='scripts/test-background-workers.ps1'},
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
      if($prior.id -cne $ids[$results.Count]){throw 'Check sequence changed; rerun checks from the beginning'}
      if($prior.exit_code -ne 0 -or (Get-FileHash $prior.log).Hash.ToLowerInvariant() -cne $prior.log_sha256){throw 'Cannot resume changed/failing prior checks'}
      $results+=$prior
    }
    if($results.Count -ne $offset){throw 'Missing prior checks'}
    $checks=@($checks|Select-Object -Skip $offset)
  }
  $expectedCount=$results.Count+$checks.Count
  foreach($check in $checks){
    $log="target/evidence/m9-$($check.id).log";$arguments=$check.args;$timer=[Diagnostics.Stopwatch]::StartNew()
    try{
      if($check.script){
        $scriptArgs=if($check.script_args){$check.script_args}else{@{}}
        $command='./'+$check.script+(@($scriptArgs.GetEnumerator()|Sort-Object Key|ForEach-Object {' -'+$_.Key+' '+$_.Value}) -join '')
        & (Join-Path $projectRoot $check.script) @scriptArgs *> $log
      }else{
        $command='cargo '+($arguments -join ' ')
        & cargo @arguments *> $log
      }
      $code=$LASTEXITCODE
    }catch{
      $_ | Out-String | Add-Content -LiteralPath $log -Encoding utf8
      $code=1
    }
    $timer.Stop()
    $results+=@{id=$check.id;command=$command;exit_code=$code;seconds=$timer.Elapsed.TotalSeconds;log=$log;log_sha256=(Get-FileHash $log).Hash.ToLowerInvariant()}
    @{schema_version=1;completed=($results.Count -eq $expectedCount);checks=$results;desktop_emulator='not_started'}|ConvertTo-Json -Depth 7|Set-Content target/evidence/m9-checks.json -Encoding utf8NoBOM
    Write-Output "$($check.id): exit $code"
    if($code){Get-Content $log -Tail 45;if(-not $ContinueAfterFailure){throw "Required check failed: $($check.id)"}}
  }
  if(@($results|Where-Object exit_code -ne 0).Count){throw 'Required checks failed; see target/evidence/m9-checks.json. Continuing other checks never changes a failure to a pass.'}
}finally{Pop-Location}
