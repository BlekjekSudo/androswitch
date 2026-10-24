param(
  [string]$Compiler = 'C:/w64devkit/bin/gcc.exe',
  [string]$OutputDirectory = 'local-artifacts/tests/background-workers'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (!(Test-Path -LiteralPath $Compiler)) {
  $command = Get-Command $Compiler -CommandType Application -ErrorAction SilentlyContinue
  if (!$command) { throw 'Host GCC with pthread support is required; pass -Compiler with its path.' }
  $Compiler = $command.Source
}
$Compiler = (Resolve-Path -LiteralPath $Compiler).Path
if (![IO.Path]::IsPathRooted($OutputDirectory)) {
  $OutputDirectory = Join-Path $projectRoot $OutputDirectory
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$checks = [Collections.Generic.List[object]]::new()
$receipt = [ordered]@{
  schema_version = 1
  run_id = [guid]::NewGuid().ToString('N')
  scope = 'Production C worker transport with real host pthreads, clocks and events; no Java scheduling, emulator or hardware proof.'
  status = 'running'
  compiler = $Compiler
  checks = $checks
}
$receiptPath = Join-Path $OutputDirectory 'result.json'
function Save-Result {
  $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $receiptPath -Encoding utf8
}
function Invoke-Check([string]$Id, [string]$Program, [string[]]$ArgumentList) {
  $output = & $Program @ArgumentList 2>&1
  $code = $LASTEXITCODE
  $output | ForEach-Object { Write-Output $_ }
  $log = Join-Path $OutputDirectory "$Id.log"
  [IO.File]::WriteAllText($log, ($output | Out-String), [Text.UTF8Encoding]::new($false))
  $checks.Add(@{
    id = $Id; program = $Program; arguments = $ArgumentList; exit_code = $code
    log = $log; log_sha256 = (Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()
  })
  Save-Result
  if ($code -ne 0) { throw "Worker contract failed: $Id (exit $code)" }
}
$previousPath = $env:PATH
Push-Location $projectRoot
try {
  $env:PATH = (Split-Path -Parent $Compiler) + [IO.Path]::PathSeparator + $previousPath
  Save-Result
  $executable = Join-Path $OutputDirectory 'background-workers.exe'
  Invoke-Check 'build' $Compiler @(
    '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-pthread',
    '-Iplatform/androswitch/tests/fake-workers', '-Iplatform/switch-smoke',
    'platform/androswitch/tests/background_workers.c', 'platform/androswitch/workers.c',
    'platform/switch-smoke/worker_admission.c', '-o', $executable
  )
  $scenarios = @(
    'normal-delay', 'serial', 'wake', 'wake-during-callback', 'cancel-idle', 'cancel-active', 'callback-failure',
    'wait-error', 'self-quiesce', 'self-cancel', 'concurrent-quiesce', 'create-failure',
    'start-failure', 'start-close-failure', 'join-failure', 'close-failure', 'timeout',
    'total-deadline', 'capacity', 'reap', 'invalid', 'wall-clock', 'admission-closed', 'admission-race'
  )
  foreach ($scenario in $scenarios) {
    Invoke-Check $scenario $executable @($scenario)
  }
  $receipt.status = 'passed'
  $receipt.worker_transport_cases = $scenarios.Count
  $receipt.source_snapshots = @(@(
    'platform/androswitch/workers.c', 'platform/androswitch/workers.h',
    'platform/androswitch/tests/background_workers.c',
    'platform/androswitch/tests/fake-workers/switch.h', 'scripts/test-background-workers.ps1'
    'platform/switch-smoke/worker_admission.c', 'platform/switch-smoke/worker_admission.h'
  ) | ForEach-Object {
    @{ path = $_; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() }
  })
  Save-Result
  Write-Output "Worker transport checks passed: $($scenarios.Count) host cases."
  Write-Output "Receipt: $receiptPath"
} catch {
  $receipt.status = 'failed'
  $receipt.error = $_.Exception.Message
  Save-Result
  throw
} finally {
  $env:PATH = $previousPath
  Pop-Location
}
