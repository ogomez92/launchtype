# Build + deploy + relaunch Launchtype (Rust). Replaces the Python release.ps1.
# Run from the repo root under PowerShell: pwsh ./scripts/deploy.ps1
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$target = Join-Path $env:USERPROFILE "stuff\software\launchtype"
$prismSdk = if ($env:PRISM_SDK_DIR) { $env:PRISM_SDK_DIR } else { "D:\code\libs\prism\prism-sdk-v0.16.7" }

Push-Location $repo
try {
    cargo build --release -p launchtype
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $dist = Join-Path $repo "dist"
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    New-Item -ItemType Directory -Force $dist | Out-Null

    Copy-Item (Join-Path $repo "target\release\launchtype.exe") $dist
    # Prism runtime DLLs (dynamic linking).
    $prismBin = Join-Path $prismSdk "windows\x64\dynamic\release\bin"
    Copy-Item (Join-Path $prismBin "prism.dll") $dist
    Copy-Item (Join-Path $prismBin "tolk.dll") $dist
    # Assets.
    foreach ($asset in @("sounds", "locale")) {
        $src = Join-Path $repo "assets\$asset"
        if (Test-Path $src) { Copy-Item -Recurse $src (Join-Path $dist $asset) }
    }

    # Stop the running instance before overwriting.
    Get-Process launchtype -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 300

    New-Item -ItemType Directory -Force $target | Out-Null
    # Copy program + assets only. User data (commands.json, settings.json,
    # timers.json, alarms.json, clipboard_history.json, realtime_history.json,
    # snippets/, screenshots/) lives in $target and is never touched.
    Copy-Item (Join-Path $dist "*") $target -Recurse -Force

    Start-Process (Join-Path $target "launchtype.exe") -WorkingDirectory $target
    Write-Host "Deployed and relaunched from $target"
}
finally {
    Pop-Location
}
