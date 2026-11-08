#!/bin/sh
set -eu
name=$1
program=$2
binary=${3:-target/release/$program}
dir="dist/$program.AppDir"
rm -rf "$dir"
mkdir -p "$dir/usr/bin"
cp "$binary" "$dir/usr/bin/$program"
chmod +x "$dir/usr/bin/$program"
if [ -n "${NOTICES:-}" ]; then
  mkdir -p "$dir/usr/share/doc/$program"
  cp "$NOTICES" "$dir/usr/share/doc/$program/"
fi
cat > "$dir/AppRun" <<RUN
#!/bin/sh
here=\$(dirname "\$(readlink -f "\$0")")
exec "\$here/usr/bin/$program" "\$@"
RUN
chmod +x "$dir/AppRun"
cat > "$dir/$program.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$name
Exec=$program
Icon=$program
Categories=Network;HamRadio;
Terminal=false
DESKTOP
cat > "$dir/$program.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
<rect width="256" height="256" rx="40" fill="#1d4f91"/>
<text x="128" y="170" font-family="sans-serif" font-size="120" font-weight="bold" fill="#ffffff" text-anchor="middle">CW</text>
</svg>
SVG
ln -sf "$program.svg" "$dir/.DirIcon"
tool=${APPIMAGETOOL:-appimagetool}
if ! command -v "$tool" > /dev/null 2>&1; then
  echo "$dir (appimagetool not found)" >&2
  exit 1
fi
ARCH=x86_64 "$tool" "$dir" "dist/$program-x86_64.AppImage"
echo "dist/$program-x86_64.AppImage"
