use std::{collections::HashMap, path::PathBuf, time::Duration};

use sysglance_core::{
    counter::{Wrap, per_second},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{Source, origin, text};

pub fn stalled(line: &str) -> Option<(&str, u64)> {
    let mut fields = line.split_whitespace();
    let kind = fields.next()?;
    let total = fields
        .find_map(|field| field.strip_prefix("total="))?
        .parse()
        .ok()?;
    Some((kind, total))
}

pub struct Pressure {
    root: PathBuf,
    totals: HashMap<String, u64>,
}

impl Pressure {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            totals: HashMap::new(),
        }
    }
}

impl Source for Pressure {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device> {
        let mut readings: Vec<Reading> = Vec::new();
        for resource in ["cpu", "memory", "io", "irq"] {
            let path = self.root.join(resource);
            let Ok(content) = text(&path) else { continue };
            for (kind, total) in content.lines().filter_map(stalled) {
                if resource == "cpu" && kind == "full" && total == 0 {
                    continue;
                }
                let key = format!("pressure/{resource}/{kind}");
                let before = self.totals.insert(key.clone(), total);
                let value = before
                    .and_then(|before| per_second(before, total, Wrap::Never, elapsed))
                    .map_or(Value::Unavailable(Unavailable::Pending), |micros| {
                        Value::plain(Measure::Fraction((micros / 1e6).min(1.0)))
                    });
                let label = match kind {
                    "some" => format!("{resource}: some tasks stalled"),
                    "full" => format!("{resource}: all tasks stalled"),
                    other => format!("{resource}: {other}"),
                };
                readings.push(Reading {
                    key,
                    label,
                    value,
                    origin: origin(&path),
                });
            }
        }
        if readings.is_empty() {
            readings.push(Reading {
                key: "pressure/status".to_owned(),
                label: "pressure stall".to_owned(),
                value: Value::Unavailable(Unavailable::NotSupported),
                origin: format!(
                    "{}: this kernel has no PSI, or was booted with psi=0",
                    self.root.display()
                ),
            });
        }
        vec![Device {
            key: "pressure".to_owned(),
            title: "Pressure stall".to_owned(),
            detail: "PSI · share of the last second".to_owned(),
            category: Category::System,
            readings,
        }]
    }
}

#[cfg(test)]
mod tests;
