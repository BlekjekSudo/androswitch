param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'golden/native/fixture.c'
$output=Join-Path $projectRoot 'golden/native/artifacts'
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin'
New-Item -ItemType Directory -Force -Path $output | Out-Null
$flags=@('-shared','-fPIC','-O1','-nostdlib','-fno-stack-protector','-Wl,--hash-style=both','-Wl,--build-id=none','-Wl,--pack-dyn-relocs=none')
$files=@()
foreach($spec in @(@('x86_64','x86_64-linux-android26'),@('arm64-v8a','aarch64-linux-android26'),@('armeabi-v7a','armv7a-linux-androideabi26'))){
    $abi,$target=$spec
    $compiler=Join-Path $ndk "$target-clang.cmd"
    & $compiler @flags $source -o "$output/$abi.so"
    if($LASTEXITCODE -ne 0){throw "Native oracle fixture build failed: $abi"}
    $files+=@{abi=$abi;file="golden/native/artifacts/$abi.so";sha256=(Get-FileHash -LiteralPath "$output/$abi.so").Hash.ToLowerInvariant()}
}
@{schema_version=1;role='Host-only upstream native CPU oracle; not device ABI support';source='golden/native/fixture.c';source_sha256=(Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant();ndk='28.2.13676358';api=26;flags=$flags;files=$files} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
