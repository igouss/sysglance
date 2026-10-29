# sysglance design

sysglance shows every sensor and counter this machine exposes in one GPUI window, refreshed once a second.

## Shape

- `core/` (`sysglance-core`, hex role `domain`): readings, units, limits, levels, counter rates, history. It does no I/O: its `Cargo.toml` lists no dependencies and `core/clippy.toml` bans file, process, network, clock and thread calls. Gate: `scripts/check.sh` (clippy `-D warnings`, `hex-lint`).
- `helper/` (`sysglance-helper`, hex role `composition-root`): the root-side reader for MSRs and SMART. It is a separate crate so that the code running as root links neither GPUI nor anything else the window needs. See Privilege.
- `src/sources/`: one adapter per kernel interface. Each takes its root directory as a parameter, so tests run against fixture trees in a temp dir. They implement `Source::collect(elapsed)`, and counter sources keep their previous raw value to turn it into a rate.
- `src/sampler.rs`: one thread collects every source each second and publishes the arranged devices with a generation number.
- `src/view.rs`: GPUI with `gpui-component` (dark theme, scrollbar, tooltips). It polls the generation every 100 ms and redraws only when it changes, so an idle window costs no GPU time.
- **Frames do no formatting.** Force: scrolling redraws every frame. When a sample arrives, `src/view/model.rs` builds a display model: shared strings, and each sparkline rasterised once into a small image (`src/spark.rs`). A frame only places it.
- **A card is one painted element.** Force: about 210 rows are on screen at once, and GPUI's per-element style, layout and hitbox work for ~6 elements per row cost 8–11 ms of main-thread CPU per frame, so frames slipped to 25–45 ms. `src/view/paint.rs` paints a card's title, rows, values and sparkline images in one canvas, the way Zed's editor paints its lines. Sparklines as quads cost CPU (40 scene primitives per row) and as paths cost GPU time on the UHD 630; as images they are one sprite. Hovering a row shows its source, limits and reason in the footer.
- **Only cards near the viewport are built.** Force: the same. `src/layout.rs` packs cards into masonry columns from their known heights (cards have fixed row heights), and a frame builds elements only for cards within 400 px of the visible area. Gate: `src/layout/tests.rs`.

## Invariants

- **A limit of zero is unset.** Force: drivers fill unset limits with 0. jc42 reports `max = crit = 0` and the NCT6798 reports `min = max = 0` on most voltage inputs, so their alarm bits read tripped. `Limits::new` drops zero and non-finite limits. Gate: `core/src/level.rs` tests.
- **An alarm counts only against a limit that is set.** Same force: a tripped `max_alarm` on an unset `max` is a false alarm. Gate: `Limits::trusted` tests.
- **A level is computed from the value it describes.** `Value::measured` derives the level from the measure and limits, so the two cannot disagree. The one exception is an ATA SMART attribute; see SMART.
- **An unavailable reading says why.** `Unavailable` is `Pending` (a rate needs a second sample), `NeedsPrivilege(how)`, `ModuleNotLoaded`, `NotSupported` or `Unreadable(error)`. The tooltip shows the reason and the sysfs path.
- **Code carries no comments** (cm-k7c). Gate: `scripts/no-comments.sh`, which `scripts/check.sh` runs.
- **Wayland only.** Force: Omarchy runs Hyprland, which has no X11 session, so X11 code is dead weight in the binary. Every GPUI crate is built with `default-features = false, features = ["wayland"]`. Gate: `scripts/wayland-only.sh` fails when any X11 crate (`x11`, `x11rb`, `xcb`, `xim`, ...) is in the resolved dependency tree. It goes red when `x11` is enabled.
- **Every commit passes the gate as committed.** Force: a check of the working tree passes a commit that leaves a needed change unstaged. The pre-commit hook runs `scripts/check-snapshot.sh`, which exports the index to a temp dir and runs `scripts/check.sh` there. `scripts/check-snapshot.sh <rev>` checks any past commit the same way.

## Theme

The window follows the Omarchy theme. `src/omarchy.rs` reads `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` (default `~/.local/state`), and `src/view/theme.rs` checks it every second and applies it on change, so `omarchy-theme-set` recolours the open window within a second. The font is `omarchy-font-current`'s.

