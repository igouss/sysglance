use std::{fs, path::Path, time::Duration};

use sysglance_core::{
    measure::Measure,
    reading::{Device, Unavailable, Value},
};
use tempfile::TempDir;

use super::{Cpu, Jiffies, jiffies, share};
use crate::sources::Source;

const SECOND: Duration = Duration::from_secs(1);

fn write(root: &Path, file: &str, content: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn stat(user: u64, idle: u64, iowait: u64, ctxt: u64) -> String {
    format!(
        "cpu  {user} 0 0 {idle} {iowait} 0 0 0 0 0\ncpu0 0 0 0 {} 0 0 0 0 0 0\nctxt {ctxt}\nintr {} 1 2\nprocesses {}\nprocs_running 3\nprocs_blocked 1\n",
        user + idle + iowait,
        ctxt * 2,
        ctxt / 10
    )
}

fn device<'a>(devices: &'a [Device], key: &str) -> &'a Device {
    devices.iter().find(|d| d.key == key).unwrap()
}

fn value<'a>(device: &'a Device, key: &str) -> &'a Value {
    &device.readings.iter().find(|r| r.key == key).unwrap().value
}

fn measure(value: &Value) -> Measure {
    match value {
        Value::Measured { measure, .. } => *measure,
        Value::Text(_) | Value::Unavailable(_) => panic!("not measured: {value:?}"),
    }
}

#[test]
fn a_cpu_line_sums_busy_and_total_jiffies() {
    let (name, counted) = jiffies("cpu0 10 1 2 100 5 3 4 6 0 0").unwrap();
    assert_eq!(name, "cpu0");
    assert_eq!(
        counted,
        Jiffies {
            busy: 26,
            iowait: 5,
            total: 131
        }
    );
}

#[test]
fn a_four_field_cpu_line_is_read() {
    assert_eq!(jiffies("cpu0 1 2 3 4").map(|(_, j)| j.total), Some(10));
}

#[test]
fn the_clock_range_reads_in_gigahertz() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "800000\n",
    );
    for (file, khz) in [
        ("cpuinfo_min_freq", "800000"),
        ("base_frequency", "3800000"),
        ("cpuinfo_max_freq", "5100000"),
    ] {
        write(
            root.path(),
            &format!("sys/devices/system/cpu/cpufreq/policy0/{file}"),
            &format!("{khz}\n"),
        );
    }
    let devices = Cpu::new(root.path().join("proc"), root.path().join("sys")).collect(SECOND);
    assert_eq!(
        *value(device(&devices, "cpu:clock"), "cpu:clock/range"),
        Value::Text("0.8/3.8/5.1 GHz".to_owned())
    );
}

#[test]
fn a_short_cpu_line_is_rejected() {
    assert_eq!(jiffies("cpu0 10 1"), None);
}

#[test]
fn a_non_numeric_cpu_line_is_rejected() {
    assert_eq!(jiffies("cpu0 10 x 2 100"), None);
}

#[test]
fn no_elapsed_jiffies_have_no_share() {
    let same = Jiffies {
        busy: 1,
        iowait: 0,
        total: 2,
    };
    assert_eq!(share(same, same, |j| j.busy), None);
}

#[test]
fn the_busy_share_is_busy_over_total() {
    let before = Jiffies {
        busy: 10,
        iowait: 0,
        total: 100,
    };
    let after = Jiffies {
        busy: 40,
        iowait: 0,
        total: 200,
    };
    assert_eq!(share(before, after, |j| j.busy), Some(0.3));
}

#[test]
fn load_is_pending_until_the_second_sample() {
    let root = TempDir::new().unwrap();
    write(root.path(), "proc/stat", &stat(0, 0, 0, 0));
    let devices = Cpu::new(root.path().join("proc"), root.path().join("sys")).collect(SECOND);
    assert_eq!(
        *value(device(&devices, "cpu:load"), "cpu:load/all"),
        Value::Unavailable(Unavailable::Pending)
    );
}

