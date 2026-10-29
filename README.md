# sysglance

A GPUI window that shows every temperature, voltage, fan, clock, power and load counter this Linux machine exposes, once a second, with a two-minute sparkline per reading. Readings at or past their driver's limits turn amber or red. A reading that cannot be read says why, and hovering any row shows its sysfs source.

## Run

```sh
cargo run --release
```

## Check

```sh
scripts/check.sh
cargo test live -- --ignored
```

The second command compares the hwmon readings against `sensors -j` on this machine.

See [docs/design.md](docs/design.md) for the invariants and the privilege model.
