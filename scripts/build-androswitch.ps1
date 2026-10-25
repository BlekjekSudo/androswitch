param(
    [string]$Toolchain='nightly',
    [string]$TargetDirectory='target/switch',
    [ValidateSet('Wsl','Windows')][string]$LinkBackend='Wsl',
    [string]$WindowsDevkitPro='C:/devkitPro',
    [string]$WindowsMake='C:/w64devkit/bin/make.exe',
    [string]$MesaSdk='',
    [string]$MesaRustLibDirectory=''
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location $root
$previousDevkitPro=$env:DEVKITPRO
try {
    $targetRoot=[IO.Path]::GetFullPath($TargetDirectory,$root)
    $mesaArguments=@()
    $productOutput=if($MesaSdk){'product-mesa-windows'}else{'product-windows'}
    if($MesaSdk){
        $MesaSdk=[IO.Path]::GetFullPath($MesaSdk,$root).Replace('\','/')
        foreach($name in @('include/EGL/egl.h','include/vulkan/vulkan.h','lib/libEGL.a','lib/libvulkan.a')){
            if(-not(Test-Path -LiteralPath "$MesaSdk/$name")){throw "Mesa SDK missing: $name"}
        }
        $mesaArguments=@("MESA_SDK=$MesaSdk")
        if(-not $MesaRustLibDirectory){$MesaRustLibDirectory="$root/local-artifacts/dependencies/mesa-switch-0ff52079/rust-std/rust-std-nightly-aarch64-unknown-linux-gnu/rust-std-aarch64-unknown-linux-gnu/lib/rustlib/aarch64-unknown-linux-gnu/lib"}
        $sdkRust=@(Get-ChildItem -LiteralPath $MesaRustLibDirectory -Filter '*.rlib' -File | Where-Object {$_.Name -notlike 'libpanic_abort-*'})
        if($sdkRust.Count -lt 3){throw 'Matching pinned Mesa shader-compiler Rust runtime component is missing'}
        New-Item -ItemType Directory -Force "$root/local-artifacts/$productOutput"|Out-Null
        [IO.File]::WriteAllLines("$root/local-artifacts/$productOutput/mesa-rust-libs.rsp",@($sdkRust|ForEach-Object {'"'+$_.FullName.Replace('\','/')+'"'}),[Text.UTF8Encoding]::new($false))

    }

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
        & $WindowsMake -C "$root/platform/androswitch" "DEVKITPRO=$env:DEVKITPRO" "RUST_LIBRARY=$($targetRoot.Replace('\','/'))/aarch64-nintendo-switch-freestanding/release/libas_switch_smoke.a" "OUTPUT_DIR=$($root.Replace('\','/'))/local-artifacts/$productOutput" @mesaArguments
        if($LASTEXITCODE){throw 'Windows product NRO link failed'}
        New-Item -ItemType Directory -Force "$root/local-artifacts/product" | Out-Null
        Copy-Item -LiteralPath "$root/local-artifacts/$productOutput/androswitch.nro" -Destination "$root/local-artifacts/product/androswitch.nro"
    }else{
        if($MesaSdk){throw 'The pinned unified SDK path currently uses the Windows linker adapter; use -LinkBackend Windows.'}
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
    $sourceCommit=(& git -c core.fsmonitor=false rev-parse HEAD|Out-String).Trim()
    if($LASTEXITCODE){throw 'Cannot record runtime source commit'}
    $sourceFiles=@(& git -c core.fsmonitor=false ls-files --cached --others --exclude-standard -- crates platform third_party Cargo.toml Cargo.lock scripts/build-androswitch.ps1)
    if($LASTEXITCODE){throw 'Cannot enumerate runtime source snapshot'}
    @{source_commit=$sourceCommit;files=@($sourceFiles|Sort-Object -Unique|Where-Object {Test-Path -LiteralPath $_ -PathType Leaf}|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 5|Set-Content "$root/target/evidence/product-source.json" -Encoding utf8NoBOM
    $mesaInputs=if($MesaSdk){@($sdkRust)+@(Get-ChildItem "$MesaSdk/lib" -File -Filter '*.a')}else{@()}
    @{schema_version=2;status='built';source_commit=$sourceCommit;source_manifest_sha256=(Get-FileHash "$root/target/evidence/product-source.json").Hash.ToLowerInvariant();mesa_sdk=if($MesaSdk){$MesaSdk}else{$null};mesa_rust_libraries=$MesaRustLibDirectory;mesa_static_inputs=@($mesaInputs|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}});target='aarch64-nintendo-switch-freestanding';target_directory=$targetRoot;rust_toolchain=$Toolchain;rustc_verbose=$rustVersion;cargo_version=$cargoVersion;link_backend=$LinkBackend;c_compiler_version=$compilerVersion;windows_devkitpro=if($LinkBackend -eq 'Windows'){$env:DEVKITPRO}else{$null};profile='release panic=abort';nro_sha256=$artifact.Hash.ToLowerInvariant();nro_bytes=(Get-Item $artifact.Path).Length;sqlite_source_lock_sha256=(Get-FileHash "$root/third_party/sqlite.lock.json").Hash.ToLowerInvariant();executed=$false} | ConvertTo-Json -Depth 6 | Set-Content "$root/target/evidence/product-build.json" -Encoding utf8NoBOM
    $artifact
}finally{$env:DEVKITPRO=$previousDevkitPro;Pop-Location}
