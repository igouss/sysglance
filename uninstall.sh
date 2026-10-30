#!/usr/bin/env bash
set -euo pipefail
rm -f "$HOME/.local/bin/sysglance" "$HOME/.local/share/applications/sysglance.desktop"
echo "sysglance: removed the binary and desktop entry"
system=(/etc/sudoers.d/sysglance-helper /usr/local/libexec/sysglance-helper /etc/udev/rules.d/60-sysglance-rapl.rules /etc/modules-load.d/sysglance.conf)
present=()
for file in "${system[@]}"; do
    if sudo test -e "$file" 2>/dev/null; then
        present+=("$file")
    fi
done
if [ "${#present[@]}" -eq 0 ]; then
    exit 0
fi
sudo rm -f "${present[@]}"
if command -v udevadm >/dev/null; then
    sudo udevadm control --reload
fi
printf 'sysglance: removed %s\n' "${present[@]}"
echo "sysglance: energy counters return to root-only at next boot; loaded modules stay loaded until then"
