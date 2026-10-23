param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk",[string]$HostGcc='C:/w64devkit/bin/gcc.exe',[string]$KotlinCache="$env:USERPROFILE/.gradle/caches/modules-2/files-2.1")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m4'
$output=Join-Path $projectRoot 'local-artifacts/m4/fixture'
$classes=Join-Path $output 'classes'
$kotlinClasses=Join-Path $output 'kotlin-classes'
$native=Join-Path $output 'native'
$hostNativeRoot=Join-Path $output 'host'
New-Item -ItemType Directory -Force -Path $classes,$kotlinClasses,$native,$hostNativeRoot,"$output/host-storage" | Out-Null
$ndk=Join-Path $AndroidSdk 'ndk/28.2.13676358/toolchains/llvm/prebuilt/windows-x86_64/bin'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
$flags=@('-shared','-fPIC','-O1','-fno-stack-protector','-fno-emulated-tls','-nostdlib','-Wl,--hash-style=both','-Wl,--pack-dyn-relocs=none','-Wl,-z,max-page-size=4096')
& "$ndk/aarch64-linux-android26-clang.cmd" @flags "$source/native/bridge.c" '-lc' '-Wl,-soname,libbridge.so' -o "$native/libbridge.so"
if($LASTEXITCODE){throw 'NDK JNI C build failed'}
New-Item -ItemType Directory -Force -Path "$native/emulated" | Out-Null
$emulatedFlags=@($flags | Where-Object {$_ -ne '-fno-emulated-tls'})
& "$ndk/aarch64-linux-android26-clang.cmd" @emulatedFlags "$source/native/bridge.c" '-lc' '-Wl,-soname,libbridge.so' -o "$native/emulated/libbridge.so"
if($LASTEXITCODE){throw 'NDK emulated TLS C build failed'}
& "$ndk/aarch64-linux-android26-clang++.cmd" @flags '-fno-exceptions' '-fno-rtti' "$source/native/kotlin_bridge.cpp" '-Wl,-soname,libkotlinbridge.so' -o "$native/libkotlinbridge.so"
if($LASTEXITCODE){throw 'NDK JNI C++ build failed'}
& $HostGcc -shared -O1 '-I' "$projectRoot/third_party/android" "$source/native/bridge.c" -lpthread -o "$hostNativeRoot/bridge.dll"
if($LASTEXITCODE){throw 'JVM native C build failed'}
$hostCpp=Join-Path (Split-Path -Parent $HostGcc) 'g++.exe'
& $hostCpp -shared -O1 '-I' "$projectRoot/third_party/android" "$source/native/kotlin_bridge.cpp" -o "$hostNativeRoot/kotlinbridge.dll"
if($LASTEXITCODE){throw 'JVM native C++ build failed'}
$javaFiles=@(Get-ChildItem -LiteralPath "$source/java" -Recurse -File | Where-Object Extension -eq '.java' | ForEach-Object FullName)
& javac --release 8 -g:none -d $classes @javaFiles
if($LASTEXITCODE){throw 'javac failed'}
function Jar([string]$Group,[string]$Artifact,[string]$Version){
    $files=@(Get-ChildItem -LiteralPath "$KotlinCache/$Group/$Artifact/$Version" -Recurse -File | Where-Object Name -eq "$Artifact-$Version.jar")
    if($files.Count -ne 1){throw "Missing cached compiler dependency: $Group/$Artifact/$Version"};return $files[0].FullName
}
$stdlib=Jar 'org.jetbrains.kotlin' 'kotlin-stdlib' '2.2.20'
$compiler=@((Jar 'org.jetbrains.kotlin' 'kotlin-compiler-embeddable' '2.2.20'),$stdlib,(Jar 'org.jetbrains.kotlin' 'kotlin-script-runtime' '2.2.20'),(Jar 'org.jetbrains.kotlin' 'kotlin-reflect' '1.6.10'),(Jar 'org.jetbrains.kotlin' 'kotlin-daemon-embeddable' '2.2.20'),(Jar 'org.jetbrains.kotlinx' 'kotlinx-coroutines-core-jvm' '1.8.0'))
$annotations=@(Get-ChildItem -LiteralPath "$KotlinCache/org.jetbrains/annotations" -Recurse -File | Where-Object Extension -eq '.jar' | Select-Object -First 1 -ExpandProperty FullName)
if($annotations){$compiler+=$annotations}
& java -cp ($compiler -join ';') org.jetbrains.kotlin.cli.jvm.K2JVMCompiler -no-stdlib -no-reflect -language-version 2.0 -jvm-target 1.8 -classpath $stdlib -d $kotlinClasses "$source/kotlin/org/androswitch/fixture/KotlinBridge.kt"
if($LASTEXITCODE){throw 'Kotlin compile failed'}
Push-Location -LiteralPath "$source/rust"
try {
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/aarch64-linux-android26-clang.cmd"
    & cargo +nightly rustc --release --target aarch64-linux-android --target-dir "$output/rust-target" '-Zbuild-std=core' -- '-C' 'link-arg=-nostdlib' '-C' 'link-arg=-Wl,--pack-dyn-relocs=none'
    if($LASTEXITCODE){throw 'Rust ARM64 JNI build failed'}
    & cargo build --release --target x86_64-pc-windows-gnu --target-dir "$output/rust-target"
    if($LASTEXITCODE){throw 'Rust JVM JNI build failed'}
}finally{Remove-Item Env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER -ErrorAction SilentlyContinue;Pop-Location}
Copy-Item -LiteralPath "$output/rust-target/aarch64-linux-android/release/librustbridge.so" -Destination "$native/librustbridge.so"
Copy-Item -LiteralPath "$output/rust-target/x86_64-pc-windows-gnu/release/rustbridge.dll" -Destination "$hostNativeRoot/rustbridge.dll"
$expected=[ordered]@{}
$oracle=@(& java "-Djava.library.path=$hostNativeRoot" "-Duser.dir=$output/host-storage" -cp $classes org.androswitch.fixture.Bridge)
if($LASTEXITCODE){throw 'JVM JNI C oracle failed'}
foreach($line in $oracle){$parts=$line.Split('=',2);$expected[$parts[0]]=$parts[1]}
$kotlinOracle=& java "-Djava.library.path=$hostNativeRoot" -cp "$kotlinClasses;$stdlib" org.androswitch.fixture.KotlinBridgeKt
if($LASTEXITCODE){throw 'JVM Kotlin/C++ oracle failed'}
$expected['kotlin']=$kotlinOracle.Trim()
$specs=@(
    @('java-jni','org.fixture.jnibridge',$classes,@('libbridge.so','librustbridge.so')),
    @('kotlin-jni','org.fixture.kotlinbridge',$kotlinClasses,@('libkotlinbridge.so')),
    @('java-jni-emutls','org.fixture.jniemutls',$classes,@('libbridge.so','librustbridge.so'))
)
foreach($spec in $specs){
    $name,$package,$classRoot,$libraries=$spec
    $dex=Join-Path $output "$name-dex"
    New-Item -ItemType Directory -Force -Path $dex | Out-Null
    $files=@(Get-ChildItem -LiteralPath $classRoot -Recurse -File | Where-Object Extension -eq '.class' | ForEach-Object FullName)
    & "$tools/d8.bat" --min-api 24 --lib $android --output $dex @files
    if($LASTEXITCODE){throw 'D8 failed'}
    $manifest=Join-Path $output "$name.xml"
    '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="'+$package+'" android:versionCode="1"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="35"/><application android:label="JNI fixture"/></manifest>' | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM
    $apk=Join-Path $output "$name.apk"
    & "$tools/aapt2.exe" link -o $apk -I $android --manifest $manifest
    if($LASTEXITCODE){throw 'aapt2 failed'}
    $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
    try{
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null
        foreach($library in $libraries){$libraryPath=if($name -eq 'java-jni-emutls' -and $library -eq 'libbridge.so'){"$native/emulated/$library"}else{"$native/$library"};[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$libraryPath,"lib/arm64-v8a/$library",[IO.Compression.CompressionLevel]::Optimal) | Out-Null}
    }finally{$zip.Dispose()}
}
$javaCases=@(@('numeric','numeric','()D'),@('registration','registration','()I'),@('reentry','reentry','()I'),@('strings','strings','()I'),@('arrays','arrayTest','()I'),@('references','referenceTest','()I'),@('exception','exceptionTest','()I'),@('callback-exception','callbackExceptionTest','()I'),@('thread','threadTest','()I'),@('storage','storageTest','()I'))
$javaCalls=@($javaCases | ForEach-Object {@{id=$_[0];class='Lorg/androswitch/fixture/Bridge;';method=$_[1];descriptor=$_[2];args=@()}})
$javaCalls+=@{id='many';class='Lorg/androswitch/fixture/Bridge;';method='manyArguments';descriptor='()D';args=@()}
$javaCalls+=@{id='float-callback';class='Lorg/androswitch/fixture/Bridge;';method='floatCallbackTest';descriptor='()D';args=@()}
$javaCalls+=@{id='stack-callback';class='Lorg/androswitch/fixture/Bridge;';method='stackCallbackTest';descriptor='()D';args=@()}
$javaCalls+=@{id='synchronization';class='Lorg/androswitch/fixture/Bridge;';method='synchronizationTest';descriptor='()I';args=@()}
$javaCalls+=@{id='rust';class='Lorg/androswitch/fixture/RustBridge;';method='exercise';descriptor='()J';args=@()}
$javaCalls+=@{id='unsupported';class='Lorg/androswitch/fixture/Bridge;';method='missingApiTest';descriptor='()I';args=@()}
# Install/update uses immutable revisions so test re-runs retain normal user-data semantics.
@{schema_version=1;session_id='m4-java';storage=@{operation='update';package='org.fixture.jnibridge'};calls=$javaCalls} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$output/java-request.json" -Encoding utf8NoBOM
@{schema_version=1;session_id='m4-emutls';storage=@{operation='update';package='org.fixture.jniemutls'};calls=$javaCalls} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$output/emutls-request.json" -Encoding utf8NoBOM
$javaExpected=@($javaCases | ForEach-Object {$expected[$_[0]]})
$javaExpected+=@($expected['many'],$expected['float-callback'],$expected['stack-callback'],$expected['synchronization'],$expected['rust'],@{error='JNI_API_UNAVAILABLE'})
@{calls=$javaExpected;native=@()} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$output/java-expected.json" -Encoding utf8NoBOM
@{schema_version=1;session_id='m4-kotlin';calls=@(@{id='kotlin';class='Lorg/androswitch/fixture/KotlinBridgeKt;';method='exercise';descriptor='()I';args=@()})} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$output/kotlin-request.json" -Encoding utf8NoBOM
@{calls=@($expected['kotlin']);native=@()} | ConvertTo-Json | Set-Content -LiteralPath "$output/kotlin-expected.json" -Encoding utf8NoBOM
$files=@(Get-ChildItem -LiteralPath $native -File | ForEach-Object {@{name=$_.Name;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}})
$toolchains=@{java=((& java -version 2>&1 | Out-String).Trim());host_gcc=((& $HostGcc --version | Select-Object -First 1));host_gpp=((& $hostCpp --version | Select-Object -First 1));rust_host=(& rustc --version);rust_android=(& rustc +nightly --version);kotlin_language='2.0';compiler_jars=@($compiler | ForEach-Object {@{file=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}});android_tools=@("$tools/lib/d8.jar","$tools/aapt2.exe",$android | ForEach-Object {@{file=$_;sha256=(Get-FileHash -LiteralPath $_).Hash.ToLowerInvariant()}})}
[ordered]@{schema_version=1;ndk='28.2.13676358';api=26;kotlin='2.2.20';toolchains=$toolchains;jvm_results=$expected;files=$files;sources=@(Get-ChildItem -LiteralPath $source -Recurse -File | Where-Object {$_.Extension -in @('.java','.kt','.c','.cpp','.rs')} | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$output/build-receipt.json" -Encoding utf8NoBOM
