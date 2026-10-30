use std::{fs, path::Path, time::Duration};

use sysglance_core::{
    level::Level,
    measure::Measure,
    reading::{Category, Device, Unavailable, Value},
};
use tempfile::TempDir;

use super::{Hwmon, Kind, channel};
use crate::sources::Source;

const SECOND: Duration = Duration::from_secs(1);

fn chip(root: &Path, dir: &str, files: &[(&str, &str)]) {
    let path = root.join(dir);
    fs::create_dir_all(&path).unwrap();
    for (name, content) in files {
        let file = path.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }
}

fn collect(root: &Path) -> Vec<Device> {
    Hwmon::new(root).collect(SECOND)
}

fn only(devices: &[Device]) -> &Device {
    assert_eq!(devices.len(), 1);
    &devices[0]
}

fn measured(value: &Value) -> (Measure, Level) {
    match value {
        Value::Measured { measure, level, .. } => (*measure, *level),
        Value::Text(_) | Value::Unavailable(_) => panic!("not measured: {value:?}"),
    }
}

#[test]
fn a_missing_root_has_no_devices() {
    assert_eq!(Hwmon::new("/nonexistent/hwmon").collect(SECOND).len(), 0);
}

#[test]
fn an_empty_root_has_no_devices() {
    let root = TempDir::new().unwrap();
    assert_eq!(collect(root.path()).len(), 0);
}

#[test]
fn a_chip_without_channels_is_left_out() {
    let root = TempDir::new().unwrap();
    chip(root.path(), "hwmon0", &[("name", "empty\n")]);
    assert_eq!(collect(root.path()).len(), 0);
}

#[test]
fn a_temperature_reads_in_celsius_against_its_limits() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "coretemp\n"),
            ("temp1_input", "89000\n"),
            ("temp1_label", "Package id 0\n"),
            ("temp1_max", "82000\n"),
            ("temp1_crit", "100000\n"),
            ("temp1_crit_alarm", "0\n"),
        ],
    );
    let devices = collect(root.path());
    let device = only(&devices);
    assert_eq!(device.category, Category::Processor);
    assert_eq!(device.readings[0].label, "Package id 0");
    assert_eq!(
        measured(&device.readings[0].value),
        (Measure::Celsius(89.0), Level::High)
    );
}

#[test]
fn zero_limits_do_not_raise_their_alarms() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "jc42\n"),
            ("temp1_input", "58750\n"),
            ("temp1_max", "0\n"),
            ("temp1_crit", "0\n"),
            ("temp1_min", "0\n"),
            ("temp1_max_alarm", "1\n"),
            ("temp1_crit_alarm", "1\n"),
        ],
    );
    let devices = collect(root.path());
    assert_eq!(
        measured(&only(&devices).readings[0].value),
        (Measure::Celsius(58.75), Level::Unrated)
    );
}

#[test]
fn an_alarm_on_a_set_voltage_limit_raises_the_level() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "nct6798\n"),
            ("in0_input", "1120\n"),
            ("in0_max", "1744\n"),
            ("in0_min", "0\n"),
            ("in0_alarm", "1\n"),
        ],
    );
    let devices = collect(root.path());
    let device = only(&devices);
    assert_eq!(device.category, Category::Board);
    assert_eq!(
        measured(&device.readings[0].value),
        (Measure::Volts(1.12), Level::High)
    );
}

#[test]
fn a_pwm_is_a_duty_cycle_labelled_with_its_control_mode() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "nct6798\n"),
            ("pwm1", "153\n"),
            ("pwm1_enable", "1\n"),
            ("pwm2", "51\n"),
            ("pwm2_enable", "5\n"),
            ("pwm3", "0\n"),
            ("pwm3_enable", "0\n"),
        ],
    );
    let devices = collect(root.path());
    let labels: Vec<&str> = only(&devices)
        .readings
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["pwm1 · manual", "pwm2 · auto, mode 5", "pwm3 · full speed"]
    );
    assert_eq!(
        measured(&only(&devices).readings[0].value).0,
        Measure::Fraction(0.6)
    );
}

