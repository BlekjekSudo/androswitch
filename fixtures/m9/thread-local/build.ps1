param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/thread-local-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path $output | Out-Null
foreach($variant in @('original','renamed')){
  $variantOut=Join-Path $output $variant
  New-Item -ItemType Directory -Force -Path "$variantOut/classes","$variantOut/dex" | Out-Null
  $namespace=if($variant -eq 'original'){'org.androswitch.contracts'}else{'net.example.locals'}
  $sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
  if($variant -eq 'renamed'){
    $generated=Join-Path $variantOut 'java'
    New-Item -ItemType Directory -Force -Path $generated | Out-Null
    $sources=@($sources | ForEach-Object {
      $destination=Join-Path $generated ([IO.Path]::GetFileName($_))
      (Get-Content -LiteralPath $_ -Raw).Replace('org.androswitch.contracts','net.example.locals') | Set-Content -LiteralPath $destination -Encoding utf8NoBOM
      $destination
    })
  }
  & javac --release 8 -g:none -cp $androidJar -d "$variantOut/classes" @sources
  if($LASTEXITCODE){throw "ThreadLocal Java compilation failed: $variant"}
  $oracle=& java -cp "$variantOut/classes;$androidJar" "$namespace.ThreadLocalValidation"
  if($LASTEXITCODE){throw "ThreadLocal JVM oracle failed: $variant"}
  $oracle | Set-Content "$variantOut/oracle.tsv" -Encoding utf8NoBOM
  $classes=@(Get-ChildItem "$variantOut/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
  & "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$variantOut/dex" @classes
  if($LASTEXITCODE){throw "ThreadLocal DEX compilation failed: $variant"}
  $specs=[Collections.Generic.List[object]]::new()
  $specs.Add(@('thread-local',"org.fixture.threadlocal$variant",'ThreadLocalActivity'))
  if($variant -eq 'original'){$specs.Add(@('thread-local-proof','org.fixture.threadlocalproof','ThreadLocalProofActivity'))}
  foreach($spec in $specs){
    "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"SDK ThreadLocal proof`"><activity android:name=`"$namespace.$($spec[2])`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$variantOut/$($spec[0]).xml" -Encoding utf8NoBOM
    $apk="$variantOut/$($spec[0]).apk"
    & "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$variantOut/$($spec[0]).xml"
    if($LASTEXITCODE){throw "ThreadLocal APK packaging failed: $variant/$($spec[0])"}
    $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
    try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$variantOut/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
  }
}
@{schema_version=1;api=26;build_tools='35.0.0';java_version=@(& java -version 2>&1 | ForEach-Object ToString);sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath) | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-ChildItem $output -Recurse -File | Where-Object {$_.Name -in @('oracle.tsv','classes.dex') -or $_.Extension -eq '.apk'} | ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})} | ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
