use sysglance_core::{
    level::Level,
    measure::Measure,
    reading::{Device, Unavailable, Value},
};

use super::device;
use crate::sources::helper::Drive;

fn drive(path: &str) -> Drive {
    Drive {
        path: path.to_owned(),
        kind: "sat".to_owned(),
    }
}

fn report(path: &str, json: &str) -> Device {
    device(&drive(path), Ok(json))
}

const NVME: &str = include_str!("nvme-sn850x.json");
const SATA: &str = include_str!("sata-850-evo.json");

fn reading<'a>(device: &'a Device, key: &str) -> &'a sysglance_core::reading::Reading {
    device
        .readings
        .iter()
        .find(|r| r.key == format!("smart:/dev/x sat/{key}"))
        .unwrap_or_else(|| panic!("no {key} in {:?}", device.readings))
}

fn value<'a>(device: &'a Device, key: &str) -> &'a Value {
    &reading(device, key).value
}

fn measure(value: &Value) -> Measure {
    match value {
        Value::Measured { measure, .. } => *measure,
        Value::Text(_) | Value::Unavailable(_) => panic!("not measured: {value:?}"),
    }
}

fn nvme() -> Device {
    report("/dev/x", NVME)
}

fn sata() -> Device {
    report("/dev/x", SATA)
}

#[test]
fn a_drive_is_titled_by_its_model_and_detailed_by_path_protocol_and_firmware() {
    let drive = report("/dev/nvme0", NVME);
    assert_eq!(drive.key, "smart:/dev/nvme0 sat");
    assert_eq!(drive.title, "WD_BLACK SN850X HS 2000GB");
    assert_eq!(
        drive.detail,
        "SMART · /dev/nvme0 · NVMe · firmware 620331WD"
    );
}

#[test]
fn a_passing_drive_is_healthy() {
    let drive = nvme();
    let health = value(&drive, "health");
    assert_eq!(measure(health), Measure::Tripped(false));
    assert_eq!(health.level(), Level::Normal);
}

#[test]
fn a_failing_drive_is_critical() {
    let drive = report("/dev/x", r#"{"smart_status":{"passed":false}}"#);
    let health = value(&drive, "health");
    assert_eq!(measure(health), Measure::Tripped(true));
    assert_eq!(health.level(), Level::Critical);
}

#[test]
fn nvme_counts_read_and_written_in_512000_byte_units() {
    let drive = nvme();
    assert_eq!(
        measure(value(&drive, "data_units_written")),
        Measure::Bytes(15_270_610.0 * 512_000.0)
    );
    assert_eq!(
        measure(value(&drive, "data_units_read")),
        Measure::Bytes(6_705_238.0 * 512_000.0)
    );
}

#[test]
fn nvme_times_are_minutes_and_power_on_is_hours() {
    let drive = nvme();
    assert_eq!(
        measure(value(&drive, "controller_busy_time")),
        Measure::Seconds(258.0 * 60.0)
    );
    assert_eq!(
        measure(value(&drive, "powered-on")),
        Measure::Seconds(18_216.0 * 3600.0)
    );
}

#[test]
fn nvme_error_counters_are_counts() {
    let drive = nvme();
    assert_eq!(
        measure(value(&drive, "unsafe_shutdowns")),
        Measure::Count(55)
    );
    assert_eq!(measure(value(&drive, "media_errors")), Measure::Count(0));
    assert_eq!(measure(value(&drive, "cycles")), Measure::Count(64));
}

#[test]
fn spare_is_rated_against_the_drives_own_threshold() {
    let drive = nvme();
    let ample = value(&drive, "spare");
    assert_eq!(measure(ample), Measure::Fraction(1.0));
    assert_eq!(ample.level(), Level::Normal);
    let low = report(
        "/dev/x",
        r#"{"spare_available":{"current_percent":10,"threshold_percent":10}}"#,
    );
    assert_eq!(value(&low, "spare").level(), Level::Low);
}

#[test]
fn endurance_is_high_only_once_the_rated_life_is_used() {
    let drive = sata();
    let worn = value(&drive, "endurance");
    assert_eq!(measure(worn), Measure::Fraction(0.99));
    assert_eq!(worn.level(), Level::Normal);
    let spent = report("/dev/x", r#"{"endurance_used":{"current_percent":100}}"#);
    assert_eq!(value(&spent, "endurance").level(), Level::High);
}

#[test]
fn no_critical_warning_is_normal() {
    let drive = nvme();
    let warning = reading(&drive, "critical-warning");
    assert_eq!(measure(&warning.value), Measure::Tripped(false));
    assert_eq!(warning.value.level(), Level::Normal);
    assert_eq!(warning.origin, "smartctl -a /dev/x · critical_warning 0x00");
}

#[test]
fn a_critical_warning_is_critical_and_names_its_bits() {
    let drive = report(
        "/dev/x",
        r#"{"nvme_smart_health_information_log":{"critical_warning":5}}"#,
    );
    let warning = reading(&drive, "critical-warning");
    assert_eq!(warning.value.level(), Level::Critical);
    assert_eq!(
        warning.origin,
        "smartctl -a /dev/x · critical_warning 0x05: spare below threshold, reliability degraded"
    );
}

#[test]
fn an_ata_attribute_shows_its_raw_value_and_explains_the_normalised_one() {
    let drive = sata();
    let wear = reading(&drive, "attribute/177");
    assert_eq!(wear.label, "Wear_Leveling_Count");
    assert_eq!(measure(&wear.value), Measure::Count(2384));
    assert_eq!(wear.value.level(), Level::Unrated);
    assert_eq!(
        wear.origin,
        "smartctl -a /dev/x · attribute 177: raw 2384, normalised 1, worst 1, threshold 0"
    );
}

#[test]
fn an_ata_attribute_with_a_threshold_is_rated() {
    assert_eq!(value(&sata(), "attribute/5").level(), Level::Normal);
}

#[test]
fn every_ata_attribute_becomes_a_row() {
    let attributes = sata()
        .readings
        .iter()
        .filter(|r| r.key.contains("/attribute/"))
        .count();
    assert_eq!(attributes, 14);
}

#[test]
fn an_attribute_failing_now_is_critical_and_one_failed_in_the_past_is_high() {
    let drive = report(
        "/dev/x",
        r#"{"ata_smart_attributes":{"table":[
            {"id":5,"name":"a","value":5,"worst":5,"thresh":10,"when_failed":"now","raw":{"string":"900"}},
            {"id":6,"name":"b","value":50,"worst":5,"thresh":10,"when_failed":"past","raw":{"string":"3 (x)"}},
            {"id":7,"name":"c","value":50,"worst":50,"thresh":0,"when_failed":"","raw":{"string":"0/0"}}
        ]}}"#,
    );
    assert_eq!(value(&drive, "attribute/5").level(), Level::Critical);
    assert_eq!(measure(value(&drive, "attribute/6")), Measure::Count(3));
    assert_eq!(value(&drive, "attribute/6").level(), Level::High);
    assert_eq!(value(&drive, "attribute/7"), &Value::Text("0/0".to_owned()));
}

