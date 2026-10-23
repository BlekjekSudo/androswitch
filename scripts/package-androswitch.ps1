param([switch]$WithoutSamples,[string]$OutputRoot='local-artifacts/releases',[string]$EvidencePath='reports/m6-m7.json')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$output=if([IO.Path]::IsPathRooted($OutputRoot)){$OutputRoot}else{Join-Path $projectRoot $OutputRoot}
$id=[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$package=Join-Path $output "androswitch-m7-alpha-$id";$sd=Join-Path $package 'switch/androswitch'
$nro=Join-Path $projectRoot 'local-artifacts/product/androswitch.nro'
if(-not(Test-Path $nro)){throw 'Build and verify the product NRO first'}
$evidencePath=if([IO.Path]::IsPathRooted($EvidencePath)){$EvidencePath}else{Join-Path $projectRoot $EvidencePath}
if(-not(Test-Path $evidencePath)){throw 'M6/M7 evidence report is required before packaging'}
$evidence=Get-Content $evidencePath -Raw | ConvertFrom-Json
if($evidence.product_nro_sha256 -cne (Get-FileHash $nro).Hash.ToLowerInvariant()){throw 'The product NRO does not match the recorded emulator evidence'}
foreach($directory in @('runtime','apks','Android/data','Android/obb','DCIM','Download','installed','userdata','cache','logs')){New-Item -ItemType Directory -Force -Path "$sd/$directory" | Out-Null}
Copy-Item $nro "$sd/androswitch.nro"
foreach($name in @('CREDITS.md','NOTICE.md','LICENSE-MIT','LICENSE-APACHE')){Copy-Item "$projectRoot/$name" "$package/$name"}
Copy-Item "$projectRoot/docs/USING_ANDROSWITCH.md" "$package/READ-ME.md"
Copy-Item "$projectRoot/docs/HARDWARE_FEEDBACK.md" "$package/HARDWARE_FEEDBACK.md"
if(Test-Path "$projectRoot/docs/STARTUP_FAILURES.md"){
  $startup=Get-Content "$projectRoot/docs/STARTUP_FAILURES.md" -Raw
  $startup.Replace('(../reports/','(reports/') | Set-Content "$package/STARTUP_FAILURES.md" -Encoding utf8NoBOM
  New-Item -ItemType Directory -Force -Path "$package/reports" | Out-Null
  Copy-Item $evidencePath (Join-Path "$package/reports" ([IO.Path]::GetFileName($evidencePath)))
  if(Test-Path "$projectRoot/reports/mekorama-startup-20261024.json"){Copy-Item "$projectRoot/reports/mekorama-startup-20261024.json" "$package/reports/mekorama-startup-20261024.json"}
}
Copy-Item "$projectRoot/docs/QUICK_START_ID.md" "$package/MULAI-DI-SINI.md"
$quick=Get-Content "$package/MULAI-DI-SINI.md" -Raw
$quick.Replace('(USING_ANDROSWITCH.md)','(READ-ME.md)').Replace('(M6_M7_E2E.md)','(https://github.com/BlekjekSudo/androswitch/blob/main/docs/M6_M7_E2E.md)') | Set-Content "$package/MULAI-DI-SINI.md" -Encoding utf8NoBOM
New-Item -ItemType Directory -Force -Path "$package/licenses" | Out-Null
& "$PSScriptRoot/collect-product-licenses.ps1" -OutputRoot "$package/licenses/dependencies" | Out-Null
Copy-Item "$projectRoot/third_party/fonts/DejaVu.LICENSE" "$package/licenses/DejaVu.LICENSE"
Copy-Item "$projectRoot/third_party/so_util.LICENSE" "$package/licenses/so_util.LICENSE"
Copy-Item "$projectRoot/third_party/libnx-framebuffer.LICENSE" "$package/licenses/libnx-framebuffer.LICENSE"
Copy-Item "$projectRoot/third_party/README.md" "$package/licenses/SOURCE-NOTICES.md"
$samples=@()
if(-not $WithoutSamples){
  foreach($spec in @(@('m5/fixture/java-ui.apk','java-ui.apk'),@('m5/fixture/kotlin-ui.apk','kotlin-ui.apk'),@('m6/fixture/mixed-media.apk','mixed-media.apk'),@('m6/fixture/native-media.apk','native-media.apk'))){
    $source=Join-Path $projectRoot "local-artifacts/$($spec[0])";if(-not(Test-Path $source)){throw "Sample missing: $source"};Copy-Item $source "$sd/apks/$($spec[1])";$samples+=@{file="switch/androswitch/apks/$($spec[1])";sha256=(Get-FileHash $source).Hash.ToLowerInvariant()}
  }
  @"
Original AndroSwitch fixture sources are MIT OR Apache-2.0 in this repository:
https://github.com/BlekjekSudo/androswitch/tree/codex/androswitch-m5-m7/fixtures
Kotlin sample includes Kotlin stdlib 2.2.20, copyright JetBrains s.r.o. and Kotlin
Programming Language contributors, Apache-2.0. See the bundled LICENSE-APACHE.
The samples are normal SDK/NDK APKs, without package-specific runtime hooks.
"@ | Set-Content "$package/licenses/SAMPLE-APKS.txt" -Encoding utf8NoBOM
}
$commit=(& git -C $projectRoot rev-parse HEAD).Trim()
$manifest=@{schema_version=1;project='AndroSwitch';stage='M7 development alpha';source_commit=$commit;implementation_commit=$evidence.implementation_commit;source_url='https://github.com/BlekjekSudo/androswitch';upstream=@{project='RustyDroid';authors='nadabgfccvd and contributors';revision='6388281f66d254c8e01a0fdc7a5a9a30239bc762'};nro_sha256=(Get-FileHash $nro).Hash.ToLowerInvariant();verification=$evidence.verification;samples=$samples}
$manifest | ConvertTo-Json -Depth 10 | Set-Content "$package/package.json" -Encoding utf8NoBOM
$archive="$package.zip";[IO.Compression.ZipFile]::CreateFromDirectory($package,$archive)
@{schema_version=1;folder=$package;archive=$archive;sha256=(Get-FileHash $archive).Hash.ToLowerInvariant();manifest=$manifest} | ConvertTo-Json -Depth 12 | Set-Content "$output/latest.json" -Encoding utf8NoBOM
Write-Output "$output/latest.json"
