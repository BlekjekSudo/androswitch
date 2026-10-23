param([string]$AndroidSdk = "$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $projectRoot 'fixtures/m3/native'
$outputRoot = Join-Path $projectRoot 'local-artifacts/m3/fixture'
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$toolRoot = Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin'
$cc = Join-Path $toolRoot 'aarch64-linux-android26-clang.cmd'
$flags = @('-shared','-fPIC','-O1','-fno-stack-protector','-nostdlib','-Wl,--hash-style=both','-Wl,--pack-dyn-relocs=none','-Wl,-z,max-page-size=4096')
& $cc @flags "$sourceRoot/dependency.c" '-Wl,-soname,libcalculation.so' -o "$outputRoot/libcalculation.so"
if ($LASTEXITCODE) { throw 'dependency clang failed' }
& $cc @flags '-fno-builtin' "$sourceRoot/probe.c" "-L$outputRoot" '-lcalculation' '-lc' '-Wl,-soname,libarithmetic.so' -o "$outputRoot/libarithmetic.so"
if ($LASTEXITCODE) { throw 'probe clang failed' }
& $cc @flags '-fno-emulated-tls' "$sourceRoot/tls.c" '-Wl,-soname,libthreadstate.so' -o "$outputRoot/libthreadstate.so"
if ($LASTEXITCODE) { throw 'TLS clang failed' }
& $cc @flags "$sourceRoot/missing.c" -o "$outputRoot/libmissing.so"
if ($LASTEXITCODE) { throw 'Missing-symbol fixture build failed' }
& $cc @flags "$sourceRoot/syscall.S" -o "$outputRoot/libsyscall.so"
if ($LASTEXITCODE) { throw 'Direct-syscall fixture build failed' }
# Strip every section header, preserving the program-header/dynamic loader view.
foreach ($name in @('libarithmetic.so','libcalculation.so','libthreadstate.so')) {
    $path = Join-Path $outputRoot $name
    & "$toolRoot/llvm-strip.exe" --strip-all $path
    if ($LASTEXITCODE) { throw 'strip failed' }
    $bytes = [IO.File]::ReadAllBytes($path)
    [Array]::Clear($bytes,40,8)
    [Array]::Clear($bytes,58,6)
    [IO.File]::WriteAllBytes($path,$bytes)
}
$files = @(Get-ChildItem -LiteralPath $outputRoot -Filter '*.so' | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()} })
[ordered]@{schema_version=1;ndk='28.2.13676358';api=26;section_headers=$false;files=$files;clang=((& $cc --version | Out-String).Trim())} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$outputRoot/build-receipt.json" -Encoding utf8NoBOM
& "$toolRoot/llvm-readelf.exe" -r "$outputRoot/libthreadstate.so"
$apk = Join-Path $outputRoot 'native-loader.apk'
Copy-Item -LiteralPath "$projectRoot/local-artifacts/m1/fixture/vm-semantics.apk" -Destination $apk
$zip = [IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
try { foreach ($name in @('libarithmetic.so','libcalculation.so','libthreadstate.so','libmissing.so','libsyscall.so')) { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$outputRoot/$name","lib/arm64-v8a/$name",[IO.Compression.CompressionLevel]::Optimal) | Out-Null } } finally { $zip.Dispose() }
$cases = @(
    @('libarithmetic.so','mixed_integer','(IJ)J',@('-13','9876543210'),'69135802549'),
    @('libarithmetic.so','mixed_float','(IDFJ)D',@('3','1.75','-2.5','12'),'10.625'),
    @('libarithmetic.so','allocation','(I)I',@('257'),'257'),
    @('libthreadstate.so','tls_next','(I)I',@('3'),'44'),
    @('libthreadstate.so','tls_next','(I)I',@('-7'),'37'),
    @('libmissing.so','missing_import','()I',@(),@{error='SYMBOL_NOT_FOUND'}),
    @('libsyscall.so','direct_linux_syscall','()J',@(),@{error='LINUX_SYSCALL_UNSUPPORTED'})
)
$calls = @($cases | ForEach-Object { @{library=$_[0];symbol=$_[1];descriptor=$_[2];args=@($_[3])} })
if($calls.Count -ne 7){throw 'Invalid native fixture table'}
$request = @{schema_version=1;session_id='m3-reference';calls=@();native=$calls}
$request | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$outputRoot/request.json" -Encoding utf8NoBOM
@{native=@($cases | ForEach-Object { $_[4] });calls=@()} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$outputRoot/expected.json" -Encoding utf8NoBOM
