#!/usr/bin/env bash
# Development helper: makes data/ in the repository from your own original disk image, for building from source and
# running the tests:
#   ./tools/data/setup.sh /path/to/Great_Giana_Sisters_The.g64
# The finished game does not need this: it asks for the .g64 on first start and extracts into its own data folder.
# Only the Rust extractor is used; Python is not needed.
set -euo pipefail
if [ $# -ne 1 ]; then
    echo "usage: $0 <your original disk image .g64>" >&2
    exit 2
fi
g64="$(realpath "$1")"
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
cargo run --quiet --release -p giana-extract -- "$g64" data
cargo run --quiet --release -p giana-extract -- --check data
echo "data/ is ready: cargo build --release -p giana-app"
