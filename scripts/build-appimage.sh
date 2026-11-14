#!/usr/bin/env bash
set -euo pipefail

# Build AppImage for ZenithBar
echo "=== Building ZenithBar AppImage ==="

BIN_PATH="${1:-target/release/ZenithBar}"
APP_DIR="AppDir"
OUTPUT_NAME="${2:-ZenithBar-x86_64.AppImage}"

if [ ! -f "$BIN_PATH" ]; then
    echo "Binary not found at $BIN_PATH. Compiling release binary..."
    cargo build --release
    BIN_PATH="target/release/ZenithBar"
fi

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/usr/bin"
mkdir -p "$APP_DIR/usr/share/applications"
mkdir -p "$APP_DIR/usr/share/icons/hicolor/512x512/apps"

# Copy binary
cp "$BIN_PATH" "$APP_DIR/usr/bin/ZenithBar"
chmod +x "$APP_DIR/usr/bin/ZenithBar"

# Copy desktop file and icons
cp assets/zenithbar.desktop "$APP_DIR/zenithbar.desktop"
cp assets/zenithbar.desktop "$APP_DIR/usr/share/applications/zenithbar.desktop"
cp assets/icon.png "$APP_DIR/zenithbar.png"
cp assets/icon.png "$APP_DIR/usr/share/icons/hicolor/512x512/apps/zenithbar.png"

# Create AppRun
cat << 'EOF' > "$APP_DIR/AppRun"
#!/bin/sh
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH:-}"
exec "${HERE}/usr/bin/ZenithBar" "$@"
EOF
chmod +x "$APP_DIR/AppRun"

# Download appimagetool if not present
if ! command -v appimagetool &> /dev/null; then
    if [ ! -f "appimagetool-x86_64.AppImage" ]; then
        echo "Downloading appimagetool..."
        curl -sLo appimagetool-x86_64.AppImage https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x appimagetool-x86_64.AppImage
    fi
    APPIMAGETOOL="./appimagetool-x86_64.AppImage"
else
    APPIMAGETOOL="appimagetool"
fi

# Package AppImage
echo "Generating $OUTPUT_NAME..."
ARCH=x86_64 "$APPIMAGETOOL" --appimage-extract-and-run "$APP_DIR" "$OUTPUT_NAME"

echo "=== AppImage successfully created: $OUTPUT_NAME ==="
