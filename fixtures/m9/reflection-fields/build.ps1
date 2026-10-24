param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/reflection-fields-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
$receipts=@()
foreach($spec in @(@('reflection-fields','org.fixture.fields','org.androswitch.fields'),@('renamed','net.example.renamedfields','net.example.fieldcontracts'))){
  $name=$spec[0];$namespace=$spec[2];$variant=Join-Path $output $name
  New-Item -ItemType Directory -Force -Path "$variant/java","$variant/classes","$variant/dex" | Out-Null
  foreach($source in Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File){
    $relative=[IO.Path]::GetRelativePath("$PSScriptRoot/java",$source.FullName)
    $destination=Join-Path "$variant/java" $relative
    New-Item -ItemType Directory -Force -Path (Split-Path $destination) | Out-Null
    [IO.File]::WriteAllText($destination,([IO.File]::ReadAllText($source.FullName).Replace('org.androswitch.fields',$namespace)))
  }
  $sources=@(Get-ChildItem "$variant/java" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
  & javac --release 8 -g:none -cp $androidJar -d "$variant/classes" @sources
  if($LASTEXITCODE){throw 'Field reflection Java compilation failed'}
  & "$PSScriptRoot/alias-class.ps1" -Path "$variant/classes/$($namespace.Replace('.','/'))/StaticAlias.class"
  & java -cp "$variant/classes" "$namespace.FieldCases" | Set-Content "$variant/jvm-oracle.txt" -Encoding utf8NoBOM
  if($LASTEXITCODE){throw 'Field reflection JVM oracle failed'}
  $classes=@(Get-ChildItem "$variant/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
  & "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$variant/dex" @classes
  if($LASTEXITCODE){throw 'Field reflection DEX compilation failed'}
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"Field reflection proof`"><activity android:name=`"$namespace.FieldActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$variant/manifest.xml" -Encoding utf8NoBOM
  & "$tools/aapt2.exe" link -o "$output/$name.apk" -I $androidJar --manifest "$variant/manifest.xml"
  if($LASTEXITCODE){throw 'Field reflection APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open("$output/$name.apk",[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$variant/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
  $receipts+=@{name=$name;namespace=$namespace;path="$output/$name.apk";sha256=(Get-FileHash "$output/$name.apk").Hash.ToLowerInvariant();oracle=@{path="$variant/jvm-oracle.txt";sha256=(Get-FileHash "$variant/jvm-oracle.txt").Hash.ToLowerInvariant()}}
}
@{schema_version=1;api=26;build_tools='35.0.0';java_release=8;class_file_transform='StaticAlias: omega -> alpha UTF8 field name only; I/J descriptors and values unchanged';sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath,"$PSScriptRoot/alias-class.ps1")|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});fixtures=$receipts} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
