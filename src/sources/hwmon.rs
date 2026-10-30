use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use sysglance_core::{
    counter::{Wrap, per_second},
    level::{Alarms, Limits},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{Source, integer, optional, origin, text};

pub struct Hwmon {
    root: PathBuf,
    energy: HashMap<String, u64>,
}

impl Hwmon {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            energy: HashMap::new(),
        }
    }
}

impl Source for Hwmon {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut chips: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        chips.sort();
        chips
            .iter()
            .filter_map(|chip| self.chip(chip, elapsed))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Temperature,
    Voltage,
    Current,
    Power,
    Energy,
    Fan,
    Pwm,
    Frequency,
    Humidity,
    Intrusion,
}

impl Kind {
    fn parse(prefix: &str) -> Option<Self> {
        Some(match prefix {
            "temp" => Self::Temperature,
            "in" => Self::Voltage,
            "curr" => Self::Current,
            "power" => Self::Power,
            "energy" => Self::Energy,
            "fan" => Self::Fan,
            "pwm" => Self::Pwm,
            "freq" => Self::Frequency,
            "humidity" => Self::Humidity,
            "intrusion" => Self::Intrusion,
            _ => return None,
        })
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Temperature => "temp",
            Self::Voltage => "in",
            Self::Current => "curr",
            Self::Power => "power",
            Self::Energy => "energy",
            Self::Fan => "fan",
            Self::Pwm => "pwm",
            Self::Frequency => "freq",
            Self::Humidity => "humidity",
            Self::Intrusion => "intrusion",
        }
    }
}

fn channel(file: &str) -> Option<(Kind, u32)> {
    let digits = file.find(|c: char| c.is_ascii_digit())?;
    let kind = Kind::parse(&file[..digits])?;
    let rest = &file[digits..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let index = rest[..end].parse().ok()?;
    let attribute = &rest[end..];
    if attribute.is_empty() && kind != Kind::Pwm {
        return None;
    }
    if !attribute.is_empty() && !attribute.starts_with('_') {
        return None;
    }
    Some((kind, index))
}

struct Chip<'a> {
    dir: &'a Path,
    key: &'a str,
}

impl Chip<'_> {
    fn path(&self, kind: Kind, index: u32, attribute: &str) -> PathBuf {
        let name = if attribute.is_empty() {
            format!("{}{index}", kind.prefix())
        } else {
            format!("{}{index}_{attribute}", kind.prefix())
        };
        self.dir.join(name)
    }

    fn scaled(&self, kind: Kind, index: u32, attribute: &str, scale: f64) -> Option<f64> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "sysfs values are far below 2^52"
        )]
        let value = optional(&self.path(kind, index, attribute))? as f64;
        Some(value / scale)
    }

    fn flag(&self, kind: Kind, index: u32, attribute: &str) -> Option<bool> {
        optional(&self.path(kind, index, attribute)).map(|value| value != 0)
    }

    fn alarms(&self, kind: Kind, index: u32) -> Alarms {
        Alarms {
            low: self.flag(kind, index, "min_alarm"),
            high: self.flag(kind, index, "max_alarm"),
            critical: self.flag(kind, index, "crit_alarm"),
            any: self.flag(kind, index, "alarm"),
        }
    }

    fn limits(&self, kind: Kind, index: u32, scale: f64) -> Limits {
        Limits::new(
            self.scaled(kind, index, "min", scale),
            self.scaled(kind, index, "max", scale),
            self.scaled(kind, index, "crit", scale),
        )
    }

    fn label(&self, kind: Kind, index: u32) -> String {
        text(&self.path(kind, index, "label"))
            .ok()
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| format!("{}{index}", kind.prefix()))
    }
}

const fn scale(kind: Kind) -> f64 {
    match kind {
        Kind::Temperature | Kind::Voltage | Kind::Current | Kind::Humidity => 1000.0,
        Kind::Power | Kind::Energy => 1_000_000.0,
        Kind::Fan | Kind::Pwm | Kind::Frequency | Kind::Intrusion => 1.0,
    }
}

fn measure(kind: Kind, value: f64) -> Measure {
    match kind {
        Kind::Temperature => Measure::Celsius(value),
        Kind::Voltage => Measure::Volts(value),
        Kind::Current => Measure::Amps(value),
        Kind::Power | Kind::Energy => Measure::Watts(value),
        Kind::Fan => Measure::Rpm(value),
        Kind::Pwm => Measure::Fraction(value),
        Kind::Frequency => Measure::Hertz(value),
        Kind::Humidity => Measure::Fraction(value / 100.0),
        Kind::Intrusion => Measure::Tripped(value != 0.0),
    }
}

fn control(mode: i64) -> String {
    match mode {
        0 => "full speed".to_owned(),
        1 => "manual".to_owned(),
        n => format!("auto, mode {n}"),
    }
}

