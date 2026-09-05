@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
  echo cargo is not available in PATH. Please install Rust first.
  pause
  exit /b 1
)

cargo build --release
if errorlevel 1 (
  pause
  exit /b 1
)

if not exist "%~dp0dist" mkdir "%~dp0dist"
copy /Y "%~dp0target\release\mo-stock-watch.exe" "%~dp0dist\mo-stock-watch.exe" >nul
if errorlevel 1 (
  echo Failed to update dist\mo-stock-watch.exe. Close the app and try again.
  pause
  exit /b 1
)

start "" "%~dp0dist\mo-stock-watch.exe"
