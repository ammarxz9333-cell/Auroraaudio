[CmdletBinding()]
param(
    [string]$OutputDir,
    [string]$CacheDir,
    [switch]$SkipFaultProfiles
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $OutputDir) { $OutputDir = Join-Path $Root 'artifacts\aurora-sim-windows' }
if (-not $CacheDir) { $CacheDir = Join-Path $Root '.cache\aurora-sim-windows' }
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)
$CacheDir = [System.IO.Path]::GetFullPath($CacheDir)

$Manifest = Join-Path $Root 'config\external-components-v1.json'
$Patch = Join-Path $Root 'validation\immersive\omniphony-v0.5.2-low-latency-stdout.patch'
$MovingAnalyzer = Join-Path $Root 'validation\immersive\aurora_joc_moving_evidence.py'
$VirtualAnalyzer = Join-Path $Root 'validation\virtual-hardware\aurora_full_system_sim.py'
$Pacer = Join-Path $Root 'validation\virtual-hardware\pace_orender.py'
$TelemetrySource = Join-Path $Root 'validation\virtual-hardware\aurora_moving_telemetry.rs'

$DolbyUrl = 'https://ott.dolby.com/OnDelKits/DDP/Dolby_Digital_Plus_Online_Delivery_Kit_v1.4.1/Test_Signals/elementary_streams/audio.zip'
$DolbyName = 'Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3'
$ExpectedZipSha = 'f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62'
$ExpectedSourceSha = '2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f'
$ExpectedDerivedSha = '0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0'
$ExpectedFirstAuBytes = 2560

$HarlettyUrl = 'https://github.com/harletty/harletty-bridge/releases/download/v0.7.4/harletty-bridge-v0.7.4-windows-x86_64.zip'
$HarlettyZipSha = '3ed126e5bb837882c5c2abbc5d35d1ebede199f81ed64127fb968bfe86bd6686'
$AsioCommit = '496a0765b8bb9c26f764f22f9a9712a937177db2'
$AsioUrl = "https://github.com/audiosdk/asio/archive/$AsioCommit.zip"

function Phase([string]$Text) {
    Write-Host "`n== AuroraSim Windows: $Text ==" -ForegroundColor Cyan
}

function Need-Command([string]$Name, [string]$InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Missing required command '$Name'. $InstallHint"
    }
}

function Invoke-Native([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $Exe $($Arguments -join ' ')"
    }
}

