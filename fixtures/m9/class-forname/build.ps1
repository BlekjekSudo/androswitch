param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/class-forname-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex" | Out-Null
$sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Class.forName Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | Where-Object Name -ne 'MissingBase.class')
# Use identical absent dependency for ordinary JVM and Android DEX inputs.
# No compiled class or APK is committed; the missing dependency is intentional.
$oracleStream=[IO.File]::Open("$output/oracle.jar",[IO.FileMode]::Create)
$zip=[IO.Compression.ZipArchive]::new($oracleStream,[IO.Compression.ZipArchiveMode]::Create)
try{foreach($class in $classes){$entry=[IO.Path]::GetRelativePath("$output/classes",$class.FullName).Replace('\','/');[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$class.FullName,$entry,[IO.Compression.CompressionLevel]::Optimal)|Out-Null}}finally{$zip.Dispose()}
& java -cp "$output/oracle.jar" org.androswitch.contracts.ForNameOracle | Set-Content "$output/jvm-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Class.forName JVM oracle failed'}
& java -cp "$output/oracle.jar" org.androswitch.contracts.ForNameOracle defaults | Set-Content "$output/default-interface-jvm-oracle.txt" -Encoding utf8NoBOM
if($LASTEXITCODE){throw 'Default interface initialization JVM oracle failed'}
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @($classes | ForEach-Object FullName)
if($LASTEXITCODE){throw 'Class.forName DEX compilation failed'}
foreach($spec in @(@('class-forname','org.fixture.forname'),@('renamed','net.example.renamedforname'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"Class loading contracts`"><activity android:name=`"org.androswitch.contracts.ForNameActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  & "$tools/aapt2.exe" link -o "$output/$($spec[0]).apk" -I $androidJar --manifest "$output/$($spec[0]).xml"
  if($LASTEXITCODE){throw 'Class.forName APK packaging failed'}
  $zip=[IO.Compression.ZipFile]::Open("$output/$($spec[0]).apk",[IO.Compression.ZipArchiveMode]::Update)
  try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}
}
@{schema_version=1;api=26;build_tools='35.0.0';absent_dependency='org.androswitch.contracts.MissingBase';sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-Item "$output/class-forname.apk","$output/renamed.apk"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});oracle=@{path="$output/jvm-oracle.txt";sha256=(Get-FileHash "$output/jvm-oracle.txt").Hash.ToLowerInvariant()};default_interface_oracle=@{path="$output/default-interface-jvm-oracle.txt";sha256=(Get-FileHash "$output/default-interface-jvm-oracle.txt").Hash.ToLowerInvariant()}} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
