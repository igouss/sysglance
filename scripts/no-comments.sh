#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
found=0
while IFS= read -r -d '' file; do
    if grep -nE '^\s*//|/\*' "$file"; then
        echo "  in $file" >&2
        found=1
    fi
done < <(git ls-files -z --cached --others --exclude-standard -- '*.rs')
while IFS= read -r -d '' file; do
    if grep -nE '^\s*#([^!\[]|$)' "$file"; then
        echo "  in $file" >&2
        found=1
    fi
done < <(git ls-files -z --cached --others --exclude-standard -- '*.toml' '*.sh' '*.rules')
if [ "$found" -ne 0 ]; then
    echo "no-comments: code carries no comments; the why goes in docs/ (cm-k7c)" >&2
    exit 1
fi
