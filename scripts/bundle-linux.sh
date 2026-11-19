#!/bin/sh
# Builds Parapet as an AppImage: dist/Parapet-x86_64.AppImage
# GTK 3, an AppIndicator library, OpenSSL 3 and a Vulkan driver come from the system, as every desktop
# distribution ships them. Run on Linux with Rust installed:  sh scripts/bundle-linux.sh
set -eu
cd "$(dirname "$0")/.."

ARCH=$(uname -m)
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"

cargo build --release

APPDIR=dist/Parapet.AppDir
rm -rf dist
mkdir -p "$APPDIR/usr/bin"
cp target/release/parapet "$APPDIR/usr/bin/parapet"
cp linux/parapet.desktop "$APPDIR/parapet.desktop"
cp assets/logo-256.png "$APPDIR/parapet.png"
ln -s usr/bin/parapet "$APPDIR/AppRun"

TOOL=dist/appimagetool
curl -fsSL -o "$TOOL" "$TOOL_URL"
chmod +x "$TOOL"
# no FUSE on build machines: let the tool unpack itself instead of mounting
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream "$APPDIR" "dist/Parapet-$ARCH.AppImage"
rm -rf "$TOOL" "$APPDIR"
echo "dist/Parapet-$ARCH.AppImage"
