param(
    [string]$SimicsProject = 'C:\Users\ammar\simics-projects\aurora',
    [string]$PcmPath = '',
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (!$PcmPath) { $PcmPath = Join-Path $repoRoot 'artifacts/aurora-sim-windows/aurora-output-dsp-7.1.4.f32' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repoRoot 'artifacts/simics' }
$null = New-Item -ItemType Directory -Force $OutputDirectory
$env:AURORA_SIMICS_ROOT = $repoRoot
$env:AURORA_SIMICS_PCM = (Resolve-Path $PcmPath).Path
$env:AURORA_SIMICS_OUT = Join-Path (Resolve-Path $OutputDirectory).Path 'aurora-simics.json'
$env:AURORA_SIMICS_RUN_ID = [guid]::NewGuid().ToString()
$target = Join-Path $PSScriptRoot 'aurora.simics'
& (Join-Path $SimicsProject 'simics.bat') --batch-mode $target 2>&1 | Tee-Object -FilePath (Join-Path $OutputDirectory 'simics.log')
if ($LASTEXITCODE -ne 0) { throw "Simics exited with code $LASTEXITCODE" }
$report = Get-Content $env:AURORA_SIMICS_OUT -Raw | ConvertFrom-Json
if ($report.verdict -ne 'pass' -or $report.run_id -ne $env:AURORA_SIMICS_RUN_ID) {
    throw 'Simics acceptance report failed or belongs to an earlier run'
}
