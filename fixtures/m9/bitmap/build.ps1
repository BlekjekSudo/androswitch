param([string]$AndroidSdk="$env:LOCALAPPDATA/Android/Sdk")
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path "$PSScriptRoot/../../..").Path
$output=Join-Path $projectRoot 'target/bitmap-fixture'
$tools=Join-Path $AndroidSdk 'build-tools/35.0.0'
$androidJar=Join-Path $AndroidSdk 'platforms/android-35/android.jar'
New-Item -ItemType Directory -Force -Path "$output/classes","$output/dex","$output/generated","$output/res" | Out-Null
function Write-Be32([IO.Stream]$stream,[uint32]$value){$b=[BitConverter]::GetBytes($value);[Array]::Reverse($b);$stream.Write($b)}
function Write-Chunk([IO.Stream]$stream,[string]$tag,[byte[]]$payload){
  Write-Be32 $stream $payload.Length
  [byte[]]$body=[Text.Encoding]::ASCII.GetBytes($tag)+$payload
  $stream.Write($body)
  [uint32]$crc=[uint32]::MaxValue
  foreach($b in $body){$crc=$crc-bxor$b;for($i=0;$i-lt8;$i++){if($crc-band1){$crc=($crc-shr1)-bxor[uint32]3988292384}else{$crc=$crc-shr1}}}
  Write-Be32 $stream ($crc-bxor[uint32]::MaxValue)
}
function Write-Png([string]$path,[int]$width,[int]$height,[int]$kind,[byte[]]$samples,[byte[]]$palette=@(),[byte[]]$alpha=@()){
  New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
  $file=[IO.File]::Create($path)
  try{
    $file.Write([byte[]]@(137,80,78,71,13,10,26,10))
    $header=[IO.MemoryStream]::new();Write-Be32 $header $width;Write-Be32 $header $height;$header.Write([byte[]]@(8,$kind,0,0,0));Write-Chunk $file 'IHDR' $header.ToArray();$header.Dispose()
    if($palette.Length){Write-Chunk $file 'PLTE' $palette}
    if($alpha.Length){Write-Chunk $file 'tRNS' $alpha}
    $packed=[IO.MemoryStream]::new()
    $z=[IO.Compression.ZLibStream]::new($packed,[IO.Compression.CompressionLevel]::Optimal,$true)
    $stride=$width*$(if($kind-eq6){4}elseif($kind-eq2){3}else{1})
    for($y=0;$y-lt$height;$y++){$z.WriteByte(0);$z.Write($samples,$y*$stride,$stride)}
    $z.Dispose();Write-Chunk $file 'IDAT' $packed.ToArray();$packed.Dispose();Write-Chunk $file 'IEND' @()
  }finally{$file.Dispose()}
}
$quadrants=[byte[]]@(255,0,0,255,0,255,0,255,0,0,255,255,255,255,0,255)
Write-Png "$output/res/drawable-nodpi/quadrants.png" 2 2 6 $quadrants
Write-Png "$output/res/drawable-nodpi/palette.png" 2 1 3 ([byte[]]@(0,1)) ([byte[]]@(255,255,255,0,0,0)) ([byte[]]@(128,0))
Write-Png "$output/res/drawable-mdpi/density.png" 2 2 6 $quadrants
$blue=[byte[]]::new(4*4*4);for($i=0;$i-lt$blue.Length;$i+=4){$blue[$i+2]=255;$blue[$i+3]=255}
Write-Png "$output/res/drawable-xhdpi/density.png" 4 4 6 $blue
Write-Png "$output/res/drawable/versioned.png" 2 2 6 $quadrants
Write-Png "$output/res/drawable-v26/versioned.png" 4 4 6 $blue
Write-Png "$output/res/drawable-v27/versioned.png" 2 2 6 $quadrants
Write-Png "$output/res/drawable/localized.png" 2 2 6 $quadrants
Write-Png "$output/res/drawable-fr/localized.png" 4 4 6 $blue
Write-Png "$output/res/drawable/oriented.png" 2 2 6 $quadrants
Write-Png "$output/res/drawable-land/oriented.png" 4 4 6 $blue
$large=[byte[]]::new(1024*1024*4);for($i=3;$i-lt$large.Length;$i+=4){$large[$i]=255}
Write-Png "$output/res/drawable-nodpi/large.png" 1024 1024 6 $large
New-Item -ItemType Directory -Force -Path "$output/res/raw","$output/res/values" | Out-Null
[IO.File]::WriteAllBytes("$output/res/raw/corrupt.bin",[byte[]]@(137,80,78,71,13,10,26,10,0,0))
[IO.File]::WriteAllBytes("$output/res/raw/unsupported.bin",[Text.Encoding]::ASCII.GetBytes('RIFF0000WEBP'))
# A valid CRC but oversized IHDR must fail admission before any pixel allocation.
$huge=[IO.File]::Create("$output/res/raw/huge.bin")
try{$huge.Write([byte[]]@(137,80,78,71,13,10,26,10));$header=[IO.MemoryStream]::new();Write-Be32 $header 4097;Write-Be32 $header 4097;$header.Write([byte[]]@(8,6,0,0,0));Write-Chunk $huge 'IHDR' $header.ToArray();$header.Dispose();Write-Chunk $huge 'IEND' @()}finally{$huge.Dispose()}
'<resources><item name="alias" type="drawable">@drawable/density</item></resources>' | Set-Content "$output/res/values/aliases.xml" -Encoding utf8NoBOM
& "$tools/aapt2.exe" compile --no-crunch --dir "$output/res" -o "$output/resources.zip"
if($LASTEXITCODE){throw 'Bitmap resource compilation failed'}
foreach($spec in @(@('bitmap','org.fixture.bitmap'),@('renamed','net.example.renamedbitmap'))){
  "<manifest xmlns:android=`"http://schemas.android.com/apk/res/android`" package=`"$($spec[1])`" android:versionCode=`"1`"><uses-sdk android:minSdkVersion=`"26`" android:targetSdkVersion=`"26`"/><application android:label=`"PNG resource contracts`"><activity android:name=`"org.androswitch.contracts.BitmapActivity`" android:exported=`"true`"><intent-filter><action android:name=`"android.intent.action.MAIN`"/><category android:name=`"android.intent.category.LAUNCHER`"/></intent-filter></activity></application></manifest>" | Set-Content "$output/$($spec[0]).xml" -Encoding utf8NoBOM
  & "$tools/aapt2.exe" link -o "$output/$($spec[0]).apk" -I $androidJar --manifest "$output/$($spec[0]).xml" --custom-package org.androswitch.contracts --java "$output/generated" "$output/resources.zip"
  if($LASTEXITCODE){throw 'Bitmap APK link failed'}
}
$sources=@(Get-ChildItem "$PSScriptRoot/java","$output/generated" -Recurse -Filter '*.java' -File | ForEach-Object FullName)
& javac --release 8 -g:none -cp $androidJar -d "$output/classes" @sources
if($LASTEXITCODE){throw 'Bitmap Java compilation failed'}
$classes=@(Get-ChildItem "$output/classes" -Recurse -Filter '*.class' -File | ForEach-Object FullName)
& "$tools/d8.bat" --min-api 26 --lib $androidJar --output "$output/dex" @classes
if($LASTEXITCODE){throw 'Bitmap DEX compilation failed'}
foreach($name in @('bitmap','renamed')){$zip=[IO.Compression.ZipFile]::Open("$output/$name.apk",[IO.Compression.ZipArchiveMode]::Update);try{[IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,"$output/dex/classes.dex",'classes.dex',[IO.Compression.CompressionLevel]::Optimal)|Out-Null}finally{$zip.Dispose()}}
@{schema_version=1;api=26;build_tools='35.0.0';sources=@(@(Get-ChildItem "$PSScriptRoot/java" -Recurse -File)+@(Get-Item $PSCommandPath)|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});resources=@(Get-ChildItem "$output/res" -Recurse -File|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}});apks=@(Get-Item "$output/bitmap.apk","$output/renamed.apk"|ForEach-Object{@{path=$_.FullName;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 8|Set-Content "$output/build-receipt.json" -Encoding utf8NoBOM
Write-Output "$output/build-receipt.json"
