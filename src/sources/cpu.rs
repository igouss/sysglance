use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use sysglance_core::{
    counter::{Wrap, per_second},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{Core, Source, cores, cpus, optional, origin, text};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Jiffies {
    busy: u64,
    iowait: u64,
    total: u64,
}

pub fn jiffies(line: &str) -> Option<(String, Jiffies)> {
    let mut fields = line.split_whitespace();
    let name = fields.next()?.to_owned();
    let values: Vec<u64> = fields.map(str::parse).collect::<Result<_, _>>().ok()?;
    let at = |i: usize| values.get(i).copied().unwrap_or(0);
    let (user, nice, system, idle, iowait, irq, softirq, steal) =
        (at(0), at(1), at(2), at(3), at(4), at(5), at(6), at(7));
    if values.len() < 4 {
        return None;
    }
    Some((
        name,
        Jiffies {
            busy: user + nice + system + irq + softirq + steal,
            iowait,
            total: user + nice + system + idle + iowait + irq + softirq + steal,
        },
    ))
}

pub fn share(before: Jiffies, after: Jiffies, part: fn(Jiffies) -> u64) -> Option<f64> {
    let total = after
        .total
        .checked_sub(before.total)
        .filter(|total| *total > 0)?;
    let done = part(after).checked_sub(part(before))?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "jiffy deltas between samples are small"
    )]
    let fraction = done as f64 / total as f64;
    Some(fraction.min(1.0))
}

pub struct Cpu {
    proc: PathBuf,
    sys: PathBuf,
    jiffies: HashMap<String, Jiffies>,
    counters: HashMap<String, u64>,
}

impl Cpu {
    pub fn new(proc: impl Into<PathBuf>, sys: impl Into<PathBuf>) -> Self {
        Self {
            proc: proc.into(),
            sys: sys.into(),
            jiffies: HashMap::new(),
            counters: HashMap::new(),
        }
    }

    fn rate(&mut self, key: &str, now: u64, elapsed: Duration) -> Value {
        let before = self.counters.insert(key.to_owned(), now);
        before
            .and_then(|before| per_second(before, now, Wrap::Never, elapsed))
            .map_or_else(warming, |rate| Value::plain(Measure::PerSecond(rate)))
    }

