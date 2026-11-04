param([switch]$DebugBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$configuration = if ($DebugBuild) { 'debug' } else { 'release' }
$exe = Join-Path $projectRoot "target\$configuration\mado-mail.exe"
$report = Join-Path $projectRoot 'target\color-check.json'
$started = Get-Date
$process = Start-Process -FilePath $exe -ArgumentList '--color-check' -PassThru -RedirectStandardOutput (Join-Path $projectRoot 'target\color-check.stdout.log') -RedirectStandardError (Join-Path $projectRoot 'target\color-check.stderr.log')
$null = $process.Handle
if (-not $process.WaitForExit(60000)) { throw "Synthetic color check timed out. Process $($process.Id) remains open for inspection." }
$process.WaitForExit()
if ($process.ExitCode -ne 0) { throw "Color check exited with code $($process.ExitCode). See target/color-check.stderr.log." }
if (-not (Test-Path $report) -or (Get-Item $report).LastWriteTime -lt $started) { throw 'Color check did not write a fresh report.' }
$result = Get-Content -Raw $report | ConvertFrom-Json
if (-not $result.ok) { throw $result.error }
$result | Format-List
