#!/bin/bash
# Builds Adaptaudio.app and the release archive for GitHub Releases (macOS, Apple Silicon).
# Usage: tools/bundle-macos/bundle.sh
# Output: dist/Adaptaudio-<version>-macos-arm64.zip and the same name with .sha256.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUNDLE_ID="${BUNDLE_ID:-com.martycow.adaptaudio}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
DIST="$ROOT/dist"

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
    echo "error: this script builds the macOS arm64 bundle; run it on an Apple Silicon Mac" >&2
    exit 1
fi

# The version lives in [workspace.package] of the root Cargo.toml.
VERSION="$(awk '
    /^\[/ { in_section = ($0 == "[workspace.package]") }
    in_section && /^version[[:space:]]*=/ { gsub(/[" ]/, "", $0); sub(/^version=/, "", $0); print; exit }
' "$ROOT/Cargo.toml")"
if [[ ! "$VERSION" =~ ^[0-9]+(\.[0-9]+)*([-+][0-9A-Za-z.+-]+)?$ ]]; then
    echo "error: could not read the workspace version from Cargo.toml (got '$VERSION')" >&2
    exit 1
fi

echo "==> Building adaptaudio $VERSION (release)"
(cd "$ROOT" && cargo build --release -p adaptaudio)

BINARY="$TARGET_DIR/release/adaptaudio"
if [[ ! -x "$BINARY" ]]; then
    echo "error: $BINARY not found after the build" >&2
    exit 1
fi

APP="$DIST/Adaptaudio.app"
ZIP_NAME="Adaptaudio-$VERSION-macos-arm64.zip"

echo "==> Assembling $APP"
mkdir -p "$DIST"
# Only the files this script made before are replaced; nothing else in dist/ is touched.
rm -rf "$APP"
rm -f "$DIST/$ZIP_NAME" "$DIST/$ZIP_NAME.sha256"
mkdir -p "$APP/Contents/MacOS"
cp "$BINARY" "$APP/Contents/MacOS/adaptaudio"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>ru</string>
    <key>CFBundleExecutable</key>
    <string>adaptaudio</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Adaptaudio</string>
    <key>CFBundleDisplayName</key>
    <string>Adaptaudio</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.music</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Adaptaudio записывает звук со входа аудиоинтерфейса: синтезаторы, семплы и живые инструменты.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "==> Signing (ad-hoc)"
codesign --force --deep -s - "$APP"
codesign --verify --deep --strict "$APP"

echo "==> Zipping"
ditto -c -k --keepParent "$APP" "$DIST/$ZIP_NAME"
(cd "$DIST" && shasum -a 256 "$ZIP_NAME" > "$ZIP_NAME.sha256")

echo
echo "Done:"
echo "  $APP"
echo "  $DIST/$ZIP_NAME"
echo "  $DIST/$ZIP_NAME.sha256  ($(cut -d' ' -f1 "$DIST/$ZIP_NAME.sha256"))"
echo
echo "To publish (not run by this script; OWNER/NAME is your GitHub repository):"
echo "  gh release create v$VERSION \\"
echo "      \"$DIST/$ZIP_NAME\" \\"
echo "      \"$DIST/$ZIP_NAME.sha256\" \\"
echo "      --repo OWNER/NAME --title \"Adaptaudio $VERSION\" --notes \"Что нового: ...\""
