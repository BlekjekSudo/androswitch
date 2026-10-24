param([string]$AndroidSdk = "$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$fixtureRoot = Join-Path $projectRoot 'fixtures/m1'
$outputRoot = Join-Path $projectRoot 'local-artifacts/m1/fixture'
$classes = Join-Path $outputRoot 'classes'
$dexDir = Join-Path $outputRoot 'dex'
New-Item -ItemType Directory -Force -Path $classes,$dexDir | Out-Null
$tools = Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar = Join-Path $AndroidSdk 'platforms/android-35/android.jar'
foreach ($file in @("$tools/d8.bat","$tools/aapt2.exe",$androidJar)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Fixture build dependency missing: $file" }
}
$javaSource = Join-Path $fixtureRoot 'java/org/androswitch/fixture/Probe.java'
& javac --release 8 -g:none -d $classes $javaSource
if ($LASTEXITCODE -ne 0) { throw 'javac failed.' }
$classFiles = @(Get-ChildItem -LiteralPath $classes -Filter '*.class' -Recurse | Sort-Object FullName | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 24 --lib $androidJar --output $dexDir @classFiles
if ($LASTEXITCODE -ne 0) { throw 'D8 failed.' }
$apk = Join-Path $outputRoot 'vm-semantics.apk'
& "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest (Join-Path $fixtureRoot 'AndroidManifest.xml')
if ($LASTEXITCODE -ne 0) { throw 'aapt2 link failed.' }
$archive = [IO.Compression.ZipFile]::Open($apk, [IO.Compression.ZipArchiveMode]::Update)
try { [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive,(Join-Path $dexDir 'classes.dex'),'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null } finally { $archive.Dispose() }
$oracle = @(& java -cp $classes org.androswitch.fixture.Probe)
if ($LASTEXITCODE -ne 0) { throw 'JVM oracle failed.' }
$expected = [ordered]@{}
foreach ($line in $oracle) { $parts = $line.Split('=',2); $expected[$parts[0]] = $parts[1] }
$cases = @(
    @('arithmetic','arithmetic','(I)I',@('-123')),
    @('arrays','arrays','(I)I',@('32')),
    @('objects','objects','(I)I',@('14')),
    @('caught-zero','caught','(I)I',@('0')),
    @('caught-value','caught','(I)I',@('5')),
    @('wide','wide','(JI)J',@('-123456789012345','17')),
    @('floating','floating','(DF)D',@('1.75','-2.5')),
    @('switch-hit','selection','(I)I',@('200')),
    @('switch-miss','selection','(I)I',@('4')),
    @('chain','chain','(I)I',@('3')),
    @('static-first','next','()I',@()),
    @('static-second','next','()I',@()),
    @('missing-api','unavailable','()J',@()),
    @('missing-field','platformField','()Ljava/lang/Object;',@()),
    @('class-monitor','monitor','()I',@()),
    @('missing-native','nativeMissing','()I',@()),
    @('fuel-limit','spin','()I',@()),
    @('stack-limit','recurse','(I)I',@('0'))
)
$calls = @($cases | ForEach-Object { [ordered]@{id=$_[0];class='Lorg/androswitch/fixture/Probe;';method=$_[1];descriptor=$_[2];args=@($_[3])} })
$request = [ordered]@{schema_version=1;session_id='host-reference';calls=$calls}
$request | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputRoot 'request.json') -Encoding utf8NoBOM
$expected | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputRoot 'jvm-expected.json') -Encoding utf8NoBOM
$hashes = @($javaSource,(Join-Path $fixtureRoot 'AndroidManifest.xml'),$apk,"$tools/lib/d8.jar","$tools/aapt2.exe",$androidJar | ForEach-Object { [ordered]@{file=$_;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $_).Hash.ToLowerInvariant()} })
[ordered]@{schema_version=1;java_version=((& java -version 2>&1 | Out-String).Trim());build_tools='35.0.0';compile_sdk=35;min_sdk=24;files=$hashes;jvm=$expected} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputRoot 'build-receipt.json') -Encoding utf8NoBOM
Get-Item -LiteralPath $apk
