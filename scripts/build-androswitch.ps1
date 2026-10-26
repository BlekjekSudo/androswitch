param(
    [string]$Toolchain='nightly',
    [string]$TargetDirectory='target/switch',
    [ValidateSet('Wsl','Windows')][string]$LinkBackend='Wsl',
    [string]$WindowsDevkitPro='C:/devkitPro',
    [string]$WindowsMake='C:/w64devkit/bin/make.exe',
    [string]$MesaSdk='',
    [string]$MesaRustLibDirectory='',
    [string]$MesaObjcopy="$env:LOCALAPPDATA/Android/Sdk/ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-objcopy.exe",
    [switch]$RustOnly
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location $root
$previousDevkitPro=$env:DEVKITPRO
$previousRustc=$env:RUSTC
$previousRustdoc=$env:RUSTDOC
$previousJobs=$env:CARGO_BUILD_JOBS
try {
    $targetRoot=[IO.Path]::GetFullPath($TargetDirectory,$root)
    $mesaArguments=@()
    $productOutput=if($MesaSdk){'product-mesa-std-windows'}else{'product-std-windows'}
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
        $mesaLink="$root/local-artifacts/$productOutput/mesa-link"
        & python -X utf8 "$root/scripts/prepare-mesa-std-link.py" --sdk $MesaSdk --rust-libs $MesaRustLibDirectory --output $mesaLink --objcopy $MesaObjcopy --portlibs "$WindowsDevkitPro/portlibs/switch/lib"
        if($LASTEXITCODE){throw 'Mesa Rust personality isolation failed'}
        $mesaArguments+="MESA_LINK_LIB=$($mesaLink.Replace('\','/'))/lib"
        [IO.File]::WriteAllLines("$root/local-artifacts/$productOutput/mesa-rust-libs.rsp",@(Get-ChildItem "$mesaLink/rust" -Filter '*.rlib' -File|Sort-Object Name|ForEach-Object {'"'+$_.FullName.Replace('\','/')+'"'}),[Text.UTF8Encoding]::new($false))

    }

    & python -X utf8 "$root/scripts/prepare-horizon-std.py"
    if($LASTEXITCODE){throw 'Pinned Horizon libc preparation failed'}
    & python -X utf8 "$root/scripts/prepare-horizon-std-toolchain.py" --toolchain $Toolchain
    if($LASTEXITCODE){throw 'Task-local Horizon std toolchain preparation failed'}
    $stdOverlay=Get-Content "$root/local-artifacts/horizon-std/toolchain.json" -Raw|ConvertFrom-Json
    $env:RUSTC=Join-Path $stdOverlay.overlay_sysroot 'bin/rustc.exe'
    $env:RUSTDOC=Join-Path $stdOverlay.overlay_sysroot 'bin/rustdoc.exe'
    $libcPatch='patch.crates-io.libc.path="'+$root.Replace('\','/')+'/local-artifacts/horizon-std/libc-0.2.189"'
    $rustVersion=(& rustc "+$Toolchain" --version --verbose | Out-String).Trim()
    if($LASTEXITCODE){throw 'Cannot identify the product Rust compiler'}
    $cargoVersion=(& cargo "+$Toolchain" --version | Out-String).Trim()
    if($LASTEXITCODE){throw 'Cannot identify the product Cargo version'}
    & python "$root/scripts/fetch-sqlite.py"
    if($LASTEXITCODE){throw 'Pinned SQLite source preparation failed'}
    $env:CARGO_BUILD_JOBS='1'
    $trackedInputs=@(& git -c core.fsmonitor=false ls-files --cached --others --exclude-standard -- crates platform third_party Cargo.toml Cargo.lock scripts/build-androswitch.ps1 scripts/prepare-horizon-std.py scripts/prepare-horizon-std-toolchain.py scripts/prepare-mesa-std-link.py)
    if($LASTEXITCODE){throw 'Cannot enumerate product inputs'}
    $beforeBuild=@{}
    foreach($path in @($trackedInputs|Sort-Object -Unique)){
        if(Test-Path -LiteralPath $path -PathType Leaf){$beforeBuild[$path]=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()}
    }
    & cargo "+$Toolchain" build -p as-switch-smoke --release --target platform/horizon-std/aarch64-androswitch-newlib.json --target-dir $targetRoot '-Zbuild-std=core,alloc,std,panic_abort' -Zjson-target-spec --config $libcPatch --config 'profile.release.panic="abort"'
    if($LASTEXITCODE){throw 'Device Rust build failed'}
    if($RustOnly){Write-Output "$targetRoot/aarch64-androswitch-newlib/release/libas_switch_smoke.a";return}
    function WslPath([string]$value){$p=[IO.Path]::GetFullPath($value);if($p -notmatch '^([A-Za-z]):[\\/]'){throw 'Expected Windows drive path'};'/mnt/'+$Matches[1].ToLowerInvariant()+'/'+$p.Substring(3).Replace('\','/')}
    if($LinkBackend -eq 'Windows'){
        foreach($path in @($WindowsMake,"$WindowsDevkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe","$WindowsDevkitPro/libnx/switch.specs","$WindowsDevkitPro/tools/bin/elf2nro.exe")){
            if(-not(Test-Path -LiteralPath $path -PathType Leaf)){throw "Windows linker dependency missing: $path"}
        }
        # Separate object directories prevent mixing compiler/toolchain builds.
        $env:DEVKITPRO=[IO.Path]::GetFullPath($WindowsDevkitPro).Replace('\','/')
        & $WindowsMake -C "$root/platform/androswitch" "DEVKITPRO=$env:DEVKITPRO" "RUST_LIBRARY=$($targetRoot.Replace('\','/'))/aarch64-androswitch-newlib/release/libas_switch_smoke.a" "OUTPUT_DIR=$($root.Replace('\','/'))/local-artifacts/$productOutput" @mesaArguments
        if($LASTEXITCODE){throw 'Windows product NRO link failed'}
        New-Item -ItemType Directory -Force "$root/local-artifacts/product" | Out-Null
        Copy-Item -LiteralPath "$root/local-artifacts/$productOutput/androswitch.nro" -Destination "$root/local-artifacts/product/androswitch.nro"
    }else{
        if($MesaSdk){throw 'The pinned unified SDK path currently uses the Windows linker adapter; use -LinkBackend Windows.'}
        & wsl -d Ubuntu -- make -C (WslPath "$root/platform/androswitch") "RUST_LIBRARY=$(WslPath "$targetRoot/aarch64-androswitch-newlib/release/libas_switch_smoke.a")" "OUTPUT_DIR=$(WslPath "$root/local-artifacts/product")"
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
    $sourceFiles=@(& git -c core.fsmonitor=false ls-files --cached --others --exclude-standard -- crates platform third_party Cargo.toml Cargo.lock scripts/build-androswitch.ps1 scripts/prepare-horizon-std.py scripts/prepare-horizon-std-toolchain.py scripts/prepare-mesa-std-link.py)
    if($LASTEXITCODE){throw 'Cannot enumerate runtime source snapshot'}
    $afterPaths=@($sourceFiles|Sort-Object -Unique|Where-Object {Test-Path -LiteralPath $_ -PathType Leaf})
    if(Compare-Object @($beforeBuild.Keys|Sort-Object) @($afterPaths|Sort-Object)){throw 'Product input inventory changed during build'}
    foreach($entry in $beforeBuild.GetEnumerator()){
        if(-not(Test-Path -LiteralPath $entry.Key -PathType Leaf) -or (Get-FileHash -LiteralPath $entry.Key).Hash.ToLowerInvariant() -cne $entry.Value){throw "Product input changed during build: $($entry.Key)"}
    }
    @{source_commit=$sourceCommit;files=@($sourceFiles|Sort-Object -Unique|Where-Object {Test-Path -LiteralPath $_ -PathType Leaf}|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 5|Set-Content "$root/target/evidence/product-source.json" -Encoding utf8NoBOM
    $mesaInputs=if($MesaSdk){@($sdkRust)+@(Get-ChildItem "$MesaSdk/lib" -File -Filter '*.a')}else{@()}
    if($MesaSdk){Copy-Item -LiteralPath "$mesaLink/link-adaptation.json" -Destination "$root/target/evidence/product-mesa-link.json"}
    $sdkInputs=if($LinkBackend -eq 'Windows'){@('libnx/lib/libnx.a','libnx/include/switch/kernel/thread.h','libnx/switch.specs','devkitA64/aarch64-none-elf/include/pthread.h','devkitA64/aarch64-none-elf/lib/pic/libsysbase.a','devkitA64/aarch64-none-elf/lib/pic/libc.a','devkitA64/aarch64-none-elf/lib/pic/libm.a')|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath "$WindowsDevkitPro/$_").Hash.ToLowerInvariant()}}}else{@()}
    @{schema_version=1;runtime_sdk=$sdkInputs;libc=(Get-Content "$root/local-artifacts/horizon-std/libc-source.json" -Raw|ConvertFrom-Json);mesa_link_adaptation_sha256=if($MesaSdk){(Get-FileHash "$root/target/evidence/product-mesa-link.json").Hash.ToLowerInvariant()}else{$null};mesa_objcopy=if($MesaSdk){@{path=$MesaObjcopy;sha256=(Get-FileHash $MesaObjcopy).Hash.ToLowerInvariant()}}else{$null}}|ConvertTo-Json -Depth 6|Set-Content "$root/target/evidence/product-std-platform.json" -Encoding utf8NoBOM
    @{schema_version=2;status='built';source_commit=$sourceCommit;source_manifest_sha256=(Get-FileHash "$root/target/evidence/product-source.json").Hash.ToLowerInvariant();mesa_sdk=if($MesaSdk){$MesaSdk}else{$null};mesa_rust_libraries=$MesaRustLibDirectory;mesa_static_inputs=@($mesaInputs|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}});rust_library_profile='std';execution_backend='embedded-horizon';std_overlay=$stdOverlay;target='aarch64-androswitch-newlib';target_directory=$targetRoot;rust_toolchain=$Toolchain;rustc_verbose=$rustVersion;cargo_version=$cargoVersion;link_backend=$LinkBackend;c_compiler_version=$compilerVersion;windows_devkitpro=if($LinkBackend -eq 'Windows'){$env:DEVKITPRO}else{$null};profile='release panic=abort';nro_sha256=$artifact.Hash.ToLowerInvariant();nro_bytes=(Get-Item $artifact.Path).Length;sqlite_source_lock_sha256=(Get-FileHash "$root/third_party/sqlite.lock.json").Hash.ToLowerInvariant();executed=$false} | ConvertTo-Json -Depth 6 | Set-Content "$root/target/evidence/product-build.json" -Encoding utf8NoBOM
    $artifact
}finally{$env:DEVKITPRO=$previousDevkitPro;$env:RUSTC=$previousRustc;$env:RUSTDOC=$previousRustdoc;$env:CARGO_BUILD_JOBS=$previousJobs;Pop-Location}
