param([Parameter(Mandatory)][string]$EvidenceDirectory, [switch]$Diagnostic)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$evidence=(Get-Item -LiteralPath $EvidenceDirectory).FullName
$summary=Get-Content (Join-Path $evidence 'conformance-summary.json') -Raw|ConvertFrom-Json
$build=Get-Content "$repo/target/evidence/product-build.json" -Raw|ConvertFrom-Json
$apkBuild=Get-Content "$repo/local-artifacts/conformance/build-receipt.json" -Raw|ConvertFrom-Json
$nro="$repo/local-artifacts/product/androswitch.nro";$apk="$repo/local-artifacts/conformance/AndroSwitchConformance.apk"
if($summary.nro_sha256 -cne (Get-FileHash $nro).Hash.ToLowerInvariant() -or $build.nro_sha256 -cne $summary.nro_sha256){throw 'NRO differs from observed evidence'}
if($summary.apk_sha256 -cne (Get-FileHash $apk).Hash.ToLowerInvariant()){throw 'APK differs from observed evidence'}
if($summary.original_sd_restore -cne 'restored' -or -not $summary.launcher_completed){throw 'Completed launcher workflow and restored SD required for this test bundle'}
if(-not $Diagnostic -and -not $summary.acceptance_passed){throw '219-case acceptance is incomplete. Use -Diagnostic only for an explicitly unfinished hardware/debug test bundle.'}
$id=[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
$folder=Join-Path $repo "local-artifacts/releases/androswitch-conformance-$id"
$sd=Join-Path $folder 'switch/androswitch'
foreach($name in @('apks','Android/data','Android/obb','DCIM','Download','installed','userdata','cache','logs')){New-Item -ItemType Directory -Force "$sd/$name"|Out-Null}
Copy-Item -LiteralPath $nro -Destination "$sd/androswitch.nro"
Copy-Item -LiteralPath $apk -Destination "$sd/apks/AndroSwitchConformance.apk"
foreach($name in @('CREDITS.md','NOTICE.md','LICENSE-MIT','LICENSE-APACHE')){Copy-Item -LiteralPath "$repo/$name" -Destination "$folder/$name"}
New-Item -ItemType Directory -Force "$folder/docs","$folder/evidence","$folder/licenses"|Out-Null
Copy-Item "$repo/docs/CONFORMANCE_SUITE.md" "$folder/docs/CONFORMANCE_SUITE.md"
$suiteReadme=[IO.File]::ReadAllText("$repo/fixtures/conformance/README.md").Replace('../../CREDITS.md','../CREDITS.md').Replace('../../docs/CONFORMANCE_SUITE.md','CONFORMANCE_SUITE.md')
[IO.File]::WriteAllText("$folder/docs/SUITE-SOURCE-README.md",$suiteReadme)
Copy-Item "$repo/local-artifacts/conformance/catalog.json" "$folder/evidence/catalog.json"
Copy-Item "$repo/local-artifacts/conformance/jvm-oracle.tsv" "$folder/evidence/jvm-oracle.tsv"
Copy-Item "$repo/local-artifacts/conformance/signature-verification.log" "$folder/evidence/signature-verification.log"
Copy-Item "$repo/local-artifacts/conformance/build-receipt.json" "$folder/evidence/apk-build.json"
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

219 kasus tanpa sensor: 97 Java/DEX, 89 Android/platform dan 33 JNI/native/grafik.
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
emulator; build ini belum diverifikasi pada Switch fisik atau Android referensi.

AndroSwitch dibangun di atas RustyDroid oleh nadabgfccvd dan kontributornya:
https://github.com/nadabgfccvd/RustyDroid
Riwayat, atribusi, dan lisensi dipertahankan. Lihat CREDITS.md dan NOTICE.md.
Source runtime dan suite: https://github.com/BlekjekSudo/androswitch
'@|Set-Content "$folder/MULAI-DI-SINI.md" -Encoding utf8NoBOM
$manifest=@{
    schema_version=1;kind='development_conformance_bundle';total_cases=$summary.total;acceptance_passed=[bool]$summary.acceptance_passed;diagnostic=[bool]$Diagnostic
    source_commit=$build.source_commit;apk_source_commit=$apkBuild.source_commit;runtime_source_manifest_sha256=$build.source_manifest_sha256;upstream=@{project='RustyDroid';authors='nadabgfccvd and contributors';revision='6388281f66d254c8e01a0fdc7a5a9a30239bc762'}
    apk_sha256=$summary.apk_sha256;nro_sha256=$summary.nro_sha256;catalog_sha256=$summary.catalog_sha256
    emulator_counts=$summary.counts;hardware='not_run';android_reference='not_run';m9_independent_game_gate='open'
    files=@(Get-ChildItem $folder -Recurse -File|ForEach-Object {@{path=[IO.Path]::GetRelativePath($folder,$_.FullName).Replace('\','/');bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})
}
$manifest|ConvertTo-Json -Depth 15|Set-Content "$folder/package.json" -Encoding utf8NoBOM
$zip="$folder.zip";[IO.Compression.ZipFile]::CreateFromDirectory($folder,$zip)
$archive=[IO.Compression.ZipFile]::OpenRead($zip)
try {
    foreach($entry in $archive.Entries){
        if($entry.FullName -match '\.(p12|jks|keystore|keys)$'){throw 'Unexpected private signing/system key in package'}
        if($entry.FullName -ieq 'switch/androswitch/apks/AndroSwitchConformance.apk'){$stream=$entry.Open();try{$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()}finally{$stream.Dispose()};if($hash -cne $summary.apk_sha256){throw 'ZIP APK checksum mismatch'}}
    }
}finally{$archive.Dispose()}
@{schema_version=1;folder=$folder;archive=$zip;sha256=(Get-FileHash $zip).Hash.ToLowerInvariant();manifest=$manifest}|ConvertTo-Json -Depth 16|Set-Content "$repo/local-artifacts/releases/latest-conformance.json" -Encoding utf8NoBOM
Write-Output "$repo/local-artifacts/releases/latest-conformance.json"
