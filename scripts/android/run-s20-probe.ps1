$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$work = Join-Path $env:TEMP "Aurora-S20-Probe"
$toolsDir = Join-Path $work "platform-tools"
$adb = Join-Path $toolsDir "adb.exe"
$report = Join-Path ([Environment]::GetFolderPath("Desktop")) "Aurora-S20-Report.txt"
$zipUrl = "https://dl.google.com/android/repository/platform-tools-latest-windows.zip"

New-Item -ItemType Directory -Force -Path $work | Out-Null
if (-not (Test-Path $adb)) {
    $archive = Join-Path $work "platform-tools.zip"
    Write-Host "Downloading official Android platform-tools..."
    Invoke-WebRequest -UseBasicParsing -Uri $zipUrl -OutFile $archive
    if (Test-Path $toolsDir) { Remove-Item -Recurse -Force $toolsDir }
    Expand-Archive -Force -Path $archive -DestinationPath $work
}

& $adb start-server | Out-Null
Write-Host "Unlock the phone and approve USB debugging when prompted."

$serial = $null
for ($attempt = 0; $attempt -lt 120; $attempt++) {
    $lines = @(& $adb devices)
    $authorized = @($lines | Where-Object { $_ -match "^([^\s]+)\s+device$" })
    $unauthorized = @($lines | Where-Object { $_ -match "\s+unauthorized$" })
    if ($authorized.Count -eq 1) {
        $serial = ($authorized[0] -split "\s+")[0]
        break
    }
    if ($authorized.Count -gt 1) {
        throw "More than one authorized Android device is connected. Disconnect the others."
    }
    if ($unauthorized.Count -gt 0) {
        Write-Host "Waiting for approval on the phone..." -ForegroundColor Yellow
    } else {
        Write-Host "Waiting for USB device... use a data cable and select File transfer/Android Auto." -ForegroundColor Yellow
    }
    Start-Sleep -Seconds 2
}
if (-not $serial) { throw "No authorized phone appeared within four minutes." }

function AdbText([string[]] $Arguments) {
    $result = & $adb -s $serial @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "adb failed: $($Arguments -join ' ')`n$result" }
    return ($result -join "`n").Trim()
}

function AddSection([System.Collections.Generic.List[string]] $Lines, [string] $Name, [string[]] $Command) {
    $Lines.Add("")
    $Lines.Add("=== $Name ===")
    try { $Lines.Add((AdbText $Command)) } catch { $Lines.Add("ERROR: $($_.Exception.Message)") }
}

$out = [System.Collections.Generic.List[string]]::new()
$out.Add("Aurora S20 non-destructive capability probe")
$out.Add("Generated UTC: $([DateTime]::UtcNow.ToString('o'))")
$out.Add("ADB serial: $serial")
AddSection $out "IDENTITY" @("shell", "getprop", "ro.product.model")
AddSection $out "DEVICE" @("shell", "getprop", "ro.product.device")
AddSection $out "SOC" @("shell", "getprop", "ro.soc.model")
AddSection $out "ABI" @("shell", "getprop", "ro.product.cpu.abilist")
AddSection $out "ANDROID" @("shell", "getprop", "ro.build.version.release")
AddSection $out "SDK" @("shell", "getprop", "ro.build.version.sdk")
AddSection $out "KERNEL" @("shell", "uname", "-a")
AddSection $out "CPU" @("shell", "cat", "/proc/cpuinfo")
AddSection $out "MEMORY" @("shell", "cat", "/proc/meminfo")
AddSection $out "USB" @("shell", "getprop", "sys.usb.config")
AddSection $out "AUDIO FEATURES" @("shell", "pm", "list", "features")
AddSection $out "AUDIO POLICY FILES" @("shell", "sh", "-c", "ls -l /vendor/etc/audio* /vendor/etc/*audio* 2>/dev/null")
AddSection $out "MEDIA CODECS" @("shell", "sh", "-c", "dumpsys media.codec | grep -Ei 'name:|audio/(eac3|ac3|true-hd|mlp|aac|opus|flac)' | head -400")
AddSection $out "AUDIO SERVICE" @("shell", "sh", "-c", "dumpsys audio | grep -Ei 'sample|channel|encoding|device|spatial|dolby|output' | head -500")
AddSection $out "THERMAL" @("shell", "dumpsys", "thermalservice")
AddSection $out "BATTERY" @("shell", "dumpsys", "battery")
AddSection $out "STORAGE" @("shell", "df", "-h", "/data/local/tmp")

[IO.File]::WriteAllLines($report, $out, [Text.UTF8Encoding]::new($false))
Write-Host "Probe finished. No files were written to the phone." -ForegroundColor Green
Write-Host "Report: $report" -ForegroundColor Green
Start-Process explorer.exe "/select,`"$report`""
