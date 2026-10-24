param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/text-utils-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File|ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'TextUtils Java compilation failed'}
& java -cp "$output/classes" org.androswitch.contracts.TextOracle | Set-Content "$output/jvm-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'TextUtils JVM oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'TextUtils DEX compilation failed'}
foreach($spec in @(@('text-utils','org.fixture.textutils'),@('renamed','net.example.renamedtextutils'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"CharSequence contracts`"><activity android:name=`"org.androswitch.contracts.TextUtilsActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>"|Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  & "$tools/aapt2.exe" link -o "$output/$($spec[0]).apk" -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'TextUtils APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open("$output/$($spec[0]).apk",[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-Item "$output/text-utils.apk","$output/renamed.apk"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});oracle=@{path="$output/jvm-oracle.txt";sha256=(Get-FileHash "$output/jvm-oracle.txt").Hash.ToLowerInvariant()}}|ConvertTo-Json -Depth 8|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
