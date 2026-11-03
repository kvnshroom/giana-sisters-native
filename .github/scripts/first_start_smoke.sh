#!/usr/bin/env bash
# Starts the game like a player does the very first time: no game data anywhere, no display (SDL dummy driver).
# It has to reach the first-start page and keep running; a crash there (as in 0.1.1, whose page needed the game's
# own font) fails this check.
#   first_start_smoke.sh <path to giana-app>
set -uo pipefail
bin="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/home"
cp "$bin" "$tmp/bin/"
cd "$tmp/home"
export HOME="$tmp/home" XDG_DATA_HOME="$tmp/home/.local/share" XDG_CONFIG_HOME="$tmp/home/.config" APPDATA="$tmp/home/AppData"
export SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy
"$tmp/bin/$(basename "$bin")" --no-update-check >"$tmp/out.txt" 2>&1 &
pid=$!
sleep 4
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"
  wait "$pid" 2>/dev/null
  running=yes
else
  wait "$pid"
  echo "exit code $?"
  running=no
fi
cat "$tmp/out.txt"
if [ "$running" != yes ] || grep -q panicked "$tmp/out.txt" || ! grep -q "Asking for the disk image" "$tmp/out.txt"; then
  echo "first start without game data failed" >&2
  exit 1
fi
echo "first start without game data: ok"
