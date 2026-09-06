$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Report = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Aurora-S20-Native-Test.txt'
$Cache = Join-Path $env:TEMP 'Aurora-S20-Native'
$PlatformToolsZip = Join-Path $Cache 'platform-tools-latest-windows.zip'
$PlatformToolsDir = Join-Path $Cache 'platform-tools'
$NdkZip = Join-Path $Cache 'android-ndk-r27d-windows.zip'
$NdkRoot = Join-Path $Cache 'android-ndk-r27d'
$Source = Join-Path $PSScriptRoot 'aurora_s20_native_test.c'
$LocalBinary = Join-Path $Cache 'aurora_s20_native_test'
$RemoteBinary = '/data/local/tmp/aurora_s20_native_test'
$NdkUrl = 'https://dl.google.com/android/repository/android-ndk-r27d-windows.zip'
$NdkSha1 = '56607cbccd3642d4a1991f6bb3114a00f884f426'

New-Item -ItemType Directory -Force -Path $Cache | Out-Null
$lines = [System.Collections.Generic.List[string]]::new()
function Add-Line([string]$Text = '') { $script:lines.Add($Text) }
function Add-Section([string]$Title, [string]$Body) {
    Add-Line ''
    Add-Line ('==== ' + $Title + ' ====')
    Add-Line $Body.TrimEnd()
}
function Save-Report { $script:lines | Set-Content -LiteralPath $Report -Encoding UTF8 }
function Fail-Test([string]$Message) {
    Add-Section 'FATAL' $Message
    Save-Report
    Write-Host "`nFAILED: $Message" -ForegroundColor Red
    Write-Host "Report: $Report"
    Read-Host 'Press Enter to close'
    exit 1
}

Add-Line 'Aurora Galaxy S20 Native Validation'
Add-Line ('UTC: ' + [DateTime]::UtcNow.ToString('yyyy-MM-dd HH:mm:ss'))
Add-Line 'No root, unlock, format, APK installation, or system modification was used.'

if (-not (Test-Path -LiteralPath $Source)) { Fail-Test "Missing source file: $Source" }

$Adb = Get-Command adb.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
if (-not $Adb) {
    $Adb = Join-Path $PlatformToolsDir 'adb.exe'
    if (-not (Test-Path -LiteralPath $Adb)) {
        Write-Host 'Downloading official Android Platform Tools...'
        Invoke-WebRequest -Uri 'https://dl.google.com/android/repository/platform-tools-latest-windows.zip' -OutFile $PlatformToolsZip
        if (Test-Path -LiteralPath $PlatformToolsDir) { Remove-Item -Recurse -Force $PlatformToolsDir }
        Expand-Archive -LiteralPath $PlatformToolsZip -DestinationPath $Cache -Force
    }
}
if (-not (Test-Path -LiteralPath $Adb)) { Fail-Test 'adb.exe was not found.' }

& $Adb start-server | Out-Null
$deviceLines = & $Adb devices 2>&1
$authorized = @($deviceLines | Select-String -Pattern "\tdevice$")
if ($authorized.Count -ne 1) {
    Add-Section 'ADB devices' ($deviceLines | Out-String)
    Fail-Test 'Connect exactly one unlocked and authorized Android phone, then run again.'
}
$Serial = (($authorized[0].Line -split "\t")[0]).Trim()
function AdbText {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Command)
    (& $script:Adb -s $script:Serial @Command 2>&1 | Out-String)
}

$model = (AdbText shell getprop ro.product.model).Trim()
$abi = (AdbText shell getprop ro.product.cpu.abi).Trim()
$sdk = (AdbText shell getprop ro.build.version.sdk).Trim()
Add-Section 'Device' "serial=$Serial`nmodel=$model`nabi=$abi`nsdk=$sdk"
if ($abi -ne 'arm64-v8a') { Fail-Test "This package expects arm64-v8a, but the phone reported: $abi" }

$thermalBefore = AdbText shell dumpsys thermalservice
$batteryBefore = AdbText shell dumpsys battery
Add-Section 'Thermal before' $thermalBefore
Add-Section 'Battery before' $batteryBefore
Save-Report

$Compiler = Join-Path $NdkRoot 'toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android33-clang.cmd'
if (-not (Test-Path -LiteralPath $Compiler)) {
    if (-not (Test-Path -LiteralPath $NdkZip)) {
        Write-Host 'Downloading official Android NDK r27d (about 782 MB)...' -ForegroundColor Cyan
        Write-Host 'This is cached in your TEMP folder for later runs.'
        Invoke-WebRequest -Uri $NdkUrl -OutFile $NdkZip
    }
    Write-Host 'Verifying Android NDK archive...'
    $actualSha1 = (Get-FileHash -LiteralPath $NdkZip -Algorithm SHA1).Hash.ToLowerInvariant()
    if ($actualSha1 -ne $NdkSha1) {
        Remove-Item -Force $NdkZip
        Fail-Test "NDK checksum mismatch. Expected $NdkSha1 but got $actualSha1. The archive was deleted."
    }
    Write-Host 'Extracting Android NDK...'
    if (Test-Path -LiteralPath $NdkRoot) { Remove-Item -Recurse -Force $NdkRoot }
    Expand-Archive -LiteralPath $NdkZip -DestinationPath $Cache -Force
}
if (-not (Test-Path -LiteralPath $Compiler)) { Fail-Test "Android compiler was not found at: $Compiler" }

