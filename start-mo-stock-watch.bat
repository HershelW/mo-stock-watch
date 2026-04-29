@echo off
setlocal
cd /d "%~dp0"

set "APP_EXE=%~dp0target\debug\mo-stock-watch.exe"

if exist "%APP_EXE%" (
  start "" "%APP_EXE%"
  exit /b 0
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo mo-stock-watch.exe was not found, and cargo is not available in PATH.
  echo Run this once from the project folder after installing Rust:
  echo   cargo build
  pause
  exit /b 1
)

echo mo-stock-watch.exe was not found. Building and starting...
cargo run
