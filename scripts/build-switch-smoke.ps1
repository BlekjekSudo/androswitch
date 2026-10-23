param([string]$Toolchain = 'nightly')
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $rustRoot = & rustc "+$Toolchain" --print sysroot
    if ($LASTEXITCODE -ne 0) { throw 'Nightly toolchain is unavailable.' }
    $manifest = Get-Content -LiteralPath (Join-Path $rustRoot 'lib/rustlib/multirust-channel-manifest.toml') -Raw
    if ($manifest -notmatch '(?m)^date = "2026-08-26"$') { throw 'Use nightly-2026-08-26 with rust-src for the pinned M1 toolchain.' }
    & cargo "+$Toolchain" build -p as-switch-smoke --release --target aarch64-nintendo-switch-freestanding --target-dir target/switch '-Zbuild-std=core,alloc' --config 'profile.release.panic="abort"'
    if ($LASTEXITCODE -ne 0) { throw 'Portable Rust static library build failed.' }
    function To-WslPath([string]$Value) {
        $absolute = [IO.Path]::GetFullPath($Value)
        if ($absolute -notmatch '^([A-Za-z]):[\\/]') { throw "Expected a Windows drive path: $absolute" }
        return '/mnt/' + $Matches[1].ToLowerInvariant() + '/' + $absolute.Substring(3).Replace('\','/')
    }
    $makeRoot = To-WslPath (Join-Path $projectRoot 'platform/switch-smoke')
    $library = To-WslPath (Join-Path $projectRoot 'target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a')
    $outputDir = To-WslPath (Join-Path $projectRoot 'local-artifacts/m1')
    & wsl -d Ubuntu -- make -C $makeRoot "RUST_LIBRARY=$library" "OUTPUT_DIR=$outputDir"
    if ($LASTEXITCODE -ne 0) { throw 'libnx smoke NRO link failed.' }
    Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $projectRoot 'local-artifacts/m1/androswitch-smoke.nro')
} finally { Pop-Location }
