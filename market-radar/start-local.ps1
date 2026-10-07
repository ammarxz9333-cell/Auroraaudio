param([Parameter(Mandatory=$true)][string]$Python)
$ErrorActionPreference = 'Stop'
$radarRoot = $PSScriptRoot
$radarRuntime = Join-Path $radarRoot 'runtime'
New-Item -ItemType Directory -Force -Path $radarRuntime | Out-Null
$radarMutex = [Threading.Mutex]::new($false, 'Local\AmmarMarketRadarContinuous')
if (-not $radarMutex.WaitOne(0)) { exit 0 }
try {
    $env:PYTHONUNBUFFERED = '1'
    $env:RADAR_BIND = '127.0.0.1'
    $env:RADAR_PORT = '8787'
    $env:ALPACA_FEED = 'iex'
    $radarEnvFile = Join-Path $radarRoot '.env'
    if (Test-Path -LiteralPath $radarEnvFile) {
        foreach ($radarLine in Get-Content -LiteralPath $radarEnvFile) {
            if ($radarLine -match '^\s*([A-Z][A-Z0-9_]*)=(.*)$') {
                [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2].Trim().Trim('"').Trim("'"), 'Process')
            }
        }
    }
    # Keep the local endpoint private even if the environment file was copied from Docker.
    $env:RADAR_BIND = '127.0.0.1'
    while (-not (Test-Path -LiteralPath (Join-Path $radarRuntime 'STOP'))) {
        $radarStamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $radarProcess = Start-Process -FilePath $Python -ArgumentList '-m','continuous.service' -WorkingDirectory $radarRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $radarRuntime "$radarStamp.stdout.log") -RedirectStandardError (Join-Path $radarRuntime "$radarStamp.stderr.log")
        $radarProcess.Id | Set-Content -LiteralPath (Join-Path $radarRuntime 'service.pid')
        $radarProcess.WaitForExit()
        Start-Sleep -Seconds 10
    }
} finally { $radarMutex.ReleaseMutex(); $radarMutex.Dispose() }
