@echo off
rem Swap app state to the tutorial profile (see README).
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0profile-swap.ps1" -To tutorial %*
exit /b %errorlevel%
