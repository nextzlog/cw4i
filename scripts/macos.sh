#!/bin/sh
set -eu
name=$1
program=$2
shift 2
[ $# -gt 0 ] || set -- "target/release/$program"
version=$(awk -F'"' '/^\[/ { section = $0 } section == "[workspace.package]" && /^version *=/ { print $2; exit }' Cargo.toml)
[ -n "$version" ]
app="dist/$name.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
if [ $# -gt 1 ]; then
  lipo -create -output "$app/Contents/MacOS/$program" "$@"
else
  cp "$1" "$app/Contents/MacOS/$program"
fi
if [ -n "${NOTICES:-}" ]; then
  mkdir -p "$app/Contents/Resources"
  cp "$NOTICES" "$app/Contents/Resources/"
fi
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>dev.nextzlog.$program</string>
  <key>CFBundleExecutable</key><string>$program</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>NSMicrophoneUsageDescription</key><string>CW4ISR decodes Morse code from the audio input.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app"
echo "$app"