`colors.toml` keys mean different things in light and dark themes (`light_foreground` is the strong text colour in a light theme, and `white`'s `lighter_background` is darker than its background), so the mapping depends on `mode`:

| sysglance | dark theme | light theme |
|---|---|---|
| window | `background` | `background` |
| card | `lighter_background` | `dark_background` |
| border | `selection` | `selection` |
| text | `foreground` | `foreground` |
| labels | `light_foreground` | `light_foreground` |
| faint text | `dark_foreground` | `dark_foreground` |
| normal / low / high / critical | `green` / `blue` / `yellow` / `red` | same |

Without an Omarchy theme the window uses gpui-component's dark theme. Gate: `src/omarchy/tests.rs`; `cargo test omarchy -- --ignored` parses every theme under `/usr/share/omarchy/themes`.

## Frame time

`sysglance --bench-scroll` scrolls the view 37 px every frame for 900 frames, then prints fps, frame-time percentiles and main-thread CPU per frame, and exits. Use main-thread CPU per frame to compare changes: wall-clock fps on this machine swings between 25 and 56 fps on identical code with the background load. CPU time per frame drifts too, between 4 and 6.5 ms for the same binary as `llama-server` loads the sibling hyperthreads, so compare two builds only by interleaving their runs in one sitting. A number from an earlier session is not a baseline. On 2026-10-29, with the load average around 9, it read 60.0 fps, p99 18 ms and 3.5–4.1 ms of main-thread CPU per frame, against 8–11 ms before cards were painted.

For a CPU profile, build `cargo build --profile profiling` (release with line tables, unstripped) and run `perf record -g --call-graph dwarf target/profiling/sysglance --bench-scroll`.

## Known-answer check

`cargo test live -- --ignored` reads this machine's `/sys/class/hwmon` and runs `sensors -j` in the same second. Every lm_sensors `*_input` must match a sysglance reading with the same label: temperatures within 3 °C, fans within 150 rpm, voltages within 50 mV. It goes red when the temperature scale is wrong by a factor of ten.

## Privilege

Decided by the operator on 2026-10-29: "udev + small helper". The GUI runs unprivileged.

- RAPL `energy_uj` is root-only by default because of the PLATYPUS power side channel. `system/60-sysglance-rapl.rules` makes it readable by group `wheel`, which re-opens that channel to members of `wheel`. Without the rule, the power readings say `needs privilege` and name the rule.
- **The helper is the only privileged code, and it decides nothing.** Force: MSRs (`/dev/cpu/<n>/msr`, mode 0600 and `CAP_SYS_RAWIO`) and SMART (smartctl's NVMe admin and ATA pass-through ioctls) are root-only. `helper/` (`sysglance-helper`) is a std-only binary that links nothing but `sysglance-core`. It takes no arguments and no input: it reads the fixed register list in `core/src/msr.rs` (`PACKAGE` on cpu0, `PER_CPU` on every cpu) and runs `/usr/bin/smartctl --json=c -a -n standby` on each drive that `smartctl --scan` lists. It never writes an MSR, so the sticky log bits it reads stay as the hardware set them. It writes text lines (`msr <cpu> <address> <value>`, `smart <drive> <json>`, `end`) and exits when the reader goes away. All decoding happens unprivileged in `src/sources/msr.rs` and `src/sources/smart.rs`, against fixtures captured from this machine.
- **sudo, not file capabilities.** Flagged assumption: the operator's "udev + small helper" allows either. File capabilities would need `CAP_SYS_RAWIO` plus a udev rule on the msr devices, and smartctl's ioctls need `CAP_SYS_ADMIN`, which is root in all but name. `system/sysglance-helper.sudoers` lets group `wheel` run exactly `/usr/local/libexec/sysglance-helper` with no arguments (`""`) and no password. The binary sits in a root-owned path, because a NOPASSWD rule on a user-writable file would be a root shell. `install.sh` checks the rule with `visudo -c` before installing it, because a broken sudoers file breaks sudo.
- **One helper per window.** `src/sources/helper.rs` starts `sudo -n /usr/local/libexec/sysglance-helper` once, so sudo logs one line per launch rather than one a second. A reader thread folds the stream into a `Link` (`Starting`, `Streaming`, `Refused`, `Lost`) with `evolve`. The helper reads registers every second and SMART every 30 s; `-n standby` leaves a sleeping disk asleep. A helper that exits before its first frame shows `needs privilege`, naming `install.sh` and showing sudo's message; one that dies later shows `unreadable`. Gate: `src/sources/helper/tests.rs`.
- `install.sh` installs the binary, the desktop entry, the udev rule and `system/sysglance-modules.conf`, which loads `drivetemp` (SATA drive temperatures through hwmon) and `msr` at boot.

## MSRs

Register meanings are model-specific, so registers are decoded only on Skylake-client models (Skylake, Kaby Lake, Comet Lake: `SKYLAKE_CLIENT_MODELS`, the models turbostat gives the same feature table). On any other CPU the card says `not supported`. Readings the kernel already exposes elsewhere are left out: per-core temperatures (coretemp), clocks (cpufreq derives them from APERF/MPERF) and energy (powercap).

- **Limit reasons** (`0x64F`): the low 16 bits say what limits the clock now, the high 16 what has limited it since boot. Turbostat does not decode this register on Skylake-client parts. Flagged assumption: the bit names follow Intel's SDM table for this register (PROCHOT, thermal, residency regulation, average thermal, VR thermal, VR current, electrical, PL1, PL2, max turbo, turbo transition). Known answer on this machine under all-core load: `max turbo` at 4.7 GHz, the turbo ladder's eight-core ratio.
- **Package thermal status** (`0x1B1`): each status bit has its log bit one place above it.
- **Core voltage** (`0x198` bits 47:32 ÷ 8192): the VID the core requests, not the voltage measured at the socket.
- **TjMax and TCC offset** (`0x1A2`), the **turbo ladder** (`0x1AD`, one active core first), and the **package C-state limit** (`0xE2`, turbostat's Haswell-and-later table). This board's firmware sets the limit to PC0, so the package residency counters never advance, and the card leaves out any residency counter that is still zero.
- **SMIs** (`0x34`) as a rate. Elapsed time is the TSC delta over the base clock (`0xCE` ratio × 100 MHz). The TSC really runs at 3792 MHz against a nominal 3800, so the rate reads about 0.2 % high.
- **Core C-state residency** (`0x3FC`–`0x3FE`) is the hardware's own count, read once per physical core. It can differ from cpuidle's requested-state time because the hardware demotes requests.

## SMART

One card per drive. It is titled by the model and sits next to that drive's hwmon temperature card. Temperatures stay with hwmon, so SMART does not repeat them.

- Overall health and NVMe `critical_warning` are flags whose critical limit is 1, so a failure is red. The warning's bits are named in the footer.
- **Spare available** is rated against the drive's own threshold. **Endurance used** is rated High at 100 %. Flagged assumption: the NVMe spec defines 100 % as the rated life used up, and smartctl reports ATA wear on the same scale.
- An ATA attribute row shows the raw value; the footer gives the normalised value, the worst value and the threshold. **The level comes from smartctl's verdict, not from the number shown.** Force: the drive compares its normalised value, not the raw count, with its threshold. `when_failed = now` is Critical. Flagged assumption: `past` is High. An attribute with a threshold is otherwise Normal; one without a threshold is unrated. This is the one exception to "a level is computed from the value it describes".
- smartctl's exit status is a bit mask. Bits 0–2 (bad command line, device not opened or asleep, a command failed) make the report unreadable, and the card shows smartctl's message. The higher bits are findings already in the report: bit 6, error-log entries, is set on the Samsung 850 EVO here.
- Committed fixtures have serial number, WWN and time removed.

## Power limits

The package card shows PL1 (`long_term`) and PL2 (`short_term`) from powercap. A limit of 4000 W or more is shown as `unlimited`: ASUS firmware writes 4095.875 W when it stops enforcing Intel's limits. The rated power (`constraint_0_max_power_uw`, 125 W on the i7-10700K) is the package power's high limit, so sustained draw above TDP turns the reading amber.

## Unverified

- NCT6798 voltage channels `in0`–`in14` show raw volts at the chip pin. On ASUS boards `in1` is usually +5 V read ×5 and `in4` is usually +12 V read ×12. That mapping has not been checked against this board's BIOS hardware monitor page.
