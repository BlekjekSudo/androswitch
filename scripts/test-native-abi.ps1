param(
  [string]$AndroidSdk = "$env:LOCALAPPDATA/Android/Sdk",
  [ValidateSet('Wsl','Msys')][string]$HostBackend = 'Wsl',
  [switch]$SkipRustTests
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'local-artifacts/m9/native-abi'
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$hostRoot = if ($HostBackend -eq 'Msys') { Join-Path $outputRoot 'msys' } else { $outputRoot }
New-Item -ItemType Directory -Force -Path $hostRoot | Out-Null
$checks = [Collections.Generic.List[object]]::new()
$receiptPath = Join-Path $hostRoot 'host-result.json'
$receipt = [ordered]@{
  schema_version = 1; status = 'running'; host_backend = $HostBackend
  scope = 'Host C contracts and ARM64 CPU oracle. Wsl and Msys are distinct host backends; neither proves Horizon execution.'
  rust_loader_tests = 'not_run'; checks = $checks
}
function Save-Result {
  $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $receiptPath -Encoding utf8
}
function Invoke-Check([string]$Id, [string]$Program, [string[]]$ArgumentList) {
  $output = & $Program @ArgumentList 2>&1
  $code = $LASTEXITCODE
  $output | ForEach-Object { Write-Output $_ }
  $log = Join-Path $hostRoot "$Id.log"
  [IO.File]::WriteAllText($log, ($output | Out-String), [Text.UTF8Encoding]::new($false))
  $checks.Add(@{
    id = $Id; program = $Program; arguments = $ArgumentList; exit_code = $code
    log = $log; log_sha256 = (Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()
  })
  Save-Result
  if ($code -ne 0) { throw "Native ABI check failed on $($HostBackend): $Id (exit $code)" }
}
function Invoke-HostContract([string]$Id, [string[]]$Sources, [string[]]$Extra = @()) {
  $flags = @('-pipe','-std=gnu11','-O2','-Wall','-Wextra','-Werror','-pthread','-Iplatform/switch-smoke')
  if ($HostBackend -eq 'Msys') {
    $executable = Join-Path $hostRoot "$Id.exe"
    Invoke-Check "$Id-build" $msysGcc ($flags + $Sources + $Extra + @('-o', $executable))
    Invoke-Check "$Id-run" $executable @()
  } else {
    # Source names/output IDs are fixed. Quote the only variable shell path.
    $relative = "local-artifacts/m9/native-abi/$Id"
    $command = "cd '$linuxRoot' && cc " + (($flags + $Sources + $Extra + @('-o',$relative)) -join ' ') + " && $relative"
    Invoke-Check $Id 'wsl.exe' @('-d','Ubuntu','--','bash','-lc',$command)
  }
}
$previousPath = $env:PATH
Push-Location $projectRoot
try {
  Save-Result
  if ($HostBackend -eq 'Msys') {
    $msysGcc = 'C:/devkitPro/msys2/usr/bin/gcc.exe'
    $switchGcc = 'C:/devkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe'
    foreach ($compiler in @($msysGcc,$switchGcc)) {
      if (!(Test-Path -LiteralPath $compiler)) { throw "Required $HostBackend compiler is missing: $compiler" }
    }
    # Process-local lookup only; global PATH and profiles are untouched.
    $env:PATH = (Split-Path -Parent $msysGcc) + [IO.Path]::PathSeparator + $previousPath
    $machine = (& $msysGcc -dumpmachine | Out-String).Trim()
    if ($LASTEXITCODE -or $machine -notmatch 'cygwin|msys') { throw "Expected the POSIX MSYS/Cygwin compiler, got: $machine" }
    $version = (& $msysGcc --version | Select-Object -First 1)
    if ($LASTEXITCODE) { throw 'MSYS host compiler version query failed' }
    $receipt.host_compiler = @{ path = $msysGcc; version = $version; target = $machine; runtime = 'MSYS POSIX/Cygwin; not GNU/Linux WSL' }
    $receipt.switch_compiler = $switchGcc
  } else {
    $linuxRoot = (& wsl.exe -d Ubuntu -- wslpath -u ($projectRoot.Replace('\','/')) | Out-String).Trim()
    if ($LASTEXITCODE -or !$linuxRoot -or $linuxRoot.Contains("'")) { throw 'WSL path conversion failed or returned an unsupported quoted path' }
    $receipt.host_compiler = @{ path = 'Ubuntu:cc'; runtime = 'Ubuntu WSL' }
    $receipt.switch_compiler = '/opt/devkitpro/devkitA64/bin/aarch64-none-elf-gcc'
  }
  Save-Result
  $toolRoot = Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin'
  $cc = Join-Path $toolRoot 'aarch64-linux-android26-clang.cmd'
  $flags = @('-shared','-fPIC','-O1','-fno-stack-protector','-nostdlib','-Wl,--hash-style=both','-Wl,--pack-dyn-relocs=none','-Wl,-z,max-page-size=4096','-Wl,--no-rosegment')
  Invoke-Check 'elf-literal' $cc ($flags + @('fixtures/m9/native-abi/elf_data.c','-o',"$outputRoot/libliteral.so"))
  Invoke-Check 'elf-syscall' $cc ($flags + @('fixtures/m3/native/syscall.S','-o',"$outputRoot/libsyscall.so"))
  Invoke-HostContract 'contracts' @(
    'fixtures/m9/native-abi/contracts.c', 'platform/switch-smoke/bionic_random.c',
    'platform/switch-smoke/bionic_cxx.c', 'platform/switch-smoke/bionic_libc.c',
    'platform/switch-smoke/bionic_locale.c', 'platform/switch-smoke/bionic_multibyte.c',
    'platform/switch-smoke/bionic_io.c', 'platform/switch-smoke/bionic_keys.c',
    'platform/switch-smoke/bionic_log.c', 'platform/switch-smoke/bionic_vfs.c',
    'platform/switch-smoke/missing_import.c'
  ) @('-lm')
  Invoke-HostContract 'dlfcn' @('fixtures/m9/native-abi/dlfcn.c','platform/switch-smoke/bionic_dlfcn.c')
  Invoke-HostContract 'locales' @('fixtures/m9/native-abi/locales.c','platform/switch-smoke/bionic_locale.c','platform/switch-smoke/bionic_multibyte.c')
  Invoke-HostContract 'multibyte' @('fixtures/m9/native-abi/multibyte.c','platform/switch-smoke/bionic_locale.c','platform/switch-smoke/bionic_multibyte.c')
  Invoke-HostContract 'random' @('fixtures/m9/native-abi/random.c','platform/switch-smoke/bionic_random.c')
  Invoke-HostContract 'finalization' @('fixtures/m9/native-abi/finalization.c','platform/switch-smoke/bionic_finalize.c','platform/switch-smoke/bionic_cxx.c','platform/switch-smoke/bionic_keys.c')
  Invoke-HostContract 'cpu-time' @('fixtures/m9/native-abi/cpu-time/contracts.c','platform/androswitch/cpu_time.c') @('-Ifixtures/m9/native-abi/cpu-time','-Iplatform/androswitch')
  Invoke-HostContract 'sdk-futex' @('fixtures/m9/native-abi/sdk-futex/contracts.c','platform/androswitch/sdk_futex.c') @('-Ifixtures/m9/native-abi/sdk-futex','-Iplatform/androswitch')
  Invoke-HostContract 'sdk-compat' @('fixtures/m9/native-abi/sdk-compat/contracts.c','platform/androswitch/sdk_compat.c') @('-Ifixtures/m9/native-abi/sdk-compat','-Iplatform/androswitch')
  # The POSIX DLL/tool directory is needed only by host C compilation/runs.
  # Restore Windows Python/Cargo/linker lookup before the target/oracle phase.
  $env:PATH = $previousPath
  $errnoFlags = @('-pipe','-D__SWITCH__','-O2','-fno-jump-tables','-fno-tree-switch-conversion','-fno-stack-protector','-Iplatform/switch-smoke','-c','fixtures/m9/native-abi/errno_probe.c','-o')
  if ($HostBackend -eq 'Msys') {
    Invoke-Check 'switch-errno-oracle' $switchGcc ($errnoFlags + @("$outputRoot/errno-oracle.o"))
  } else {
    $command = "cd '$linuxRoot' && /opt/devkitpro/devkitA64/bin/aarch64-none-elf-gcc " + (($errnoFlags + @('local-artifacts/m9/native-abi/errno-oracle.o')) -join ' ')
    Invoke-Check 'switch-errno-oracle' 'wsl.exe' @('-d','Ubuntu','--','bash','-lc',$command)
  }
  Invoke-Check 'arm64-bridge-oracle' 'python' @('-X','utf8',"$projectRoot/scripts/test-native-bridge.py",'--ndk',(Join-Path $AndroidSdk 'ndk/28.2.13676358'),'--errno-object',"$outputRoot/errno-oracle.o")
  if ($SkipRustTests) {
    $receipt.rust_loader_tests = 'skipped_explicitly'
    Write-Output 'Rust loader tests skipped explicitly with -SkipRustTests.'
  } else {
    Invoke-Check 'rust-loader' 'cargo' @('test','-p','as-native','--locked','--','--include-ignored')
    $receipt.rust_loader_tests = 'passed'
  }
  $receipt.status = if ($SkipRustTests) { 'passed_C_and_ARM64_oracle_Rust_skipped' } else { 'passed' }
  Save-Result
  Write-Output "Native ABI checks completed on $HostBackend. Receipt: $receiptPath"
} catch {
  $receipt.status = 'failed'
  $receipt.error = $_.Exception.Message
  Save-Result
  throw
} finally {
  $env:PATH = $previousPath
  Pop-Location
}
