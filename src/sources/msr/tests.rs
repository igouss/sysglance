use std::{fs, io::Cursor, path::Path, sync::Mutex};

use sysglance_core::{
    measure::Measure,
    msr::{PACKAGE_RESIDENCY, PLATFORM_INFO, SMI_COUNT, TSC, TURBO_RATIO_LIMIT},
    reading::{Device, Unavailable, Value},
};
use tempfile::TempDir;

use super::{decodable, devices};
use crate::sources::helper::{Link, Registers, frames};

const COMET_LAKE: &str = "processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 165\nmodel name\t: Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz\n";

fn topology(cpus: u32, cores: u32) -> TempDir {
    let sys = tempfile::tempdir().unwrap();
    for cpu in 0..cpus {
        let dir = sys
            .path()
            .join(format!("devices/system/cpu/cpu{cpu}/topology"));
        fs::create_dir_all(&dir).unwrap();
        let core = cpu % cores;
        fs::write(dir.join("core_id"), format!("{core}\n")).unwrap();
        fs::write(
            dir.join("thread_siblings_list"),
            format!("{core},{}\n", core + cores),
        )
        .unwrap();
    }
    sys
}

fn fixture() -> (Registers, Registers) {
    let link = Mutex::new(Link::Starting);
    frames(Cursor::new(include_str!("two-frames.txt")), &link);
    match link.into_inner().unwrap() {
        Link::Streaming {
            previous: Some(previous),
            latest,
            ..
        } => (previous, latest),
        other @ (Link::Starting
        | Link::Streaming { previous: None, .. }
        | Link::Refused(_)
        | Link::Lost(_)) => panic!("not two frames: {other:?}"),
    }
}

fn comet_lake(sys: &Path) -> Vec<Device> {
    let (first, second) = fixture();
    devices(sys, Some(&first), &second)
}

fn comet_lake_first_frame(sys: &Path) -> Vec<Device> {
    let (_, second) = fixture();
    devices(sys, None, &second)
}

fn device<'a>(devices: &'a [Device], key: &str) -> &'a Device {
    devices.iter().find(|d| d.key == key).unwrap()
}

fn value<'a>(device: &'a Device, key: &str) -> &'a Value {
    &device.readings.iter().find(|r| r.key == key).unwrap().value
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn fraction(value: &Value) -> f64 {
    match value {
        Value::Measured {
            measure: Measure::Fraction(fraction),
            ..
        } => *fraction,
        Value::Measured { .. } | Value::Text(_) | Value::Unavailable(_) => {
            panic!("not a fraction: {value:?}")
        }
    }
}

#[test]
fn a_comet_lake_is_decodable() {
    assert!(decodable(COMET_LAKE));
}

#[test]
fn another_intel_model_is_not_decodable() {
    assert!(!decodable(&COMET_LAKE.replace("165", "151")));
}

#[test]
fn another_vendor_or_family_is_not_decodable() {
    assert!(!decodable(
        &COMET_LAKE.replace("GenuineIntel", "AuthenticAMD")
    ));
    assert!(!decodable(
        &COMET_LAKE.replace("family\t: 6", "family\t: 15")
    ));
}

#[test]
fn empty_cpuinfo_is_not_decodable() {
    assert!(!decodable(""));
}

#[test]
fn no_registers_means_the_msr_module_is_missing() {
    let sys = topology(0, 1);
    let found = devices(sys.path(), None, &Registers::new());
    assert_eq!(
        value(device(&found, "msr:package"), "msr:package/registers"),
        &Value::Unavailable(Unavailable::ModuleNotLoaded("msr"))
    );
}

#[test]
fn limit_reasons_say_what_limits_the_clock_now_and_since_boot() {
    let sys = topology(16, 8);
    let package = device(&comet_lake(sys.path()), "msr:package").clone();
    assert_eq!(
        value(&package, "msr:package/limits/now"),
        &text("max turbo")
    );
    assert_eq!(
        value(&package, "msr:package/limits/logged"),
        &text("max turbo, transitions")
    );
    assert_eq!(value(&package, "msr:package/thermal/now"), &text("nothing"));
    assert_eq!(
        value(&package, "msr:package/thermal/logged"),
        &text("nothing")
    );
}

#[test]
fn the_package_reads_voltage_temperature_target_and_turbo_ladder() {
    let sys = topology(16, 8);
    let package = device(&comet_lake(sys.path()), "msr:package").clone();
    assert_eq!(
        value(&package, "msr:package/vid"),
        &Value::plain(Measure::Volts(1.281_494_140_625))
    );
    assert_eq!(
        value(&package, "msr:package/tjmax"),
        &Value::plain(Measure::Celsius(100.0))
    );
    assert_eq!(
        value(&package, "msr:package/tcc-offset"),
        &Value::plain(Measure::Celsius(0.0))
    );
    assert_eq!(
        value(&package, "msr:package/turbo"),
        &text("1-3: 5.1, 4-5: 4.8, 6-8: 4.7 GHz")
    );
    assert_eq!(value(&package, "msr:package/cstate-limit"), &text("PC0"));
}

