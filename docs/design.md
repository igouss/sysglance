# sysglance design

sysglance shows every sensor and counter a Linux computer exposes in one GPUI window, refreshed once a second. Each section below states an invariant or decision, the force behind it, and the gate that enforces it where there is one.

## Shape

- `core/` (`sysglance-core`, hex role `domain`): readings, units, limits, levels, counter rates, history and MSR decoding.
  - It does no I/O. Its `Cargo.toml` lists no dependencies, and `core/clippy.toml` bans file, process, network, clock and thread calls.
  - Gate: `scripts/check.sh` (clippy `-D warnings`, `hex-lint`).
- `helper/` (`sysglance-helper`, hex role `composition-root`): the root-side reader for MSRs and SMART. It is a separate crate so that the code running as root links neither GPUI nor anything else the window needs. See Privilege.
- `src/sources/`: one adapter per kernel interface.
  - Each takes its root directory as a parameter, so tests run against fixture trees in a temp dir.
  - Each implements `Source::collect(elapsed)`. Counter sources keep their previous raw value to turn it into a rate.
- `src/sampler.rs`: one thread collects every source each second and publishes the arranged devices with a generation number.
- `src/view.rs`: GPUI with `gpui-component` (theme, scrollbar). It polls the generation every 100 ms and redraws only when it changes, so an idle window costs no GPU time.
- **Frames do no formatting.** Force: scrolling redraws every frame. When a sample arrives, `src/view/model.rs` builds a display model: shared strings, and each sparkline rasterised once into a small image (`src/spark.rs`). A frame only places it.
- **A card is one painted element.**
  - Force: a large window has about 210 rows on screen at once. GPUI's per-element style, layout and hitbox work for about 6 elements per row cost 8–11 ms of main-thread CPU per frame, so frames slipped to 25–45 ms.
  - `src/view/paint.rs` paints a card's title, rows, values and sparkline images in one canvas, the way Zed's editor paints its lines.
  - Sparklines drawn as quads cost CPU (40 scene primitives per row), and drawn as paths they cost GPU time on an integrated GPU. As images they are one sprite each.
  - Hovering a row shows its source, limits and reason in the footer.
- **Only cards near the viewport are built.** Force: the same. `src/layout.rs` packs cards into masonry columns from their known heights (rows have fixed heights). A frame builds elements only for cards within 400 px of the visible area. Gate: `src/layout/tests.rs`.

## Invariants

- **A limit of zero is unset.**
  - Force: drivers fill unset limits with 0. For example, jc42 reports `max = crit = 0` and NCT67xx chips report `min = max = 0` on most voltage inputs, so their alarm bits read tripped.
  - `Limits::new` drops zero and non-finite limits. Gate: `core/src/level.rs` tests.
  - The same force leaves out MSRs that read zero where zero is meaningless (VID, TjMax, the turbo ladder, the base clock). Virtual machines report them that way.
- **An alarm counts only against a limit that is set.** Same force: a tripped `max_alarm` on an unset `max` is a false alarm. Gate: `Limits::trusted` tests.
- **A level is computed from the value it describes.** `Value::measured` derives the level from the measure and limits, so the two cannot disagree. The one exception is an ATA SMART attribute; see SMART.
- **An unavailable reading says why.**
  - `Unavailable` is one of:
    - `Pending`: a rate needs a second sample;
    - `NeedsPrivilege(how)`;
    - `ModuleNotLoaded`;
    - `NotSupported`;
    - `Unreadable(error)`.
  - The footer shows the reason and the source.
  - A source with nothing to show either leaves its card out (no cpufreq, no hwmon chips) or shows one card saying why: no PSI in the kernel, the helper missing or silent, smartctl missing.
- **Nothing assumes one machine.**
  - Force: sysglance is public and runs on whatever Linux computer starts it. Every source reads what exists and skips what doesn't.
  - Machine-dependent facts are detected at runtime:
    - the cpufreq driver and every policy's range;
    - turbo through `intel_pstate` or `cpufreq/boost`;
    - online CPUs, physical cores and sockets;
    - the kernel's page size, from `AT_PAGESZ` in `/proc/self/auxv`;
    - the CPU model, for MSR decoding;
    - the Omarchy theme;
    - whether the helper is installed;
    - where smartctl lives.
  - Gate: tests over fixture trees for each case (offline CPUs, two sockets, hybrid policies, `boost`, 16K and 64K pages, laptop PL1).
