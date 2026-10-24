param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/inherited-dispatch-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Inherited dispatch Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Inherited dispatch DEX compilation failed'}
foreach($spec in @(@('dispatch','org.fixture.dispatch'),@('renamed','net.example.renameddispatch'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"Receiver dispatch contracts`"><activity android:name=`"org.androswitch.contracts.DispatchActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  $apk="$output/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'Inherited dispatch APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-Item "$output/dispatch.apk","$output/renamed.apk" | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
