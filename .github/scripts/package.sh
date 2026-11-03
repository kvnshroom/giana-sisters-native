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
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>C64 disk image (G64)</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>CFBundleTypeExtensions</key>
      <array><string>g64</string></array>
      <key>LSItemContentTypes</key>
      <array><string>io.github.kvnshroom.giana-sisters-native.g64</string></array>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>io.github.kvnshroom.giana-sisters-native.g64</string>
      <key>UTTypeDescription</key><string>C64 disk image (G64)</string>
      <key>UTTypeConformsTo</key>
      <array><string>public.data</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key>
        <array><string>g64</string></array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
PLIST
    # ad-hoc signature (no developer identity): the bundle is at least consistently signed, but still not notarized
    if command -v codesign >/dev/null; then
      codesign --force --deep --sign - "$app"
    fi
    cat >"$d/INSTALL.txt" <<'TXT'
The Great Giana Sisters - native port (macOS, Apple Silicon and Intel)

Nothing to install (SDL2 is built in). The app is not notarized by Apple, so macOS
blocks it the first time. To allow it:
  1. Double-click "Giana Sisters.app" once. macOS says it cannot be opened. Click "Done".
  2. Open System Settings > Privacy & Security, scroll down to "Security" and click
     "Open Anyway" next to "Giana Sisters" (macOS 15 and later; older versions: right-click
     the app > Open > Open). Confirm with your password.
  Or in Terminal, once:   xattr -dr com.apple.quarantine "Giana Sisters.app"

On the first start click "CHOOSE FILE..." (or press Enter) and pick your own copy of the game
as a .g64 disk image. You can also drop the .g64 on the window or on the app icon.
No game data is included. See README.md.
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
