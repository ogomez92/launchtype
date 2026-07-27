#!/usr/bin/env bash
# Assemble Launchtype.app. Run on macOS from the repo root:
#   ./scripts/bundle-mac.sh
# Uses the vendored Prism slice by default; set PRISM_SDK_DIR to build against a
# full prism-sdk-vX.Y.Z instead.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
PRISM_SDK_DIR="${PRISM_SDK_DIR:-$REPO/vendor/prism-sdk}"
APP="$REPO/dist/Launchtype.app"

# Read the workspace version so the bundle cannot drift from Cargo.toml.
VERSION="$(awk '/^\[workspace\.package\]/{s=1;next} /^\[/{s=0} s && /^version *=/{gsub(/[">=[:space:]]|version/,"");print;exit}' "$REPO/Cargo.toml")"
[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

# libprism.a is universal, so the app can be too. Set UNIVERSAL=0 for a faster
# host-only build while iterating.
UNIVERSAL="${UNIVERSAL:-1}"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)

if [ "$UNIVERSAL" = 1 ]; then
    installed="$(rustup target list --installed)"
    for t in "${TARGETS[@]}"; do
        if ! grep -qx "$t" <<<"$installed"; then
            echo "warning: rust target $t is not installed, falling back to a" \
                 "host-only build. Run: rustup target add $t" >&2
            UNIVERSAL=0
        fi
    done
fi

if [ "$UNIVERSAL" = 1 ]; then
    for t in "${TARGETS[@]}"; do
        cargo build --release -p launchtype --target "$t"
    done
else
    cargo build --release -p launchtype
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

if [ "$UNIVERSAL" = 1 ]; then
    lipo -create -output "$APP/Contents/MacOS/launchtype" \
        "$REPO/target/aarch64-apple-darwin/release/launchtype" \
        "$REPO/target/x86_64-apple-darwin/release/launchtype"
else
    cp "$REPO/target/release/launchtype" "$APP/Contents/MacOS/launchtype"
fi
cp -R "$REPO/assets/sounds" "$APP/Contents/Resources/sounds"
cp -R "$REPO/assets/locale" "$APP/Contents/Resources/locale"

# Prism links statically and reaches VoiceOver through the Apple frameworks, so
# there is no runtime library to ship alongside the executable.

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>launchtype</string>
    <key>CFBundleIdentifier</key><string>dev.ogomez.launchtype</string>
    <key>CFBundleName</key><string>Launchtype</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <!-- Background launcher: no Dock icon, summoned by the global hotkey. -->
    <key>LSUIElement</key><true/>
    <key>NSAppleEventsUsageDescription</key>
    <string>Launchtype launches applications you select.</string>
</dict>
</plist>
PLIST

# A malformed Info.plist makes codesign reject the bundle, so fail loudly here.
plutil -lint "$APP/Contents/Info.plist" >/dev/null

codesign --force --deep -s - "$APP"
echo "Bundled $APP ($VERSION, $(lipo -archs "$APP/Contents/MacOS/launchtype"))"
echo "Note: the first screenshot prompts for the Screen Recording permission."
echo "Data files (commands.json, ...) live NEXT TO the .app bundle."
