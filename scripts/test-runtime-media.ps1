# Pure host contracts; this never starts a desktop emulator.
param([string]$CC='C:/w64devkit/bin/gcc.exe')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$compiler=(Get-Command $CC -ErrorAction Stop).Source
$run=Join-Path $projectRoot ('local-artifacts/m9/media-contracts/'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$scenarioNames=@('invalid','create-failure','start-failure','start-close-failure','normal','pause-preserve','pause-recreate','pause-exit','pause-before-create','pause-preserve-exit','pause-toggle-discard','suspend-failure','resume-failure','queue-before-context','initialize-failure','choose-failure','egl-create-failure','attach-failure','callback-failure','queue-failure','poll-failure','close-failure','self-join','wait-failure','thread-close-failure','timeout','concurrent-join')
$cases=@(
    @{id='memory-policy';sources=@('platform/androswitch/tests/memory_policy.c');deps=@('platform/androswitch/memory_policy.h');flags=@();scenarios=@('')},
    @{id='pcm-volume';sources=@('platform/androswitch/tests/pcm_volume.c');deps=@('platform/androswitch/pcm_volume.h');flags=@('-lm');scenarios=@('')},
    @{id='media-format';sources=@('platform/androswitch/tests/media_format.c','platform/androswitch/media_format.c');deps=@('platform/androswitch/media_format.h');flags=@();scenarios=@('')},
    @{id='glsurface-worker';sources=@('platform/androswitch/tests/glsurface_worker.c','platform/androswitch/glsurface.c');deps=@('platform/androswitch/glsurface.h','platform/androswitch/media.h','platform/switch-smoke/native.h','platform/androswitch/tests/fake-switch/switch.h');flags=@('-pthread','-Iplatform/androswitch/tests/fake-switch','-Iplatform/switch-smoke');scenarios=$scenarioNames}
)
$paths=@($cases|ForEach-Object {$_.sources;$_.deps}|Sort-Object -Unique)
$sources=@($paths|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath (Join-Path $projectRoot $_)).Hash.ToLowerInvariant()}})
$receipt=@{schema_version=1;status='running';scope='Production portable C helpers and real host threads with fake EGL/libnx/VM boundaries; not Horizon rendering or application execution';desktop_emulator='not_started';physical_device='not_run';compiler=$compiler;source_files=$sources;checks=@()}
function Save-Receipt {$receipt|ConvertTo-Json -Depth 16|Set-Content (Join-Path $run 'verification.json') -Encoding utf8NoBOM}
function Invoke-Check([string]$Id,[string]$Executable,[string[]]$Arguments){
    $log=Join-Path $run ($Id+'.log')
    & $Executable @Arguments *> $log
    $code=$LASTEXITCODE
    $receipt.checks+=@{id=$Id;command=@($Executable)+$Arguments;exit_code=$code;log=$log;log_sha256=(Get-FileHash -LiteralPath $log).Hash.ToLowerInvariant()}
    Save-Receipt
    if($code){$receipt.status='failed';Save-Receipt;Get-Content -LiteralPath $log;throw "Contract failed: $Id"}
}
Push-Location $projectRoot
try{
    foreach($case in $cases){
        $exe=Join-Path $run ($case.id+'.exe')
        $arguments=@('-std=c11','-O2','-Wall','-Wextra','-Werror')+$case.sources+$case.flags+@('-o',$exe)
        Invoke-Check ($case.id+'-compile') $compiler $arguments
        foreach($scenario in $case.scenarios){
            $name=$case.id
            $arguments=@()
            if($scenario){$name+='-'+$scenario;$arguments=@($scenario)}
            Invoke-Check $name $exe $arguments
        }
    }
    foreach($source in $sources){
        if((Get-FileHash -LiteralPath $source.path).Hash.ToLowerInvariant() -cne $source.sha256){throw "Source changed during tests: $($source.path)"}
    }
    $receipt.status='passed_host_contracts';Save-Receipt
    Write-Output "Host media/memory contracts passed: $run/verification.json"
}catch{
    $receipt.status='failed';$receipt.error=$_.Exception.Message;Save-Receipt;throw
}finally{Pop-Location}
