use std::{fs, path::PathBuf, time::Duration};

use sysglance_core::reading::{Category, Device, Reading, Unavailable, Value};

use super::{Source, origin, text};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub manufacturer: String,
    pub part: String,
    pub bytes: u64,
    pub transfers: u32,
    pub ranks: u8,
    pub width: u8,
}

const DDR4: u8 = 0x0c;

fn manufacturer(bank: u8, code: u8) -> String {
    match (bank & 0x7f, code) {
        (0, 0xce) => "Samsung".to_owned(),
        (0, 0x2c) => "Micron".to_owned(),
        (0, 0xad) => "SK hynix".to_owned(),
        (1, 0x98) => "Kingston".to_owned(),
        (2, 0x9e) => "Corsair".to_owned(),
        (4, 0xcd) => "G.Skill".to_owned(),
        (4, 0xef) => "Team Group".to_owned(),
        (5, 0x9b) => "Crucial".to_owned(),
        (bank, code) => format!("JEDEC bank {} code {code:#04x}", bank + 1),
    }
}

pub fn ddr4(spd: &[u8]) -> Option<Module> {
    if spd.len() < 350 || spd[2] != DDR4 {
        return None;
    }
    let die_megabits = 256_u64 << (spd[4] & 0x0f);
    let width = 4_u8 << (spd[12] & 0x07);
    let ranks = ((spd[12] >> 3) & 0x07) + 1;
    let bus = 8_u64 << (spd[13] & 0x07);
    let bytes = die_megabits / 8 * 1024 * 1024 * (bus / u64::from(width)) * u64::from(ranks);
    let fine = i64::from(i8::from_ne_bytes([spd[125]]));
    let cycle = i64::from(spd[18]) * 125 + fine;
    let transfers = u32::try_from(2_000_000 / cycle.max(1)).ok()?;
    let part = String::from_utf8_lossy(&spd[329..349])
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_owned();
    Some(Module {
        manufacturer: manufacturer(spd[320], spd[321]),
        part,
        bytes,
        transfers,
        ranks,
        width,
    })
}

pub struct Spd {
    root: PathBuf,
}

impl Spd {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Source for Spd {
    fn collect(&mut self, _elapsed: Duration) -> Vec<Device> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut chips: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                text(&path.join("name")).is_ok_and(|name| name == "ee1004" || name == "spd5118")
            })
            .collect();
        chips.sort();
        let readings: Vec<Reading> = chips
            .iter()
            .map(|chip| {
                let path = chip.join("eeprom");
                let address = chip
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let value = match fs::read(&path) {
                    Err(e) => Value::Unavailable(Unavailable::Unreadable(e.to_string())),
                    Ok(spd) => {
                        ddr4(&spd).map_or(Value::Unavailable(Unavailable::NotSupported), |module| {
                            Value::Text(format!(
                                "{} {} · {} GiB DDR4-{} · {}R×{}",
                                module.manufacturer,
                                module.part,
                                module.bytes >> 30,
                                module.transfers,
                                module.ranks,
                                module.width
                            ))
                        })
                    }
                };
                Reading {
                    key: format!("spd/{address}"),
                    label: format!("module {address}"),
                    value,
                    origin: origin(&path),
                }
            })
            .collect();
        if readings.is_empty() {
            return Vec::new();
        }
        vec![Device {
            key: "spd".to_owned(),
            title: "Memory modules".to_owned(),
            detail: "SPD EEPROM · JEDEC rating, not the running speed".to_owned(),
            category: Category::Memory,
            readings,
        }]
    }
}

#[cfg(test)]
mod tests;
