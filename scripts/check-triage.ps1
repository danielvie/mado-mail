param([switch]$ScrollbarOnly, [switch]$DebugBuild, [switch]$DetachOnly, [switch]$MiddleOnly, [switch]$SelectedActionsOnly, [switch]$SpacesOnly, [switch]$SpaceRemovalOnly, [switch]$LabelsOnly, [switch]$ReaderVisibilityOnly, [switch]$LabelRemovalOnly)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$configuration = if ($DebugBuild) { 'debug' } else { 'release' }
$exe = Join-Path $projectRoot "target\$configuration\mado-mail.exe"
$report = Join-Path $projectRoot $(if ($DetachOnly) { 'target\detach-check.json' } else { 'target\triage-check.json' })
$started = Get-Date
$arguments = @('--triage-check')
if ($ScrollbarOnly) { $arguments += '--scrollbar-only' }
if ($DetachOnly) { $arguments += '--detach-only' }
if ($MiddleOnly) { $arguments += '--middle-only' }
if ($SelectedActionsOnly) { $arguments += '--selected-actions-only' }
if ($SpacesOnly) { $arguments += '--spaces-only' }
if ($SpaceRemovalOnly) { $arguments += '--space-removal-only' }
if ($LabelsOnly) { $arguments += '--labels-only' }
if ($LabelRemovalOnly) { $arguments += '--label-removal-only' }
if ($ReaderVisibilityOnly) { $arguments += '--reader-visibility-only' }
$process = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru
if (-not $process.WaitForExit(60000)) { throw "Synthetic triage check timed out. Process $($process.Id) remains open for inspection." }
if ($process.ExitCode -ne 0) { throw "Triage check exited with code $($process.ExitCode)." }
if (-not (Test-Path $report) -or (Get-Item $report).LastWriteTime -lt $started) { throw 'Triage check did not write a fresh report.' }
$result = Get-Content -Raw $report | ConvertFrom-Json
if (-not $result.ok) { throw $result.error }
$result | Format-List
