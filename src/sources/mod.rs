pub mod cpu;
pub mod hwmon;

use std::{fs, io, path::Path, time::Duration};

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