function Sha256([string]$Path) {
    return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Assert-Sha([string]$Path, [string]$Expected, [string]$Label) {
    $actual = Sha256 $Path
    if ($actual -ne $Expected) {
        throw "$Label SHA-256 mismatch: expected $Expected got $actual"
    }
}

function Download-Verified([string]$Url, [string]$Destination, [string]$Sha, [string]$Label) {
    if (Test-Path $Destination) {
        try {
            Assert-Sha $Destination $Sha $Label
            return
        } catch {
            Remove-Item -Force $Destination
        }
    }
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Destination
    Assert-Sha $Destination $Sha $Label
}

function Clone-Pinned([string]$Url, [string]$Version, [string]$Commit, [string]$Destination) {
    if (Test-Path (Join-Path $Destination '.git')) {
        Invoke-Native 'git' @('-C', $Destination, 'fetch', '--depth', '1', 'origin', $Version)
        Invoke-Native 'git' @('-C', $Destination, 'reset', '--hard', $Commit)
        Invoke-Native 'git' @('-C', $Destination, 'clean', '-fdx')
    } else {
        if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
        Invoke-Native 'git' @('clone', '--quiet', '--depth', '1', '--branch', $Version, $Url, $Destination)
    }
    $actual = (& git -C $Destination rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -ne $Commit) {
        throw "Pinned commit mismatch for $Url: expected $Commit got $actual"
    }
}

Need-Command 'git' 'Install Git for Windows (winget install -e --id Git.Git).'
Need-Command 'python' 'Install Python (winget install -e --id Python.Python.3.13).'
Need-Command 'ffmpeg' 'Install FFmpeg and reopen the terminal.'
Need-Command 'rustup' 'Install Rustup (winget install -e --id Rustlang.Rustup).'
Need-Command 'cargo' 'Install Rustup and reopen the terminal.'
Need-Command 'git' ''

foreach ($required in @($Manifest, $Patch, $MovingAnalyzer, $VirtualAnalyzer, $Pacer, $TelemetrySource)) {
    if (-not (Test-Path $required)) { throw "Missing Aurora validation dependency: $required" }
}

New-Item -ItemType Directory -Force -Path $OutputDir, $CacheDir | Out-Null
$Work = Join-Path $OutputDir 'work'
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force -Path $Work | Out-Null

$config = Get-Content -Raw -Encoding UTF8 $Manifest | ConvertFrom-Json
$components = @{}
foreach ($component in $config.components) { $components[$component.id] = $component }
$omnip = $components['omniphony']
if (-not $omnip) { throw 'Omniphony pin missing from external-components-v1.json' }

Phase 'prepare pinned Windows renderer dependencies'
Invoke-Native 'rustup' @('toolchain', 'install', 'stable', '--profile', 'minimal')

$AsioZip = Join-Path $CacheDir "asio-$AsioCommit.zip"
$AsioRoot = Join-Path $CacheDir 'asio-sdk'
$AsioDir = Join-Path $AsioRoot "asio-$AsioCommit"
if (-not (Test-Path (Join-Path $AsioDir 'common\asio.h'))) {
    if (Test-Path $AsioRoot) { Remove-Item -Recurse -Force $AsioRoot }
    New-Item -ItemType Directory -Force -Path $AsioRoot | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri $AsioUrl -OutFile $AsioZip
    Expand-Archive -Force -Path $AsioZip -DestinationPath $AsioRoot
}
foreach ($header in @('common\asio.h', 'common\asiosys.h', 'host\asiodrivers.h')) {
    if (-not (Test-Path (Join-Path $AsioDir $header))) { throw "ASIO SDK is missing $header" }
}
$env:CPAL_ASIO_DIR = $AsioDir

if (-not $env:LIBCLANG_PATH) {
    $llvmCandidates = @(
        'C:\Program Files\LLVM\bin',
        'C:\Program Files (x86)\LLVM\bin'
    )
    foreach ($candidate in $llvmCandidates) {
        if (Test-Path (Join-Path $candidate 'libclang.dll')) {
            $env:LIBCLANG_PATH = $candidate
            break
        }
    }
}
if (-not $env:LIBCLANG_PATH) {
    Write-Warning 'LIBCLANG_PATH is not set. If the Omniphony build reports a libclang/bindgen error, install LLVM: winget install -e --id LLVM.LLVM'
}

$OmnipDir = Join-Path $CacheDir 'Omniphony'
Clone-Pinned ([string]$omnip.upstream) ([string]$omnip.tested_version) ([string]$omnip.pinned_commit) $OmnipDir
$OmnipRenderer = Join-Path $OmnipDir 'omniphony-renderer'
Invoke-Native 'git' @('-C', $OmnipRenderer, 'apply', '--check', $Patch)
Invoke-Native 'git' @('-C', $OmnipRenderer, 'apply', $Patch)

$OmnipTarget = Join-Path $CacheDir 'omniphony-target'
New-Item -ItemType Directory -Force -Path $OmnipTarget | Out-Null
$oldCargoTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $OmnipTarget
try {
    Invoke-Native 'cargo' @('+stable', 'build', '--release', '--manifest-path', (Join-Path $OmnipRenderer 'Cargo.toml'), '-p', 'omniphony-renderer')
} finally {
    $env:CARGO_TARGET_DIR = $oldCargoTarget
}
$Orender = Join-Path $OmnipTarget 'release\orender.exe'
$Layout = Join-Path $OmnipDir 'layouts\7.1.4.yaml'
if (-not (Test-Path $Orender)) { throw "Missing built orender.exe: $Orender" }
if (-not (Test-Path $Layout)) { throw "Missing 7.1.4 layout: $Layout" }

Phase 'acquire verified Harletty Windows bridge'
$HarlettyZip = Join-Path $CacheDir 'harletty-bridge-v0.7.4-windows-x86_64.zip'
$HarlettyDir = Join-Path $CacheDir 'harletty-bridge-v0.7.4-windows'
Download-Verified $HarlettyUrl $HarlettyZip $HarlettyZipSha 'Harletty Windows bridge archive'
if (Test-Path $HarlettyDir) { Remove-Item -Recurse -Force $HarlettyDir }
Expand-Archive -Force -Path $HarlettyZip -DestinationPath $HarlettyDir
$Bridge = Get-ChildItem -Path $HarlettyDir -Recurse -Filter 'harletty_bridge.dll' | Select-Object -First 1 -ExpandProperty FullName
if (-not $Bridge) { throw 'harletty_bridge.dll missing from verified release archive' }

Phase 'acquire checksum-pinned official Dolby carrier and derive exact suffix'
$Zip = Join-Path $Work 'audio.zip'
$Source = Join-Path $Work $DolbyName
$Derived = Join-Path $Work 'Living-Room-Atmos_after_first_au.ec3'
Download-Verified $DolbyUrl $Zip $ExpectedZipSha 'Dolby archive'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
try {
    $matches = @($archive.Entries | Where-Object { $_.Name -eq $DolbyName })
    if ($matches.Count -ne 1) { throw "Expected exactly one $DolbyName in Dolby archive; found $($matches.Count)" }
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($matches[0], $Source, $true)
} finally {
    $archive.Dispose()
}
Assert-Sha $Source $ExpectedSourceSha 'Dolby source carrier'
$sourceBytes = [System.IO.File]::ReadAllBytes($Source)
if ($sourceBytes.Length -le $ExpectedFirstAuBytes) { throw 'Dolby carrier is unexpectedly short' }
if ($sourceBytes[0] -ne 0x0b -or $sourceBytes[1] -ne 0x77) { throw 'Unexpected E-AC-3 syncword' }
$frmsiz = (($sourceBytes[2] -band 0x07) -shl 8) -bor $sourceBytes[3]
$firstAuBytes = 2 * ($frmsiz + 1)
if ($firstAuBytes -ne $ExpectedFirstAuBytes) { throw "Unexpected first AU size: $firstAuBytes" }
$derivedBytes = New-Object byte[] ($sourceBytes.Length - $firstAuBytes)
[Array]::Copy($sourceBytes, $firstAuBytes, $derivedBytes, 0, $derivedBytes.Length)
[System.IO.File]::WriteAllBytes($Derived, $derivedBytes)
Assert-Sha $Derived $ExpectedDerivedSha 'Derived moving JOC suffix'

Phase 'wrap exact moving JOC suffix as IEC61937 type 0x15'
$MovingIec = Join-Path $Work 'moving.spdif'
Invoke-Native 'ffmpeg' @('-nostdin', '-hide_banner', '-loglevel', 'error', '-y', '-i', $Derived, '-map', '0:a:0', '-c:a', 'copy', '-f', 'spdif', $MovingIec)
$iec = [System.IO.File]::ReadAllBytes($MovingIec)
$sync = [byte[]](0x72, 0xf8, 0x1f, 0x4e)
for ($i = 0; $i -lt 4; $i++) { if ($iec[$i] -ne $sync[$i]) { throw 'IEC61937 carrier does not start with sync' } }
$burstBytes = -1
for ($offset = 4; $offset -le $iec.Length - 4; $offset++) {
    if ($iec[$offset] -eq $sync[0] -and $iec[$offset+1] -eq $sync[1] -and $iec[$offset+2] -eq $sync[2] -and $iec[$offset+3] -eq $sync[3]) {
        $burstBytes = $offset
        break
    }
}
if ($burstBytes -le 0 -or ($iec.Length % $burstBytes) -ne 0) { throw 'Cannot derive IEC61937 burst geometry' }
$bursts = [int]($iec.Length / $burstBytes)
if ($bursts -ne 2360) { throw "Expected 2360 IEC61937 bursts, got $bursts" }
for ($index = 0; $index -lt $bursts; $index++) {
    $offset = $index * $burstBytes
    if (($iec[$offset + 4] -band 0x1f) -ne 0x15) { throw "Non-E-AC-3 IEC61937 type at burst $index" }
}
Write-Host "AURORA-WINDOWS-IEC61937-PASS bursts=$bursts burst_bytes=$burstBytes"

Phase 'capture moving-object Harletty telemetry'
$HarnessDir = Join-Path $Work 'telemetry-harness'
$HarnessSrc = Join-Path $HarnessDir 'src'
New-Item -ItemType Directory -Force -Path $HarnessSrc | Out-Null
$OmnipCargoPath = (Join-Path $OmnipRenderer '').Replace('\','/')
$cargoToml = @"
[package]
name = "aurora-moving-joc-telemetry-windows"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$($OmnipCargoPath)bridge_api" }
spdif = { path = "$($OmnipCargoPath)spdif" }
serde_json = "1"
"@
Set-Content -Encoding UTF8 -Path (Join-Path $HarnessDir 'Cargo.toml') -Value $cargoToml
Copy-Item -Force $TelemetrySource (Join-Path $HarnessSrc 'main.rs')
$Telemetry = Join-Path $OutputDir 'aurora-bridge-telemetry.json'
$HarnessTarget = Join-Path $CacheDir 'windows-telemetry-target'
$oldCargoTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $HarnessTarget
try {
    Invoke-Native 'cargo' @('+stable', 'run', '--quiet', '--release', '--manifest-path', (Join-Path $HarnessDir 'Cargo.toml'), '--', $Bridge, $MovingIec, $Telemetry)
} finally {
    $env:CARGO_TARGET_DIR = $oldCargoTarget
}
if (-not (Test-Path $Telemetry)) { throw 'Moving-object telemetry was not produced' }

Phase 'render full moving carrier to 7.1.4'
$Unpaced = Join-Path $OutputDir 'aurora-moving-unpaced-7.1.4.f32'
$UnpacedLog = Join-Path $OutputDir 'orender-unpaced.log'
$process = Start-Process -FilePath $Orender -ArgumentList @(
    $MovingIec, '--bridge-path', $Bridge, '--enable-vbap', '--speaker-layout', $Layout,
    '--output-backend', 'file', '--output-file', $Unpaced, '--output-file-format', 'raw-f32'
) -NoNewWindow -Wait -PassThru -RedirectStandardOutput $UnpacedLog -RedirectStandardError (Join-Path $OutputDir 'orender-unpaced.err.log')
if ($process.ExitCode -ne 0) { throw "Unpaced Omniphony render failed with exit code $($process.ExitCode)" }
if (-not (Test-Path $Unpaced)) { throw 'Unpaced 7.1.4 render missing' }
$frameBytes = 12 * 4
$unpacedBytes = (Get-Item $Unpaced).Length
if (($unpacedBytes % $frameBytes) -ne 0) { throw 'Unpaced render is not whole 12-channel f32 frames' }
$frames = [int64]($unpacedBytes / $frameBytes)
if (($frames % $bursts) -ne 0 -or ($frames / $bursts) -ne 1536) { throw "Unexpected render cadence: frames=$frames bursts=$bursts" }
Write-Host "AURORA-WINDOWS-7.1.4-SHAPE-PASS bursts=$bursts frames=$frames"

Phase 'run media-paced full carrier through the real Windows renderer'
$Paced = Join-Path $OutputDir 'aurora-moving-paced-7.1.4.f32'
$PacedLog = Join-Path $OutputDir 'orender-paced.log'
$PacingJson = Join-Path $OutputDir 'pacing.json'
Invoke-Native 'python' @($Pacer, '--orender', $Orender, '--bridge', $Bridge, '--layout', $Layout, '--carrier', $MovingIec, '--unpaced-render', $Unpaced, '--paced-render', $Paced, '--log', $PacedLog, '--report', $PacingJson)

Phase 'run Aurora moving-JOC fail-closed evidence analyzer'
$MovingEvidence = Join-Path $OutputDir 'aurora-joc-moving-evidence.json'
Invoke-Native 'python' @($MovingAnalyzer, 'analyze', '--input', $Derived, '--expected-sha256', $ExpectedDerivedSha, '--provenance', 'Windows local functional run; exact checksum-pinned no-reencode suffix from official Dolby DDP Online Delivery Kit v1.4.1; authoritative independent OpenJOC reference remains Linux CI', '--telemetry', $Telemetry, '--pcm', $Unpaced, '--pacing', $PacingJson, '--sample-rate', '48000', '--channels', '12', '--output', $MovingEvidence)

Phase 'drive merged deterministic virtual TDM16/DAC hardware model'
$VirtualReport = Join-Path $OutputDir 'aurora-full-system-sim.json'
Invoke-Native 'python' @($VirtualAnalyzer, 'run', '--render', $Paced, '--joc-evidence', $MovingEvidence, '--report', $VirtualReport, '--fault', 'none', '--tdm-slots', '16', '--latency-frames', '256')

if (-not $SkipFaultProfiles) {
    Phase 'verify virtual hardware fails closed under injected faults'
    foreach ($fault in @('dropout', 'channel-silence', 'disconnect', 'drift')) {
        $faultReport = Join-Path $OutputDir "fault-$fault.json"
        & python $VirtualAnalyzer run --render $Paced --joc-evidence $MovingEvidence --report $faultReport --fault $fault --tdm-slots 16 --latency-frames 256
        $rc = $LASTEXITCODE
        if ($rc -ne 1) { throw "Expected fail-closed exit 1 for fault '$fault', got $rc" }
        $payload = Get-Content -Raw -Encoding UTF8 $faultReport | ConvertFrom-Json
        if ($payload.verdict -ne 'fail' -or $payload.failures.Count -lt 1) { throw "Fault '$fault' did not produce explicit failure evidence" }
        Write-Host "AURORA-WINDOWS-NEGATIVE-PASS fault=$fault failures=$($payload.failures.Count)"
    }
}

$final = Get-Content -Raw -Encoding UTF8 $VirtualReport | ConvertFrom-Json
if ($final.verdict -ne 'pass') { throw "Final AuroraSim verdict is $($final.verdict)" }
Write-Host "`nAURORA-WINDOWS-FULL-SYSTEM-SIM-PASS" -ForegroundColor Green
Write-Host "frames=$($final.virtual_hardware.sink_frames) channels=$($final.channel_health.active_channel_indices.Count)/12 xruns=$($final.virtual_hardware.xrun_count)"
Write-Host "report=$VirtualReport"
Write-Host 'Truth boundary: local Windows functional/simulation evidence; not physical eARC/UAC2/TDM/DAC or independent OpenJOC reference proof.'
