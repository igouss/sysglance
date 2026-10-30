use serde_json::Value as Json;
use sysglance_core::{
    level::{Alarms, Level, Limits},
    measure::Measure,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::helper::Drive;

const UNREADABLE: u64 = 0b111;
const NVME_DATA_UNIT_BYTES: u64 = 512_000;
const NVME_CRITICAL_WARNINGS: [(u64, &str); 6] = [
    (0, "spare below threshold"),
    (1, "temperature"),
    (2, "reliability degraded"),
    (3, "read-only"),
    (4, "volatile backup failed"),
    (5, "persistent memory read-only"),
];

fn number(json: &Json, pointer: &str) -> Option<u64> {
    json.pointer(pointer).and_then(Json::as_u64)
}

fn string<'a>(json: &'a Json, pointer: &str) -> Option<&'a str> {
    json.pointer(pointer).and_then(Json::as_str)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "SMART counters shown here are far below 2^52"
)]
const fn float(value: u64) -> f64 {
    value as f64
}

struct Rows<'a> {
    key: String,
    drive: &'a str,
    readings: Vec<Reading>,
}

impl Rows<'_> {
    fn push(&mut self, key: &str, label: &str, value: Value, origin: String) {
        self.readings.push(Reading {
            key: format!("{}/{key}", self.key),
            label: label.to_owned(),
            value,
            origin,
        });
    }

    fn field(&mut self, key: &str, label: &str, value: Option<Value>, field: &str) {
        if let Some(value) = value {
            let origin = format!("smartctl -a {} · {field}", self.drive);
            self.push(key, label, value, origin);
        }
    }
}

fn tripped(on: bool) -> Value {
    Value::measured(
        Measure::Tripped(on),
        Limits::new(None, None, Some(1.0)),
        Alarms::default(),
    )
}

fn percent(json: &Json, pointer: &str) -> Option<f64> {
    number(json, pointer).map(|percent| float(percent) / 100.0)
}

fn common(rows: &mut Rows, json: &Json) {
    let health = json
        .pointer("/smart_status/passed")
        .and_then(Json::as_bool)
        .map(|passed| tripped(!passed));
    rows.field("health", "overall health", health, "smart_status.passed");
    let used = percent(json, "/endurance_used/current_percent").map(|used| {
        Value::measured(
            Measure::Fraction(used),
            Limits::new(None, Some(1.0), None),
            Alarms::default(),
        )
    });
    rows.field("endurance", "endurance used", used, "endurance_used");
    let spare = percent(json, "/spare_available/current_percent").map(|spare| {
        Value::measured(
            Measure::Fraction(spare),
            Limits::new(
                percent(json, "/spare_available/threshold_percent"),
                None,
                None,
            ),
            Alarms::default(),
        )
    });
    rows.field("spare", "spare available", spare, "spare_available");
    let hours = number(json, "/power_on_time/hours")
        .map(|hours| Value::plain(Measure::Seconds(float(hours) * 3600.0)));
    rows.field("powered-on", "powered on", hours, "power_on_time.hours");
    let cycles = number(json, "/power_cycle_count").map(|n| Value::plain(Measure::Count(n)));
    rows.field("cycles", "power cycles", cycles, "power_cycle_count");
}

fn nvme(rows: &mut Rows, json: &Json) {
    const LOG: &str = "/nvme_smart_health_information_log";
    let at = |field: &str| number(json, &format!("{LOG}/{field}"));
    if let Some(warning) = at("critical_warning") {
        let names: Vec<&str> = NVME_CRITICAL_WARNINGS
            .iter()
            .filter(|(bit, _)| warning >> bit & 1 == 1)
            .map(|(_, name)| *name)
            .collect();
        let value = tripped(warning != 0);
        let origin = format!(
            "smartctl -a {} · critical_warning {warning:#04x}{}",
            rows.drive,
            if names.is_empty() {
                String::new()
            } else {
                format!(": {}", names.join(", "))
            }
        );
        rows.push("critical-warning", "critical warning", value, origin);
    }
    let bytes = |units: u64| Value::plain(Measure::Bytes(float(units * NVME_DATA_UNIT_BYTES)));
    let minutes = |minutes: u64| Value::plain(Measure::Seconds(float(minutes) * 60.0));
    let count = |n: u64| Value::plain(Measure::Count(n));
    for (field, label, value) in [
        ("data_units_read", "read", at("data_units_read").map(bytes)),
        (
            "data_units_written",
            "written",
            at("data_units_written").map(bytes),
        ),
        (
            "media_errors",
            "media errors",
            at("media_errors").map(count),
        ),
        (
            "num_err_log_entries",
            "error log entries",
            at("num_err_log_entries").map(count),
        ),
        (
            "unsafe_shutdowns",
            "unsafe shutdowns",
            at("unsafe_shutdowns").map(count),
        ),
        (
            "controller_busy_time",
            "controller busy",
            at("controller_busy_time").map(minutes),
        ),
        (
            "warning_temp_time",
            "time above warning temperature",
            at("warning_temp_time").map(minutes),
        ),
        (
            "critical_comp_time",
            "time above critical temperature",
            at("critical_comp_time").map(minutes),
        ),
    ] {
        rows.field(field, label, value, field);
    }
}

