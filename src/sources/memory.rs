use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

use sysglance_core::{
    counter::{Wrap, per_second},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{Source, origin, text};

const AT_PAGESZ: u64 = 6;
const DEFAULT_PAGE: u64 = 4096;
const WORD: usize = size_of::<usize>();

pub fn page_size(auxv: &[u8]) -> Option<u64> {
    let word = |bytes: &[u8]| -> Option<u64> {
        let array: [u8; WORD] = bytes.try_into().ok()?;
        u64::try_from(usize::from_ne_bytes(array)).ok()
    };
    auxv.as_chunks::<{ 2 * WORD }>().0.iter().find_map(|pair| {
        let (key, value) = pair.split_at(WORD);
        (word(key)? == AT_PAGESZ).then(|| word(value)).flatten()
    })
}

pub fn table(content: &str) -> HashMap<&str, u64> {
    content
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':').or_else(|| line.split_once(' '))?;
            let number = rest.split_whitespace().next()?.parse().ok()?;
            Some((name.trim(), number))
        })
        .collect()
}

pub struct Memory {
    proc: PathBuf,
    sys: PathBuf,
    page: f64,
    counters: HashMap<&'static str, u64>,
}

impl Memory {
    pub fn new(proc: impl Into<PathBuf>, sys: impl Into<PathBuf>) -> Self {
        let proc = proc.into();
        let page = fs::read(proc.join("self/auxv"))
            .ok()
            .and_then(|auxv| page_size(&auxv))
            .unwrap_or(DEFAULT_PAGE);
        #[expect(
            clippy::cast_precision_loss,
            reason = "page sizes are small powers of two"
        )]
        let page = page as f64;
        Self {
            proc,
            page,
            sys: sys.into(),
            counters: HashMap::new(),
        }
    }

    fn ram(&mut self, elapsed: Duration) -> Option<Device> {
        let path = self.proc.join("meminfo");
        let content = text(&path).ok()?;
        let info = table(&content);
        let kib = |name: &str| info.get(name).map(|kib| bytes(kib * 1024));
        let source = origin(&path);
        let mut readings: Vec<Reading> = Vec::new();
        let mut push = |key: &str, label: &str, value: Option<Value>, source: &str| {
            if let Some(value) = value {
                readings.push(Reading {
                    key: format!("memory:ram/{key}"),
                    label: label.to_owned(),
                    value,
                    origin: source.to_owned(),
                });
            }
        };
        let total = info.get("MemTotal").copied();
        let available = info.get("MemAvailable").copied();
        if let (Some(total), Some(available)) = (total, available) {
            push(
                "used",
                "in use",
                Some(fraction(total.saturating_sub(available), total)),
                &source,
            );
        }
        push("total", "total", kib("MemTotal"), &source);
        push("available", "available", kib("MemAvailable"), &source);
        push("free", "free", kib("MemFree"), &source);
        push("cached", "page cache", kib("Cached"), &source);
        push("anon", "anonymous", kib("AnonPages"), &source);
        push("shmem", "shared", kib("Shmem"), &source);
        push("slab", "kernel slab", kib("Slab"), &source);
        push("dirty", "dirty", kib("Dirty"), &source);
        push("writeback", "writeback", kib("Writeback"), &source);
        let swap_total = info.get("SwapTotal").copied();
        let swap_free = info.get("SwapFree").copied();
        if let (Some(total), Some(free)) = (swap_total, swap_free) {
            push(
                "swap",
                "swap in use",
                Some(fraction(total.saturating_sub(free), total)),
                &source,
            );
            push(
                "swap_bytes",
                "swap used",
                Some(bytes(total.saturating_sub(free) * 1024)),
                &source,
            );
        }
        let path = self.proc.join("vmstat");
        if let Ok(content) = text(&path) {
            let vmstat = table(&content);
            let source = origin(&path);
            for (name, label, scale) in [
                ("pswpin", "swapped in", self.page),
                ("pswpout", "swapped out", self.page),
                ("pgmajfault", "major faults", 1.0),
                ("pgfault", "page faults", 1.0),
            ] {
                let Some(&now) = vmstat.get(name) else {
                    continue;
                };
                let before = self.counters.insert(name, now);
                let rate = before.and_then(|before| per_second(before, now, Wrap::Never, elapsed));
                let value = rate.map_or(Value::Unavailable(Unavailable::Pending), |rate| {
                    if scale > 1.0 {
                        Value::plain(Measure::BytesPerSecond(rate * scale))
                    } else {
                        Value::plain(Measure::PerSecond(rate))
                    }
                });
                push(name, label, Some(value), &source);
            }
            push(
                "oom",
                "OOM kills since boot",
                vmstat
                    .get("oom_kill")
                    .map(|&n| Value::plain(Measure::Count(n))),
                &source,
            );
        }
        Some(Device {
            key: "memory:ram".to_owned(),
            title: "RAM and swap".to_owned(),
            detail: "/proc/meminfo · /proc/vmstat".to_owned(),
            category: Category::Memory,
            readings,
        })
    }

    fn swaps(&self) -> Option<Device> {
        let path = self.proc.join("swaps");
        let content = text(&path).ok()?;
        let readings: Vec<Reading> = content
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let [name, kind, size, used, priority] = fields.as_slice() else {
                    return None;
                };
                let (size, used) = (size.parse::<u64>().ok()?, used.parse::<u64>().ok()?);
                Some(Reading {
                    key: format!("memory:swaps/{name}"),
                    label: format!("{name} ({kind}, prio {priority})"),
                    value: fraction(used, size),
                    origin: origin(&path),
                })
            })
            .collect();
        if readings.is_empty() {
            return None;
        }
        Some(Device {
            key: "memory:swaps".to_owned(),
            title: "Swap devices".to_owned(),
            detail: "/proc/swaps · share in use".to_owned(),
            category: Category::Memory,
            readings,
        })
    }

    fn zram(&self) -> Vec<Device> {
        let Ok(entries) = fs::read_dir(self.sys.join("block")) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("zram"))
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|name| {
                let dir = self.sys.join("block").join(&name);
                let path = dir.join("mm_stat");
                let stat: Vec<u64> = text(&path)
                    .ok()?
                    .split_whitespace()
                    .filter_map(|field| field.parse().ok())
                    .collect();
                let [original, compressed, used, ..] = stat.as_slice() else {
                    return None;
                };
                let algorithm = text(&dir.join("comp_algorithm")).ok().and_then(|all| {
                    all.split_whitespace()
                        .find_map(|a| a.strip_prefix('[')?.strip_suffix(']').map(str::to_owned))
                });
                let source = origin(&path);
                let reading = |key: &str, label: &str, value: Value| Reading {
                    key: format!("memory:{name}/{key}"),
                    label: label.to_owned(),
                    value,
                    origin: source.clone(),
                };
                #[expect(clippy::cast_precision_loss, reason = "byte counts far below 2^52")]
                let ratio = if *compressed > 0 {
                    *original as f64 / *compressed as f64
                } else {
                    0.0
                };
                let mut readings = vec![
                    reading("original", "stored", bytes(*original)),
                    reading("compressed", "compressed to", bytes(*compressed)),
                    reading("used", "RAM used", bytes(*used)),
                    reading(
                        "ratio",
                        "compression ratio",
                        Value::plain(Measure::Scalar(ratio)),
                    ),
                ];
                if let Some(algorithm) = algorithm {
                    readings.push(reading("algorithm", "algorithm", Value::Text(algorithm)));
                }
                Some(Device {
                    key: format!("memory:{name}"),
                    title: format!("Compressed swap {name}"),
                    detail: "zram mm_stat".to_owned(),
                    category: Category::Memory,
                    readings,
                })
            })
            .collect()
    }
}

fn bytes(count: u64) -> Value {
    #[expect(clippy::cast_precision_loss, reason = "byte counts far below 2^52")]
    let count = count as f64;
    Value::plain(Measure::Bytes(count))
}

fn fraction(part: u64, whole: u64) -> Value {
    if whole == 0 {
        return Value::plain(Measure::Fraction(0.0));
    }
    #[expect(clippy::cast_precision_loss, reason = "memory sizes far below 2^52")]
    let share = part as f64 / whole as f64;
    Value::plain(Measure::Fraction(share))
}

impl Source for Memory {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device> {
        let mut devices: Vec<Device> = [self.ram(elapsed), self.swaps()]
            .into_iter()
            .flatten()
            .collect();
        devices.extend(self.zram());
        devices
    }
}

#[cfg(test)]
mod tests;