    fn load(&mut self, elapsed: Duration) -> Option<Device> {
        let path = self.proc.join("stat");
        let stat = fs::read_to_string(&path).ok()?;
        let source = origin(&path);
        let mut readings: Vec<Reading> = Vec::new();
        let mut counters: Vec<(&str, &str, u64)> = Vec::new();
        for line in stat.lines() {
            if line.starts_with("cpu") {
                let Some((name, now)) = jiffies(line) else {
                    continue;
                };
                let before = self.jiffies.insert(name.clone(), now);
                let busy = before.and_then(|before| share(before, now, |j| j.busy));
                let (key, label) = if name == "cpu" {
                    ("cpu:load/all".to_owned(), "all threads".to_owned())
                } else {
                    (format!("cpu:load/{name}"), name.clone())
                };
                readings.push(Reading {
                    key,
                    label,
                    value: busy.map_or_else(warming, |busy| Value::plain(Measure::Fraction(busy))),
                    origin: source.clone(),
                });
                if name == "cpu" {
                    readings.push(Reading {
                        key: "cpu:load/iowait".to_owned(),
                        label: "waiting on I/O".to_owned(),
                        value: before
                            .and_then(|before| share(before, now, |j| j.iowait))
                            .map_or_else(warming, |share| Value::plain(Measure::Fraction(share))),
                        origin: source.clone(),
                    });
                }
                continue;
            }
            let mut fields = line.split_whitespace();
            let (Some(name), Some(first)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Ok(value) = first.parse::<u64>() else {
                continue;
            };
            match name {
                "ctxt" => counters.push(("ctxt", "context switches", value)),
                "intr" => counters.push(("intr", "interrupts", value)),
                "processes" => counters.push(("processes", "processes started", value)),
                "procs_running" => {
                    readings.push(gauge("cpu:load/running", "runnable tasks", value, &source));
                }
                "procs_blocked" => readings.push(gauge(
                    "cpu:load/blocked",
                    "tasks blocked on I/O",
                    value,
                    &source,
                )),
                _ => {}
            }
        }
        for (key, label, value) in counters {
            let key = format!("cpu:load/{key}");
            let value = self.rate(&key, value, elapsed);
            readings.push(Reading {
                key,
                label: label.to_owned(),
                value,
                origin: source.clone(),
            });
        }
        readings.extend(self.averages());
        let model = text(&self.proc.join("cpuinfo"))
            .ok()
            .and_then(|info| {
                info.lines().find_map(|line| {
                    line.strip_prefix("model name")?
                        .split_once(':')
                        .map(|(_, m)| m.trim().to_owned())
                })
            })
            .unwrap_or_else(|| "CPU".to_owned());
        Some(Device {
            key: "cpu:load".to_owned(),
            title: "CPU load".to_owned(),
            detail: model,
            category: Category::Processor,
            readings,
        })
    }

    fn averages(&self) -> Vec<Reading> {
        let mut readings: Vec<Reading> = Vec::new();
        let path = self.proc.join("loadavg");
        if let Ok(loadavg) = text(&path) {
            for (field, minutes) in loadavg.split_whitespace().zip(["1", "5", "15"]) {
                if let Ok(load) = field.parse::<f64>() {
                    readings.push(Reading {
                        key: format!("cpu:load/avg{minutes}"),
                        label: format!("load average, {minutes} min"),
                        value: Value::plain(Measure::Scalar(load)),
                        origin: origin(&path),
                    });
                }
            }
        }
        let path = self.proc.join("uptime");
        if let Some(up) = text(&path)
            .ok()
            .and_then(|uptime| uptime.split_whitespace().next()?.parse::<f64>().ok())
        {
            readings.push(Reading {
                key: "cpu:load/uptime".to_owned(),
                label: "uptime".to_owned(),
                value: Value::plain(Measure::Seconds(up)),
                origin: origin(&path),
            });
        }
        readings
    }

    fn clocks(&self) -> Option<Device> {
        let mut readings: Vec<Reading> = Vec::new();
        for (index, dir) in cpus(&self.sys) {
            let path = dir.join("cpufreq/scaling_cur_freq");
            let Some(khz) = optional(&path) else { continue };
            #[expect(
                clippy::cast_precision_loss,
                reason = "frequencies in kHz are far below 2^52"
            )]
            let hertz = |khz: i64| khz as f64 * 1000.0;
            readings.push(Reading {
                key: format!("cpu:clock/cpu{index}"),
                label: format!("cpu{index}"),
                value: Value::plain(Measure::Hertz(hertz(khz))),
                origin: origin(&path),
            });
        }
        if readings.is_empty() {
            return None;
        }
        let policy = self.sys.join("devices/system/cpu/cpufreq/policy0");
        let pstate = self.sys.join("devices/system/cpu/intel_pstate");
        let settings = [
            ("driver", policy.join("scaling_driver")),
            ("governor", policy.join("scaling_governor")),
            (
                "energy preference",
                policy.join("energy_performance_preference"),
            ),
            ("pstate mode", pstate.join("status")),
        ];
        for (label, path) in settings {
            if let Ok(setting) = text(&path) {
                readings.push(Reading {
                    key: format!("cpu:clock/{label}"),
                    label: label.to_owned(),
                    value: Value::Text(setting),
                    origin: origin(&path),
                });
            }
        }
        if let Some(no_turbo) = optional(&pstate.join("no_turbo")) {
            readings.push(Reading {
                key: "cpu:clock/turbo".to_owned(),
                label: "turbo".to_owned(),
                value: Value::Text(if no_turbo == 0 { "enabled" } else { "disabled" }.to_owned()),
                origin: origin(&pstate.join("no_turbo")),
            });
        }
        let range = |file: &str| optional(&policy.join(file));
        if let (Some(low), Some(base), Some(high)) = (
            range("cpuinfo_min_freq"),
            range("base_frequency"),
            range("cpuinfo_max_freq"),
        ) {
            readings.push(Reading {
                key: "cpu:clock/range".to_owned(),
                label: "min / base / max".to_owned(),
                value: Value::Text(format!("{}/{}/{} GHz", ghz(low), ghz(base), ghz(high))),
                origin: origin(&policy),
            });
        }
        Some(Device {
            key: "cpu:clock".to_owned(),
            title: "CPU clocks".to_owned(),
            detail: "cpufreq · intel_pstate".to_owned(),
            category: Category::Processor,
            readings,
        })
    }

    fn idle(&mut self, elapsed: Duration) -> Option<Device> {
        let mut residency: Vec<(String, PathBuf, u64)> = Vec::new();
        for (_, dir) in cpus(&self.sys) {
            let Ok(states) = fs::read_dir(dir.join("cpuidle")) else {
                continue;
            };
            for state in states.filter_map(Result::ok).map(|entry| entry.path()) {
                let (Ok(name), Some(time)) =
                    (text(&state.join("name")), optional(&state.join("time")))
                else {
                    continue;
                };
                let Ok(time) = u64::try_from(time) else {
                    continue;
                };
                match residency.iter_mut().find(|(known, _, _)| *known == name) {
                    Some(entry) => entry.2 += time,
                    None => residency.push((name, state.join("time"), time)),
                }
            }
        }
        if residency.is_empty() {
            return None;
        }
        let threads = cpus(&self.sys)
            .iter()
            .filter(|(_, dir)| dir.join("cpuidle").exists())
            .count()
            .max(1);
        residency.sort_by(|a, b| a.0.cmp(&b.0));
        let readings = residency
            .into_iter()
            .map(|(name, path, time)| {
                let key = format!("cpu:idle/{name}");
                let before = self.counters.insert(key.clone(), time);
                #[expect(clippy::cast_precision_loss, reason = "a thread count")]
                let threads = threads as f64;
                let value = before
                    .and_then(|before| per_second(before, time, Wrap::Never, elapsed))
                    .map_or_else(warming, |micros| {
                        Value::plain(Measure::Fraction((micros / 1e6 / threads).min(1.0)))
                    });
                Reading {
                    key,
                    label: format!("{name} residency"),
                    value,
                    origin: origin(&path),
                }
            })
            .collect();
        Some(Device {
            key: "cpu:idle".to_owned(),
            title: "CPU idle states".to_owned(),
            detail: "cpuidle · share of all threads' time".to_owned(),
            category: Category::Processor,
            readings,
        })
    }

    fn throttling(&self) -> Option<Device> {
        let mut readings: Vec<Reading> = Vec::new();
        for Core { dir, id: core } in cores(&self.sys) {
            let throttle = dir.join("thermal_throttle");
            if !throttle.exists() {
                continue;
            }
            if readings.is_empty() {
                for (file, label) in [
                    ("package_throttle_count", "package events"),
                    ("package_throttle_total_time_ms", "package time throttled"),
                    ("package_throttle_max_time_ms", "package longest throttle"),
                ] {
                    readings.push(throttle_reading(
                        &throttle,
                        file,
                        label,
                        "cpu:throttle/package",
                    ));
                }
            }
            readings.push(throttle_reading(
                &throttle,
                "core_throttle_count",
                &format!("core {core} events"),
                &format!("cpu:throttle/core{core}"),
            ));
        }
        if readings.is_empty() {
            return None;
        }
        Some(Device {
            key: "cpu:throttle".to_owned(),
            title: "CPU thermal throttling".to_owned(),
            detail: "thermal_throttle · since boot".to_owned(),
            category: Category::Processor,
            readings,
        })
    }
}

