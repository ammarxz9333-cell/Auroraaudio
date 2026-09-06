@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-s20-probe.ps1"
echo.
if errorlevel 1 echo Probe failed. Read the message above.
pause
