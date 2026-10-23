param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m8/java'
$output=Join-Path $projectRoot 'local-artifacts/m8/fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
$sources=@(Get-ChildItem $source -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'M8 Java compilation failed'}
& java -cp "$output/classes" org.androswitch.persistence.Contracts > "$output/jvm-oracle.txt"
if($LASTEXITCODE){throw 'JVM collections/reflection oracle failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'M8 DEX compilation failed'}
foreach($spec in @(@('preferences','org.fixture.preferences','Persistent Preferences'),@('renamed','net.example.renamednotes','Renamed Preferences'),@('database','org.fixture.database','Persistent Notes'),@('database-renamed','net.example.renameddb','Renamed Notes'))){
  $activity=if($spec[0].StartsWith('database')){'DatabaseActivity'}else{'PreferencesActivity'}
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><uses-permission android:name=`"android.permission.READ_EXTERNAL_STORAGE`"/><application android:label=`"$($spec[2])`"><activity android:name=`"org.androswitch.persistence.$activity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  $apk="$output/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'M8 APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$projectRoot/fixtures/m8" -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*.apk" -File | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});jvm_oracle=(Get-Content "$output/jvm-oracle.txt")} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
