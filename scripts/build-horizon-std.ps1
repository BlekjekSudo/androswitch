param(
    [string]$Toolchain='nightly-2026-08-26',
    [string]$TargetDirectory='C:/Users/Administrator/.cache/androswitch/std-probe-target',
    [string]$DevkitPro='C:/devkitPro',
    [string]$Make='C:/w64devkit/bin/make.exe',
    [string]$OutputDirectory='local-artifacts/horizon-std/candidate'
)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $root
$oldDevkit=$env:DEVKITPRO
$oldJobs=$env:CARGO_BUILD_JOBS
try {
    $env:CARGO_BUILD_JOBS='1'
    $output=[IO.Path]::GetFullPath($OutputDirectory,$root)
    $target=[IO.Path]::GetFullPath($TargetDirectory,$root)
    New-Item -ItemType Directory -Force -Path $output|Out-Null
    & python -X utf8 scripts/prepare-horizon-std.py
    if($LASTEXITCODE){throw 'Pinned libc preparation failed.'}
    $version=(& rustc "+$Toolchain" --version --verbose|Out-String).Trim()
    if($LASTEXITCODE -or $version -notmatch '787af2b8c80638c51a4fc8e44f84e6891f243ec7'){throw 'Use the pinned 2026-08-26 nightly.'}
    $paths=@(Get-ChildItem -LiteralPath experiments/horizon-std -Recurse -File|Where-Object {$_.Name -ne 'README.md'}|ForEach-Object {[IO.Path]::GetRelativePath($root,$_.FullName).Replace('\','/')})
    $paths+=@('scripts/prepare-horizon-std.py','scripts/build-horizon-std.ps1','scripts/test-horizon-std-ryujinx.ps1')
    $source=@($paths|Sort-Object|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}})
    $source|ConvertTo-Json -Depth 4|Set-Content -LiteralPath "$output/source.json" -Encoding utf8NoBOM
    $patch='patch.crates-io.libc.path="'+$root.Replace('\','/')+'/local-artifacts/horizon-std/libc-0.2.189"'
    & cargo "+$Toolchain" rustc --manifest-path experiments/horizon-std/Cargo.toml --release --locked --target experiments/horizon-std/aarch64-androswitch-newlib.json --target-dir $target '-Zbuild-std=core,alloc,std,panic_abort' -Zjson-target-spec --config $patch -- -Dwarnings *> "$output/build.log"
    if($LASTEXITCODE){throw "std build failed; see $output/build.log"}
    $env:DEVKITPRO=[IO.Path]::GetFullPath($DevkitPro).Replace('\','/')
    & $Make -C experiments/horizon-std "DEVKITPRO=$env:DEVKITPRO" "RUST_LIBRARY=$($target.Replace('\','/'))/aarch64-androswitch-newlib/release/libandroswitch_std_probe.a" "OUTPUT_DIR=$($output.Replace('\','/'))" *> "$output/link.log"
    if($LASTEXITCODE){throw "NRO link failed; see $output/link.log"}
    foreach($item in $source){if((Get-FileHash -LiteralPath $item.path).Hash.ToLowerInvariant() -cne $item.sha256){throw "Source changed during build: $($item.path)"}}
    $nro=Join-Path $output 'androswitch-std-probe.nro'
    $sdk=@('libnx/lib/libnx.a','devkitA64/aarch64-none-elf/lib/pic/libsysbase.a','devkitA64/aarch64-none-elf/lib/pic/libc.a','devkitA64/aarch64-none-elf/lib/pic/libm.a','libnx/switch.specs')
    $receipt=@{
        schema_version=1;status='built_not_executed';rustc_verbose=$version
        source_commit=(& 'C:/Program Files/Git/cmd/git.exe' -c core.fsmonitor=false rev-parse HEAD|Out-String).Trim()
        source_manifest_sha256=(Get-FileHash -LiteralPath "$output/source.json").Hash.ToLowerInvariant()
        nro_sha256=(Get-FileHash -LiteralPath $nro).Hash.ToLowerInvariant();nro_bytes=(Get-Item -LiteralPath $nro).Length
        target='aarch64-androswitch-newlib';panic='abort';rust_std_source='installed matching rust-src; no Rust source patches'
        libc=(Get-Content local-artifacts/horizon-std/libc-source.json -Raw|ConvertFrom-Json)
        libc_patch_sha256=(Get-FileHash local-artifacts/horizon-std/libc-adaptation.patch).Hash.ToLowerInvariant()
        c_compiler=(& "$DevkitPro/devkitA64/bin/aarch64-none-elf-gcc.exe" -dumpfullversion|Out-String).Trim()
        sdk_inputs=@($sdk|ForEach-Object {@{path=$_;sha256=(Get-FileHash -LiteralPath "$DevkitPro/$_").Hash.ToLowerInvariant()}})
    }
    $receipt|ConvertTo-Json -Depth 7|Set-Content -LiteralPath "$output/build.json" -Encoding utf8NoBOM
    Copy-Item -LiteralPath local-artifacts/horizon-std/libc-adaptation.patch -Destination "$output/libc-adaptation.patch"
    Write-Output "std proof built: $nro"
}finally{$env:DEVKITPRO=$oldDevkit;$env:CARGO_BUILD_JOBS=$oldJobs;Pop-Location}
