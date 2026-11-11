@echo off
rem Restore the default app state profile (see README).
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0profile-swap.ps1" -To default %*
exit /b %errorlevel%