#[test]
fn load_and_rates_come_from_two_samples() {
    let root = TempDir::new().unwrap();
    write(root.path(), "proc/stat", &stat(0, 0, 0, 1000));
    let mut cpu = Cpu::new(root.path().join("proc"), root.path().join("sys"));
    cpu.collect(SECOND);
    write(root.path(), "proc/stat", &stat(25, 50, 25, 3000));
    let devices = cpu.collect(Duration::from_secs(2));
    let load = device(&devices, "cpu:load");
    assert_eq!(
        measure(value(load, "cpu:load/all")),
        Measure::Fraction(0.25)
    );
    assert_eq!(
        measure(value(load, "cpu:load/iowait")),
        Measure::Fraction(0.25)
    );
    assert_eq!(
        measure(value(load, "cpu:load/ctxt")),
        Measure::PerSecond(1000.0)
    );
    assert_eq!(measure(value(load, "cpu:load/running")), Measure::Count(3));
    assert_eq!(measure(value(load, "cpu:load/blocked")), Measure::Count(1));
    assert_eq!(
        measure(value(load, "cpu:load/cpu0")),
        Measure::Fraction(0.0)
    );
    assert_eq!(
        measure(value(load, "cpu:load/intr")),
        Measure::PerSecond(2000.0)
    );
    assert_eq!(
        measure(value(load, "cpu:load/processes")),
        Measure::PerSecond(100.0)
    );
}

#[test]
fn load_averages_and_uptime_are_read() {
    let root = TempDir::new().unwrap();
    write(root.path(), "proc/stat", &stat(0, 0, 0, 0));
    write(
        root.path(),
        "proc/loadavg",
        "8.65 7.57 7.23 10/2000 12345\n",
    );
    write(root.path(), "proc/uptime", "660000.50 100.0\n");
    write(
        root.path(),
        "proc/cpuinfo",
        "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz\n",
    );
    let devices = Cpu::new(root.path().join("proc"), root.path().join("sys")).collect(SECOND);
    let load = device(&devices, "cpu:load");
    assert_eq!(load.detail, "Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz");
    assert_eq!(
        measure(value(load, "cpu:load/avg15")),
        Measure::Scalar(7.23)
    );
    assert_eq!(
        measure(value(load, "cpu:load/uptime")),
        Measure::Seconds(660_000.5)
    );
}

#[test]
fn clocks_read_each_cpu_in_hertz() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
        "4700000\n",
    );
    write(
        root.path(),
        "sys/devices/system/cpu/cpu1/cpufreq/scaling_cur_freq",
        "800000\n",
    );
    write(
        root.path(),
        "sys/devices/system/cpu/intel_pstate/no_turbo",
        "0\n",
    );
    let devices = Cpu::new(root.path().join("proc"), root.path().join("sys")).collect(SECOND);
    let clocks = device(&devices, "cpu:clock");
    assert_eq!(
        measure(value(clocks, "cpu:clock/cpu0")),
        Measure::Hertz(4.7e9)
    );
    assert_eq!(
        measure(value(clocks, "cpu:clock/cpu1")),
        Measure::Hertz(8e8)
    );
    assert_eq!(
        *value(clocks, "cpu:clock/turbo"),
        Value::Text("enabled".to_owned())
    );
}

#[test]
fn idle_residency_is_a_share_of_every_threads_time() {
    let root = TempDir::new().unwrap();
    let state = |cpu: u32, time: u64| {
        write(
            root.path(),
            &format!("sys/devices/system/cpu/cpu{cpu}/cpuidle/state1/name"),
            "C1_ACPI\n",
        );
        write(
            root.path(),
            &format!("sys/devices/system/cpu/cpu{cpu}/cpuidle/state1/time"),
            &format!("{time}\n"),
        );
    };
    state(0, 0);
    state(1, 0);
    let mut cpu = Cpu::new(root.path().join("proc"), root.path().join("sys"));
    cpu.collect(SECOND);
    state(0, 1_000_000);
    state(1, 500_000);
    let devices = cpu.collect(SECOND);
    assert_eq!(
        measure(value(device(&devices, "cpu:idle"), "cpu:idle/C1_ACPI")),
        Measure::Fraction(0.75)
    );
}

