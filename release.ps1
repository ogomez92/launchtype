# release.ps1
# Builds Launchtype and deploys it to the install location, replacing any
# running instance.
#
# Usage:  pwsh ./release.ps1   (or right-click > Run with PowerShell)

$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot
$DistDir     = Join-Path $ProjectRoot "dist\launchtype"
$TargetDir   = "C:\Users\nitropc\stuff\software\launchtype"
$TargetExe   = Join-Path $TargetDir "launchtype.exe"

Write-Host "==> Ensuring build dependencies are installed..." -ForegroundColor Cyan
& uv sync --extra build
if ($LASTEXITCODE -ne 0) {
    throw "Failed to sync build dependencies (exit code $LASTEXITCODE)."
}

Write-Host "==> Building Launchtype with PyInstaller..." -ForegroundColor Cyan
& uv run --extra build python -m PyInstaller ".\main.spec" --noconfirm
if ($LASTEXITCODE -ne 0) {
    throw "PyInstaller build failed with exit code $LASTEXITCODE."
}

# Copy bundled assets next to the executable (mirrors the documented build steps)
Write-Host "==> Copying assets into the build folder..." -ForegroundColor Cyan
& xcopy "sounds" (Join-Path $DistDir "sounds") /E /H /C /I /Y | Out-Null
& xcopy "locale" (Join-Path $DistDir "locale") /E /H /C /I /Y | Out-Null

if (-not (Test-Path (Join-Path $DistDir "launchtype.exe"))) {
    throw "Expected build output not found at $DistDir\launchtype.exe."
}

# Stop any running instance so the files can be overwritten. Match both by
# process name and by any process running out of the target folder (covers the
# case where the deployed exe is the one holding a lock).
Write-Host "==> Stopping any running Launchtype instance..." -ForegroundColor Cyan
$running = @(Get-Process -Name "launchtype" -ErrorAction SilentlyContinue)
$running += @(
    Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($TargetDir, [System.StringComparison]::OrdinalIgnoreCase) }
)
$running = $running | Sort-Object -Property Id -Unique
if ($running) {
    $running | Stop-Process -Force
    # Give the OS a moment to release the file handles
    Start-Sleep -Milliseconds 800
    Write-Host "    Stopped $($running.Count) process(es)." -ForegroundColor DarkGray
} else {
    Write-Host "    No running instance found." -ForegroundColor DarkGray
}

# Ensure the target directory exists
if (-not (Test-Path $TargetDir)) {
    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
}

# Deploy the full build output so the exe at $TargetExe has everything it needs.
# Retry briefly in case a lock (antivirus, lingering handle) is still clearing.
Write-Host "==> Deploying to $TargetDir ..." -ForegroundColor Cyan
$deployed = $false
for ($attempt = 1; $attempt -le 3; $attempt++) {
    $output = & xcopy "$DistDir\*" "$TargetDir\" /E /H /C /I /Y /R 2>&1
    if ($LASTEXITCODE -eq 0 -and ($output -notmatch "Sharing violation|Infracci")) {
        $deployed = $true
        break
    }
    Write-Host "    Copy attempt $attempt hit locked files; retrying..." -ForegroundColor Yellow
    Start-Sleep -Seconds 1
}
if (-not $deployed) {
    Write-Host $output
    throw "Deployment failed: some files in $TargetDir were locked and could not be replaced. Close any program using them (Explorer windows, antivirus) and re-run."
}

if (-not (Test-Path $TargetExe)) {
    throw "Deployment failed: $TargetExe was not created."
}
Write-Host "==> Done. Deployed to $TargetExe" -ForegroundColor Green

# Launch the freshly deployed app
Write-Host "==> Launching Launchtype..." -ForegroundColor Cyan
Start-Process -FilePath $TargetExe -WorkingDirectory $TargetDir
Write-Host "==> Launchtype started." -ForegroundColor Green