- **Code carries no comments.** The why lives in this document. Gate: `scripts/no-comments.sh`, which `scripts/check.sh` runs.
- **Wayland only.**
  - Force: X11 support would double the windowing code paths for a monitor whose users run Wayland compositors.
  - Every GPUI crate is built with `default-features = false, features = ["wayland"]`. Started without `WAYLAND_DISPLAY`, sysglance says so and exits 1.
  - Gate: `scripts/wayland-only.sh` fails when any X11 crate (`x11`, `x11rb`, `xcb`, `xim`, ...) is in the resolved dependency tree. It goes red when `x11` is enabled.
- **Every commit passes the gate as committed.**
  - Force: a check of the working tree passes a commit that leaves a needed change unstaged.
  - The pre-commit hook (`.githooks/pre-commit`, enabled with `git config core.hooksPath .githooks`) runs `scripts/check-snapshot.sh`. That script exports the index to a temp dir and runs `scripts/check.sh` there.
  - `scripts/check-snapshot.sh <rev>` checks any past commit the same way.

## Theme

Without Omarchy, the window uses gpui-component's dark theme and the default font.

On Omarchy:
- `src/omarchy.rs` reads `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (default `~/.local/state`).
- `src/view/theme.rs` checks that file every second and applies it when it changes, so `omarchy-theme-set` recolours the open window within a second.
- The font is the one `omarchy-font-current` reports.

`colors.toml` keys mean different things in light and dark themes, so the mapping depends on `mode`. For example, `light_foreground` is the strong text colour in a light theme, and the `white` theme's `lighter_background` is darker than its background.

| sysglance | dark theme | light theme |
|---|---|---|
| window | `background` | `background` |
| card | `lighter_background` | `dark_background` |
| border | `selection` | `selection` |
| text | `foreground` | `foreground` |
| labels | `light_foreground` | `light_foreground` |
| faint text | `dark_foreground` | `dark_foreground` |
| normal / low / high / critical | `green` / `blue` / `yellow` / `red` | same |

Gates:
- `src/omarchy/tests.rs`.
- `cargo test omarchy -- --ignored` parses every theme under `/usr/share/omarchy/themes` on an Omarchy system.

## Frame time

`sysglance --bench-scroll` scrolls the view 37 px every frame for 900 frames. It then prints fps, frame-time percentiles and main-thread CPU per frame, and exits.

- **Compare changes by main-thread CPU per frame.** Wall-clock fps swings widely with other load on the machine. CPU time per frame drifts too, so compare two builds only by interleaving their runs in one sitting. A number from an earlier session is not a baseline.
- **Painted cards hold 60 fps.** On a 2560×1440 window on an Intel UHD 630, painted cards run at 60 fps with about 4 ms of main-thread CPU per frame. Per-row elements took 8–11 ms.

For a CPU profile:
1. Build with `cargo build --profile profiling` (release with line tables, unstripped).
2. Run `perf record -g --call-graph dwarf target/profiling/sysglance --bench-scroll`.

## Known-answer check

`cargo test live -- --ignored` reads the running machine's `/sys/class/hwmon` and runs `sensors -j` in the same second.
- Every lm_sensors `*_input` must match a sysglance reading with the same label: temperatures within 3 °C, fans within 150 rpm, voltages within 50 mV.
- It goes red when the temperature scale is wrong by a factor of ten.

## Privilege

The window runs unprivileged and works without anything below. `install.sh --user-only` installs nothing privileged. Each privileged reading names what would enable it.

### RAPL

RAPL `energy_uj` is root-only by default because of the PLATYPUS power side channel (CVE-2020-8694).
- `install.sh` detects the admin group the installing user is in (`wheel`, `sudo` or `admin`). It fills that group into `system/60-sysglance-rapl.rules`, which makes `intel-rapl*` counters readable by that group.
- This re-opens the side channel to that group's processes. README says so.
- Without the rule, power readings say `needs privilege`.

### The helper

**The helper is the only privileged code, and it decides nothing.**
- Force: MSRs and SMART are root-only. MSRs need `/dev/cpu/<n>/msr`, mode 0600, and `CAP_SYS_RAWIO`. SMART needs smartctl's NVMe admin and ATA pass-through ioctls.
- `helper/` (`sysglance-helper`) is a std-only binary that links nothing but `sysglance-core`. It takes no arguments and no input.
- It reads the fixed register list in `core/src/msr.rs`: `PACKAGE` on cpu0 and `PER_CPU` on every CPU. It never writes an MSR, so the sticky log bits it reads stay as the hardware set them.
- It looks for smartctl in `/usr/sbin`, then `/usr/bin`, both root-owned. It runs `smartctl --scan-open` and then, for each drive, `smartctl --json=c -a -n standby -d <type> <path>`, with the type the scan reported.
  - The type must match `[A-Za-z0-9,+_]+`, so it can't become an option.
  - Passing the scanned type matters. A SATA disk scans as `sat` and reports its ATA attributes. Forcing `-d scsi` loses them. Drives behind a RAID controller share one path and differ only by type (`megaraid,0`, `megaraid,1`).
- SMART runs on its own thread every 30 s, with a 20 s timeout per drive, so a hung drive never stops register frames.
- Wire format: one text line per fact, and `end` closes a frame.
  - `msr <cpu> <address> <value>`
  - `smart-round` opens a complete set of drives. The window replaces its set, so an unplugged drive's card goes away.
  - `smart <path> <type> <json>`
  - `smart-error <path> <type> <why>`
  - `smart-unavailable <why>`
- The helper exits when the window goes away: its next write fails with a broken pipe.
- All decoding happens unprivileged, in `src/sources/msr.rs` and `src/sources/smart.rs`.
- Gate: `helper/src/tests.rs`, which runs a fake smartctl, including one that hangs.

### sudo, not file capabilities

- File capabilities would need `CAP_SYS_RAWIO` plus a udev rule on the msr devices. smartctl's ioctls need `CAP_SYS_ADMIN`, which is root in all but name.
- `system/sysglance-helper.sudoers` is filled in with the detected group. It lets that group run exactly `/usr/local/libexec/sysglance-helper` with no arguments (`""`) and no password.
- `install.sh` protects that rule:
  - It refuses unless `/`, `/usr`, `/usr/local` and `/usr/local/libexec` are root-owned and not group- or world-writable. A NOPASSWD rule on a file someone else can replace would be a root shell.
  - It checks the rule with `visudo -c` before installing it.
  - It checks the whole sudo configuration again afterwards, and removes the rule if the configuration went from valid to invalid.
  - It confirms the grant with `sudo -l`.
- `uninstall.sh` removes the sudoers rule first, then the helper, the udev rule and the module list.

### One helper per window

- `src/sources/helper.rs` checks that the helper file exists, then starts `sudo -n` once. sudo logs one line per launch rather than one a second.
- A reader thread folds the stream into a `Link` with `evolve`. The states are `Starting`, `Streaming`, `Refused` and `Lost`. The time of the last frame is kept beside the link, outside `evolve`.
- What the card shows:
  - A missing helper, or one that exits before its first frame, shows `needs privilege` with the reason.
  - One that dies later shows `unreadable`.
  - One that sends nothing for 5 s shows only `unreadable`, never its last values as if they were current.
- Gate: `src/sources/helper/tests.rs`.

### Modules

`install.sh` loads `drivetemp` (SATA drive temperatures through hwmon) and `msr`, and lists for boot only the ones the running kernel has.

## MSRs

Register meanings are model-specific. Registers are decoded only on Skylake-client models (`SKYLAKE_CLIENT_MODELS`: Skylake, Kaby Lake, Coffee Lake and Comet Lake), the models turbostat gives the same feature table. On any other CPU the card says `not supported`. These are single-socket parts, so package registers come from cpu0.

Readings the kernel already exposes elsewhere are left out: per-core temperatures (coretemp), clocks (cpufreq derives them from APERF/MPERF) and energy (powercap).

- **Limit reasons** (`0x64F`): the low 16 bits say what limits the clock now, the high 16 what has limited it since boot.
  - Assumption: turbostat does not decode this register on Skylake-client parts, so the bit names follow Intel's SDM table for it: PROCHOT, thermal, residency regulation, average thermal, VR thermal, VR current, electrical, PL1, PL2, max turbo, transitions.
  - Known answer on an i7-10700K under all-core load: `max turbo` at 4.7 GHz, the turbo ladder's eight-core ratio.
- **Package thermal status** (`0x1B1`): each status bit has its log bit one place above it.
- **Core voltage** (`0x198` bits 47:32 ÷ 8192): the VID the core requests, not the voltage measured at the socket.
- **TjMax and TCC offset** (`0x1A2`).
- **Turbo ladder** (`0x1AD`), shown as runs of active-core counts: `1-3:5.1 4-5:4.8 6-8:4.7 GHz`.
- **Package C-state limit** (`0xE2`), using turbostat's Haswell-and-later table. A board whose firmware limits the package to PC0 never advances the package residency counters. The card leaves out any residency counter that is still zero.
- **SMIs** (`0x34`) as a rate.
  - Elapsed time is the TSC delta over the nominal base clock (`0xCE` ratio × 100 MHz).
  - A TSC that runs slightly off nominal skews the rate by the same fraction, about 0.2 % on the i7-10700K (3792 against 3800 MHz).
- **Core C-state residency** (`0x3FC`–`0x3FE`) is the hardware's own count, read once per online physical core. It can differ from cpuidle's requested-state time because the hardware demotes requests.

## SMART

There is one card per drive. It is titled by the model and sits next to that drive's hwmon temperature card. Temperatures stay with hwmon, so SMART does not repeat them.

- **Health flags.** Overall health and NVMe `critical_warning` are flags whose critical limit is 1, so a failure is red. The warning's bits are named in the footer.
- **Spare available** is rated against the drive's own threshold.
- **Endurance used** is rated High at 100 %. Assumption: the NVMe spec defines 100 % as the rated life used up, and smartctl reports ATA wear on the same scale. A drive at 99 % shows as normal.
- **ATA attributes.** An attribute row shows the raw value. The footer gives the normalised value, the worst value and the threshold.
  - **The level comes from smartctl's verdict, not from the number shown.** Force: the drive compares its normalised value, not the raw count, with its threshold.
  - `when_failed = now` is Critical. Assumption: `past` is High.
  - An attribute with a threshold is otherwise Normal; one without a threshold is unrated.
  - This is the one exception to "a level is computed from the value it describes".
- **Exit status.** smartctl's exit status is a bit mask.
  - Bits 0–2 (bad command line, device not opened or asleep, a command failed) make the report unreadable, and the card shows smartctl's message.
  - The exception is a drive in a low-power mode. smartctl prints `Device is in <MODE> mode, exit(<n>)` for it, and the card says `asleep, not woken`.
  - The higher bits are findings already in the report, such as bit 6 (error-log entries).
- **No SMART.** A drive that reports `smart_support.available: false` shows `not supported`.
- **Fixtures.** Committed fixtures have serial number, WWN and time removed.

## Power limits

- The package card shows PL1 (`long_term`) and PL2 (`short_term`) from powercap.
- A limit of 4000 W or more is shown as `unlimited`. Desktop firmware writes values like 4095.875 W when it stops enforcing Intel's limits.
- **The package power's high limit is the enforced PL1, or the rated power (`constraint_0_max_power_uw`, TDP) when PL1 is unlimited.**
  - Assumption: sustained draw at the limit the CPU actually enforces is what deserves amber.
  - Laptops often set PL1 above TDP, and rating them against TDP would show amber under every sustained load.
- The MMIO RAPL zone of newer laptops counts the same package energy as the MSR zone. It gets its own card, titled `(MMIO)`.

## Unverified

- **Motherboard voltages.** Super I/O voltage channels (`in0`, `in1`, ...) show raw volts at the chip pin. Boards scale rails through resistor dividers. On many ASUS boards `in1` is +5 V read ×5 and `in4` is +12 V read ×12, but the mapping is per board and is not applied.
- **Non-Arch builds.** Build package names for Debian, Ubuntu and Fedora in README come from the `-sys` crates. Only the Arch build has been run.
