#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
tree=$(cargo tree --locked --edges normal,build --prefix none --target x86_64-unknown-linux-gnu)
if x11=$(grep -E '^(x11|x11rb|x11-dl|x11-clipboard|xcb|xim|xkbcommon-x11)[ -]' <<<"$tree" | sort -u) && [ -n "$x11" ]; then
    echo "wayland-only: X11 crates in the dependency tree:" >&2
    echo "$x11" >&2
    exit 1
fi