#[test]
fn throttling_counts_package_once_and_each_physical_core() {
    let root = TempDir::new().unwrap();
    for (cpu, core, siblings, count) in [
        (0, 0, "0,2", 4),
        (1, 1, "1,3", 0),
        (2, 0, "0,2", 9),
        (3, 1, "1,3", 0),
    ] {
        let dir = format!("sys/devices/system/cpu/cpu{cpu}");
        write(
            root.path(),
            &format!("{dir}/topology/core_id"),
            &format!("{core}\n"),
        );
        write(
            root.path(),
            &format!("{dir}/topology/thread_siblings_list"),
            &format!("{siblings}\n"),
        );
        write(
            root.path(),
            &format!("{dir}/thermal_throttle/core_throttle_count"),
            &format!("{count}\n"),
        );
        write(
            root.path(),
            &format!("{dir}/thermal_throttle/package_throttle_count"),
            "1\n",
        );
        write(
            root.path(),
            &format!("{dir}/thermal_throttle/package_throttle_total_time_ms"),
            "6\n",
        );
        write(
            root.path(),
            &format!("{dir}/thermal_throttle/package_throttle_max_time_ms"),
            "6\n",
        );
    }
    let devices = Cpu::new(root.path().join("proc"), root.path().join("sys")).collect(SECOND);
    let throttle = device(&devices, "cpu:throttle");
    let labels: Vec<&str> = throttle.readings.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "package events",
            "package time throttled",
            "package longest throttle",
            "core 0 events",
            "core 1 events"
        ]
    );
    assert_eq!(
        measure(&throttle.readings[1].value),
        Measure::Seconds(0.006)
    );
    assert_eq!(measure(&throttle.readings[3].value), Measure::Count(4));
}

#[test]
fn nothing_to_read_means_no_devices() {
    let root = TempDir::new().unwrap();
    assert_eq!(
        Cpu::new(root.path().join("proc"), root.path().join("sys"))
            .collect(SECOND)
            .len(),
        0
    );
}

fn throttling_cpu(root: &Path, cpu: u32, package: u32, core: u32, siblings: &str) {
    let dir = format!("sys/devices/system/cpu/cpu{cpu}");
    write(
        root,
        &format!("{dir}/topology/core_id"),
        &format!("{core}\n"),
    );
    write(
        root,
        &format!("{dir}/topology/physical_package_id"),
        &format!("{package}\n"),
    );
    write(
        root,
        &format!("{dir}/topology/thread_siblings_list"),
        &format!("{siblings}\n"),
    );
    write(
        root,
        &format!("{dir}/thermal_throttle/core_throttle_count"),
        "0\n",
    );
    write(
        root,
        &format!("{dir}/thermal_throttle/package_throttle_count"),
        "0\n",
    );
}

fn throttle_labels(root: &Path) -> Vec<String> {
    let devices = Cpu::new(root.join("proc"), root.join("sys")).collect(SECOND);
    device(&devices, "cpu:throttle")
        .readings
        .iter()
        .map(|r| r.label.clone())
        .collect()
}

#[test]
fn each_package_of_a_multi_socket_machine_is_named() {
    let root = TempDir::new().unwrap();
    throttling_cpu(root.path(), 0, 0, 0, "0");
    throttling_cpu(root.path(), 1, 1, 0, "1");
    assert_eq!(
        throttle_labels(root.path()),
        [
            "package 0 events",
            "package 0 time throttled",
            "package 0 longest throttle",
            "package 0 core 0 events",
            "package 1 events",
            "package 1 time throttled",
            "package 1 longest throttle",
            "package 1 core 0 events",
        ]
    );
}

