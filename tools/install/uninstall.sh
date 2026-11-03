#!/bin/sh
# Removes the game again (same as: ./install.sh --uninstall). Add --purge to also delete settings and game data.
exec "$(dirname "$0")/install.sh" --uninstall "$@"
