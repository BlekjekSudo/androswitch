param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk",[string]$KotlinCache="$env:USERPROFILE/.gradle/caches/modules-2/files-2.1")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m5'
$output=Join-Path $projectRoot 'local-artifacts/m5/fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$android=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/java-classes","$output/kotlin-classes","$output/generated","$output/java-dex","$output/kotlin-dex" | Out-Null
$javaManifest='<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.uiproof" android:versionCode="1"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="26"/><application android:label="Java UI and Canvas" android:name="org.androswitch.ui.ProofApplication"><activity android:name="org.androswitch.ui.MainActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity><activity android:name="org.androswitch.ui.DetailActivity"/></application></manifest>'
$javaManifest | Set-Content "$output/java-manifest.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" compile --dir "$source/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'Resource compile failed'}
& "$tools/aapt2.exe" link -o "$output/java-ui.apk" -I $android --manifest "$output/java-manifest.xml" --java "$output/generated" --custom-package org.androswitch.ui "$output/resources.zip"
if($LASTEXITCODE){throw 'Resource link failed'}
$java=@(Get-ChildItem "$source/java","$output/generated" -Recurse -File | Where-Object Extension -eq '.java' | ForEach-Object FullName)
& javac --release 8 -g:none -cp $android -d "$output/java-classes" @java
if($LASTEXITCODE){throw 'Java compile failed'}
function Jar([string]$Group,[string]$Name,[string]$Version){$files=@(Get-ChildItem "$KotlinCache/$Group/$Name/$Version" -Recurse -File | Where-Object Name -eq "$Name-$Version.jar");if($files.Count -ne 1){throw "Missing Kotlin dependency $Name"};$files[0].FullName}
$stdlib=Jar org.jetbrains.kotlin kotlin-stdlib 2.2.20
$compiler=@((Jar org.jetbrains.kotlin kotlin-compiler-embeddable 2.2.20),$stdlib,(Jar org.jetbrains.kotlin kotlin-script-runtime 2.2.20),(Jar org.jetbrains.kotlin kotlin-reflect 1.6.10),(Jar org.jetbrains.kotlin kotlin-daemon-embeddable 2.2.20),(Jar org.jetbrains.kotlinx kotlinx-coroutines-core-jvm 1.8.0))
$compiler+=@(Get-ChildItem "$KotlinCache/org.jetbrains/annotations" -Recurse -File | Where-Object Extension -eq '.jar' | Select-Object -First 1 -ExpandProperty FullName)
& java -cp ($compiler -join ';') org.jetbrains.kotlin.cli.jvm.K2JVMCompiler -no-stdlib -no-reflect -language-version 2.0 -jvm-target 1.8 -classpath "$stdlib;$android" -d "$output/kotlin-classes" "$source/kotlin/org/androswitch/kui/MainActivity.kt"
if($LASTEXITCODE){throw 'Kotlin compile failed'}
'<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.fixture.kotlinui" android:versionCode="1"><uses-sdk android:minSdkVersion="24" android:targetSdkVersion="26"/><application android:label="Kotlin UI"><activity android:name="org.androswitch.kui.MainActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity></application></manifest>' | Set-Content "$output/kotlin-manifest.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" link -o "$output/kotlin-ui.apk" -I $android --manifest "$output/kotlin-manifest.xml"
if($LASTEXITCODE){throw 'Kotlin APK link failed'}
foreach($language in @('java','kotlin')){
    $classes=@(Get-ChildItem "$output/$language-classes" -Recurse -File | Where-Object Extension -eq '.class' | ForEach-Object FullName)
    if($language -eq 'kotlin'){$classes+= $stdlib}
    & "$tools/d8.bat" --min-api 24 --lib $android --output "$output/$language-dex" @classes
    if($LASTEXITCODE){throw "DEX compile failed: $language"}
    $zip=[IO.Compression.ZipFile]::Open("$output/$language-ui.apk",[IO.Compression.ZipArchiveMode]::Update)
    try{foreach($dex in Get-ChildItem "$output/$language-dex" -File | Where-Object Extension -eq '.dex'){[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$dex.FullName,$dex.Name,[IO.Compression.CompressionLevel]::Optimal) | Out-Null}}finally{$zip.Dispose()}
}
@{session_id='m5-java';events=@(@{kind='touch';action=0;x=100;y=64},@{kind='touch';action=1;x=100;y=64},@{kind='wait';ms=1500},@{kind='gc'},@{kind='touch';action=0;x=100;y=110},@{kind='touch';action=1;x=100;y=110},@{kind='key';code=4},@{kind='touch';action=0;x=60;y=190},@{kind='touch';action=1;x=60;y=190})} | ConvertTo-Json -Depth 8 | Set-Content "$output/java-request.json" -Encoding utf8NoBOM
@{session_id='m5-kotlin';events=@(@{kind='key';code=23},@{kind='gc'},@{kind='key';code=23})} | ConvertTo-Json -Depth 8 | Set-Content "$output/kotlin-request.json" -Encoding utf8NoBOM
@{schema_version=1;build_tools='35.0.0';kotlin='2.2.20';sources=@(Get-ChildItem $source -Recurse -File | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*-ui.apk" -File | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
