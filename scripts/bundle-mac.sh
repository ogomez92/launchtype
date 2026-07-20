#!/usr/bin/env bash
# Assemble Launchtype.app. Run on macOS from the repo root:
#   PRISM_SDK_DIR=/path/to/prism-sdk-v0.16.7 ./scripts/bundle-mac.sh
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
PRISM_SDK_DIR="${PRISM_SDK_DIR:?set PRISM_SDK_DIR to the prism SDK directory}"
APP="$REPO/dist/Launchtype.app"

cargo build --release -p launchtype

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"

cp "$REPO/target/release/launchtype" "$APP/Contents/MacOS/launchtype"
cp -R "$REPO/assets/sounds" "$APP/Contents/Resources/sounds"
cp -R "$REPO/assets/locale" "$APP/Contents/Resources/locale"

# Prism links statically, but the screen-reader client library ships as a
# dylib and is looked up at runtime.
cp "$PRISM_SDK_DIR"/macos/universal/static/release/lib/libtolk*.dylib \
   "$APP/Contents/Frameworks/" 2>/dev/null || true

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>launchtype</string>
    <key>CFBundleIdentifier</key><string>dev.ogomez.launchtype</string>
    <key>CFBundleName</key><string>Launchtype</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <!-- Background launcher: no Dock icon, summoned by the global hotkey. -->
    <key>LSUIElement</key><true/>
    <key>NSAppleEventsUsageDescription</key>
    <string>Launchtype launches applications you select.</string>
</dict>
PLIST

codesign --force --deep -s - "$APP"
echo "Bundled $APP"
echo "Note: the first screenshot prompts for the Screen Recording permission."
echo "Data files (commands.json, ...) live NEXT TO the .app bundle."
