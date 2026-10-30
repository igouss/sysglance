use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use sysglance_core::{
    counter::{Wrap, per_second},
    level::{Alarms, Limits},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{Source, optional, origin, text};

const UNLIMITED_WATTS: f64 = 4000.0;
pub const GRANT: &str = "run install.sh: it lets your admin group read RAPL energy counters";

pub struct Powercap {
    root: PathBuf,
    energy: HashMap<PathBuf, u64>,
}

impl Powercap {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            energy: HashMap::new(),
        }
    }

    fn power(&mut self, zone: &Path, elapsed: Duration) -> Value {
        let path = zone.join("energy_uj");
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return Value::Unavailable(Unavailable::NeedsPrivilege(GRANT));
            }
            Err(e) => return Value::Unavailable(Unavailable::Unreadable(e.to_string())),
        };
        let Ok(now) = raw.trim().parse::<u64>() else {
            return Value::Unavailable(Unavailable::Unreadable(format!("not a number: {raw:?}")));
        };
        let wrap = optional(&zone.join("max_energy_range_uj"))
            .and_then(|range| u64::try_from(range).ok())
            .map_or(Wrap::Never, Wrap::At);
        let before = self.energy.insert(zone.to_path_buf(), now);
        let Some(microwatts) = before.and_then(|before| per_second(before, now, wrap, elapsed))
        else {
            return Value::Unavailable(Unavailable::Pending);
        };
        let tdp = watts(&zone.join("constraint_0_max_power_uw"));
        Value::measured(
            Measure::Watts(microwatts / 1e6),
            Limits::new(None, tdp, None),
            Alarms::default(),
        )
    }
}

fn watts(path: &Path) -> Option<f64> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "power limits are far below 2^52 µW"
    )]
    optional(path).map(|microwatts| microwatts as f64 / 1e6)
}

fn constraints(zone: &Path, key: &str) -> Vec<Reading> {
    (0..)
        .map_while(|index| {
            let name = text(&zone.join(format!("constraint_{index}_name"))).ok()?;
            let limit_path = zone.join(format!("constraint_{index}_power_limit_uw"));
            let limit = watts(&limit_path)?;
            let window = optional(&zone.join(format!("constraint_{index}_time_window_us")));
            let described = if limit >= UNLIMITED_WATTS {
                format!("{limit:.0} W (unlimited)")
            } else {
                format!("{limit:.0} W")
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "time windows are far below 2^52 µs"
            )]
            let described = window.map_or_else(
                || described.clone(),
                |us| format!("{described} over {}", Measure::Seconds(us as f64 / 1e6)),
            );
            Some(Reading {
                key: format!("{key}/constraint{index}"),
                label: format!("{} limit", name.replace('_', " ")),
                value: Value::Text(described),
                origin: origin(&limit_path),
            })
        })
        .collect()
}

fn zones(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut zones: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("energy_uj").exists())
        .collect();
    zones.sort();
    zones
}

impl Source for Powercap {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device> {
        let mut devices: Vec<Device> = Vec::new();
        for zone in zones(&self.root) {
            let Some(directory) = zone
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Ok(name) = text(&zone.join("name")) else {
                continue;
            };
            let key = format!("powercap:{directory}");
            let power = Reading {
                key: format!("{key}/power"),
                label: format!("{name} power"),
                value: self.power(&zone, elapsed),
                origin: origin(&zone.join("energy_uj")),
            };
            let parent = directory
                .rsplit_once(':')
                .map(|(parent, _)| parent.to_owned());
            let is_top = parent.as_deref().is_none_or(|parent| !parent.contains(':'));
            if is_top {
                let mut readings = vec![power];
                if let Some(tdp) =
                    watts(&zone.join("constraint_0_max_power_uw")).filter(|tdp| *tdp > 0.0)
                {
                    readings.push(Reading {
                        key: format!("{key}/tdp"),
                        label: "rated power (TDP)".to_owned(),
                        value: Value::plain(Measure::Watts(tdp)),
                        origin: origin(&zone.join("constraint_0_max_power_uw")),
                    });
                }
                readings.extend(constraints(&zone, &key));
                devices.push(Device {
                    key,
                    title: format!("RAPL {name}"),
                    detail: format!("powercap {directory}"),
                    category: Category::Power,
                    readings,
                });
            } else if let Some(device) = parent
                .map(|parent| format!("powercap:{parent}"))
                .and_then(|parent| devices.iter_mut().find(|device| device.key == parent))
            {
                let after = device
                    .readings
                    .iter()
                    .take_while(|reading| reading.key.ends_with("/power"))
                    .count();
                device.readings.insert(after, power);
            }
        }
        devices
    }
}

#[cfg(test)]
mod tests;
