#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin/sysglance"
desktop="$HOME/.local/share/applications/sysglance.desktop"
libexec=/usr/local/libexec
helper="$libexec/sysglance-helper"
sudoers=/etc/sudoers.d/sysglance-helper
rapl=/etc/udev/rules.d/60-sysglance-rapl.rules
modules=/etc/modules-load.d/sysglance.conf

say() { printf 'sysglance: %s\n' "$*"; }

RUSTFLAGS="${RUSTFLAGS:--C target-cpu=native}" cargo build --release --locked \
    --manifest-path "$repo/Cargo.toml" -p sysglance -p sysglance-helper
install -Dm755 "$repo/target/release/sysglance" "$bin"
mkdir -p "$(dirname "$desktop")"
sed "s|@EXEC@|$bin|" "$repo/system/sysglance.desktop" > "$desktop"
say "installed $bin and a desktop entry"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) say "note: $HOME/.local/bin is not on PATH; the desktop entry still works" ;;
esac

if [ "${1:-}" = "--user-only" ]; then
    say "skipped the privileged helper (--user-only); MSR, SMART and RAPL readings say how to enable them"
    exit 0
fi
if ! command -v sudo >/dev/null; then
    say "sudo not found; skipped the privileged helper. sysglance runs without MSR, SMART and RAPL readings"
    exit 0
fi

group=""
for candidate in wheel sudo admin; do
    if id -nG | tr ' ' '\n' | grep -qx "$candidate"; then
        group=$candidate
        break
    fi
done
if [ -z "$group" ]; then
    say "you are in none of the groups wheel, sudo or admin; skipped the privileged helper"
    exit 0
fi
say "privileged readings will be enabled for group $group"

for dir in / /usr /usr/local "$libexec"; do
    [ -e "$dir" ] || continue
    read -r owner mode < <(stat -c '%u %a' "$dir")
    if [ "$owner" != 0 ] || [ $((8#$mode & 8#022)) -ne 0 ]; then
        say "$dir is not root-owned or is group/world-writable; a passwordless rule on a file inside it would be a root shell. Aborting the privileged part."
        exit 1
    fi
done

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
sed -e "s|@GROUP@|$group|" -e "s|@HELPER@|$helper|" "$repo/system/sysglance-helper.sudoers" > "$scratch/sudoers"
sudo visudo -cqf "$scratch/sudoers"
baseline=ok
if ! sudo visudo -cq; then
    baseline=broken
    say "sudo's configuration already fails 'visudo -c' before this install; run it to see which file"
fi
sudo install -Dm755 -o root -g root "$repo/target/release/sysglance-helper" "$helper"
sudo install -Dm440 -o root -g root "$scratch/sudoers" "$sudoers"
if [ "$baseline" = ok ] && ! sudo visudo -cq; then
    sudo rm -f "$sudoers"
    say "sudo rejected its configuration with the new rule; removed it again"
    exit 1
fi
if sudo -l -U "$USER" "$helper" >/dev/null 2>&1; then
    say "installed $helper; $group may run it without a password"
else
    say "installed $helper, but sudo does not grant it to $USER (is /etc/sudoers.d included?)"
fi

if command -v smartctl >/dev/null || [ -x /usr/sbin/smartctl ]; then
    :
else
    say "smartctl not found; install smartmontools for SMART readings"
fi

loaded=()
for module in drivetemp msr; do
    if sudo modprobe "$module" 2>/dev/null; then
        loaded+=("$module")
    else
        say "kernel module $module is not available; its readings stay unavailable"
    fi
done
if [ "${#loaded[@]}" -gt 0 ]; then
    printf '%s\n' "${loaded[@]}" | sudo install -Dm644 /dev/stdin "$modules"
    say "loading ${loaded[*]} at boot"
fi

if command -v udevadm >/dev/null && [ -d /sys/class/powercap ]; then
    sed -e "s|@GROUP@|$group|" -e "s|@CHGRP@|$(command -v chgrp)|" -e "s|@CHMOD@|$(command -v chmod)|" \
        "$repo/system/60-sysglance-rapl.rules" > "$scratch/rapl.rules"
    sudo install -Dm644 "$scratch/rapl.rules" "$rapl"
    sudo udevadm control --reload
    sudo udevadm trigger --subsystem-match=powercap --action=add
    say "RAPL energy counters are readable by $group (this re-opens the PLATYPUS power side channel to that group; see README)"
else
    say "no udev or no powercap; skipped the RAPL rule"
fi
