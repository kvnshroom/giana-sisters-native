#!/bin/sh
# Installer for The Great Giana Sisters - native port (Linux). No root needed, no extra packages needed.
#
#   ./install.sh                     install (or update) for the current user
#   ./install.sh --modules DIR       also add the ROM modules (*.grom, or hack disk images) in DIR to the game
#   ./install.sh --g64 FILE          also import your game disk image (.g64) now
#   ./install.sh --uninstall         remove everything this installer put on the computer
#   ./install.sh --uninstall --purge also remove settings, high scores, modules and the extracted game data
#   --yes                            do not ask questions
#
# Everything created is listed in ~/.local/share/giana-native/install-manifest; uninstalling removes exactly that.
set -eu

APP=giana-sisters
data_home=${XDG_DATA_HOME:-}
[ -n "$data_home" ] || data_home=$HOME/.local/share
config_home=${XDG_CONFIG_HOME:-}
[ -n "$config_home" ] || config_home=$HOME/.config

opt_dir=$HOME/.local/opt/$APP
bin_dir=$HOME/.local/bin
apps_dir=$data_home/applications
icon_dir=$data_home/icons/hicolor/128x128/apps
mime_dir=$data_home/mime
game_data_dir=$data_home/giana-native
config_dir=$config_home/giana-native
manifest=$game_data_dir/install-manifest

say() { printf '%s\n' "$*"; }
fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

mode=install
purge=no
yes=no
modules_dir=
g64_file=
while [ $# -gt 0 ]; do
  case $1 in
    --uninstall) mode=uninstall ;;
    --purge) purge=yes ;;
    --yes | -y) yes=yes ;;
    --modules)
      [ $# -ge 2 ] || fail "--modules needs a folder"
      modules_dir=$2
      shift
      ;;
    --g64)
      [ $# -ge 2 ] || fail "--g64 needs a file"
      g64_file=$2
      shift
      ;;
    -h | --help)
      sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) fail "unknown option $1 (try --help)" ;;
  esac
  shift
done

[ -n "${HOME:-}" ] && [ -d "$HOME" ] || fail "HOME is not set to a folder."
[ "$(id -u)" != 0 ] || fail "please run this as your normal user, not as root (it installs into your home folder)."

# ---------- manifest helpers ----------
new_entries=$(mktemp)
trap 'rm -f "$new_entries"' EXIT

# mkdir -p that records every folder it really creates (parents first)
make_dir() {
  d=$1
  missing=
  while [ ! -d "$d" ]; do
    missing="$d
$missing"
    d=$(dirname "$d")
  done
  printf '%s' "$missing" | while IFS= read -r m; do
    [ -n "$m" ] || continue
    mkdir "$m"
    printf 'dir %s\n' "$m" >>"$new_entries"
  done
  return 0
}

# put a file in place (new name first, then rename, so a running game is not disturbed) and record it
put_file() { # src dest mode
  make_dir "$(dirname "$2")"
  cp "$1" "$2.tmp.$$"
  chmod "$3" "$2.tmp.$$"
  mv -f "$2.tmp.$$" "$2"
  printf 'file %s\n' "$2" >>"$new_entries"
}

safe_path() { # absolute, inside HOME, not HOME itself
  case $1 in
    "$HOME"/?*) return 0 ;;
    *) return 1 ;;
  esac
}

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
  command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$mime_dir" >/dev/null 2>&1 || true
  if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -f "$data_home/icons/hicolor/index.theme" ]; then
    gtk-update-icon-cache -q -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true
  fi
  return 0
}

ask() { # question; true when yes
  [ "$yes" = yes ] && return 0
  if [ -t 0 ]; then
    printf '%s [y/N] ' "$1"
    read -r answer || answer=n
    case $answer in y | Y | yes | YES) return 0 ;; esac
  fi
  return 1
}

# ---------- uninstall ----------
if [ "$mode" = uninstall ]; then
  if [ -f "$manifest" ]; then
    mf=$(mktemp)
    trap 'rm -f "$new_entries" "$mf"' EXIT
    cp "$manifest" "$mf"
    say "Removing the game..."
    # files and links first, then folders from the deepest up (only when empty)
    while IFS= read -r line; do
      kind=${line%% *}
      path=${line#* }
      case $kind in file | link) ;; *) continue ;; esac
      safe_path "$path" || {
        say "Skipping unexpected entry: $path"
        continue
      }
      if [ -e "$path" ] || [ -L "$path" ]; then rm -f "$path" && say "  removed $path"; fi
    done <"$mf"
    rm -f "$opt_dir/.giana-app.new" "$opt_dir/giana-app.old" 2>/dev/null || true
    sed '1!G;h;$!d' "$mf" | while IFS= read -r line; do
      [ "${line%% *}" = dir ] || continue
      path=${line#* }
      safe_path "$path" || continue
      rmdir "$path" 2>/dev/null && say "  removed folder $path" || true
    done
    rm -f "$manifest"
    refresh_caches
  else
    say "No installation record found ($manifest): nothing to remove."
  fi

  if [ "$purge" = yes ]; then
    if ask "Also delete your settings, high scores, modules and the extracted game data ($config_dir and $game_data_dir)?"; then
      for d in "$config_dir" "$game_data_dir"; do
        if [ -d "$d" ] && safe_path "$d"; then rm -rf "$d" && say "  removed $d"; fi
      done
    else
      say "Settings and game data were kept."
    fi
  else
    if [ -d "$config_dir" ] || [ -d "$game_data_dir" ]; then
      say "Kept your settings, high scores, modules and game data. To delete them too, run: $0 --uninstall --purge"
    fi
  fi
  rmdir "$game_data_dir" 2>/dev/null || true
  say "Done. The game is uninstalled."
  exit 0
fi

# ---------- install ----------
here=$(cd "$(dirname "$0")" && pwd)
[ -f "$here/giana-app" ] || fail "giana-app is not next to install.sh. Run the installer from the unpacked folder."
icon_src=$here/giana-sisters.png
[ -f "$icon_src" ] || icon_src=

case $(uname -m) in
  x86_64 | amd64) ;;
  *) fail "this package is for 64-bit Intel/AMD (x86_64) computers; this one is $(uname -m)." ;;
