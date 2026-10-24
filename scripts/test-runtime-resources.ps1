param(
    [string]$Compiler = 'C:/w64devkit/bin/gcc.exe',
    [string]$TargetCompiler = 'C:/devkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe',
    [string]$LibnxInclude = 'C:/devkitPro/libnx/include',
    [string]$OutputDirectory = 'local-artifacts/tests/runtime-resources'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
foreach ($program in @($Compiler,$TargetCompiler)) {
    if (!(Test-Path -LiteralPath $program)) { throw "Required compiler missing: $program" }
}
if (!(Test-Path -LiteralPath "$LibnxInclude/switch.h")) { throw "libnx headers missing: $LibnxInclude" }
if (![IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $projectRoot $OutputDirectory }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$checks = [Collections.Generic.List[object]]::new()
$receiptPath = Join-Path $OutputDirectory 'result.json'
$receipt = [ordered]@{
    schema_version=1; run_id=[guid]::NewGuid().ToString('N'); status='running'
    scope='Production scalar resource wrapper with fake kernel query under host C; strict real libnx ARM64 compile. No real Horizon syscall or hardware execution.'
    checks=$checks
}
function Save-Result {
    $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
}
function Invoke-Check([string]$Id,[string]$Program,[string[]]$ArgumentList) {
    $output = & $Program @ArgumentList 2>&1
    $code = $LASTEXITCODE
    $output | ForEach-Object { Write-Output $_ }
    $log = Join-Path $OutputDirectory "$Id.log"
    [IO.File]::WriteAllText($log,($output | Out-String),[Text.UTF8Encoding]::new($false))
    $checks.Add(@{id=$Id;program=$Program;arguments=$ArgumentList;exit_code=$code;log=$log;log_sha256=(Get-FileHash -LiteralPath $log).Hash.ToLowerInvariant()})
    Save-Result
    if ($code) { throw "Resource contract failed: $Id (exit $code)" }
}
$previousPath = $env:PATH
Push-Location $projectRoot
try {
    $env:PATH = (Split-Path -Parent $Compiler) + [IO.Path]::PathSeparator + (Split-Path -Parent $TargetCompiler) + [IO.Path]::PathSeparator + $previousPath
    Save-Result
    Invoke-Check 'host-version' $Compiler @('--version')
    Invoke-Check 'host-machine' $Compiler @('-dumpmachine')
    Invoke-Check 'target-version' $TargetCompiler @('--version')
    Invoke-Check 'target-machine' $TargetCompiler @('-dumpmachine')
    $executable = Join-Path $OutputDirectory 'resource-info.exe'
    Invoke-Check 'host-build' $Compiler @('-std=c11','-O2','-Wall','-Wextra','-Werror','-Iplatform/switch-smoke/tests/fake-resources','-Iplatform/switch-smoke','platform/switch-smoke/resource_info.c','platform/switch-smoke/tests/resource_info.c','-o',$executable)
    Invoke-Check 'host-contract' $executable @()
    $flags = @('-march=armv8-a+crc+crypto','-mtune=cortex-a57','-mtp=soft','-fPIE','-O2','-Wall','-Wextra','-Werror',"-I$LibnxInclude",'-Iplatform/switch-smoke','-c','platform/switch-smoke/resource_info.c')
    Invoke-Check 'target-smoke' $TargetCompiler ($flags + @('-o',(Join-Path $OutputDirectory 'resource-info-smoke.o')))
    Invoke-Check 'target-product' $TargetCompiler ($flags + @('-D__SWITCH__','-o',(Join-Path $OutputDirectory 'resource-info-product.o')))
    $receipt.sources = @(@('platform/switch-smoke/resource_info.c','platform/switch-smoke/resource_info.h','platform/switch-smoke/tests/resource_info.c','platform/switch-smoke/tests/fake-resources/switch.h','scripts/test-runtime-resources.ps1',"$LibnxInclude/switch/kernel/svc.h") | ForEach-Object {
        @{path=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}
    })
    $receipt.artifacts = @(Get-Item $executable,(Join-Path $OutputDirectory 'resource-info-smoke.o'),(Join-Path $OutputDirectory 'resource-info-product.o') | ForEach-Object {
        @{path=$_.FullName;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}
    })
    $receipt.status='passed'
    Save-Result
    Write-Output "Resource C contracts and both ARM64 objects passed. Receipt: $receiptPath"
} catch {
    $receipt.status='failed';$receipt.error=$_.Exception.Message;Save-Result;throw
} finally {
    $env:PATH=$previousPath
    Pop-Location
}
