param([Parameter(Mandatory)][string]$EvidenceDirectory, [switch]$Diagnostic, [string]$FixtureDirectory='local-artifacts/conformance')
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$evidence=(Get-Item -LiteralPath $EvidenceDirectory).FullName
$summary=Get-Content (Join-Path $evidence 'conformance-summary.json') -Raw|ConvertFrom-Json
$build=Get-Content "$repo/target/evidence/product-build.json" -Raw|ConvertFrom-Json
$fixture=[IO.Path]::GetFullPath($FixtureDirectory,$repo)
$apkBuild=Get-Content "$fixture/build-receipt.json" -Raw|ConvertFrom-Json
$nro="$repo/local-artifacts/product/androswitch.nro";$apk="$fixture/AndroSwitchConformance.apk"
if($summary.nro_sha256 -cne (Get-FileHash $nro).Hash.ToLowerInvariant() -or $build.nro_sha256 -cne $summary.nro_sha256){throw 'NRO differs from observed evidence'}
if($summary.apk_sha256 -cne (Get-FileHash $apk).Hash.ToLowerInvariant()){throw 'APK differs from observed evidence'}
if($summary.catalog_sha256 -cne (Get-FileHash "$fixture/catalog.json").Hash.ToLowerInvariant()){throw 'Catalog differs from observed evidence'}
if($summary.original_sd_restore -cne 'restored' -or -not $summary.launcher_completed){throw 'Completed launcher workflow and restored SD required for this test bundle'}
if(-not $Diagnostic -and -not $summary.acceptance_passed){throw 'Catalog acceptance is incomplete. Use -Diagnostic only for an explicitly unfinished hardware/debug test bundle.'}
$id=[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$folder=Join-Path $repo "local-artifacts/releases/androswitch-conformance-$id"
$sd=Join-Path $folder 'switch/androswitch'
foreach($name in @('apks','Android/data','Android/obb','DCIM','Download','installed','userdata','cache','logs')){New-Item -ItemType Directory -Force "$sd/$name"|Out-Null}
Copy-Item -LiteralPath $nro -Destination "$sd/androswitch.nro"
Copy-Item -LiteralPath $apk -Destination "$sd/apks/AndroSwitchConformance.apk"
foreach($name in @('CREDITS.md','NOTICE.md','LICENSE-MIT','LICENSE-APACHE')){Copy-Item -LiteralPath "$repo/$name" -Destination "$folder/$name"}
New-Item -ItemType Directory -Force "$folder/docs","$folder/evidence","$folder/licenses"|Out-Null
Copy-Item "$repo/docs/CONFORMANCE_SUITE.md" "$folder/docs/CONFORMANCE_SUITE.md"
New-Item -ItemType Directory -Force "$folder/docs/images","$folder/reports"|Out-Null
Copy-Item -LiteralPath "$repo/docs/images/conformance-sdk.png" -Destination "$folder/docs/images/conformance-sdk.png"
foreach($name in @('conformance-mesa-20261025.json','conformance-hardware-20261025.json','m9-mesa-independent-apps-20261025.json')){
    $reportPath=Join-Path $repo "reports/$name"
    if(Test-Path -LiteralPath $reportPath){Copy-Item -LiteralPath $reportPath -Destination "$folder/reports/$name"}
}
$suiteReadme=[IO.File]::ReadAllText("$repo/fixtures/conformance/README.md").Replace('../../CREDITS.md','../CREDITS.md').Replace('../../docs/CONFORMANCE_SUITE.md','CONFORMANCE_SUITE.md')
[IO.File]::WriteAllText("$folder/docs/SUITE-SOURCE-README.md",$suiteReadme)
Copy-Item -LiteralPath "$fixture/catalog.json" -Destination "$folder/evidence/catalog.json"
Copy-Item -LiteralPath "$fixture/jvm-oracle.tsv" -Destination "$folder/evidence/jvm-oracle.tsv"
Copy-Item -LiteralPath "$fixture/signature-verification.log" -Destination "$folder/evidence/signature-verification.log"
Copy-Item -LiteralPath "$fixture/build-receipt.json" -Destination "$folder/evidence/apk-build.json"
Copy-Item "$repo/target/evidence/product-build.json" "$folder/evidence/nro-build.json"
if($build.source_manifest_sha256){
    $sourceManifest="$repo/target/evidence/product-source.json"
    if((Get-FileHash $sourceManifest).Hash.ToLowerInvariant() -cne $build.source_manifest_sha256){throw 'Runtime source snapshot differs from build receipt'}
    Copy-Item -LiteralPath $sourceManifest -Destination "$folder/evidence/nro-source.json"
}
if($build.mesa_sdk){
    Copy-Item -LiteralPath "$repo/docs/MESA_SWITCH_PORT.md" -Destination "$folder/docs/MESA_SWITCH_PORT.md"
    Copy-Item -LiteralPath "$repo/local-artifacts/dependencies/mesa-switch-0ff52079/receipt.json" -Destination "$folder/evidence/mesa-sdk.json"
}
foreach($name in @('conformance-report.json','conformance-summary.json','verification.json')){Copy-Item -LiteralPath (Join-Path $evidence $name) -Destination (Join-Path "$folder/evidence" $name)}
& "$PSScriptRoot/collect-product-licenses.ps1" -OutputRoot "$folder/licenses/dependencies"|Out-Null
foreach($name in @('fonts/DejaVu.LICENSE','so_util.LICENSE','libnx-framebuffer.LICENSE')){Copy-Item -LiteralPath "$repo/third_party/$name" -Destination (Join-Path "$folder/licenses" ([IO.Path]::GetFileName($name)))}
@'
# AndroSwitch — satu APK pengujian

Salin folder `switch` ke root SD, gabungkan dengan folder yang sudah ada, lalu
buka AndroSwitch melalui title/application override. Install dan buka
`AndroSwitchConformance.apk`, lalu pilih **Run all / resume**.

__CASE_COUNT__ kasus tanpa sensor. Daftar, keluarga API, dan hasil aktual disertakan
di evidence; jumlah kasus mengikuti katalog APK yang diuji.
Jika runtime keluar karena API belum tersedia, buka APK lagi agar lanjut ke
kasus berikutnya. Kasus itu disimpan sebagai INTERRUPTED, bukan lulus.
Pilih kategori untuk pengujian terarah. Reset results mengarsipkan hasil lama;
gunakan reset saat menguji runtime baru. Export report menyimpan JSON ke
`switch/androswitch/Download/AndroSwitch-Conformance/report.json`. Salin juga
folder logs saat melaporkan crash. Data internal ada di
`userdata/org.androswitch.conformance/files/conformance/`.

Tombol renderer memilih kelompok tes GLES/Vulkan, bukan mengganti driver.
Vulkan saat ini menguji penemuan loader/perangkat, belum presentasi frame.
Build Android adalah identitas kompatibilitas virtual; clock/core Horizon dan
identitas GPU dicatat lewat query terpisah. Dukungan yang belum tersedia tetap
terlihat dalam JSON. Ini development test bundle, bukan kelulusan CTS, seluruh
Android, game tertentu, atau rilis M9 selesai. Lihat evidence untuk hasil
emulator. __HARDWARE_STATUS__ Android referensi belum diuji.

AndroSwitch dibangun di atas RustyDroid oleh nadabgfccvd dan kontributornya:
https://github.com/nadabgfccvd/RustyDroid
Riwayat, atribusi, dan lisensi dipertahankan. Lihat CREDITS.md dan NOTICE.md.
Source runtime dan suite: https://github.com/BlekjekSudo/androswitch
'@|Set-Content "$folder/MULAI-DI-SINI.md" -Encoding utf8NoBOM
$hardwarePath=Join-Path $repo 'reports/conformance-hardware-20261025.json'
$hardware=if(Test-Path $hardwarePath){Get-Content $hardwarePath -Raw|ConvertFrom-Json}else{$null}
$hardwareMatches=$hardware -and $hardware.status -ceq 'passed_user_reported' -and $hardware.reported_counts.PASS -eq $summary.total -and $hardware.package_context.expected_nro_sha256 -ceq $summary.nro_sha256 -and $hardware.package_context.expected_apk_sha256 -ceq $summary.apk_sha256 -and $hardware.package_context.expected_catalog_sha256 -ceq $summary.catalog_sha256
$hardwareText=if($hardwareMatches){'Pengguna mengonfirmasi suite ini 219/219 PASS pada Switch fisik; laporan agregat dan identitas paket ada di reports/conformance-hardware-20261025.json. Raw journal perangkat belum diarsipkan.'}else{'Build ini belum memiliki hasil pengujian Switch fisik dengan identitas paket yang cocok.'}
$startPath=Join-Path $folder 'MULAI-DI-SINI.md'
[IO.File]::WriteAllText($startPath,([IO.File]::ReadAllText($startPath)).Replace('__HARDWARE_STATUS__',$hardwareText).Replace('__CASE_COUNT__',[string]$summary.total),[Text.UTF8Encoding]::new($false))
& python -X utf8 "$PSScriptRoot/prepare-package-docs.py" $folder $build.source_commit
if($LASTEXITCODE){throw 'Packaged documentation link preparation failed'}
$manifest=@{
    schema_version=1;kind='development_conformance_bundle';total_cases=$summary.total;acceptance_passed=[bool]$summary.acceptance_passed;diagnostic=[bool]$Diagnostic
    source_commit=$build.source_commit;apk_source_commit=$apkBuild.source_commit;runtime_source_manifest_sha256=$build.source_manifest_sha256;upstream=@{project='RustyDroid';authors='nadabgfccvd and contributors';revision='6388281f66d254c8e01a0fdc7a5a9a30239bc762';synced_revision='3761815dc6a2a1427e9fc66b66e8471b78d1b170'}
    apk_sha256=$summary.apk_sha256;nro_sha256=$summary.nro_sha256;catalog_sha256=$summary.catalog_sha256
    emulator_counts=$summary.counts;emulator_acceptance_passed=[bool]$summary.acceptance_passed;hardware=if($hardwareMatches){$hardware.status}else{'not_run'};hardware_reported_counts=if($hardwareMatches){$hardware.reported_counts}else{$null};hardware_raw_journal_received=if($hardwareMatches){$hardware.verification_limits.raw_device_journal_received}else{$false};android_reference='not_run';m9_independent_game_gate='open'
    files=@(Get-ChildItem $folder -Recurse -File|ForEach-Object {@{path=[IO.Path]::GetRelativePath($folder,$_.FullName).Replace('\','/');bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
}
$manifest|ConvertTo-Json -Depth 15|Set-Content "$folder/package.json" -Encoding utf8NoBOM
$zip="$folder.zip";[IO.Compression.ZipFile]::CreateFromDirectory($folder,$zip)
$archive=[IO.Compression.ZipFile]::OpenRead($zip)
try {
    foreach($entry in $archive.Entries){
        if($entry.FullName -match '\.(p12|jks|keystore|keys)$'){throw 'Unexpected private signing/system key in package'}
        $expected=if($entry.FullName -ieq 'switch/androswitch/apks/AndroSwitchConformance.apk'){$summary.apk_sha256}elseif($entry.FullName -ieq 'switch/androswitch/androswitch.nro'){$summary.nro_sha256}else{$null}
        if($expected){$stream=$entry.Open();try{$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()}finally{$stream.Dispose()};if($hash -cne $expected){throw "ZIP checksum mismatch: $($entry.FullName)"}}
    }
}finally{$archive.Dispose()}
@{schema_version=1;folder=$folder;archive=$zip;sha256=(Get-FileHash $zip).Hash.ToLowerInvariant();manifest=$manifest}|ConvertTo-Json -Depth 16|Set-Content "$repo/local-artifacts/releases/latest-conformance.json" -Encoding utf8NoBOM
Write-Output "$repo/local-artifacts/releases/latest-conformance.json"