fn throttle_reading(dir: &Path, file: &str, label: &str, key: &str) -> Reading {
    let path = dir.join(file);
    let value = match super::integer(&path) {
        Err(why) => Value::Unavailable(why),
        #[expect(
            clippy::cast_precision_loss,
            reason = "milliseconds since boot are far below 2^52"
        )]
        Ok(ms) if file.ends_with("_ms") => Value::plain(Measure::Seconds(ms as f64 / 1000.0)),
        Ok(count) => Value::plain(Measure::Count(u64::try_from(count).unwrap_or(0))),
    };
    Reading {
        key: format!("{key}/{file}"),
        label: label.to_owned(),
        value,
        origin: origin(&path),
    }
}

fn ghz(khz: i64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "frequencies in kHz are far below 2^52"
    )]
    let ghz = khz as f64 / 1e6;
    format!("{ghz:.1}")
}

fn gauge(key: &str, label: &str, value: u64, source: &str) -> Reading {
    Reading {
        key: key.to_owned(),
        label: label.to_owned(),
        value: Value::plain(Measure::Count(value)),
        origin: source.to_owned(),
    }
}

const fn warming() -> Value {
    Value::Unavailable(Unavailable::Pending)
}

impl Source for Cpu {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device> {
        [
            self.load(elapsed),
            self.clocks(),
            self.idle(elapsed),
            self.throttling(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests;
