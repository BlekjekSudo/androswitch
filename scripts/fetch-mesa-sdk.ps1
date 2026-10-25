# Pinned official CI SDK plus its exact compiler runtime, all task-local.
param([string]$GitHubCli='C:/Program Files/GitHub CLI/gh.exe', [switch]$IncludeSource)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$root=Join-Path $repo 'local-artifacts/dependencies/mesa-switch-0ff52079'
New-Item -ItemType Directory -Force $root|Out-Null
if($IncludeSource -and -not(Test-Path "$root/source/docs/license.rst")){
    & git clone --no-checkout --filter=blob:none https://github.com/danfromtico/mesa-switch.git "$root/source"
    if($LASTEXITCODE){throw 'Mesa source clone failed'}
    & git -C "$root/source" checkout --detach 0ff52079ca9e9e837ed27eb95a8d239f439a1864
    if($LASTEXITCODE){throw 'Pinned Mesa source checkout failed'}
}
$metadata=& $GitHubCli api repos/danfromtico/mesa-switch/actions/artifacts/11781503719 | ConvertFrom-Json
if($LASTEXITCODE -or $metadata.expired -or $metadata.workflow_run.head_sha -cne '0ff52079ca9e9e837ed27eb95a8d239f439a1864'){throw 'Pinned Mesa CI artifact unavailable or source identity differs'}
if(-not(Test-Path "$root/sdk/opt/devkitpro/portlibs/switch/lib/libvulkan.a")){
    & $GitHubCli run download 38329414110 --repo danfromtico/mesa-switch --name mesa-26.2.4-switch-unified-horizon-sdk --dir "$root/sdk"
    if($LASTEXITCODE){throw 'GitHub CLI SDK download failed'}
}
$metadata|ConvertTo-Json -Depth 8|Set-Content "$root/artifact-metadata.json" -Encoding utf8NoBOM
# Rust SDK archives require these exact core/alloc/std symbols. This does not
# change the application's Rust toolchain or install std on its Switch target.
$manifestUrl='https://static.rust-lang.org/dist/2026-10-07/channel-rust-nightly.toml'
$runtimeUrl='https://static.rust-lang.org/dist/2026-10-07/rust-std-nightly-aarch64-unknown-linux-gnu.tar.xz'
$lock=Get-Content "$repo/third_party/mesa-switch-rust.lock.json" -Raw|ConvertFrom-Json
if($lock.url -cne $runtimeUrl -or $lock.compiler -cne '1.101.0-nightly (8d1a76430 2026-10-06)'){throw 'Unexpected shader compiler runtime lock'}
$archive="$root/rust-std-sdk.tar.xz"
if(-not(Test-Path $archive) -or (Get-FileHash $archive).Hash.ToLowerInvariant() -cne $lock.sha256){
    Invoke-WebRequest -Uri $runtimeUrl -OutFile $archive -TimeoutSec 300
}
if((Get-FileHash $archive).Hash.ToLowerInvariant() -cne $lock.sha256){throw 'Pinned Rust SDK runtime digest mismatch'}
& python -X utf8 "$repo/scripts/unpack-mesa-runtime.py" --archive $archive --destination "$root/rust-std"
if($LASTEXITCODE){throw 'Rust SDK runtime extraction failed'}
@{schema_version=1;source_revision=$metadata.workflow_run.head_sha;workflow_run=$metadata.workflow_run.id;artifact_id=$metadata.id;upstream_archive_digest=$metadata.digest;transport='gh run download; official artifact API';local_archive_digest_verified=$false;runtime_lock_sha256=(Get-FileHash "$repo/third_party/mesa-switch-rust.lock.json").Hash.ToLowerInvariant();files=@(Get-ChildItem "$root/sdk" -Recurse -File|ForEach-Object {@{path=[IO.Path]::GetRelativePath("$root/sdk",$_.FullName).Replace('\','/');bytes=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}})}|ConvertTo-Json -Depth 8|Set-Content "$root/receipt.json" -Encoding utf8NoBOM
Write-Output "$root/receipt.json"