fn identity(dir: &Path, name: &str) -> (Category, String, String) {
    let device = dir.join("device");
    let model = text(&device.join("model"))
        .ok()
        .filter(|model| !model.is_empty());
    let address = fs::canonicalize(&device)
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|tail| tail.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let (category, title) = match name {
        "coretemp" | "k10temp" | "zenpower" => (Category::Processor, "CPU temperatures".to_owned()),
        "jc42" | "spd5118" => (Category::Memory, format!("DIMM sensor {address}")),
        "nvme" | "drivetemp" => (Category::Storage, model.unwrap_or_else(|| name.to_owned())),
        "amdgpu" | "radeon" | "nouveau" | "i915" | "xe" => {
            (Category::Graphics, format!("GPU ({name})"))
        }
        "acpitz" => (Category::Board, "ACPI thermal zone".to_owned()),
        chip if ["nct", "it8", "w83", "f71", "f81"]
            .iter()
            .any(|prefix| chip.starts_with(prefix)) =>
        {
            (
                Category::Board,
                format!("Super I/O {}", chip.to_uppercase()),
            )
        }
        "asus_ec_sensors" | "asus-ec-sensors" | "asus_wmi_sensors" | "dell_smm" | "thinkpad"
        | "applesmc" | "hp" => (Category::Board, format!("Embedded controller ({name})")),
        chip if chip.starts_with("BAT") => (Category::Power, format!("Battery {chip}")),
        chip if chip.starts_with("iwlwifi") => (Category::Network, "Wi-Fi radio".to_owned()),
        chip => (Category::System, chip.to_owned()),
    };
    let detail = [
        Some(format!("hwmon {name}")),
        Some(address).filter(|a| !a.is_empty()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<String>>()
    .join(" · ");
    (category, title, detail)
}

impl Hwmon {
    fn chip(&mut self, dir: &Path, elapsed: Duration) -> Option<Device> {
        let name = text(&dir.join("name")).ok()?;
        let (category, title, detail) = identity(dir, &name);
        let key = format!(
            "hwmon:{name}:{}",
            fs::canonicalize(dir.join("device")).map_or_else(
                |_| dir.display().to_string(),
                |path| path.display().to_string()
            )
        );
        let mut channels: BTreeSet<(Kind, u32)> = BTreeSet::new();
        for entry in fs::read_dir(dir).ok()?.filter_map(Result::ok) {
            if let Some(found) = channel(&entry.file_name().to_string_lossy()) {
                channels.insert(found);
            }
        }
        let chip = Chip { dir, key: &key };
        let readings: Vec<Reading> = channels
            .iter()
            .filter_map(|&(kind, index)| self.reading(&chip, kind, index, elapsed))
            .collect();
        if readings.is_empty() {
            return None;
        }
        Some(Device {
            key,
            title,
            detail,
            category,
            readings,
        })
    }

    fn reading(
        &mut self,
        chip: &Chip<'_>,
        kind: Kind,
        index: u32,
        elapsed: Duration,
    ) -> Option<Reading> {
        let attribute = match kind {
            Kind::Pwm => "",
            Kind::Intrusion => "alarm",
            Kind::Power => {
                if chip.path(kind, index, "input").exists() {
                    "input"
                } else {
                    "average"
                }
            }
            Kind::Temperature
            | Kind::Voltage
            | Kind::Current
            | Kind::Energy
            | Kind::Fan
            | Kind::Frequency
            | Kind::Humidity => "input",
        };
        let path = chip.path(kind, index, attribute);
        if !path.exists() {
            return None;
        }
        let key = format!("{}/{}{index}", chip.key, kind.prefix());
        let mut label = chip.label(kind, index);
        let value = match integer(&path) {
            Err(why) => Value::Unavailable(why),
            Ok(raw) => match kind {
                Kind::Pwm => {
                    #[expect(clippy::cast_precision_loss, reason = "pwm values are 0..=255")]
                    let duty = raw as f64
                        / optional(&chip.path(kind, index, "max")).map_or(255.0, |max| max as f64);
                    if let Some(mode) = optional(&chip.path(kind, index, "enable")) {
                        label = format!("{label} · {}", control(mode));
                    }
                    Value::plain(Measure::Fraction(duty))
                }
                Kind::Energy => {
                    #[expect(clippy::cast_sign_loss, reason = "energy counters are non-negative")]
                    let now = raw.max(0) as u64;
                    let before = self.energy.insert(key.clone(), now);
                    before
                        .and_then(|before| per_second(before, now, Wrap::Never, elapsed))
                        .map_or(Value::Unavailable(Unavailable::Pending), |microwatts| {
                            Value::plain(Measure::Watts(microwatts / scale(kind)))
                        })
                }
                Kind::Temperature
                | Kind::Voltage
                | Kind::Current
                | Kind::Power
                | Kind::Fan
                | Kind::Frequency
                | Kind::Humidity
                | Kind::Intrusion => {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "sysfs values are far below 2^52"
                    )]
                    let value = raw as f64 / scale(kind);
                    let limits = match kind {
                        Kind::Power => Limits::new(
                            None,
                            chip.scaled(kind, index, "cap", scale(kind)),
                            chip.scaled(kind, index, "crit", scale(kind)),
                        ),
                        Kind::Intrusion => Limits::NONE,
                        Kind::Temperature
                        | Kind::Voltage
                        | Kind::Current
                        | Kind::Fan
                        | Kind::Frequency
                        | Kind::Humidity
                        | Kind::Pwm
                        | Kind::Energy => chip.limits(kind, index, scale(kind)),
                    };
                    Value::measured(measure(kind, value), limits, chip.alarms(kind, index))
                }
            },
        };
        Some(Reading {
            key,
            label,
            value,
            origin: origin(&path),
        })
    }
}

#[cfg(test)]
mod tests;
