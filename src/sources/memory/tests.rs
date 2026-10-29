use std::{fs, path::Path, time::Duration};

use sysglance_core::{
    measure::Measure,
    reading::{Device, Unavailable, Value},
};
use tempfile::TempDir;

use super::{Memory, table};
use crate::sources::Source;

const SECOND: Duration = Duration::from_secs(1);

fn write(root: &Path, file: &str, content: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn collect(root: &Path) -> Vec<Device> {
    Memory::new(root.join("proc"), root.join("sys")).collect(SECOND)
}

fn value<'a>(devices: &'a [Device], key: &str) -> &'a Value {
    &devices
        .iter()
        .flat_map(|d| &d.readings)
        .find(|r| r.key == key)
        .unwrap()
        .value
}

fn measure(value: &Value) -> Measure {
    match value {
        Value::Measured { measure, .. } => *measure,
        Value::Text(_) | Value::Unavailable(_) => panic!("not measured: {value:?}"),
    }
}

const MEMINFO: &str = "MemTotal:       1000 kB\nMemFree:         100 kB\nMemAvailable:    250 kB\nSwapTotal:       400 kB\nSwapFree:        300 kB\nDirty:             8 kB\n";

#[test]
fn a_table_reads_colon_and_space_separated_lines() {
    let parsed = table("MemTotal:  32 kB\npswpin 7\nbad line\n");
    assert_eq!(parsed.get("MemTotal"), Some(&32));
    assert_eq!(parsed.get("pswpin"), Some(&7));
    assert_eq!(parsed.len(), 2);
}

#[test]
fn nothing_to_read_means_no_devices() {
    let root = TempDir::new().unwrap();
    assert_eq!(collect(root.path()).len(), 0);
}

#[test]
fn ram_in_use_is_total_less_available() {
    let root = TempDir::new().unwrap();
    write(root.path(), "proc/meminfo", MEMINFO);
    let devices = collect(root.path());
    assert_eq!(
        measure(value(&devices, "memory:ram/used")),
        Measure::Fraction(0.75)
    );
    assert_eq!(
        measure(value(&devices, "memory:ram/dirty")),
        Measure::Bytes(8192.0)
    );
    assert_eq!(
        measure(value(&devices, "memory:ram/swap")),
        Measure::Fraction(0.25)
    );
    assert_eq!(
        measure(value(&devices, "memory:ram/swap_bytes")),
        Measure::Bytes(102_400.0)
    );
}

#[test]
fn swapping_is_a_byte_rate_from_two_samples() {
    let root = TempDir::new().unwrap();
    write(root.path(), "proc/meminfo", MEMINFO);
    write(
        root.path(),
        "proc/vmstat",
        "pswpin 10\npswpout 0\npgmajfault 5\noom_kill 2\n",
    );
    let mut memory = Memory::new(root.path().join("proc"), root.path().join("sys"));
    let first = memory.collect(SECOND);
    assert_eq!(
        *value(&first, "memory:ram/pswpin"),
        Value::Unavailable(Unavailable::Pending)
    );
    write(
        root.path(),
        "proc/vmstat",
        "pswpin 12\npswpout 0\npgmajfault 25\noom_kill 2\n",
    );
    let second = memory.collect(SECOND);
    assert_eq!(
        measure(value(&second, "memory:ram/pswpin")),
        Measure::BytesPerSecond(8192.0)
    );
    assert_eq!(
        measure(value(&second, "memory:ram/pgmajfault")),
        Measure::PerSecond(20.0)
    );
    assert_eq!(measure(value(&second, "memory:ram/oom")), Measure::Count(2));
}

#[test]
fn each_swap_device_reports_its_share_in_use() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "proc/swaps",
        "Filename\tType\tSize\tUsed\tPriority\n/swap/swapfile file 1000 250 0\n/dev/zram0 partition 800 800 100\n",
    );
    let devices = collect(root.path());
    assert_eq!(
        devices[0].readings[0].label,
        "/swap/swapfile (file, prio 0)"
    );
    assert_eq!(
        measure(&devices[0].readings[0].value),
        Measure::Fraction(0.25)
    );
    assert_eq!(
        measure(&devices[0].readings[1].value),
        Measure::Fraction(1.0)
    );
}

#[test]
fn an_empty_swap_table_has_no_device() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "proc/swaps",
        "Filename\tType\tSize\tUsed\tPriority\n",
    );
    assert_eq!(collect(root.path()).len(), 0);
}

#[test]
fn zram_reports_its_compression() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "sys/block/zram0/mm_stat",
        "6000 1500 1600 0 2000 1 2 3 4\n",
    );
    write(
        root.path(),
        "sys/block/zram0/comp_algorithm",
        "lzo-rle lzo lz4 [zstd] deflate\n",
    );
    write(root.path(), "sys/block/sda/stat", "0\n");
    let devices = collect(root.path());
    assert_eq!(devices.len(), 1);
    assert_eq!(
        measure(value(&devices, "memory:zram0/ratio")),
        Measure::Scalar(4.0)
    );
    assert_eq!(
        *value(&devices, "memory:zram0/algorithm"),
        Value::Text("zstd".to_owned())
    );
}

#[test]
fn an_empty_zram_has_ratio_zero() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "sys/block/zram0/mm_stat",
        "0 0 0 0 0 0 0 0 0\n",
    );
    assert_eq!(
        measure(value(&collect(root.path()), "memory:zram0/ratio")),
        Measure::Scalar(0.0)
    );
}
