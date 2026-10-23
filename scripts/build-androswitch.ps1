param([string]$Toolchain='nightly')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    & python "$root/scripts/fetch-sqlite.py"
    if($LASTEXITCODE){throw 'Pinned SQLite source preparation failed'}
    $env:CARGO_BUILD_JOBS='1'
    & cargo "+$Toolchain" build -p as-switch-smoke --release --target aarch64-nintendo-switch-freestanding --target-dir target/switch '-Zbuild-std=core,alloc' --config 'profile.release.panic="abort"'
    if($LASTEXITCODE){throw 'Device Rust build failed'}
    function WslPath([string]$value){$p=[IO.Path]::GetFullPath($value);if($p -notmatch '^([A-Za-z]):[\\/]'){throw 'Expected Windows drive path'};'/mnt/'+$Matches[1].ToLowerInvariant()+'/'+$p.Substring(3).Replace('\','/')}
    & wsl -d Ubuntu -- make -C (WslPath "$root/platform/androswitch") "RUST_LIBRARY=$(WslPath "$root/target/switch/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a")" "OUTPUT_DIR=$(WslPath "$root/local-artifacts/product")"
    if($LASTEXITCODE){throw 'Product NRO link failed'}
    $artifact=Get-FileHash "$root/local-artifacts/product/androswitch.nro"
    New-Item -ItemType Directory -Force "$root/target/evidence" | Out-Null
    @{schema_version=1;status='built';target='aarch64-nintendo-switch-freestanding';rust_toolchain=$Toolchain;profile='release panic=abort';nro_sha256=$artifact.Hash.ToLowerInvariant();nro_bytes=(Get-Item $artifact.Path).Length;sqlite_source_lock_sha256=(Get-FileHash "$root/third_party/sqlite.lock.json").Hash.ToLowerInvariant();executed=$false} | ConvertTo-Json -Depth 4 | Set-Content "$root/target/evidence/product-build.json" -Encoding utf8NoBOM
    $artifact
}finally{Pop-Location}
