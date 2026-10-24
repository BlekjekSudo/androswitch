param(
    [string]$Toolchain='nightly',
    [string]$TargetDirectory='target/switch',
    [ValidateSet('Wsl','Windows')][string]$LinkBackend='Wsl',
    [string]$WindowsDevkitPro='C:/devkitPro',
    [string]$WindowsMake='C:/w64devkit/bin/make.exe'
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location $root
$previousDevkitPro=$env:DEVKITPRO
try {
    $targetRoot=[IO.Path]::GetFullPath($TargetDirectory,$root)
    $rustVersion=(& rustc "+$Toolchain" --version --verbose | Out-String).Trim()
    if($LASTEXITCODE){throw 'Cannot identify the product Rust compiler'}
    $cargoVersion=(& cargo "+$Toolchain" --version | Out-String).Trim()
    if($LASTEXITCODE){throw 'Cannot identify the product Cargo version'}
    & python "$root/scripts/fetch-sqlite.py"
    if($LASTEXITCODE){throw 'Pinned SQLite source preparation failed'}
    $env:CARGO_BUILD_JOBS='1'
    & cargo "+$Toolchain" build -p as-switch-smoke --release --target aarch64-nintendo-switch-freestanding --target-dir $targetRoot '-Zbuild-std=core,alloc' --config 'profile.release.panic="abort"'
    if($LASTEXITCODE){throw 'Device Rust build failed'}
    function WslPath([string]$value){$p=[IO.Path]::GetFullPath($value);if($p -notmatch '^([A-Za-z]):[\\/]'){throw 'Expected Windows drive path'};'/mnt/'+$Matches[1].ToLowerInvariant()+'/'+$p.Substring(3).Replace('\','/')}
    if($LinkBackend -eq 'Windows'){
        foreach($path in @($WindowsMake,"$WindowsDevkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe","$WindowsDevkitPro/libnx/switch.specs","$WindowsDevkitPro/tools/bin/elf2nro.exe")){
            if(-not(Test-Path -LiteralPath $path -PathType Leaf)){throw "Windows linker dependency missing: $path"}
        }
        # Separate object directories prevent mixing compiler/toolchain builds.
        $env:DEVKITPRO=[IO.Path]::GetFullPath($WindowsDevkitPro).Replace('\','/')
        & $WindowsMake -C "$root/platform/androswitch" "DEVKITPRO=$env:DEVKITPRO" "RUST_LIBRARY=$($targetRoot.Replace('\','/'))/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a" "OUTPUT_DIR=$($root.Replace('\','/'))/local-artifacts/product-windows"
        if($LASTEXITCODE){throw 'Windows product NRO link failed'}
        New-Item -ItemType Directory -Force "$root/local-artifacts/product" | Out-Null
        Copy-Item -LiteralPath "$root/local-artifacts/product-windows/androswitch.nro" -Destination "$root/local-artifacts/product/androswitch.nro"
    }else{
        & wsl -d Ubuntu -- make -C (WslPath "$root/platform/androswitch") "RUST_LIBRARY=$(WslPath "$targetRoot/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a")" "OUTPUT_DIR=$(WslPath "$root/local-artifacts/product")"
    }
    if($LASTEXITCODE){throw 'Product NRO link failed'}
    $compilerVersion=if($LinkBackend -eq 'Windows'){
        (& "$WindowsDevkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe" -dumpfullversion | Out-String).Trim()
    }else{
        (& wsl -d Ubuntu -- /opt/devkitpro/devkitA64/bin/aarch64-none-elf-gcc -dumpfullversion | Out-String).Trim()
    }
    if($LASTEXITCODE -or $compilerVersion -notmatch '^\d+\.\d+\.\d+$'){throw 'Cannot identify the product C compiler version'}
    $artifact=Get-FileHash "$root/local-artifacts/product/androswitch.nro"
    New-Item -ItemType Directory -Force "$root/target/evidence" | Out-Null
    @{schema_version=1;status='built';target='aarch64-nintendo-switch-freestanding';target_directory=$targetRoot;rust_toolchain=$Toolchain;rustc_verbose=$rustVersion;cargo_version=$cargoVersion;link_backend=$LinkBackend;c_compiler_version=$compilerVersion;windows_devkitpro=if($LinkBackend -eq 'Windows'){$env:DEVKITPRO}else{$null};profile='release panic=abort';nro_sha256=$artifact.Hash.ToLowerInvariant();nro_bytes=(Get-Item $artifact.Path).Length;sqlite_source_lock_sha256=(Get-FileHash "$root/third_party/sqlite.lock.json").Hash.ToLowerInvariant();executed=$false} | ConvertTo-Json -Depth 4 | Set-Content "$root/target/evidence/product-build.json" -Encoding utf8NoBOM
    $artifact
}finally{$env:DEVKITPRO=$previousDevkitPro;Pop-Location}