#[test]
fn a_pwm_scales_by_its_own_maximum() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "amdgpu\n"),
            ("pwm1", "50\n"),
            ("pwm1_max", "100\n"),
        ],
    );
    let devices = collect(root.path());
    assert_eq!(
        measured(&only(&devices).readings[0].value).0,
        Measure::Fraction(0.5)
    );
}

#[test]
fn an_unreadable_channel_says_so() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "amdgpu\n"), ("fan1_input", "")],
    );
    let devices = collect(root.path());
    assert!(matches!(
        only(&devices).readings[0].value,
        Value::Unavailable(Unavailable::Unreadable(_))
    ));
}

#[test]
fn a_fan_below_its_minimum_is_low() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "nct6798\n"),
            ("fan1_input", "200\n"),
            ("fan1_min", "300\n"),
        ],
    );
    let devices = collect(root.path());
    assert_eq!(
        measured(&only(&devices).readings[0].value),
        (Measure::Rpm(200.0), Level::Low)
    );
}

#[test]
fn power_reads_its_average_when_there_is_no_input() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "amdgpu\n"),
            ("power1_average", "45000000\n"),
            ("power1_cap", "250000000\n"),
        ],
    );
    let devices = collect(root.path());
    assert_eq!(
        measured(&only(&devices).readings[0].value),
        (Measure::Watts(45.0), Level::Normal)
    );
}

#[test]
fn an_energy_counter_needs_two_samples_for_a_rate() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "rapl\n"), ("energy1_input", "1000000\n")],
    );
    let mut source = Hwmon::new(root.path());
    let first = source.collect(SECOND);
    assert!(matches!(
        first[0].readings[0].value,
        Value::Unavailable(Unavailable::Pending)
    ));
    fs::write(root.path().join("hwmon0/energy1_input"), "21000000\n").unwrap();
    let second = source.collect(Duration::from_secs(2));
    assert_eq!(
        measured(&second[0].readings[0].value).0,
        Measure::Watts(10.0)
    );
}

#[test]
fn an_intrusion_is_a_flag() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "nct6798\n"), ("intrusion1_alarm", "1\n")],
    );
    let devices = collect(root.path());
    assert_eq!(
        measured(&only(&devices).readings[0].value),
        (Measure::Tripped(true), Level::Unrated)
    );
}

#[test]
fn an_nvme_chip_is_titled_by_its_model() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "nvme\n"),
            ("temp1_input", "53850\n"),
            ("device/model", "WD_BLACK SN850X HS 2000GB\n"),
        ],
    );
    let devices = collect(root.path());
    assert_eq!(only(&devices).title, "WD_BLACK SN850X HS 2000GB");
    assert_eq!(only(&devices).category, Category::Storage);
}

#[test]
fn many_chips_come_back_in_directory_order() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon1",
        &[("name", "nvme\n"), ("temp1_input", "1000\n")],
    );
    chip(
        root.path(),
        "hwmon0",
        &[("name", "acpitz\n"), ("temp1_input", "1000\n")],
    );
    let names: Vec<String> = collect(root.path()).into_iter().map(|d| d.title).collect();
    assert_eq!(names, ["ACPI thermal zone", "nvme"]);
}

#[test]
fn channels_are_prefix_index_attribute() {
    assert_eq!(channel("temp12_input"), Some((Kind::Temperature, 12)));
    assert_eq!(channel("intrusion0_alarm"), Some((Kind::Intrusion, 0)));
}

#[test]
fn a_bare_pwm_is_a_channel() {
    assert_eq!(channel("pwm3"), Some((Kind::Pwm, 3)));
}

#[test]
fn a_bare_temperature_is_not_a_channel() {
    assert_eq!(channel("temp1"), None);
}

#[test]
fn files_that_are_not_channels_are_skipped() {
    assert_eq!(channel("name"), None);
    assert_eq!(channel("temp1input"), None);
    assert_eq!(channel("subsystem"), None);
    assert_eq!(channel("tempX_input"), None);
}

