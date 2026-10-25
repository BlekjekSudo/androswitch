param(
    [string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk",
    [string]$NdkVersion='28.2.13676358',
    [string]$OutputDirectory='local-artifacts/conformance'
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path "$PSScriptRoot/../..").Path
$output=[IO.Path]::GetFullPath($OutputDirectory,$repo)
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$jar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
$ndk=Join-Path $AndroidSdk "ndk/$NdkVersion/toolchains/llvm/prebuilt/windows-x86_64"
foreach($file in @($jar,"$tools/aapt2.exe","$tools/d8.bat","$tools/apksigner.bat","$ndk/bin/clang.exe")){
    if(-not(Test-Path -LiteralPath $file)){throw "Missing SDK dependency: $file"}
}
New-Item -ItemType Directory -Force "$output/classes","$output/oracle-classes","$output/dex","$output/lib/arm64-v8a","$output/signing","$output/generated"|Out-Null
$pure=Join-Path $PSScriptRoot 'java/org/androswitch/conformance/PureCases.java'
& javac --release 8 -g:none -d "$output/oracle-classes" $pure
if($LASTEXITCODE){throw 'JVM case compilation failed'}
& java -cp "$output/oracle-classes" org.androswitch.conformance.PureCases | Set-Content "$output/jvm-oracle.tsv" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'JVM reference execution failed'}
& python "$PSScriptRoot/catalog.py" --output $output
if($LASTEXITCODE){throw 'Catalog generation failed'}
$native=@(Get-ChildItem "$PSScriptRoot/native" -Filter '*.c' -File|ForEach-Object FullName)
& "$ndk/bin/clang.exe" --target=aarch64-linux-android26 "--sysroot=$ndk/sysroot" -std=c11 -Wall -Wextra -Werror -fPIC -shared -O2 -fno-emulated-tls -mno-outline-atomics -nostdlib @native '-Wl,-soname,libasconformance.so' '-Wl,--no-undefined' -lc -ldl -lm -o "$output/lib/arm64-v8a/libasconformance.so"
if($LASTEXITCODE){throw 'Native ARM64 probes failed to compile'}
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File|ForEach-Object FullName)+@("$output/generated/Catalog.java")
& javac --release 8 -g:none -cp $jar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Android SDK suite compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $jar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'DEX compilation failed'}
& "$tools/aapt2.exe" link -o "$output/unsigned.apk" -I $jar --manifest "$PSScriptRoot/AndroidManifest.xml" -A "$PSScriptRoot/assets"
if($LASTEXITCODE){throw 'APK packaging failed'}
$zip=[IO.Compression.ZipFile]::Open("$output/unsigned.apk",[IO.Compression.ZipArchiveMode]::Update)
try{
    foreach($entry in @(@("$output/dex/classes.dex",'classes.dex'),@("$output/lib/arm64-v8a/libasconformance.so",'lib/arm64-v8a/libasconformance.so'),@("$output/catalog.json",'assets/catalog.json'))){
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$entry[0],$entry[1],[IO.Compression.CompressionLevel]::Optimal)|Out-Null
    }
}finally{$zip.Dispose()}
$key="$output/signing/debug.p12"
if(-not(Test-Path -LiteralPath $key)){
    & keytool -genkeypair -keystore $key -storetype PKCS12 -storepass android -keypass android -alias conformance -keyalg RSA -keysize 2048 -validity 10000 -dname 'CN=AndroSwitch Conformance,OU=Development,O=AndroSwitch,C=ID'
    if($LASTEXITCODE){throw 'Local public development signing-key creation failed'}
}
& "$tools/apksigner.bat" sign --ks $key --ks-key-alias conformance --ks-pass pass:android --key-pass pass:android --min-sdk-version 26 --v1-signing-enabled true --v2-signing-enabled true --v3-signing-enabled false --v4-signing-enabled false --out "$output/AndroSwitchConformance.apk" "$output/unsigned.apk"
if($LASTEXITCODE){throw 'Development APK signing failed'}
& "$tools/apksigner.bat" verify --verbose "$output/AndroSwitchConformance.apk" *> "$output/signature-verification.log"
if($LASTEXITCODE){throw 'APK signature verification failed'}
@{
 schema_version=1;project='AndroSwitch Conformance';api_reference=26;sensor_tests=$false
 scope='Declared catalog of original contracts and observations; not complete Android coverage or official CTS certification'
 java=@(& java -version 2>&1|ForEach-Object ToString);ndk=$NdkVersion;build_tools='35.0.0'
 source_commit=(& 'C:/Program Files/Git/cmd/git.exe' -C $repo rev-parse HEAD).Trim()
 sources=@(Get-ChildItem $PSScriptRoot -Recurse -File|ForEach-Object {@{path=[IO.Path]::GetRelativePath($repo,$_.FullName).Replace('\','/');sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}})
 artifacts=@(Get-Item "$output/AndroSwitchConformance.apk","$output/catalog.json","$output/jvm-oracle.tsv","$output/dex/classes.dex","$output/lib/arm64-v8a/libasconformance.so","$output/signature-verification.log"|ForEach-Object {@{path=$_.FullName;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}})
} |ConvertTo-Json -Depth 8|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
