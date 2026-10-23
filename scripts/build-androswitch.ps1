param([string]$Toolchain='nightly')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $env:CARGO_BUILD_JOBS='1'
    & cargo "+$Toolchain" build -p as-switch-smoke --release --target aarch64-nintendo-switch-freestanding --target-dir target/switch '-Zbuild-std=core,alloc' --config 'profile.release.panic="abort"'
    if($LASTEXITCODE){throw 'Device Rust build failed'}
    function WslPath([string]$value){$p=[IO.Path]::GetFullPath($value);if($p -notmatch '^([A-Za-z]):[\\/]'){throw 'Expected Windows drive path'};'/mnt/'+$Matches[1].ToLowerInvariant()+'/'+$p.Substring(3).Replace('\','/')}
    & wsl -d Ubuntu -- make -C (WslPath "$root/platform/androswitch") "RUST_LIBRARY=$(WslPath "$root/target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a")" "OUTPUT_DIR=$(WslPath "$root/local-artifacts/product")"
    if($LASTEXITCODE){throw 'Product NRO link failed'}
    Get-FileHash "$root/local-artifacts/product/androswitch.nro"
}finally{Pop-Location}