#[test]
fn an_offline_cpu_is_not_a_core() {
    let root = TempDir::new().unwrap();
    throttling_cpu(root.path(), 0, 0, 0, "0");
    throttling_cpu(root.path(), 1, 0, 1, "1");
    write(root.path(), "sys/devices/system/cpu/cpu1/online", "0\n");
    let labels = throttle_labels(root.path());
    assert_eq!(labels.last().map(String::as_str), Some("core 0 events"));
}

#[test]
fn a_cpu_without_topology_is_not_a_core() {
    let root = TempDir::new().unwrap();
    throttling_cpu(root.path(), 0, 0, 0, "0");
    write(
        root.path(),
        "sys/devices/system/cpu/cpu1/thermal_throttle/core_throttle_count",
        "0\n",
    );
    write(root.path(), "sys/devices/system/cpu/cpu1/online", "1\n");
    let labels = throttle_labels(root.path());
    assert_eq!(labels.last().map(String::as_str), Some("core 0 events"));
}

fn policy(root: &Path, index: u32, files: &[(&str, &str)]) {
    write(
        root,
        &format!("sys/devices/system/cpu/cpu{index}/cpufreq/scaling_cur_freq"),
        "800000\n",
    );
    for (file, content) in files {
        write(
            root,
            &format!("sys/devices/system/cpu/cpufreq/policy{index}/{file}"),
            &format!("{content}\n"),
        );
    }
}

fn clocks(root: &Path) -> Device {
    let devices = Cpu::new(root.join("proc"), root.join("sys")).collect(SECOND);
    device(&devices, "cpu:clock").clone()
}

#[test]
fn the_range_spans_every_policy_and_drops_a_base_they_disagree_on() {
    let root = TempDir::new().unwrap();
    policy(
        root.path(),
        0,
        &[
            ("cpuinfo_min_freq", "800000"),
            ("base_frequency", "1800000"),
            ("cpuinfo_max_freq", "3600000"),
        ],
    );
    policy(
        root.path(),
        1,
        &[
            ("cpuinfo_min_freq", "400000"),
            ("base_frequency", "2400000"),
            ("cpuinfo_max_freq", "5000000"),
        ],
    );
    let clocks = clocks(root.path());
    let range = clocks
        .readings
        .iter()
        .find(|r| r.key == "cpu:clock/range")
        .unwrap();
    assert_eq!(range.label, "min / max");
    assert_eq!(range.value, Value::Text("0.4/5.0 GHz".to_owned()));
}

#[test]
fn the_detail_names_the_running_cpufreq_driver() {
    let root = TempDir::new().unwrap();
    policy(root.path(), 0, &[("scaling_driver", "amd-pstate-epp")]);
    assert_eq!(clocks(root.path()).detail, "cpufreq · amd-pstate-epp");
}

#[test]
fn without_a_driver_the_detail_is_cpufreq() {
    let root = TempDir::new().unwrap();
    policy(root.path(), 0, &[]);
    assert_eq!(clocks(root.path()).detail, "cpufreq");
}

#[test]
fn cpufreq_boost_reports_turbo_where_intel_pstate_is_absent() {
    let root = TempDir::new().unwrap();
    policy(root.path(), 0, &[]);
    write(root.path(), "sys/devices/system/cpu/cpufreq/boost", "0\n");
    assert_eq!(
        *value(&clocks(root.path()), "cpu:clock/turbo"),
        Value::Text("disabled".to_owned())
    );
    write(root.path(), "sys/devices/system/cpu/cpufreq/boost", "1\n");
    assert_eq!(
        *value(&clocks(root.path()), "cpu:clock/turbo"),
        Value::Text("enabled".to_owned())
    );
}
