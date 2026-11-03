#!/usr/bin/env bash
# Tests the Linux package and its installer on several distributions in containers (headless, SDL dummy drivers):
# install as a normal user, --version, a 4 second first start, uninstall (nothing may be left), purge.
#   tools/install/test_in_containers.sh [path/to/giana-app] [image ...]
# Default binary: target/portable/giana-app (see tools/release/build_linux_portable.sh). Default images: six distributions.
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
engine="${CONTAINER_ENGINE:-podman}"
bin="${1:-$root/target/portable/giana-app}"
[ $# -gt 0 ] && shift
images=("$@")
[ ${#images[@]} -gt 0 ] || images=(
  docker.io/library/ubuntu:22.04 docker.io/library/ubuntu:24.04 docker.io/library/debian:stable
  docker.io/library/fedora:latest docker.io/archlinux/archlinux:latest docker.io/opensuse/tumbleweed:latest
)
[ -x "$bin" ] || {
  echo "no binary at $bin" >&2
  exit 2
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$root/.github/scripts/package.sh" linux 0.0.0-test "$bin" "$work/dist" >/dev/null
mkdir "$work/pkg"
tar -C "$work/pkg" -xzf "$work"/dist/*.tar.gz
pkgdir="$(echo "$work"/pkg/*)"
cp "$root/.github/scripts/first_start_smoke.sh" "$work/smoke.sh"

# runs inside the container as an ordinary user with an empty home folder
cat >"$work/inner.sh" <<'INNER'
set -eu
export HOME=/home/tester SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy
export PATH="$HOME/.local/bin:$PATH"
unset XDG_DATA_HOME XDG_CONFIG_HOME
[ "$(id -u)" != 0 ] || { echo "must not be root"; exit 1; }
# find is not in every minimal image: list everything below $1 with plain shell
walk() { for f in "$1"/* "$1"/.[!.]*; do [ -e "$f" ] || [ -L "$f" ] || continue; echo "$f"; [ -d "$f" ] && [ ! -L "$f" ] && walk "$f"; done; return 0; }
step() { echo "--- $*"; }
cd "$HOME"
cp -r /pkg ./pkg && cd pkg

step "install (twice: must be idempotent)"
mkdir -p "$HOME/mods" && printf 'x' >"$HOME/mods/test.grom"
sh ./install.sh --modules "$HOME/mods" >/dev/null
sh ./install.sh >out.txt && cat out.txt
for f in .local/opt/giana-sisters/giana-app .local/bin/giana-sisters .local/share/applications/giana-sisters.desktop \
         .local/share/icons/hicolor/128x128/apps/giana-sisters.png .local/share/giana-native/install-manifest; do
  [ -e "$HOME/$f" ] || { echo "missing $f"; exit 1; }
done
grep -q 'Exec="/home/tester/.local/bin/giana-sisters" %f' "$HOME/.local/share/applications/giana-sisters.desktop"
[ "$(sort "$HOME/.local/share/giana-native/install-manifest" | uniq -d | wc -l)" = 0 ] || { echo "duplicate manifest lines"; exit 1; }

step "run"
giana-sisters --version
bash /smoke.sh /home/tester/.local/opt/giana-sisters/giana-app
no_sdl="$(ldd "$HOME/.local/opt/giana-sisters/giana-app" 2>/dev/null | grep -ci 'sdl' || true)"
echo "libSDL in ldd: $no_sdl"

step "uninstall (from the installed copy)"
sh "$HOME/.local/opt/giana-sisters/uninstall.sh"
left="$(walk "$HOME" | grep -v -E '^/home/tester/pkg|^/home/tester/mods|^/home/tester/\.config|^/home/tester/\.cache' || true)"
if [ -n "$left" ]; then echo "LEFT OVER after uninstall:"; echo "$left"; exit 1; fi
[ -f "$HOME/.config/giana-native/modules/test.grom" ] || { echo "module should be kept without --purge"; exit 1; }

step "purge"
sh ./install.sh >/dev/null
sh ./install.sh --uninstall --purge --yes
left="$(walk "$HOME" | grep -v -E '^/home/tester/pkg|^/home/tester/mods|^/home/tester/\.config$|^/home/tester/\.cache' || true)"
if [ -n "$left" ]; then echo "LEFT OVER after purge:"; echo "$left"; exit 1; fi
echo "INNER OK"
INNER

fail=0
for img in "${images[@]}"; do
  echo "=================== $img"
  if ! "$engine" image exists "$img" && ! "$engine" pull -q "$img" >/dev/null 2>&1; then
    echo "SKIPPED $img (cannot be pulled)"
    continue
  fi
  if "$engine" run --rm --user 1000:1000 --tmpfs /home/tester:rw,mode=1777,exec \
    -v "$pkgdir":/pkg:ro,Z -v "$work/inner.sh":/inner.sh:ro,Z -v "$work/smoke.sh":/smoke.sh:ro,Z \
    "$img" sh /inner.sh </dev/null >"$work/log.txt" 2>&1; then
    tail -n 3 "$work/log.txt"
    echo "PASS $img"
  else
    cat "$work/log.txt"
    echo "FAIL $img"
    fail=1
  fi
done
exit $fail
