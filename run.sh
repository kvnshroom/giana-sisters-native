#!/bin/bash
cd "$(dirname "$(readlink -f "$0")")"
if [ ! -x target/release/giana-app ] || [ -n "$(find crates -name '*.rs' -newer target/release/giana-app -print -quit)" ]; then
    cargo build --release -p giana-app || exit 1
fi
exec ./target/release/giana-app "$@"
