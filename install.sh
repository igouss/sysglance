#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")" && pwd)
cargo build --release --locked --manifest-path "$repo/Cargo.toml"
install -Dm755 "$repo/target/release/sysglance" "$HOME/.local/bin/sysglance"
install -Dm644 "$repo/system/sysglance.desktop" "$HOME/.local/share/applications/sysglance.desktop"
sudo install -Dm644 "$repo/system/60-sysglance-rapl.rules" /etc/udev/rules.d/60-sysglance-rapl.rules
sudo install -Dm644 "$repo/system/sysglance-modules.conf" /etc/modules-load.d/sysglance.conf
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=powercap --action=add
sudo modprobe -a drivetemp msr
echo "installed ~/.local/bin/sysglance"
