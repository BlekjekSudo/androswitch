param([string]$Toolchain = 'nightly')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    & "$PSScriptRoot/build-androswitch.ps1" -Toolchain $Toolchain -TargetDirectory target/switch -RustOnly
    function To-WslPath([string]$Value) {
        $absolute = [IO.Path]::GetFullPath($Value)
        if ($absolute -notmatch '^([A-Za-z]):[\\/]') { throw "Expected a Windows drive path: $absolute" }
        return '/mnt/' + $Matches[1].ToLowerInvariant() + '/' + $absolute.Substring(3).Replace('\','/')
    }
    $makeRoot = To-WslPath (Join-Path $projectRoot 'platform/switch-smoke')
    $library = To-WslPath (Join-Path $projectRoot 'target/switch/aarch64-androswitch-newlib/release/libas_switch_smoke.a')
    $outputDir = To-WslPath (Join-Path $projectRoot 'local-artifacts/m1')
    & wsl -d Ubuntu -- make -C $makeRoot "RUST_LIBRARY=$library" "OUTPUT_DIR=$outputDir"
    if ($LASTEXITCODE -ne 0) { throw 'libnx smoke NRO link failed.' }
    Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $projectRoot 'local-artifacts/m1/androswitch-smoke.nro')
} finally { Pop-Location }
