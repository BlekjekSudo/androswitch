param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/package-info-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex","$output/resources"|Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File|ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Package metadata Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File|ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Package metadata DEX compilation failed'}
& "$tools/aapt2.exe" compile --dir "$PSScriptRoot/res" -o "$output/resources"
if($LASTEXITCODE){throw 'Package metadata resources compilation failed'}
$padding=[IO.File]::Open("$output/padding.bin",[IO.FileMode]::Create)
try{$padding.SetLength(2 -shl 20)}finally{$padding.Dispose()}
$specs=@(
  @('package-info','org.fixture.packagemetadata','android:versionCode="7" android:versionName="2.4-proof"',$true,''),
  @('renamed','net.example.renamedpackagemetadata','android:versionCode="7" android:versionName="2.4-proof"',$true,''),
  @('companion','org.fixture.packagecompanion','android:versionCode="42" android:versionName="9.2-other"',$false,''),
  @('defaults','org.fixture.packagedefaults','',$false,''),
  @('updated','org.fixture.packagemetadata','android:versionCode="8" android:versionName="2.5-updated"',$false,''),
  @('resource-version','org.fixture.packageresource','android:versionCode="3" android:versionName="@string/package_version"',$false,''),
  @('resource-boolean','org.fixture.packageboolean','android:versionCode="5" android:versionName="5.0"',$false,'android:debuggable="@bool/debug_value"'),
  @('min-only','org.fixture.packageminonly','android:versionCode="6"',$false,'','<uses-sdk android:minSdkVersion="26"/>'),
  @('empty-sdk','org.fixture.packageemptysdk','',$false,'','<uses-sdk/>'),
  @('no-sdk','org.fixture.packagenosdk','',$false,'','')
)
foreach($spec in $specs){
  $activity=if($spec[3]){'<activity android:name="org.androswitch.contracts.PackageMetadataActivity" android:exported="true"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity>'}else{''}
  $sdk=if($spec.Count -gt 5){$spec[5]}else{'<uses-sdk android:minSdkVersion="26" android:targetSdkVersion="26"/>'}
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" $($spec[2])>$sdk<application android:label=`"Package metadata contracts`" $($spec[4])>$activity</application></manifest>"|Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  & "$tools/aapt2.exe" link -o "$output/$($spec[0]).apk" -I $androidJar --manifest "$output/$($spec[0]).xml" "$output/resources/values_metadata.arsc.flat"
  if($LASTEXITCODE){throw 'Package metadata APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open("$output/$($spec[0]).apk",[IO.Compression.ZipArchiveMode]::Update)
  try{
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null
    if($spec[0] -eq 'companion'){[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/padding.bin",'assets/metadata-padding.bin',[IO.Compression.CompressionLevel]::NoCompression)|Out-Null}
  }finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$PSScriptRoot/java","$PSScriptRoot/res" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@($specs|ForEach-Object{Get-Item "$output/$($_[0]).apk"}|ForEach-Object{@{path=$_.FullName;bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 8|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
