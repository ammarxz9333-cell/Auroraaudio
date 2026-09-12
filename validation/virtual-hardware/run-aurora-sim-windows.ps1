[CmdletBinding()]
param(
    [string]$OutputDir,
    [string]$CacheDir,
    [switch]$SkipFaultProfiles
)

$ErrorActionPreference = 'Stop'
$Runner = Join-Path $PSScriptRoot 'run_aurora_sim_windows.py'
if (-not (Test-Path $Runner)) {
    throw "Missing Windows AuroraSim runner: $Runner"
}

$Python = Get-Command python -ErrorAction SilentlyContinue
if (-not $Python) {
    throw 'Python is required. Install Python 3.13: winget install -e --id Python.Python.3.13'
}

$Arguments = @($Runner)
if ($OutputDir) { $Arguments += @('--output-dir', $OutputDir) }
if ($CacheDir) { $Arguments += @('--cache-dir', $CacheDir) }
if ($SkipFaultProfiles) { $Arguments += '--skip-fault-profiles' }

& $Python.Source @Arguments
exit $LASTEXITCODE