#[test]
fn an_error_log_entry_does_not_make_the_report_unreadable() {
    assert!(sata().readings.len() > 1);
}

#[test]
fn a_drive_that_could_not_be_opened_says_why() {
    let drive = report(
        "/dev/x",
        r#"{"smartctl":{"exit_status":2,"messages":[{"string":"Smartctl open device: /dev/x failed: No such device","severity":"error"}]}}"#,
    );
    assert_eq!(drive.readings.len(), 1);
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::Unreadable(
            "Smartctl open device: /dev/x failed: No such device".to_owned()
        ))
    );
}

#[test]
fn a_sleeping_drive_says_it_was_not_woken() {
    let message = "Device is in STANDBY mode, exit(2)";
    let drive = report(
        "/dev/x",
        &format!(
            r#"{{"smartctl":{{"exit_status":2,"messages":[{{"string":"{message}","severity":"information"}}]}}}}"#
        ),
    );
    let row = reading(&drive, "smart");
    assert_eq!(row.value, Value::Text("asleep, not woken".to_owned()));
    assert_eq!(row.origin, message);
}

#[test]
fn a_drive_without_smart_is_not_supported() {
    let drive = report("/dev/x", r#"{"smart_support":{"available":false}}"#);
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::NotSupported)
    );
}

#[test]
fn a_report_the_helper_could_not_get_says_why() {
    let drive = device(&drive("/dev/x"), Err("smartctl gave no answer within 20 s"));
    assert_eq!(drive.title, "/dev/x");
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::Unreadable(
            "smartctl gave no answer within 20 s".to_owned()
        ))
    );
}

#[test]
fn drives_behind_one_controller_are_told_apart_by_type() {
    let first = Drive {
        path: "/dev/bus/0".to_owned(),
        kind: "megaraid,0".to_owned(),
    };
    let second = Drive {
        kind: "megaraid,1".to_owned(),
        ..first.clone()
    };
    let one = device(&first, Ok("{}"));
    let two = device(&second, Ok("{}"));
    assert_ne!(one.key, two.key);
    assert_eq!(two.detail, "SMART · /dev/bus/0 megaraid,1 · unknown");
}

#[test]
fn an_unreadable_report_without_a_message_names_the_exit_status() {
    let drive = report("/dev/x", r#"{"smartctl":{"exit_status":4}}"#);
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::Unreadable("smartctl exit status 4".to_owned()))
    );
}

#[test]
fn output_that_is_not_json_is_unreadable_and_titled_by_the_drive() {
    let drive = report("/dev/x", "smartctl: not found");
    assert_eq!(drive.title, "/dev/x");
    assert_eq!(drive.detail, "SMART · /dev/x · unknown");
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::Unreadable(
            "smartctl output is not JSON".to_owned()
        ))
    );
}

#[test]
fn an_empty_report_has_no_rows() {
    assert_eq!(report("/dev/x", "{}").readings, vec![]);
}

#[test]
fn an_open_failure_that_mentions_a_mode_is_not_sleep() {
    let message = "Read Device Identity failed: unsupported scsi mode page";
    let drive = report(
        "/dev/x",
        &format!(r#"{{"smartctl":{{"exit_status":2,"messages":[{{"string":"{message}"}}]}}}}"#),
    );
    assert_eq!(
        value(&drive, "smart"),
        &Value::Unavailable(Unavailable::Unreadable(message.to_owned()))
    );
}
