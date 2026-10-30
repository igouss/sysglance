use std::{
    fs,
    os::unix::fs::{FileExt, PermissionsExt},
    path::Path,
    time::Duration,
};

use sysglance_core::msr::{PACKAGE, PER_CPU, SMI_COUNT, TSC};

use super::{cpus, kind_allowed, one_line, registers, round, scanned};

fn fake_msr(root: &Path, cpu: u32, values: &[(u32, u64)]) {
    let dir = root.join(cpu.to_string());
    fs::create_dir_all(&dir).expect("cpu dir");
    let file = fs::File::create(dir.join("msr")).expect("msr file");
    file.set_len(0x700).expect("sparse msr file");
    for (address, value) in values {
        file.write_all_at(&value.to_le_bytes(), u64::from(*address))
            .expect("register");
    }
}

fn streamed(root: &Path) -> String {
    let mut out = Vec::new();
    registers(&mut out, &cpus(root)).expect("written");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn no_cpus_stream_no_registers() {
    let root = tempfile::tempdir().expect("tempdir");
    assert_eq!(streamed(root.path()), "");
}

#[test]
fn the_first_cpu_streams_package_and_per_cpu_registers() {
    let root = tempfile::tempdir().expect("tempdir");
    fake_msr(root.path(), 0, &[(TSC, 0xabc), (SMI_COUNT, 0xf6)]);
    let lines = streamed(root.path());
    assert_eq!(lines.lines().count(), PER_CPU.len() + PACKAGE.len());
    assert!(lines.contains("msr 0 0x10 0xabc\n"));
    assert!(lines.contains("msr 0 0x34 0xf6\n"));
}

#[test]
fn other_cpus_stream_only_per_cpu_registers_in_cpu_order() {
    let root = tempfile::tempdir().expect("tempdir");
    for cpu in [10, 0, 2] {
        fake_msr(root.path(), cpu, &[(TSC, u64::from(cpu))]);
    }
    fs::create_dir_all(root.path().join("microcode")).expect("non-cpu entry");
    let lines = streamed(root.path());
    assert_eq!(lines.lines().count(), PER_CPU.len() * 3 + PACKAGE.len());
    let tscs: Vec<&str> = lines.lines().filter(|l| l.contains(" 0x10 ")).collect();
    assert_eq!(
        tscs,
        vec!["msr 0 0x10 0x0", "msr 2 0x10 0x2", "msr 10 0x10 0xa"]
    );
}

#[test]
fn a_register_that_cannot_be_read_is_left_out() {
    let root = tempfile::tempdir().expect("tempdir");
    fake_msr(root.path(), 0, &[]);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(root.path().join("0/msr"))
        .expect("msr file");
    file.set_len(0x20).expect("truncated");
    assert_eq!(streamed(root.path()), "msr 0 0x10 0x0\n");
}

#[test]
fn a_scan_keeps_each_device_with_its_type() {
    let scan = "/dev/sda -d sat # /dev/sda [SAT], ATA device\n/dev/bus/0 -d megaraid,1 # x\n# comment\n\n/dev/nvme0 -d nvme # /dev/nvme0, NVMe device\n";
    assert_eq!(
        scanned(scan),
        vec![
            ("/dev/sda", "sat"),
            ("/dev/bus/0", "megaraid,1"),
            ("/dev/nvme0", "nvme")
        ]
    );
}

#[test]
fn a_scan_without_a_type_lets_smartctl_choose() {
    assert_eq!(scanned("/dev/sdb\n"), vec![("/dev/sdb", "auto")]);
}

#[test]
fn an_empty_scan_has_no_devices() {
    assert_eq!(scanned(""), Vec::<(&str, &str)>::new());
}

#[test]
fn a_type_that_could_be_an_option_or_a_path_is_refused() {
    assert!(kind_allowed("usbjmicron,x,0"));
    assert!(kind_allowed("sat+megaraid,2"));
    assert!(!kind_allowed("-a"));
    assert!(!kind_allowed("../x"));
    assert!(!kind_allowed(""));
    assert_eq!(scanned("/dev/sda -d --all\n"), Vec::<(&str, &str)>::new());
}

#[test]
fn a_compact_report_is_one_line() {
    assert_eq!(one_line("{\"a\":1}\n"), Some("{\"a\":1}"));
}

#[test]
fn an_empty_or_multi_line_report_is_dropped() {
    assert_eq!(one_line("  \n"), None);
    assert_eq!(one_line("{\n\"a\":1}"), None);
}

fn fake_smartctl(dir: &Path, body: &str) -> String {
    let path = dir.join("smartctl");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("script");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
    path.display().to_string()
}

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn without_smartctl_a_round_says_so() {
    let lines = round(None, SECOND);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("smart-unavailable smartctl is not installed"));
}

#[test]
fn a_round_reports_every_scanned_drive_with_its_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let smartctl = fake_smartctl(
        dir.path(),
        r#"case "$1" in --scan-open) printf '/dev/sda -d sat # a\n/dev/sdb -d sat # b\n';; *) [ "$7" = /dev/sda ] && printf '{"d":"%s %s"}\n' "$6" "$7";; esac"#,
    );
    assert_eq!(
        round(Some(&smartctl), SECOND),
        vec![
            "smart-round",
            r#"smart /dev/sda sat {"d":"sat /dev/sda"}"#,
            "smart-error /dev/sdb sat smartctl printed no report",
        ]
    );
}

#[test]
fn a_drive_that_hangs_is_reported_and_the_round_goes_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let smartctl = fake_smartctl(
        dir.path(),
        r#"case "$1" in --scan-open) printf '/dev/sda -d sat\n/dev/sdb -d sat\n';; *) [ "$7" = /dev/sda ] && exec sleep 5; printf '{}\n';; esac"#,
    );
    assert_eq!(
        round(Some(&smartctl), Duration::from_millis(300)),
        vec![
            "smart-round",
            "smart-error /dev/sda sat smartctl gave no answer within 0 s",
            "smart /dev/sdb sat {}",
        ]
    );
}

#[test]
fn a_scan_that_cannot_run_makes_smart_unavailable() {
    let lines = round(Some("/nonexistent/smartctl"), SECOND);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("smart-unavailable /nonexistent/smartctl: "),
        "{lines:?}"
    );
}
