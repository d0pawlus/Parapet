#!/bin/sh
# Builds Parapet.app (Apple silicon) and zips it: dist/Parapet-macOS-arm64.zip
# Run on a Mac with Rust installed:  sh scripts/bundle-macos.sh
set -eu
cd "$(dirname "$0")/.."

TARGET=aarch64-apple-darwin
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

cargo build --release --target "$TARGET"

APP=dist/Parapet.app
rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$TARGET/release/parapet" "$APP/Contents/MacOS/parapet"
sed "s/@VERSION@/$VERSION/g" macos/Info.plist > "$APP/Contents/Info.plist"

# the app icon, from the pixel-art logo
ICONSET=dist/Parapet.iconset
mkdir -p "$ICONSET"
cp assets/icon-16.png   "$ICONSET/icon_16x16.png"
cp assets/logo-32.png   "$ICONSET/icon_16x16@2x.png"
cp assets/logo-32.png   "$ICONSET/icon_32x32.png"
cp assets/logo-64.png   "$ICONSET/icon_32x32@2x.png"
cp assets/logo-128.png  "$ICONSET/icon_128x128.png"
cp assets/logo-256.png  "$ICONSET/icon_128x128@2x.png"
cp assets/logo-256.png  "$ICONSET/icon_256x256.png"
cp assets/logo-512.png  "$ICONSET/icon_256x256@2x.png"
cp assets/logo-512.png  "$ICONSET/icon_512x512.png"
cp assets/logo-1024.png "$ICONSET/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Parapet.icns"
rm -rf "$ICONSET"

# not notarized: an ad-hoc signature is what Apple silicon needs to run it at all
codesign --force --deep --sign - "$APP"

ditto -c -k --keepParent "$APP" dist/Parapet-macOS-arm64.zip
echo "dist/Parapet-macOS-arm64.zip"
