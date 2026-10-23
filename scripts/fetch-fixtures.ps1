param([string]$Id = 'fdroid-inspect', [switch]$ExtractDex)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$fixtureLock = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'fixtures/corpus.lock.json') | ConvertFrom-Json
$fixture = @($fixtureLock.fixtures | Where-Object { $_.id -eq $Id })
if ($fixture.Count -ne 1 -or $fixture[0].state -ne 'apk_frozen') { throw "Fixture '$Id' has no frozen APK download." }
$fixture = $fixture[0]
if ([IO.Path]::GetFileName($fixture.file) -ne $fixture.file) { throw 'Fixture filename must be a basename.' }
$cacheDir = Join-Path $projectRoot 'fixtures/cache'
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null
$outputPath = Join-Path $cacheDir $fixture.file
function Test-Fixture([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    if ((Get-Item -LiteralPath $Path).Length -ne $fixture.bytes) { return $false }
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant() -eq $fixture.sha256
}
if (-not (Test-Fixture $outputPath)) {
    if (Test-Path -LiteralPath $outputPath) { throw "Cached fixture checksum mismatch: $outputPath" }
    $partialPath = "$outputPath.partial"
    try {
        Invoke-WebRequest -Uri $fixture.source_url -OutFile $partialPath
        if (-not (Test-Fixture $partialPath)) { throw 'Downloaded fixture failed its frozen size/SHA-256 checks.' }
        Move-Item -LiteralPath $partialPath -Destination $outputPath
    } finally {
        if (Test-Path -LiteralPath $partialPath) { Remove-Item -LiteralPath $partialPath }
    }
}
if ($ExtractDex) {
    if ($null -eq $fixture.dex -or [IO.Path]::GetFileName($fixture.dex.file) -ne $fixture.dex.file) { throw 'Fixture has no valid pinned DEX entry.' }
    $dexPath = Join-Path $cacheDir $fixture.dex.file
    $archive = [IO.Compression.ZipFile]::OpenRead($outputPath)
    try {
        $entry = $archive.GetEntry($fixture.dex.entry)
        if ($null -eq $entry -or $entry.Length -ne $fixture.dex.bytes -or $entry.Length -gt 64MB) { throw 'Reference DEX length mismatch.' }
        $inputStream = $entry.Open()
        try {
            $outputStream = [IO.File]::Create($dexPath)
            try { $inputStream.CopyTo($outputStream) } finally { $outputStream.Dispose() }
        } finally { $inputStream.Dispose() }
    } finally { $archive.Dispose() }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $dexPath).Hash.ToLowerInvariant() -ne $fixture.dex.sha256) { throw 'Reference DEX checksum mismatch.' }
}
[ordered]@{id=$Id;path=$outputPath;bytes=$fixture.bytes;sha256=$fixture.sha256} | ConvertTo-Json
