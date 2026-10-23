param([int]$Cycles=12)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try{
  & python "$PSScriptRoot/fetch-m9-corpus.py"
  if($LASTEXITCODE){throw 'Independent corpus verification failed'}
  $env:CARGO_BUILD_JOBS='1'
  & cargo build -p as-framework --features host --bin as-corpus --locked
  if($LASTEXITCODE){throw 'Host corpus tool build failed'}
  $id=[DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8)
  $output="$projectRoot/local-artifacts/m9/host-$id"
  New-Item -ItemType Directory -Force $output | Out-Null
  # Owned console-only child; never enumerates, focuses, hides or closes other
  # applications. Desktop emulator execution is a separate explicit command.
  $info=[Diagnostics.ProcessStartInfo]::new()
  $info.FileName="$projectRoot/target/debug/as-corpus.exe"
  $info.WorkingDirectory=$projectRoot;$info.UseShellExecute=$false;$info.CreateNoWindow=$true
  $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
  foreach($argument in @('fixtures/m9/host-corpus.json',"$output/storage","$output/corpus.json",[string]$Cycles)){$info.ArgumentList.Add($argument)}
  $process=[Diagnostics.Process]::new();$process.StartInfo=$info
  $timer=[Diagnostics.Stopwatch]::StartNew();$peak=0L
  if(-not $process.Start()){throw 'Host corpus process did not start'}
  $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
  while(-not $process.WaitForExit(100)){$process.Refresh();try{$peak=[Math]::Max($peak,$process.PeakWorkingSet64)}catch{}}
  $timer.Stop()
  [IO.File]::WriteAllText("$output/stdout.log",$stdout.GetAwaiter().GetResult())
  [IO.File]::WriteAllText("$output/stderr.log",$stderr.GetAwaiter().GetResult())
  $exitCode=$process.ExitCode;$process.Dispose()
  @{schema_version=1;run_id=$id;profile='Cargo dev, unoptimized';command='as-corpus fixtures/m9/host-corpus.json <fresh-storage> <report> '+$Cycles;exit_code=$exitCode;elapsed_seconds=$timer.Elapsed.TotalSeconds;sampled_process_peak_working_set_bytes=$peak;sampling_ms=100;scope='Entire Windows host corpus process; includes managed heap, parser, Rust/C state and OS allocations; not device memory';corpus_sha256=if(Test-Path "$output/corpus.json"){(Get-FileHash "$output/corpus.json").Hash.ToLowerInvariant()}else{$null};tool_sha256=(Get-FileHash $info.FileName).Hash.ToLowerInvariant()}|ConvertTo-Json -Depth 5|Set-Content "$output/verification.json" -Encoding utf8NoBOM
  @{run=$output;verification="$output/verification.json"}|ConvertTo-Json|Set-Content "$projectRoot/local-artifacts/m9/latest-host.json" -Encoding utf8NoBOM
  if($exitCode){Get-Content "$output/stderr.log";throw 'Required host corpus case failed'}
  Write-Output "$output/verification.json"
}finally{Pop-Location}
