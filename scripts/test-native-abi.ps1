param([string]$AndroidSdk = "$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$outputRoot = Join-Path $projectRoot 'local-artifacts/m9/native-abi'
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$toolRoot = Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin'
$cc = Join-Path $toolRoot 'aarch64-linux-android26-clang.cmd'
$flags = @('-shared','-fPIC','-O1','-fno-stack-protector','-nostdlib','-Wl,--hash-style=both','-Wl,--pack-dyn-relocs=none','-Wl,-z,max-page-size=4096','-Wl,--no-rosegment')
& $cc @flags "$projectRoot/fixtures/m9/native-abi/elf_data.c" -o "$outputRoot/libliteral.so"
if ($LASTEXITCODE) { throw 'ELF literal fixture build failed' }
& $cc @flags "$projectRoot/fixtures/m3/native/syscall.S" -o "$outputRoot/libsyscall.so"
if ($LASTEXITCODE) { throw 'ELF syscall fixture build failed' }
$linuxRoot = (& wsl.exe -d Ubuntu -- wslpath -u ($projectRoot.Replace('\','/')) | Out-String).Trim()
if ($LASTEXITCODE -or !$linuxRoot) { throw 'WSL path conversion failed' }
& wsl.exe -d Ubuntu -- bash -lc "cd '$linuxRoot' && cc -pipe -std=gnu11 -O2 -Wall -Wextra -Werror -pthread -Iplatform/switch-smoke fixtures/m9/native-abi/contracts.c platform/switch-smoke/bionic_cxx.c platform/switch-smoke/bionic_libc.c platform/switch-smoke/bionic_io.c platform/switch-smoke/bionic_keys.c platform/switch-smoke/bionic_log.c platform/switch-smoke/bionic_vfs.c platform/switch-smoke/missing_import.c -lm -o local-artifacts/m9/native-abi/contracts && local-artifacts/m9/native-abi/contracts"
if ($LASTEXITCODE) { throw 'Native ABI C contracts failed' }
& wsl.exe -d Ubuntu -- bash -lc "cd '$linuxRoot' && /opt/devkitpro/devkitA64/bin/aarch64-none-elf-gcc -pipe -D__SWITCH__ -O2 -fno-jump-tables -fno-tree-switch-conversion -fno-stack-protector -Iplatform/switch-smoke -c fixtures/m9/native-abi/errno_probe.c -o local-artifacts/m9/native-abi/errno-oracle.o"
if ($LASTEXITCODE) { throw 'Switch/newlib errno oracle compilation failed' }
& python -X utf8 "$projectRoot/scripts/test-native-bridge.py" --errno-object "$outputRoot/errno-oracle.o"
if ($LASTEXITCODE) { throw 'ARM64 native bridge oracle failed' }
& cargo test -p as-native --locked -- --include-ignored
if ($LASTEXITCODE) { throw 'Native loader contracts failed' }
