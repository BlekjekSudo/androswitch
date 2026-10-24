param(
  [string]$Compiler = '',
  [string]$OutputDirectory = 'local-artifacts/tests/horizon-display'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (!$Compiler) {
  if (Test-Path -LiteralPath 'C:/w64devkit/bin/gcc.exe') {
    $Compiler = 'C:/w64devkit/bin/gcc.exe'
  } else {
    $candidate = Get-Command gcc -CommandType Application -ErrorAction SilentlyContinue
    if (!$candidate) { throw 'Host GCC with pthread support is required; pass -Compiler with its path.' }
    $Compiler = $candidate.Source
  }
}
if (!(Test-Path -LiteralPath $Compiler)) {
  $candidate = Get-Command $Compiler -CommandType Application -ErrorAction SilentlyContinue
  if (!$candidate) { throw "Host compiler not found: $Compiler" }
  $Compiler = $candidate.Source
}
$Compiler = (Resolve-Path -LiteralPath $Compiler).Path
if (![IO.Path]::IsPathRooted($OutputDirectory)) {
  $OutputDirectory = Join-Path $projectRoot $OutputDirectory
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$results = [Collections.Generic.List[object]]::new()
$receiptPath = Join-Path $OutputDirectory 'result.json'
$receipt = [ordered]@{
  schema_version = 1
  run_id = [guid]::NewGuid().ToString('N')
  status = 'running'
  scope = 'Host C geometry, ownership and worker contracts; no emulator, GPU or hardware execution.'
  compiler = $Compiler
  checks = $results
}
function Save-Result {
  $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $receiptPath -Encoding utf8
}
function Invoke-NativeCheck([string]$Id, [string]$Program, [string[]]$ArgumentList) {
  $log = Join-Path $OutputDirectory "$Id.log"
  $output = & $Program @ArgumentList 2>&1
  $code = $LASTEXITCODE
  $output | ForEach-Object { Write-Output $_ }
  # A successful compiler can emit no output; still create its evidence log.
  [IO.File]::WriteAllText($log, ($output | Out-String), [Text.UTF8Encoding]::new($false))
  $results.Add(@{
    id = $Id; program = $Program; arguments = $ArgumentList; exit_code = $code
    log = $log; log_sha256 = (Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()
  })
  Save-Result
  if ($code -ne 0) { throw "Host display check failed: $Id (exit $code)" }
}
$previousPath = $env:PATH
Push-Location $projectRoot
try {
  # Only this process needs the compiler's host runtime DLL directory.
  $env:PATH = (Split-Path -Parent $Compiler) + [IO.Path]::PathSeparator + $previousPath
  Save-Result
  $flags = @('-std=c11', '-O2', '-Wall', '-Wextra', '-Werror')
  $rotation = Join-Path $OutputDirectory 'display-rotation.exe'
  Invoke-NativeCheck 'rotation-build' $Compiler ($flags + @(
    '-Iplatform/androswitch/tests/fake-display',
    'platform/androswitch/tests/display_rotation.c', 'platform/androswitch/display.c', '-o', $rotation
  ))
  Invoke-NativeCheck 'rotation' $rotation @()
  $copy = Join-Path $OutputDirectory 'framebuffer-copy.exe'
  Invoke-NativeCheck 'framebuffer-copy-build' $Compiler ($flags + @(
    'platform/androswitch/tests/framebuffer_copy.c', '-o', $copy
  ))
  Invoke-NativeCheck 'framebuffer-copy' $copy @()
  $publisher = Join-Path $OutputDirectory 'ppm-publish.exe'
  Invoke-NativeCheck 'ppm-publish-build' $Compiler ($flags + @(
    'platform/androswitch/tests/ppm_publish.c', '-o', $publisher
  ))
  Invoke-NativeCheck 'ppm-publish' $publisher @((Join-Path $OutputDirectory ('capture-' + [guid]::NewGuid().ToString('N') + '.ppm')))
  $worker = Join-Path $OutputDirectory 'glsurface-worker.exe'
  Invoke-NativeCheck 'worker-build' $Compiler ($flags + @(
    '-pthread', '-Iplatform/androswitch/tests/fake-switch', '-Iplatform/switch-smoke',
    'platform/androswitch/tests/glsurface_worker.c', 'platform/androswitch/glsurface.c',
    'platform/switch-smoke/worker_admission.c', '-o', $worker
  ))
  $scenarios = @(
    'normal', 'portrait', 'portrait-pause-preserve', 'portrait-slow-termination', 'pause-preserve', 'pause-recreate', 'pause-exit',
    'pause-before-create', 'pause-preserve-exit', 'pause-toggle-discard', 'suspend-failure',
    'resume-failure', 'queue-before-context', 'invalid', 'create-failure', 'start-failure',
    'start-close-failure', 'initialize-failure', 'choose-failure', 'egl-create-failure',
    'attach-failure', 'callback-failure', 'queue-failure', 'poll-failure', 'close-failure',
    'self-join', 'wait-failure', 'thread-close-failure', 'timeout', 'concurrent-join', 'admission-closed'
  )
  foreach ($scenario in $scenarios) {
    Invoke-NativeCheck "worker-$scenario" $worker @($scenario)
  }
  $receipt.status = 'passed'
  $receipt.full_rotation_cases = 4
  $receipt.framebuffer_address_guard_cases = 4
  $receipt.ppm_publication_cases = 12
  $receipt.cpu_row_guard_cases = 12
  $receipt.worker_cases = $scenarios.Count
  $receipt.source_snapshots = @(@(
    'platform/androswitch/display.c', 'platform/androswitch/display.h',
    'platform/androswitch/display_geometry.h', 'platform/androswitch/tests/display_rotation.c',
    'platform/androswitch/tests/fake-display/switch.h', 'platform/androswitch/framebuffer_copy.h',
    'platform/androswitch/tests/framebuffer_copy.c', 'platform/androswitch/glsurface.c',
    'platform/androswitch/ppm_publish.h', 'platform/androswitch/tests/ppm_publish.c',
    'platform/androswitch/rgba_ppm.h',
    'platform/androswitch/glsurface.h', 'platform/androswitch/tests/glsurface_worker.c',
    'platform/switch-smoke/worker_admission.c', 'platform/switch-smoke/worker_admission.h',
    'platform/androswitch/tests/fake-switch/switch.h', 'scripts/test-horizon-display.ps1'
  ) | ForEach-Object {
    @{ path = $_; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() }
  })
  Save-Result
  Write-Output "Host display checks passed: 4 full rotations with CPU RGB rows, 12 row guards, 4 framebuffer cases, 12 PPM publication cases, $($scenarios.Count) worker scenarios."
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