esac

# the old record, kept so that folders created by an earlier run are still removed later
old_entries=$(mktemp)
trap 'rm -f "$new_entries" "$old_entries"' EXIT
[ -f "$manifest" ] && cp "$manifest" "$old_entries"

if [ -f "$opt_dir/giana-app" ]; then say "Updating the game in $opt_dir ..."; else say "Installing the game for your user..."; fi

put_file "$here/giana-app" "$opt_dir/giana-app" 755
[ -n "$icon_src" ] && put_file "$icon_src" "$icon_dir/$APP.png" 644
# keep a copy of the uninstaller with the game, so it still works when the unpacked folder is gone
put_file "$here/install.sh" "$opt_dir/install.sh" 755
u=$(mktemp)
printf '#!/bin/sh\nexec "$(dirname "$0")/install.sh" --uninstall "$@"\n' >"$u"
put_file "$u" "$opt_dir/uninstall.sh" 755
rm -f "$u"

make_dir "$bin_dir"
ln -sf "$opt_dir/giana-app" "$bin_dir/$APP"
printf 'link %s\n' "$bin_dir/$APP" >>"$new_entries"

# file types, so the .g64 / .grom "Open with" works
mime_xml=$(mktemp)
cat >"$mime_xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-g64-disk-image">
    <comment>C64 disk image (G64)</comment>
    <glob pattern="*.g64"/>
    <glob pattern="*.G64"/>
  </mime-type>
  <mime-type type="application/x-giana-module">
    <comment>Giana Sisters ROM module</comment>
    <glob pattern="*.grom"/>
  </mime-type>
</mime-info>
EOF
put_file "$mime_xml" "$mime_dir/packages/$APP.xml" 644
rm -f "$mime_xml"

desktop=$(mktemp)
cat >"$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Giana Sisters
GenericName=Platform game
Comment=The Great Giana Sisters - native port
Exec="$bin_dir/$APP" %f
Icon=$APP
Terminal=false
Categories=Game;ArcadeGame;
MimeType=application/x-g64-disk-image;application/x-giana-module;
StartupWMClass=giana-app
EOF
put_file "$desktop" "$apps_dir/$APP.desktop" 644
rm -f "$desktop"

# write the manifest: old entries first (parents of folders are listed before their children), then new, no duplicates
make_dir "$game_data_dir"
rm -f "$manifest.tmp"
cat "$old_entries" "$new_entries" | while IFS= read -r line; do
  grep -qxF -- "$line" "$manifest.tmp" 2>/dev/null || printf '%s\n' "$line" >>"$manifest.tmp"
done
[ -f "$manifest.tmp" ] || : >"$manifest.tmp"
# the manifest itself and the folder for it are recorded too
if ! grep -qx "file $manifest" "$manifest.tmp"; then printf 'file %s\n' "$manifest" >>"$manifest.tmp"; fi
mv -f "$manifest.tmp" "$manifest"

refresh_caches

# ---------- optional extras ----------
if [ -n "$g64_file" ]; then
  [ -f "$g64_file" ] || fail "disk image not found: $g64_file"
  if "$opt_dir/giana-app" --import "$g64_file"; then
    say "Your game disk image was imported."
  else
    say "The disk image could not be imported (see the message above). The game will ask for it on the first start."
  fi
fi

if [ -n "$modules_dir" ]; then
  [ -d "$modules_dir" ] || fail "modules folder not found: $modules_dir"
  # the game checks each module against your own game data and stores it under its id (so it needs the game data:
  # the disk image is imported first)
  count=0
  failed=0
  for f in "$modules_dir"/*.grom "$modules_dir"/*.d64 "$modules_dir"/*.g64; do
    [ -f "$f" ] || continue
    if "$opt_dir/giana-app" --add-module "$f"; then count=$((count + 1)); else failed=$((failed + 1)); fi
  done
  say "Added $count ROM module(s)."
  [ "$failed" -eq 0 ] || say "$failed module(s) could not be added (see above). Without your game data imported, modules cannot be added yet: run this again with --g64 FILE."
fi

path_hint=
case ":$PATH:" in *":$bin_dir:"*) ;; *) path_hint=yes ;; esac
say ""
say "Installed. Start it from your applications menu (\"Giana Sisters\") or by typing: $APP"
[ -z "$path_hint" ] || say "(Typing $APP works after you log out and in again, or run it as $bin_dir/$APP.)"
[ -n "$g64_file" ] || say "On the first start the game asks for your own copy of the game as a .g64 disk image."
say "To uninstall later: $opt_dir/uninstall.sh   (add --purge to also delete settings and game data)"