#[test]
fn package_residencies_that_never_advanced_are_left_out() {
    let sys = topology(16, 8);
    let package = device(&comet_lake(sys.path()), "msr:package").clone();
    assert!(
        !package
            .readings
            .iter()
            .any(|reading| reading.key.ends_with("PC6"))
    );
}

#[test]
fn a_package_residency_that_advanced_is_a_share_of_the_tsc() {
    let (pc6, _) = PACKAGE_RESIDENCY[2];
    let before = Registers::from([((0, TSC), 1000), ((0, pc6), 100)]);
    let after = Registers::from([((0, TSC), 2000), ((0, pc6), 350)]);
    let sys = topology(1, 1);
    let found = devices(sys.path(), Some(&before), &after);
    let package = device(&found, "msr:package");
    assert!((fraction(value(package, "msr:package/PC6")) - 0.25).abs() < 1e-12);
}

#[test]
fn smis_are_a_rate_over_tsc_seconds() {
    let base_38: u64 = 38 << 8;
    let before = Registers::from([((0, TSC), 0), ((0, SMI_COUNT), 10)]);
    let after = Registers::from([
        ((0, TSC), 7_600_000_000),
        ((0, SMI_COUNT), 16),
        ((0, PLATFORM_INFO), base_38),
    ]);
    let sys = topology(1, 1);
    let found = devices(sys.path(), Some(&before), &after);
    assert_eq!(
        value(device(&found, "msr:package"), "msr:package/smi"),
        &Value::plain(Measure::PerSecond(3.0))
    );
}

#[test]
fn smis_on_the_fixture_are_zero_per_second() {
    let sys = topology(16, 8);
    let package = device(&comet_lake(sys.path()), "msr:package").clone();
    assert_eq!(
        value(&package, "msr:package/smi"),
        &Value::plain(Measure::PerSecond(0.0))
    );
}

#[test]
fn rates_wait_for_a_second_frame() {
    let sys = topology(16, 8);
    let found = comet_lake_first_frame(sys.path());
    let pending = Value::Unavailable(Unavailable::Pending);
    assert_eq!(
        value(device(&found, "msr:package"), "msr:package/smi"),
        &pending
    );
    assert_eq!(
        value(device(&found, "msr:cores"), "msr:cores/core0/C7"),
        &pending
    );
}

#[test]
fn core_states_read_each_physical_core_once() {
    let sys = topology(16, 8);
    let found = comet_lake(sys.path());
    let cores = device(&found, "msr:cores");
    assert_eq!(cores.readings.len(), 8);
    assert_eq!(cores.readings[0].origin, "MSR 0x3FE on cpu0");
    assert!((fraction(value(cores, "msr:cores/core0/C7")) - 0.073_860_153_882).abs() < 1e-9);
    assert!(fraction(value(cores, "msr:cores/core1/C7")).abs() < 1e-12);
}

#[test]
fn core_states_name_those_never_entered() {
    let sys = topology(16, 8);
    let found = comet_lake(sys.path());
    assert_eq!(
        device(&found, "msr:cores").detail,
        "hardware residency · never entered: C3, C6"
    );
}

#[test]
fn without_any_core_state_there_is_no_core_card() {
    let sys = topology(2, 2);
    let latest = Registers::from([((0, TSC), 1)]);
    let found = devices(sys.path(), None, &latest);
    assert!(found.iter().all(|device| device.key != "msr:cores"));
}

#[test]
fn a_ladder_of_one_ratio_per_core_count_names_each_count() {
    let latest = Registers::from([((0, TURBO_RATIO_LIMIT), 0x2d2f_3133)]);
    let sys = topology(1, 1);
    let found = devices(sys.path(), None, &latest);
    assert_eq!(
        value(device(&found, "msr:package"), "msr:package/turbo"),
        &text("1: 5.1, 2: 4.9, 3: 4.7, 4: 4.5 GHz")
    );
}

#[test]
fn an_empty_ladder_says_none() {
    let latest = Registers::from([((0, TURBO_RATIO_LIMIT), 0)]);
    let sys = topology(1, 1);
    let found = devices(sys.path(), None, &latest);
    assert_eq!(
        value(device(&found, "msr:package"), "msr:package/turbo"),
        &text("none")
    );
}
