@echo off
rem Developer-only reset of local app state (see README).
setlocal
node --disable-warning=ExperimentalWarning "%~dp0dev-reset.mjs" %*
exit /b %errorlevel%
