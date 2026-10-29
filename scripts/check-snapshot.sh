#!/usr/bin/env bash
set -euo pipefail
repo=$(git rev-parse --show-toplevel)
revision=${1:-}
snapshot=$(mktemp -d)
trap 'rm -rf "$snapshot"' EXIT
if [ -n "$revision" ]; then
    git -C "$repo" archive "$revision" | tar -x -C "$snapshot"
else
    git -C "$repo" checkout-index --all --prefix="$snapshot/"
fi
CARGO_TARGET_DIR="$repo/target/snapshot" "$snapshot/scripts/check.sh"