#[test]
#[ignore = "reads the running machine's /sys and runs `sensors -j`"]
fn live_readings_agree_with_lm_sensors() {
    let output = std::process::Command::new("sensors")
        .arg("-j")
        .output()
        .unwrap();
    let sensors: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let devices = collect(Path::new("/sys/class/hwmon"));
    let mut compared = 0;
    for chip in sensors.as_object().unwrap().values() {
        for (label, features) in chip.as_object().unwrap() {
            let Some(features) = features.as_object() else {
                continue;
            };
            for (feature, expected) in features {
                let Some(stem) = feature.strip_suffix("_input") else {
                    continue;
                };
                let expected = expected.as_f64().unwrap();
                let matching: Vec<f64> = devices
                    .iter()
                    .flat_map(|d| &d.readings)
                    .filter(|r| r.label == *label && r.origin.ends_with(&format!("/{stem}_input")))
                    .filter_map(|r| match r.value {
                        Value::Measured { measure, .. } => Some(measure.magnitude()),
                        Value::Text(_) | Value::Unavailable(_) => None,
                    })
                    .collect();
                assert!(!matching.is_empty(), "no reading for {label} {feature}");
                let tolerance = if stem.starts_with("temp") {
                    3.0
                } else if stem.starts_with("fan") {
                    150.0
                } else if stem.starts_with("freq") {
                    2e9
                } else {
                    0.05
                };
                assert!(
                    matching.iter().any(|v| (v - expected).abs() <= tolerance),
                    "{label} {feature}: sensors {expected}, sysglance {matching:?}"
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 40, "compared only {compared} readings");
}

#[test]
fn current_frequency_and_humidity_are_read_in_their_units() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[
            ("name", "sensor\n"),
            ("curr1_input", "1500\n"),
            ("freq1_input", "300000000\n"),
            ("humidity1_input", "45000\n"),
        ],
    );
    let devices = collect(root.path());
    let measures: Vec<Measure> = only(&devices)
        .readings
        .iter()
        .map(|r| measured(&r.value).0)
        .collect();
    assert_eq!(
        measures,
        [
            Measure::Amps(1.5),
            Measure::Hertz(3e8),
            Measure::Fraction(0.45)
        ]
    );
}

#[test]
fn an_it87_chip_is_the_board() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "it8689\n"), ("temp1_input", "1000\n")],
    );
    let devices = collect(root.path());
    assert_eq!(
        (only(&devices).category, only(&devices).title.as_str()),
        (Category::Board, "Super I/O IT8689")
    );
}

#[test]
fn a_wifi_chip_is_the_network() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "iwlwifi_1\n"), ("temp1_input", "1000\n")],
    );
    assert_eq!(only(&collect(root.path())).category, Category::Network);
}

#[test]
fn an_unknown_chip_is_the_system_titled_by_its_name() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "mystery\n"), ("temp1_input", "1000\n")],
    );
    let devices = collect(root.path());
    assert_eq!(
        (
            only(&devices).category,
            only(&devices).title.as_str(),
            only(&devices).detail.as_str()
        ),
        (Category::System, "mystery", "hwmon mystery")
    );
}

#[test]
fn a_reading_names_the_file_it_came_from() {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", "acpitz\n"), ("temp1_input", "1000\n")],
    );
    let devices = collect(root.path());
    assert_eq!(
        only(&devices).readings[0].origin,
        root.path().join("hwmon0/temp1_input").display().to_string()
    );
}

fn titled(name: &str) -> (Category, String) {
    let root = TempDir::new().unwrap();
    chip(
        root.path(),
        "hwmon0",
        &[("name", &format!("{name}\n")), ("temp1_input", "1000\n")],
    );
    let devices = collect(root.path());
    (only(&devices).category, only(&devices).title.clone())
}

#[test]
fn a_fintek_chip_is_the_boards_super_io() {
    assert_eq!(
        titled("f71882fg"),
        (Category::Board, "Super I/O F71882FG".to_owned())
    );
}

#[test]
fn a_laptop_or_board_embedded_controller_is_the_board() {
    assert_eq!(
        titled("thinkpad"),
        (Category::Board, "Embedded controller (thinkpad)".to_owned())
    );
    assert_eq!(
        titled("asus_ec_sensors"),
        (
            Category::Board,
            "Embedded controller (asus_ec_sensors)".to_owned()
        )
    );
}

#[test]
fn a_battery_is_power() {
    assert_eq!(titled("BAT0"), (Category::Power, "Battery BAT0".to_owned()));
}