Write-Host 'Building Aurora ARM64 native test...'
$compileOutput = & $Compiler '-std=c11' '-O3' '-Wall' '-Wextra' '-Werror' '-fPIE' '-pie' $Source '-lm' '-o' $LocalBinary 2>&1
$compileCode = $LASTEXITCODE
Add-Section 'Compiler' (($compileOutput | Out-String) + "`nexit_code=$compileCode")
if ($compileCode -ne 0 -or -not (Test-Path -LiteralPath $LocalBinary)) {
    Fail-Test 'The ARM64 native test did not compile.'
}

Write-Host 'Sending the temporary test to the phone...'
$pushOutput = AdbText push $LocalBinary $RemoteBinary
$chmodOutput = AdbText shell chmod 700 $RemoteBinary
Add-Section 'ADB push' ($pushOutput + $chmodOutput)

try {
    Write-Host 'Running 60-second 12-channel DSP stress test on the S20...' -ForegroundColor Green
    $nativeOutput = AdbText shell $RemoteBinary 60
    $nativeExit = $LASTEXITCODE
    Add-Section 'Native Aurora result' ($nativeOutput + "`nexit_code=$nativeExit")

    Write-Host 'Reading codec and audio capabilities...'
    $codecDump = AdbText shell dumpsys media.codec
    $codecSelected = @($codecDump -split "`r?`n" | Select-String -Pattern 'eac3|e-ac-3|ac3|truehd|mlp|dolby|joc|c2\.|omx\.' -CaseSensitive:$false | Select-Object -First 500 | ForEach-Object { $_.Line })
    if ($codecSelected.Count -eq 0) { $codecSelected = @('[No matching lines in dumpsys media.codec]') }
    Add-Section 'Media codec matches' ($codecSelected -join "`n")

    $xmlCommand = "find /vendor/etc /odm/etc /system/etc -type f \( -name 'media_codecs*.xml' -o -name 'audio_policy*.xml' \) -exec grep -HniE 'eac3|e-ac-3|ac3|truehd|mlp|dolby|joc' '{}' \; 2>/dev/null"
    $xmlMatches = AdbText shell $xmlCommand
    if ([string]::IsNullOrWhiteSpace($xmlMatches)) { $xmlMatches = '[No matching codec/policy XML lines]' }
    Add-Section 'Codec and audio-policy XML matches' $xmlMatches

    $audioDump = AdbText shell dumpsys audio
    $audioSelected = @($audioDump -split "`r?`n" | Select-String -Pattern 'HDMI|USB|A2DP|BLE|encoding|format|channel|spatial|dolby|direct|offload' -CaseSensitive:$false | Select-Object -First 500 | ForEach-Object { $_.Line })
    if ($audioSelected.Count -eq 0) { $audioSelected = @('[No matching lines in dumpsys audio]') }
    Add-Section 'Audio route matches' ($audioSelected -join "`n")
} finally {
    $cleanup = AdbText shell rm -f $RemoteBinary
    Add-Section 'Phone cleanup' $cleanup
}

$thermalAfter = AdbText shell dumpsys thermalservice
$batteryAfter = AdbText shell dumpsys battery
Add-Section 'Thermal after' $thermalAfter
Add-Section 'Battery after' $batteryAfter

$resultText = ($lines -join "`n")
$mappingPass = $resultText -match 'PASS channel_mapping_roundtrip_12ch'
$finitePass = $resultText -match 'PASS finite_output'
$factorMatch = [regex]::Match($resultText, 'realtime_factor=([0-9.]+)x')
$factor = if ($factorMatch.Success) { [double]::Parse($factorMatch.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture) } else { 0.0 }
Add-Line ''
Add-Line '==== Automatic verdict ===='
if ($mappingPass -and $finitePass -and $factor -ge 1.0) {
    Add-Line ("PASS: 12-channel Aurora-style mapping and DSP sustained {0:N2}x real time on this phone." -f $factor)
} else {
    Add-Line 'FAIL/INCOMPLETE: the native routing/DSP test did not meet all checks.'
}
Add-Line 'This proves native ARM64 routing/DSP execution and measured headroom for this test load.'
Add-Line 'It does not by itself prove licensed Dolby Atmos/JOC decoding or compatibility with protected streaming apps.'
Save-Report

Write-Host "`nFinished. Report saved to:" -ForegroundColor Green
Write-Host $Report
Write-Host 'Upload Aurora-S20-Native-Test.txt here.' -ForegroundColor Yellow
Read-Host 'Press Enter to close'
