param([string]$OutputRoot='local-artifacts/product-licenses',[string]$GccVersion='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $GccVersion){
  $buildPath=Join-Path $projectRoot 'target/evidence/product-build.json'
  $build=if(Test-Path -LiteralPath $buildPath){Get-Content -LiteralPath $buildPath -Raw | ConvertFrom-Json}else{$null}
  $GccVersion=if($build.c_compiler_version){$build.c_compiler_version}elseif($build.link_backend -eq 'Windows'){'15.2.0'}else{'16.1.0'}
}
if($GccVersion -notin @('15.2.0','16.1.0')){throw "No pinned runtime license reference for GCC $GccVersion"}
$output=if([IO.Path]::IsPathRooted($OutputRoot)){$OutputRoot}else{Join-Path $projectRoot $OutputRoot}
New-Item -ItemType Directory -Force -Path "$output/platform","$output/cargo","$output/rust","$output/android" | Out-Null
$sources=@(
  @{name='mesa.txt';version='switch-mesa 20.1.0-5';url='https://raw.githubusercontent.com/devkitPro/mesa/switch-20.1.0-rc3/docs/license.html';sha256='1ddae7da415352a5b5360ff3a9d7ecf23ba81408f62eeecce0011f32e3ef9da6'},
  @{name='libnx.txt';version='libnx 4.12.0';url='https://raw.githubusercontent.com/switchbrew/libnx/v4.12.0/LICENSE.md';sha256='5b6675f85016009624d29c9222201d3c0fc6a2d5c59253367f27d3e15011cdb2'},
  @{name='nouveau.txt';version='libdrm_nouveau 1.0.1-2; source license reference';url='https://raw.githubusercontent.com/devkitPro/libdrm_nouveau/master/source/nouveau.c';sha256='ece1e4301d7bdc7b8c32a58727ff4c84dbd4d16bfa2bb2203012913b369cb8a1'},
  @{name='newlib.txt';version='newlib 4.6.0; complete notices reference';url='https://raw.githubusercontent.com/mirror/newlib-cygwin/master/COPYING.NEWLIB';sha256='fcfb5ec69b6ab52676dcc4dab7cf4338c8000ef97812dadd35b8592a640a8419'},
  @{name='gcc-COPYING.RUNTIME';version="GCC $GccVersion Runtime Library Exception";url="https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-$GccVersion/COPYING.RUNTIME";sha256='9d6b43ce4d8de0c878bf16b54d8e7a10d9bd42b75178153e3af6a815bdc90f74'},
  @{name='gcc-COPYING3';version="GCC $GccVersion GPLv3; see Runtime Library Exception";url="https://raw.githubusercontent.com/gcc-mirror/gcc/releases/gcc-$GccVersion/COPYING3";sha256='8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903'}
)
$inventory=[Collections.Generic.List[object]]::new()
$charsetFiles=@()
foreach($name in @('LICENSE','SOURCE.md')){
  $destination=Join-Path $output "android/charset/$name"
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $destination) | Out-Null
  Copy-Item -LiteralPath "$projectRoot/third_party/android-charset/$name" -Destination $destination
  $charsetFiles+=@{path="android/charset/$name";sha256=(Get-FileHash -LiteralPath $destination).Hash.ToLowerInvariant()}
}
$inventory.Add(@{component='Android API 26 ICU 58.2 charset-name recognition data';license='ICU/Unicode; full upstream notices retained';source='https://android.googlesource.com/platform/external/icu/+/android-8.0.0_r1/icu4c/source/data/mappings/convrtrs.txt';files=$charsetFiles})
Copy-Item "$projectRoot/third_party/SQLite-NOTICE.txt" "$output/platform/SQLite-NOTICE.txt"
Copy-Item "$projectRoot/third_party/sqlite.lock.json" "$output/platform/sqlite.lock.json"
$inventory.Add(@{component='SQLite 3.54.1';license='public-domain';source='https://www.sqlite.org/2026/sqlite-amalgamation-3540100.zip';files=@(@{path='platform/SQLite-NOTICE.txt';sha256=(Get-FileHash "$output/platform/SQLite-NOTICE.txt").Hash.ToLowerInvariant()},@{path='platform/sqlite.lock.json';sha256=(Get-FileHash "$output/platform/sqlite.lock.json").Hash.ToLowerInvariant()})})
foreach($source in $sources){
  $path=Join-Path $output "platform/$($source.name)"
  $cache=Join-Path $projectRoot "local-artifacts/product-licenses/platform/$($source.name)"
  if(-not(Test-Path $path)){
    if((Test-Path $cache) -and (Get-FileHash $cache).Hash -eq $source.sha256){Copy-Item $cache $path}
    else{$reply=Invoke-WebRequest -Uri $source.url -TimeoutSec 30;[IO.File]::WriteAllText($path,$reply.Content,[Text.UTF8Encoding]::new($false))}
  }
  if((Get-FileHash $path).Hash -ne $source.sha256){throw "Pinned license reference changed: $($source.name)"}
  $inventory.Add(@{component=$source.version;source=$source.url;files=@(@{path="platform/$($source.name)";sha256=$source.sha256})})
}
Push-Location $projectRoot
try{
  $tree=& cargo tree -p as-switch-smoke --target aarch64-nintendo-switch-freestanding --edges normal --prefix none --format '{p}' --locked
  if($LASTEXITCODE){throw 'Device dependency graph failed'}
  $metadata=(& cargo metadata --format-version 1 --locked | ConvertFrom-Json)
  if($LASTEXITCODE){throw 'Cargo metadata failed'}
  $packages=@{}
  foreach($line in $tree){if($line -match '^([a-zA-Z0-9_-]+) v([^ ]+)'){$packages["$($Matches[1])/$($Matches[2])"]=$true}}
  foreach($package in $metadata.packages | Where-Object {$_.source -and $packages.ContainsKey("$($_.name)/$($_.version)")}){
    $sourceRoot=Split-Path -Parent $package.manifest_path;$destination="$output/cargo/$($package.name)-$($package.version)";$files=@()
    foreach($license in Get-ChildItem $sourceRoot -File -Recurse -Depth 2 | Where-Object {$_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)'} ){
      $relative=[IO.Path]::GetRelativePath($sourceRoot,$license.FullName);if($relative.StartsWith('..')){throw 'License escaped dependency source'}
      $target=Join-Path $destination $relative;New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null;Copy-Item -LiteralPath $license.FullName -Destination $target
      $files+=@{path="cargo/$($package.name)-$($package.version)/$($relative.Replace('\','/'))";sha256=(Get-FileHash $target).Hash.ToLowerInvariant()}
    }
    if(-not $files.Count){throw "No copyright/license files found: $($package.name)"}
    $inventory.Add(@{component="$($package.name) $($package.version)";license=$package.license;source=$package.source;files=$files})
  }
  $rustRoot=(& rustc +nightly --print sysroot).Trim()
  $rustNotices=Join-Path $rustRoot 'share/doc/rust/COPYRIGHT-library.html'
  if(-not(Test-Path $rustNotices)){throw 'Device Rust library copyright notice missing'}
  Copy-Item $rustNotices "$output/rust/COPYRIGHT-library.html"
  $inventory.Add(@{component='Rust core/alloc libraries';source='installed device nightly';files=@(@{path='rust/COPYRIGHT-library.html';sha256=(Get-FileHash "$output/rust/COPYRIGHT-library.html").Hash.ToLowerInvariant()})})
  Copy-Item "$projectRoot/third_party/android/opensles" "$output/android/opensles" -Recurse -Force
  Copy-Item "$projectRoot/third_party/android/jni.h" "$output/android/jni.h"
  Copy-Item "$projectRoot/LICENSE-APACHE" "$output/android/LICENSE-APACHE"
  @{schema_version=1;scope='NRO normal Cargo dependency graph plus linked platform libraries; proc macro notices included conservatively';components=@($inventory.ToArray())} | ConvertTo-Json -Depth 12 | Set-Content "$output/inventory.json" -Encoding utf8NoBOM
  Write-Output "$output/inventory.json"
}finally{Pop-Location}
