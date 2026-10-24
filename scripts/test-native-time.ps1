param([ValidateSet('Wsl','Msys')][string]$HostBackend = 'Wsl')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'local-artifacts/m9/native-time'
if ($HostBackend -eq 'Msys') { $outputRoot = Join-Path $outputRoot 'msys' }
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$checks = [Collections.Generic.List[object]]::new()
$receipt = [ordered]@{
  schema_version = 1; status = 'running'; host_backend = $HostBackend
  scope = 'Host native-time contracts. MSYS POSIX/Cygwin execution is distinct from Ubuntu WSL and from Horizon.'
  checks = $checks
}
$receiptPath = Join-Path $outputRoot 'host-result.json'
function Save-Result {
  $receipt | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $receiptPath -Encoding utf8
}
function Invoke-Check([string]$Id, [string]$Program, [string[]]$ArgumentList) {
  $output = & $Program @ArgumentList 2>&1
  $code = $LASTEXITCODE
  $output | ForEach-Object { Write-Output $_ }
  $log = Join-Path $outputRoot "$Id.log"
  [IO.File]::WriteAllText($log, ($output | Out-String), [Text.UTF8Encoding]::new($false))
  $checks.Add(@{
    id = $Id; program = $Program; arguments = $ArgumentList; exit_code = $code
    log = $log; log_sha256 = (Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()
  })
  Save-Result
  if ($code -ne 0) { throw "Native time check failed on $($HostBackend): $Id (exit $code)" }
}
$previousPath = $env:PATH
Push-Location $projectRoot
try {
  Save-Result
  if ($HostBackend -eq 'Msys') {
    $compiler = 'C:/devkitPro/msys2/usr/bin/gcc.exe'
    if (!(Test-Path -LiteralPath $compiler)) { throw "Required MSYS POSIX compiler is missing: $compiler" }
    $env:PATH = (Split-Path -Parent $compiler) + [IO.Path]::PathSeparator + $previousPath
    $machine = (& $compiler -dumpmachine | Out-String).Trim()
    if ($LASTEXITCODE -or $machine -notmatch 'cygwin|msys') { throw "Expected the POSIX MSYS/Cygwin compiler, got: $machine" }
    $version = (& $compiler --version | Select-Object -First 1)
    if ($LASTEXITCODE) { throw 'MSYS host compiler version query failed' }
    $receipt.compiler = @{ path = $compiler; version = $version; target = $machine; runtime = 'MSYS POSIX/Cygwin; not GNU/Linux WSL' }
    $executable = Join-Path $outputRoot 'contracts.exe'
    Invoke-Check 'build' $compiler @('-pipe','-std=gnu11','-O2','-Wall','-Wextra','-Werror','-Iplatform/switch-smoke','fixtures/m9/native-abi/timezones.c','platform/switch-smoke/bionic_libc.c','-Wl,--wrap=strftime','-lm','-o',$executable)
    Invoke-Check 'contracts' $executable @()
  } else {
    $linuxRoot = (& wsl.exe -d Ubuntu -- wslpath -u ($projectRoot.Replace('\','/')) | Out-String).Trim()
    if ($LASTEXITCODE -or !$linuxRoot -or $linuxRoot.Contains("'")) { throw 'WSL path conversion failed or returned an unsupported quoted path' }
    $receipt.compiler = @{ path = 'Ubuntu:cc'; runtime = 'Ubuntu WSL' }
    Invoke-Check 'contracts' 'wsl.exe' @('-d','Ubuntu','--','bash','-lc',"cd '$linuxRoot' && cc -pipe -std=gnu11 -O2 -Wall -Wextra -Werror -Iplatform/switch-smoke fixtures/m9/native-abi/timezones.c platform/switch-smoke/bionic_libc.c -Wl,--wrap=strftime -lm -o local-artifacts/m9/native-time/contracts && local-artifacts/m9/native-time/contracts")
  }
  $receipt.status = 'passed'
  Save-Result
  Write-Output "Native time checks passed on $HostBackend. Receipt: $receiptPath"
} catch {
  $receipt.status = 'failed'
  $receipt.error = $_.Exception.Message
  Save-Result
  throw
} finally {
  $env:PATH = $previousPath
  Pop-Location
}
