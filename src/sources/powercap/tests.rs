use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};

use sysglance_core::{
    level::Level,
    measure::Measure,
    reading::{Category, Device, Unavailable, Value},
};
use tempfile::TempDir;

use super::{GRANT, Powercap};
use crate::sources::Source;

const SECOND: Duration = Duration::from_secs(1);

fn zone(root: &Path, dir: &str, name: &str, energy: u64, files: &[(&str, &str)]) {
    let path = root.join(dir);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("name"), format!("{name}\n")).unwrap();
    fs::write(path.join("energy_uj"), format!("{energy}\n")).unwrap();
    for (file, content) in files {
        fs::write(path.join(file), content).unwrap();
    }
}

fn energy(root: &Path, dir: &str, value: u64) {
    fs::write(root.join(dir).join("energy_uj"), format!("{value}\n")).unwrap();
}

fn measured(value: &Value) -> (Measure, Level) {
    match value {
        Value::Measured { measure, level, .. } => (*measure, *level),
        Value::Text(_) | Value::Unavailable(_) => panic!("not measured: {value:?}"),
    }
}

fn labels(device: &Device) -> Vec<&str> {
    device.readings.iter().map(|r| r.label.as_str()).collect()
}

const PACKAGE: [(&str, &str); 7] = [
    ("max_energy_range_uj", "1000000000\n"),
    ("constraint_0_name", "long_term\n"),
    ("constraint_0_power_limit_uw", "4095875000\n"),
    ("constraint_0_time_window_us", "55967744\n"),
    ("constraint_0_max_power_uw", "125000000\n"),
    ("constraint_1_name", "short_term\n"),
    ("constraint_1_power_limit_uw", "150000000\n"),
];

#[test]
fn no_zones_means_no_devices() {
    let root = TempDir::new().unwrap();
    assert_eq!(Powercap::new(root.path()).collect(SECOND).len(), 0);
}

#[test]
fn the_first_sample_is_pending() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &PACKAGE);
    let devices = Powercap::new(root.path()).collect(SECOND);
    assert!(matches!(
        &devices[0].readings[0].value,
        Value::Unavailable(Unavailable::Pending)
    ));
}

#[test]
fn package_power_is_the_energy_rate_against_tdp() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &PACKAGE);
    let mut source = Powercap::new(root.path());
    source.collect(SECOND);
    energy(root.path(), "intel-rapl:0", 130_000_000);
    let devices = source.collect(SECOND);
    assert_eq!(devices[0].category, Category::Power);
    assert_eq!(
        measured(&devices[0].readings[0].value),
        (Measure::Watts(130.0), Level::High)
    );
}

#[test]
fn a_counter_wraps_at_its_range() {
    let root = TempDir::new().unwrap();
    zone(
        root.path(),
        "intel-rapl:0",
        "package-0",
        999_000_000,
        &PACKAGE,
    );
    let mut source = Powercap::new(root.path());
    source.collect(SECOND);
    energy(root.path(), "intel-rapl:0", 9_000_000);
    let devices = source.collect(SECOND);
    assert_eq!(
        measured(&devices[0].readings[0].value).0,
        Measure::Watts(10.0)
    );
}

#[test]
fn limits_say_when_they_are_unlimited() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &PACKAGE);
    let devices = Powercap::new(root.path()).collect(SECOND);
    let texts: Vec<String> = devices[0]
        .readings
        .iter()
        .filter_map(|r| match &r.value {
            Value::Text(text) => Some(text.clone()),
            Value::Measured { .. } | Value::Unavailable(_) => None,
        })
        .collect();
    assert_eq!(texts, ["4096 W (unlimited) over 56.0 s", "150 W"]);
}

#[test]
fn subzones_join_their_package_in_order() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &PACKAGE);
    zone(root.path(), "intel-rapl:0:0", "core", 0, &[]);
    zone(root.path(), "intel-rapl:0:1", "uncore", 0, &[]);
    zone(root.path(), "intel-rapl:0:2", "dram", 0, &[]);
    let devices = Powercap::new(root.path()).collect(SECOND);
    assert_eq!(devices.len(), 1);
    assert_eq!(
        labels(&devices[0]),
        [
            "package-0 power",
            "core power",
            "uncore power",
            "dram power",
            "rated power (TDP)",
            "long term limit",
            "short term limit"
        ]
    );
}

#[test]
fn an_unreadable_counter_names_the_grant() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &[]);
    let counter = root.path().join("intel-rapl:0/energy_uj");
    fs::set_permissions(&counter, fs::Permissions::from_mode(0o000)).unwrap();
    let devices = Powercap::new(root.path()).collect(SECOND);
    assert_eq!(
        devices[0].readings[0].value,
        Value::Unavailable(Unavailable::NeedsPrivilege(GRANT))
    );
}

#[test]
fn an_unreadable_counter_that_is_not_a_permission_error_says_so() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &[]);
    let counter = root.path().join("intel-rapl:0/energy_uj");
    fs::remove_file(&counter).unwrap();
    fs::create_dir(&counter).unwrap();
    let devices = Powercap::new(root.path()).collect(SECOND);
    assert!(matches!(
        &devices[0].readings[0].value,
        Value::Unavailable(Unavailable::Unreadable(_))
    ));
}

#[test]
fn a_zone_without_a_name_is_skipped() {
    let root = TempDir::new().unwrap();
    zone(root.path(), "intel-rapl:0", "package-0", 0, &[]);
    fs::remove_file(root.path().join("intel-rapl:0/name")).unwrap();
    assert_eq!(Powercap::new(root.path()).collect(SECOND).len(), 0);
}

#[test]
fn a_zero_tdp_is_not_shown() {
    let root = TempDir::new().unwrap();
    zone(
        root.path(),
        "intel-rapl:0",
        "package-0",
        0,
        &[("constraint_0_max_power_uw", "0\n")],
    );
    let devices = Powercap::new(root.path()).collect(SECOND);
    assert_eq!(labels(&devices[0]), ["package-0 power"]);
}
