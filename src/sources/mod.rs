pub mod cpu;
pub mod hwmon;
pub mod memory;
pub mod powercap;
pub mod pressure;
pub mod spd;

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use sysglance_core::reading::{Device, Unavailable};

pub trait Source: Send {
    fn collect(&mut self, elapsed: Duration) -> Vec<Device>;
}

pub fn text(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map(|content| content.trim().to_owned())
}

pub fn integer(path: &Path) -> Result<i64, Unavailable> {
    let content = text(path).map_err(|e| Unavailable::Unreadable(e.to_string()))?;
    content
        .parse()
        .map_err(|_| Unavailable::Unreadable(format!("not a number: {content:?}")))
}

pub fn optional(path: &Path) -> Option<i64> {
    integer(path).ok()
}

pub fn origin(path: &Path) -> String {
    path.display().to_string()
}

pub fn cpus(sys: &Path) -> Vec<(u32, PathBuf)> {
    let Ok(entries) = fs::read_dir(sys.join("devices/system/cpu")) else {
        return Vec::new();
    };
    let mut cpus: Vec<(u32, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let index = name.strip_prefix("cpu")?.parse().ok()?;
            Some((index, entry.path()))
        })
        .collect();
    cpus.sort();
    cpus
}

pub struct Core {
    pub dir: PathBuf,
    pub id: String,
}

pub fn cores(sys: &Path) -> Vec<Core> {
    cpus(sys)
        .into_iter()
        .filter(|(cpu, dir)| {
            let first_sibling = text(&dir.join("topology/thread_siblings_list"))
                .ok()
                .and_then(|list| list.split([',', '-']).next()?.parse::<u32>().ok());
            first_sibling.is_none_or(|first| first == *cpu)
        })
        .map(|(cpu, dir)| Core {
            id: text(&dir.join("topology/core_id")).unwrap_or_else(|_| cpu.to_string()),
            dir,
        })
        .collect()
}
