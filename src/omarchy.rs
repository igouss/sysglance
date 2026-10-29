use std::{collections::HashMap, env, ffi::OsString, path::PathBuf, process::Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub background: u32,
    pub surface: u32,
    pub border: u32,
    pub foreground: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub normal: u32,
    pub low: u32,
    pub high: u32,
    pub critical: u32,
}

fn hex(value: &str) -> Option<u32> {
    let digits = value.trim().trim_matches('"').strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

fn entries(text: &str) -> HashMap<&str, &str> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim(), value.trim()))
        })
        .collect()
}

pub fn parse(text: &str) -> Option<Palette> {
    let entries = entries(text);
    let colour = |key: &str| entries.get(key).and_then(|value| hex(value));
    let dark = entries
        .get("mode")
        .is_none_or(|mode| mode.trim_matches('"') != "light");
    let background = colour("background")?;
    let foreground = colour("foreground")?;
    let surface_key = if dark {
        "lighter_background"
    } else {
        "dark_background"
    };
    Some(Palette {
        dark,
        background,
        surface: colour(surface_key).unwrap_or(background),
        border: colour("selection")
            .or_else(|| colour("muted"))
            .unwrap_or(foreground),
        foreground,
        muted: colour("light_foreground").unwrap_or(foreground),
        faint: colour("dark_foreground")
            .or_else(|| colour("muted"))
            .unwrap_or(foreground),
        accent: colour("accent")
            .or_else(|| colour("blue"))
            .unwrap_or(foreground),
        normal: colour("green").unwrap_or(foreground),
        low: colour("blue").unwrap_or(foreground),
        high: colour("yellow").unwrap_or(foreground),
        critical: colour("red").unwrap_or(foreground),
    })
}

pub fn colours_in(state: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let state = state
        .map(PathBuf::from)
        .or_else(|| home.map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(state.join("omarchy/current/theme/colors.toml"))
}

pub fn colours_path() -> Option<PathBuf> {
    colours_in(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))
}

pub fn family(succeeded: bool, stdout: &[u8]) -> Option<String> {
    let family = std::str::from_utf8(stdout).ok()?.trim().to_owned();
    (succeeded && !family.is_empty()).then_some(family)
}

pub fn font() -> Option<String> {
    let output = Command::new("omarchy-font-current").output().ok()?;
    family(output.status.success(), &output.stdout)
}

#[cfg(test)]
mod tests;
