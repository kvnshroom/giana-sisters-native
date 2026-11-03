#!/usr/bin/env bash
# Builds one release archive. Used by .github/workflows/release.yml and runnable locally.
#   package.sh <linux|windows|macos> <version> <binary> <outdir>
# Output (names are matched by the in-game update checker, do not change them):
#   giana-sisters-native-<version>-linux-x86_64.tar.gz
#   giana-sisters-native-<version>-windows-x86_64.zip
#   giana-sisters-native-<version>-macos-universal.tar.gz
set -euo pipefail
os="$1"
version="$2"
bin="$3"
out="$4"
root="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
base="giana-sisters-native-$version"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

case "$os" in
  linux)
    d="$stage/$base-linux-x86_64"
    mkdir "$d"
    install -m 755 "$bin" "$d/giana-app"
    cp "$root/assets/icon/icon.png" "$d/giana-sisters.png"
    cat >"$d/INSTALL.txt" <<'TXT'
The Great Giana Sisters - native port (Linux x86_64)

Needs SDL2:  apt install libsdl2-2.0-0     or     dnf install SDL2

Start ./giana-app. On the first start it asks for your own copy of the game as a .g64 disk image
(no game data is included). See README.md.
TXT
    ;;
  windows)
    d="$stage/$base-windows-x86_64"
    mkdir "$d"
    install -m 755 "$bin" "$d/giana-app.exe"
    cat >"$d/INSTALL.txt" <<'TXT'
The Great Giana Sisters - native port (Windows x86_64)

Nothing to install (SDL2 is built into giana-app.exe). Start giana-app.exe.
On the first start it asks for your own copy of the game as a .g64 disk image
(no game data is included). See README.md.
Windows SmartScreen may warn because the program is unsigned: "More info" > "Run anyway".
TXT
    ;;
  macos)
    d="$stage/$base-macos-universal"
    mkdir "$d"
    app="$d/Giana Sisters.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    install -m 755 "$bin" "$app/Contents/MacOS/giana-app"
    cp "$root/assets/icon/icon.icns" "$app/Contents/Resources/icon.icns"
    cat >"$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Giana Sisters</string>
  <key>CFBundleDisplayName</key><string>Giana Sisters</string>
  <key>CFBundleIdentifier</key><string>io.github.kvnshroom.giana-sisters-native</string>
  <key>CFBundleExecutable</key><string>giana-app</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
</dict>
</plist>
PLIST
    cat >"$d/INSTALL.txt" <<'TXT'
The Great Giana Sisters - native port (macOS, Apple Silicon and Intel)

Nothing to install (SDL2 is built in). The app is not signed. The first time:
right-click "Giana Sisters.app" > Open > Open, or run
    xattr -dr com.apple.quarantine "Giana Sisters.app"
On the first start it asks for your own copy of the game as a .g64 disk image
(no game data is included). See README.md.
TXT
    ;;
  *)
    echo "unknown os $os" >&2
    exit 2
    ;;
esac
cp "$root/README.md" "$root/LICENSE" "$d/"

case "$os" in
  windows)
    py=python3
    command -v python3 >/dev/null || py=python
    (cd "$stage" && "$py" -m zipfile -c "$out/$(basename "$d").zip" "$(basename "$d")")
    ;;
  *) tar -C "$stage" -czf "$out/$(basename "$d").tar.gz" "$(basename "$d")" ;;
esac
ls -l "$out"
