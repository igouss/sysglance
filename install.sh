#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")" && pwd)
cargo build --release --locked --manifest-path "$repo/Cargo.toml" -p sysglance -p sysglance-helper
visudo -cf "$repo/system/sysglance-helper.sudoers"
install -Dm755 "$repo/target/release/sysglance" "$HOME/.local/bin/sysglance"
install -Dm644 "$repo/system/sysglance.desktop" "$HOME/.local/share/applications/sysglance.desktop"
sudo install -Dm644 "$repo/system/60-sysglance-rapl.rules" /etc/udev/rules.d/60-sysglance-rapl.rules
sudo install -Dm644 "$repo/system/sysglance-modules.conf" /etc/modules-load.d/sysglance.conf
sudo install -Dm755 -o root -g root "$repo/target/release/sysglance-helper" /usr/local/libexec/sysglance-helper
sudo install -Dm440 -o root -g root "$repo/system/sysglance-helper.sudoers" /etc/sudoers.d/sysglance-helper
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=powercap --action=add
sudo modprobe -a drivetemp msr
echo "installed ~/.local/bin/sysglance"
