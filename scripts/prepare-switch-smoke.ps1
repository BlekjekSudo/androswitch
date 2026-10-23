$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$null = & (Join-Path $PSScriptRoot 'fetch-fixtures.ps1')
$fixtureLock = Get-Content -LiteralPath (Join-Path $projectRoot 'fixtures/corpus.lock.json') -Raw | ConvertFrom-Json
$fixture = $fixtureLock.fixtures | Where-Object { $_.id -eq 'fdroid-inspect' }
$apkPath = Join-Path $projectRoot "fixtures/cache/$($fixture.file)"
$nroPath = Join-Path $projectRoot 'local-artifacts/m1/androswitch-smoke.nro'
if (-not (Test-Path -LiteralPath $nroPath -PathType Leaf)) { throw 'Build the smoke NRO with scripts/build-switch-smoke.ps1 first.' }
$packageRoot = Join-Path $projectRoot 'local-artifacts/m1/package'
$smokeDir = Join-Path $packageRoot 'switch/androswitch-smoke'
New-Item -ItemType Directory -Force -Path $smokeDir | Out-Null
$dexPath = Join-Path $smokeDir 'classes.dex'
$archive = [IO.Compression.ZipFile]::OpenRead($apkPath)
try {
    $entry = $archive.GetEntry('classes.dex')
    if ($null -eq $entry -or $entry.Length -gt 64MB) { throw 'Missing or oversized reference DEX.' }
    $inputStream = $entry.Open()
    try {
        $outputStream = [IO.File]::Create($dexPath)
        try { $inputStream.CopyTo($outputStream) } finally { $outputStream.Dispose() }
    } finally { $inputStream.Dispose() }
} finally { $archive.Dispose() }
Copy-Item -LiteralPath $nroPath -Destination (Join-Path $smokeDir 'androswitch-smoke.nro')
Push-Location -LiteralPath $projectRoot
try {
    $expectedJson = & cargo run --quiet -p as-cli -- dex-summary $dexPath --json
    if ($LASTEXITCODE -ne 0) { throw 'Host reference DEX parse failed.' }
    $expected = ($expectedJson -join "`n") | ConvertFrom-Json
} finally { Pop-Location }
$receipt = [ordered]@{
    schema_version=1
    kind='partial_m1_dex_parser_smoke'
    source_apk_sha256=$fixture.sha256
    dex_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $dexPath).Hash.ToLowerInvariant()
    nro_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $nroPath).Hash.ToLowerInvariant()
    host_expected=$expected
    verification=[ordered]@{host_dex_parse='passed';horizon_emulator='pending';physical_device='pending'}
    apk_execution=$false
}
$receiptPath = Join-Path $projectRoot 'local-artifacts/m1/smoke-receipt.json'
$receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding utf8
Get-Item -LiteralPath $receiptPath, $smokeDir
