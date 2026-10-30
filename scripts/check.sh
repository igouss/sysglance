#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/no-comments.sh
scripts/wayland-only.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
if command -v hex-lint >/dev/null; then
    hex-lint
else
    echo "check: hex-lint is not installed; skipped the dependency-direction check" >&2
fi
