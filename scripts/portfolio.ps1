[CmdletBinding()]
param(
    [ValidateSet('inspect','validate','migrate','update','rebuild','export')][string]$Command = 'inspect',
    [string]$AppDir = (Join-Path $env:APPDATA 'mo-stock-watch'),
    [string]$InputFile,
    [string]$OutputFile,
    [string]$Python = (Join-Path $env:USERPROFILE '.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe'),
    [switch]$PortfolioOnly,
    [switch]$Restart
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Python -PathType Leaf)) { throw 'Python 3 is required; pass -Python with its executable path.' }
$project = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $project 'dist\mo-stock-watch.exe'
if ($Command -in @('migrate','update','rebuild')) {
    $running = @(Get-CimInstance Win32_Process -Filter "Name='mo-stock-watch.exe'")
    if (@($running | Where-Object ExecutablePath -ne $exe).Count) { throw 'A different stock-watch EXE is running. Close it before updating.' }
    foreach ($process in $running) { Stop-Process -Id $process.ProcessId -Force }
    if ($running.Count) { Start-Sleep -Milliseconds 300 }
}
$arguments = @((Join-Path $PSScriptRoot 'portfolio_ops.py'), $Command, '--app-dir', $AppDir)
if ($InputFile) { $arguments += @('--input', $InputFile) }
if ($OutputFile) { $arguments += @('--output', $OutputFile) }
if ($PortfolioOnly) { $arguments += '--portfolio-only' }
$env:PYTHONUTF8 = '1'
& $Python @arguments
if ($LASTEXITCODE -ne 0) { throw "Portfolio operation failed with exit code $LASTEXITCODE. App remains closed." }
if ($Restart) {
    & $Python (Join-Path $PSScriptRoot 'portfolio_ops.py') validate --app-dir $AppDir
    if ($LASTEXITCODE -ne 0) { throw 'Validation failed before restart' }
    $previousOverride = $env:MO_STOCK_APP_DIR
    try {
        $env:MO_STOCK_APP_DIR = [IO.Path]::GetFullPath($AppDir)
        $process = Start-Process -FilePath $exe -WorkingDirectory (Split-Path -Parent $exe) -WindowStyle Hidden -PassThru
        Start-Sleep -Seconds 2
        if ($process.HasExited) { throw 'Stock-watch exited during startup' }
        & $exe --validate-data
        if ($LASTEXITCODE -ne 0) { throw 'EXE rejected the live portfolio' }
        # Daily quote snapshots do not change the ledger's economic-state hash.
        & $Python (Join-Path $PSScriptRoot 'portfolio_ops.py') validate --app-dir $AppDir
        if ($LASTEXITCODE -ne 0) { throw 'Live portfolio validation failed' }
        Write-Output "App running: $($process.Id)"
    } finally { $env:MO_STOCK_APP_DIR = $previousOverride }
}
