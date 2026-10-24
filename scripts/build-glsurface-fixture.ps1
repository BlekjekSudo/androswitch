param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$source=Join-Path $projectRoot 'fixtures/m9/glsurface/java'
$output=Join-Path $projectRoot 'local-artifacts/m9/glsurface-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
$sources=@(Get-ChildItem $source -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'GLSurfaceView fixture Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'GLSurfaceView fixture DEX compilation failed'}
foreach($spec in @(@('surface','org.fixture.glsurface'),@('renamed','net.example.rendercontract'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"GLSurfaceView contract`"><activity android:name=`"org.androswitch.graphics.SurfaceActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  $apk="$output/$($spec[0]).apk"
  & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'GLSurfaceView fixture APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$projectRoot/fixtures/m9/glsurface" -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{file=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-ChildItem "$output/*.apk" -File | ForEach-Object {@{file=$_.Name;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
