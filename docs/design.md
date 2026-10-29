# sysglance design

sysglance shows every sensor and counter this machine exposes in one GPUI window, refreshed once a second.

## Shape

- `core/` (`sysglance-core`, hex role `domain`): readings, units, limits, levels, counter rates, history. It does no I/O: its `Cargo.toml` lists no dependencies and `core/clippy.toml` bans file, process, network, clock and thread calls. Gate: `scripts/check.sh` (clippy `-D warnings`, `hex-lint`).
- `src/sources/`: one adapter per kernel interface. Each takes its root directory as a parameter, so tests run against fixture trees in a temp dir. They implement `Source::collect(elapsed)`, and counter sources keep their previous raw value to turn it into a rate.
- `src/sampler.rs`: one thread collects every source each second and publishes the arranged devices with a generation number.
- `src/view.rs`: GPUI with `gpui-component` (dark theme, scrollbar, tooltips). It polls the generation every 100 ms and redraws only when it changes, so an idle window costs no GPU time.
- **Frames do no formatting.** Force: scrolling redraws every frame. When a sample arrives, `src/view/model.rs` builds a display model: shared strings, and each sparkline rasterised once into a small image (`src/spark.rs`). A frame only places it.
- **A card is one painted element.** Force: about 210 rows are on screen at once, and GPUI's per-element style, layout and hitbox work for ~6 elements per row cost 8–11 ms of main-thread CPU per frame, so frames slipped to 25–45 ms. `src/view/paint.rs` paints a card's title, rows, values and sparkline images in one canvas, the way Zed's editor paints its lines. Sparklines as quads cost CPU (40 scene primitives per row) and as paths cost GPU time on the UHD 630; as images they are one sprite. Hovering a row shows its source, limits and reason in the footer.
- **Only cards near the viewport are built.** Force: the same. `src/layout.rs` packs cards into masonry columns from their known heights (cards have fixed row heights), and a frame builds elements only for cards within 400 px of the visible area. Gate: `src/layout/tests.rs`.

## Invariants

- **A limit of zero is unset.** Force: drivers fill unset limits with 0. jc42 reports `max = crit = 0` and the NCT6798 reports `min = max = 0` on most voltage inputs, so their alarm bits read tripped. `Limits::new` drops zero and non-finite limits. Gate: `core/src/level.rs` tests.
- **An alarm counts only against a limit that is set.** Same force: a tripped `max_alarm` on an unset `max` is a false alarm. Gate: `Limits::trusted` tests.
- **A level is computed from the value it describes.** `Value::measured` derives the level from the measure and limits, so the two cannot disagree.
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

`sysglance --bench-scroll` scrolls the view 37 px every frame for 900 frames, then prints fps, frame-time percentiles and main-thread CPU per frame, and exits. Use main-thread CPU per frame to compare changes: wall-clock fps on this machine swings between 25 and 56 fps on identical code with the background load, while CPU time per frame holds still. On 2026-10-29, with the load average around 9, it read 60.0 fps, p99 18 ms and 3.5–4.1 ms of main-thread CPU per frame, against 8–11 ms before cards were painted.

## Known-answer check

`cargo test live -- --ignored` reads this machine's `/sys/class/hwmon` and runs `sensors -j` in the same second. Every lm_sensors `*_input` must match a sysglance reading with the same label: temperatures within 3 °C, fans within 150 rpm, voltages within 50 mV. It goes red when the temperature scale is wrong by a factor of ten.

## Privilege

Decided by the operator on 2026-10-29: "udev + small helper". The GUI runs unprivileged.

- RAPL `energy_uj` is root-only by default because of the PLATYPUS power side channel. `system/60-sysglance-rapl.rules` makes it readable by group `wheel`, which re-opens that channel to members of `wheel`. Without the rule, the power readings say `needs privilege` and name the rule.
- `install.sh` installs the binary, the desktop entry, the udev rule and `system/sysglance-modules.conf`, which loads `drivetemp` (SATA drive temperatures through hwmon) and `msr` at boot.

## Power limits

The package card shows PL1 (`long_term`) and PL2 (`short_term`) from powercap. A limit of 4000 W or more is shown as `unlimited`: ASUS firmware writes 4095.875 W when it stops enforcing Intel's limits. The rated power (`constraint_0_max_power_uw`, 125 W on the i7-10700K) is the package power's high limit, so sustained draw above TDP turns the reading amber.

## Unverified

- NCT6798 voltage channels `in0`–`in14` show raw volts at the chip pin. On ASUS boards `in1` is usually +5 V read ×5 and `in4` is usually +12 V read ×12. That mapping has not been checked against this board's BIOS hardware monitor page.
