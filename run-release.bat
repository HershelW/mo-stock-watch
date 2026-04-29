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

start "" "%~dp0target\release\mo-stock-watch.exe"
