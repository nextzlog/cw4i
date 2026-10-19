#!/bin/sh
# Wraps release builds of cw4i into cw4i.app, so that macOS starts it without a terminal.
# Usage: scripts/macos-app.sh [binary ...] (default: target/release/cw4i); output in dist/.
set -eu
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || set -- target/release/cw4i
app=dist/cw4i.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
if [ $# -gt 1 ]; then
  lipo -create -output "$app/Contents/MacOS/cw4i" "$@"
else
  cp "$1" "$app/Contents/MacOS/cw4i"
fi
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>CW4ISR</string>
  <key>CFBundleIdentifier</key><string>dev.nextzlog.cw4i</string>
  <key>CFBundleExecutable</key><string>cw4i</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSMicrophoneUsageDescription</key><string>CW4ISR decodes Morse code from the audio input.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app"
echo "$app"
