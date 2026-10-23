param(
    [Parameter(Mandatory)][string]$ReportPath,
    [ValidateSet('horizon_emulator','physical_device')][string]$Backend = 'horizon_emulator'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$receiptPath = Join-Path $projectRoot 'local-artifacts/m1/smoke-receipt.json'
$receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
$report = Get-Content -LiteralPath $ReportPath -Raw | ConvertFrom-Json
if ($report.schema_version -ne 1 -or $report.kind -ne 'dex_parser_smoke') { throw 'Unknown smoke report format.' }
if ($report.parse_result -ne 0 -or $report.allocator_checksum -ne 32640 -or $report.apk_execution -ne $false) { throw 'Smoke parse/allocation/execution-status check failed.' }
foreach ($field in @('version','file_size','classes','methods','strings','checksum_ok')) {
    if ($null -eq $report.$field -or $report.$field -ne $receipt.host_expected.$field) { throw "Smoke comparison failed: $field" }
}
if ($report.checksum_ok -ne 1) { throw 'The reference DEX checksum was not verified.' }
$result = [ordered]@{schema_version=1;backend=$Backend;status='passed';report_path=[IO.Path]::GetFullPath($ReportPath);report_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $ReportPath).Hash.ToLowerInvariant();receipt=$receipt;observed=$report}
$resultPath = Join-Path $projectRoot "local-artifacts/m1/$Backend-verification.json"
$result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $resultPath -Encoding utf8
$result | ConvertTo-Json -Depth 10