fn attribute_level(when_failed: &str, threshold: u64) -> Level {
    match when_failed {
        "now" => Level::Critical,
        "past" => Level::High,
        _ if threshold > 0 => Level::Normal,
        _ => Level::Unrated,
    }
}

fn ata(rows: &mut Rows, json: &Json) {
    let Some(table) = json
        .pointer("/ata_smart_attributes/table")
        .and_then(Json::as_array)
    else {
        return;
    };
    for attribute in table {
        let (Some(id), Some(name)) = (number(attribute, "/id"), string(attribute, "/name")) else {
            continue;
        };
        let raw = string(attribute, "/raw/string").unwrap_or_default();
        let threshold = number(attribute, "/thresh").unwrap_or(0);
        let when_failed = string(attribute, "/when_failed").unwrap_or_default();
        let value = raw
            .split_whitespace()
            .next()
            .and_then(|first| first.parse::<u64>().ok())
            .map_or_else(
                || Value::Text(raw.to_owned()),
                |count| Value::Measured {
                    measure: Measure::Count(count),
                    limits: Limits::NONE,
                    level: attribute_level(when_failed, threshold),
                },
            );
        let origin = format!(
            "smartctl -a {} · attribute {id}: raw {raw}, normalised {}, worst {}, threshold {threshold}",
            rows.drive,
            number(attribute, "/value").unwrap_or(0),
            number(attribute, "/worst").unwrap_or(0),
        );
        rows.push(&format!("attribute/{id}"), name, value, origin);
    }
}

enum Verdict {
    Readable,
    Asleep(String),
    Unreadable(String),
    Unsupported,
}

fn verdict(json: &Json) -> Verdict {
    if !json.is_object() {
        return Verdict::Unreadable("smartctl output is not JSON".to_owned());
    }
    let exit = number(json, "/smartctl/exit_status").unwrap_or(0);
    if exit & UNREADABLE != 0 {
        let message = string(json, "/smartctl/messages/0/string");
        return match message {
            Some(message) if message.starts_with("Device is in ") && message.contains(" mode") => {
                Verdict::Asleep(message.to_owned())
            }
            Some(message) => Verdict::Unreadable(message.to_owned()),
            None => Verdict::Unreadable(format!("smartctl exit status {exit}")),
        };
    }
    if json
        .pointer("/smart_support/available")
        .and_then(Json::as_bool)
        == Some(false)
    {
        return Verdict::Unsupported;
    }
    Verdict::Readable
}

pub fn unavailable(why: &str) -> Device {
    Device {
        key: "smart".to_owned(),
        title: "SMART".to_owned(),
        detail: "drive health · sysglance-helper".to_owned(),
        category: Category::Storage,
        readings: vec![Reading {
            key: "smart/status".to_owned(),
            label: "status".to_owned(),
            value: Value::Unavailable(Unavailable::Unreadable(why.to_owned())),
            origin: why.to_owned(),
        }],
    }
}

pub fn device(drive: &Drive, report: Result<&str, &str>) -> Device {
    let name = if drive.kind.contains(',') {
        format!("{} {}", drive.path, drive.kind)
    } else {
        drive.path.clone()
    };
    let mut rows = Rows {
        key: format!("smart:{} {}", drive.path, drive.kind),
        drive: &name,
        readings: Vec::new(),
    };
    let json: Json = report
        .ok()
        .and_then(|report| serde_json::from_str(report).ok())
        .unwrap_or(Json::Null);
    let origin = format!("smartctl -a -d {} {}", drive.kind, drive.path);
    match report.map(|_| verdict(&json)) {
        Err(why) => {
            let value = Value::Unavailable(Unavailable::Unreadable(why.to_owned()));
            rows.push("smart", "SMART", value, origin);
        }
        Ok(Verdict::Unreadable(why)) => {
            let value = Value::Unavailable(Unavailable::Unreadable(why));
            rows.push("smart", "SMART", value, origin);
        }
        Ok(Verdict::Asleep(message)) => {
            let value = Value::Text("asleep, not woken".to_owned());
            rows.push("smart", "SMART", value, message);
        }
        Ok(Verdict::Unsupported) => {
            let value = Value::Unavailable(Unavailable::NotSupported);
            rows.push("smart", "SMART", value, origin);
        }
        Ok(Verdict::Readable) => {
            common(&mut rows, &json);
            nvme(&mut rows, &json);
            ata(&mut rows, &json);
        }
    }
    let protocol = string(&json, "/device/protocol").unwrap_or("unknown");
    let firmware = string(&json, "/firmware_version")
        .map(|firmware| format!(" · firmware {firmware}"))
        .unwrap_or_default();
    Device {
        key: rows.key.clone(),
        title: string(&json, "/model_name").unwrap_or(&name).to_owned(),
        detail: format!("SMART · {name} · {protocol}{firmware}"),
        category: Category::Storage,
        readings: rows.readings,
    }
}

#[cfg(test)]
mod tests;
