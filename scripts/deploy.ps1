# Build + deploy + relaunch Launchtype (Rust). Replaces the Python release.ps1.
# Run from the repo root under PowerShell: pwsh ./scripts/deploy.ps1
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$target = Join-Path $env:USERPROFILE "stuff\software\launchtype"
# Defaults to the vendored slice, same as crates/prism-sys/build.rs.
$prismSdk = if ($env:PRISM_SDK_DIR) { $env:PRISM_SDK_DIR } else { Join-Path $repo "vendor\prism-sdk" }

Push-Location $repo
try {
    cargo build --release -p launchtype
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $dist = Join-Path $repo "dist"
    if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
    New-Item -ItemType Directory -Force $dist | Out-Null

    Copy-Item (Join-Path $repo "target\release\launchtype.exe") $dist
    # Prism runtime DLL (dynamic linking).
    $prismBin = Join-Path $prismSdk "windows\x64\dynamic\release\bin"
    Copy-Item (Join-Path $prismBin "prism.dll") $dist
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
    # Loose files (exe, DLLs) overwrite in place; whatever dist holds ships,
    # so adding a DLL to the dist step above needs no change here.
    Get-ChildItem $dist -File | Copy-Item -Destination $target -Force

    # Asset folders go over with rclone sync, so each ends up matching dist
    # exactly. Copy-Item only ever adds and overwrites: a sound that was renamed
    # or dropped upstream would linger in the install forever, and the
    # timer/alarm dropdowns list whatever .wav files they find in sounds/. The
    # sync is per-folder on purpose — syncing $target itself would delete the
    # user data sitting beside them.
    $rclone = (Get-Command rclone -ErrorAction SilentlyContinue).Source
    foreach ($dir in Get-ChildItem $dist -Directory) {
        $dest = Join-Path $target $dir.Name
        if ($rclone) {
            & $rclone sync $dir.FullName $dest --create-empty-src-dirs
            if ($LASTEXITCODE -ne 0) { throw "rclone sync of $($dir.Name) failed" }
        }
        else {
            # No rclone on this machine: replace the folder outright, which
            # gets the same end state for a directory that is ours alone.
            if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
            Copy-Item -Recurse $dir.FullName $dest
        }
    }

    Start-Process (Join-Path $target "launchtype.exe") -WorkingDirectory $target
    Write-Host "Deployed and relaunched from $target"
}
finally {
    Pop-Location
}
