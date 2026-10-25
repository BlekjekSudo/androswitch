param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$repo=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $repo 'target/copy-on-write-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
foreach($variant in @('original','renamed')){
  $v=Join-Path $output $variant
  New-Item -ItemType Directory -Force -Path "$v/classes","$v/dex" | Out-Null
  $ns=if($variant -eq 'original'){'org.androswitch.contracts'}else{'net.example.snapshots'}
  $sources=@(Get-ChildItem "$PSScriptRoot/java" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
  if($variant -eq 'renamed'){
    New-Item -ItemType Directory -Force -Path "$v/java" | Out-Null
    $sources=@($sources | ForEach-Object {$p=Join-Path "$v/java" ([IO.Path]::GetFileName($_));(Get-Content -LiteralPath $_ -Raw).Replace('org.androswitch.contracts',$ns) | Set-Content -LiteralPath $p -Encoding utf8NoBOM;$p})
  }
  & javac --release 8 -g:none -cp $androidJar -d "$v/classes" @sources
  if($LASTEXITCODE){throw "COW javac failed: $variant"}
  $oracle=& java -cp "$v/classes;$androidJar" "$ns.CowValidation"
  if($LASTEXITCODE){throw "COW JVM oracle failed: $variant"}
  $oracle | Set-Content "$v/oracle.tsv" -Encoding utf8NoBOM
  & java -cp "$v/classes;$androidJar" "$ns.CowSetCases" | Set-Content "$v/set-oracle.tsv" -Encoding utf8NoBOM
  if($LASTEXITCODE){throw "COW set JVM oracle failed: $variant"}
  & java -cp "$v/classes;$androidJar" "$ns.EnumMapCases" | Set-Content "$v/enum-map-oracle.tsv" -Encoding utf8NoBOM
  if($LASTEXITCODE){throw "EnumMap JVM oracle failed: $variant"}
  $classes=@(Get-ChildItem "$v/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
  & "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$v/dex" @classes
  if($LASTEXITCODE){throw "COW d8 failed: $variant"}
  $specs=[Collections.Generic.List[object]]::new();$specs.Add(@('cow',"org.fixture.cow$variant",'CowActivity'))
  if($variant -eq 'original'){$specs.Add(@('cow-proof','org.fixture.cowproof','CowProofActivity'))}
  foreach($spec in $specs){
    "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"SDK COW snapshot proof`"><activity android:name=`"$ns.$($spec[2])`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$v/$($spec[0]).xml" -Encoding utf8NoBOM
    $apk="$v/$($spec[0]).apk";& "$tools/aapt2.exe" link -o $apk -I $androidJar --manifest "$v/$($spec[0]).xml"
    if($LASTEXITCODE){throw "COW aapt2 failed: $variant"}
    $zip=[IO.Compression.ZipFile]::Open($apk,[IO.Compression.ZipArchiveMode]::Update)
    try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$v/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal) | Out-Null}finally{$zip.Dispose()}
  }
}
@{schema_version=1;api=26;build_tools='35.0.0';oracle_rows_per_namespace=16;set_oracle_rows_per_namespace=20;enum_map_oracle_rows_per_namespace=26;sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});artifacts=@(Get-ChildItem $output -Recurse -File | Where-Object {$_.Name -in @('classes.dex','oracle.tsv','set-oracle.tsv','enum-map-oracle.tsv') -or $_.Extension -eq '.apk'}|ForEach-Object {@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 8 | Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
