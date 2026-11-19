#!/bin/sh
# Builds Parapet as an AppImage: dist/Parapet-x86_64.AppImage
# GTK 3, an AppIndicator library, OpenSSL 3 and a Vulkan driver come from the system, as every desktop
# distribution ships them. Run on Linux with Rust installed:  sh scripts/bundle-linux.sh
set -eu
cd "$(dirname "$0")/.."

ARCH=$(uname -m)
# a pinned appimagetool, checked against its SHA-256 before it runs
TOOL_VERSION=1.9.1
case "$ARCH" in
    x86_64) TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 ;;
    aarch64) TOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 ;;
    *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac
TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-$ARCH.AppImage"

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
echo "$TOOL_SHA256  $TOOL" | sha256sum -c -
chmod +x "$TOOL"
# no FUSE on build machines: let the tool unpack itself instead of mounting
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream "$APPDIR" "dist/Parapet-$ARCH.AppImage"
rm -rf "$TOOL" "$APPDIR"
echo "dist/Parapet-$ARCH.AppImage"
