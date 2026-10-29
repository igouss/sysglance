# sysglance

A GPUI window that shows every temperature, voltage, fan, clock, power, load and memory counter this Linux machine exposes, once a second, with a two-minute sparkline per reading. Readings at or past their driver's own limits turn amber or red. A reading that can't be read says why. Hovering any row shows its sysfs source and limits in the footer.

It follows the Omarchy theme and font and recolours itself within a second of `omarchy-theme-set`. It is Wayland-only.

Sources so far: hwmon (every chip: Super I/O, CPU, DIMMs, NVMe, SATA via `drivetemp`, GPU, Wi-Fi), RAPL power and limits, CPU load, clocks, idle states and throttle counters, RAM, swap, paging and zram, pressure stall (PSI), DIMM SPD, CPU registers (what limits the clock, core voltage, TjMax, turbo ladder, SMIs, hardware C-state residency) and SMART health for every drive.

## Install

```sh
./install.sh
```

This builds the release binary into `~/.local/bin` and adds a desktop entry. With sudo it then:

- installs a udev rule that lets group `wheel` read RAPL energy counters;
- installs `sysglance-helper` to `/usr/local/libexec` with a sudoers rule that lets `wheel` run it, with no arguments, to read CPU registers and SMART;
- loads `drivetemp` and `msr` at boot.

SMART needs `smartctl` (smartmontools).

## Check

```sh
scripts/check.sh                   # comments, Wayland-only deps, fmt, clippy, tests, hex-lint
cargo test -- --ignored            # this machine: hwmon against `sensors -j`, every Omarchy theme
sysglance --bench-scroll           # frame times while scrolling; main-thread CPU per frame
```

See [docs/design.md](docs/design.md) for the invariants, the privilege model, the theme mapping and the frame-time budget.
