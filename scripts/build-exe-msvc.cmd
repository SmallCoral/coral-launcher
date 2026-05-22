@echo off
setlocal
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b %errorlevel%
powershell -ExecutionPolicy Bypass -File "scripts\prepare-logo.ps1"
if errorlevel 1 exit /b %errorlevel%
call npm run build:exe:raw
if errorlevel 1 exit /b %errorlevel%
copy /Y "src-tauri\target\release\coral-launcher.exe" "coral-launcher.exe" > nul
if errorlevel 1 exit /b %errorlevel%
echo Built desktop exe: %CD%\coral-launcher.exe
