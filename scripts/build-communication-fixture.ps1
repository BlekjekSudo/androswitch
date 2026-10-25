param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m9/communication/java'
$output=Join-Path $projectRoot 'local-artifacts/m9/communication-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
New-Item -ItemType Directory -Force -Path "$output/res/values" | Out-Null
'<resources><bool name="service_enabled">true</bool></resources>' | Set-Content "$output/res/values/contracts.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" compile --dir "$output/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'Communication resource compilation failed'}
$sources=@(Get-ChildItem $source -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Communication fixture Java compilation failed'}
& java -cp "$output/classes;$androidJar" org.androswitch.contracts.PairCases | Set-Content "$output/string-hash-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'String hash JVM oracle failed'}
& java -cp "$output/classes" org.androswitch.contracts.ConcurrentMapCases | Set-Content "$output/concurrent-map-oracle.tsv" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'ConcurrentHashMap JVM oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Communication fixture DEX compilation failed'}
foreach($spec in @(@('communication','org.fixture.communication'),@('renamed','net.example.renamedcommunication'),@('resource','org.fixture.resourcecommunication'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"Offline communication contracts`"><activity android:name=`"org.androswitch.contracts.CommunicationActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity><receiver android:name=`"org.androswitch.contracts.CommunicationActivity`$Receiver`" android:exported=`"true`"><intent-filter><action android:name=`"contract.MANIFEST`"/></intent-filter></receiver><service android:name=`"org.androswitch.contracts.CommunicationActivity`$LocalService`" android:exported=`"true`"><intent-filter android:priority=`"3`"><action android:name=`"contract.SERVICE`"/></intent-filter></service><service android:name=`"org.androswitch.contracts.DisabledService`" android:enabled=`"false`"><intent-filter><action android:name=`"contract.SERVICE`"/></intent-filter></service></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  $apk="$output/$($spec[0]).apk"
  if($spec[0] -eq 'resource') {
    $taskManifest=[IO.File]::ReadAllText("$output/$($spec[0]).xml").Replace('android:exported="true"><intent-filter android:priority="3"','android:enabled="@bool/service_enabled" android:exported="true"><intent-filter android:priority="3"')
    [IO.File]::WriteAllText("$output/$($spec[0]).xml",$taskManifest,[Text.UTF8Encoding]::new($false))
  }
  & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$output/$($spec[0]).xml" "$output/resources.zip"
  if($LASTEXITCODE){throw 'Communication fixture APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$projectRoot/fixtures/m9/communication" -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*.apk" -File | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
