use std::{fs, time::Duration};

use sysglance_core::{
    measure::Measure,
    reading::{Unavailable, Value},
};
use tempfile::TempDir;

use super::{Pressure, stalled};
use crate::sources::Source;

const SECOND: Duration = Duration::from_secs(1);

fn psi(some: u64, full: u64) -> String {
    format!(
        "some avg10=0.00 avg60=0.06 avg300=0.12 total={some}\nfull avg10=0.00 avg60=0.05 avg300=0.07 total={full}\n"
    )
}

#[test]
fn a_line_yields_its_kind_and_total() {
    assert_eq!(
        stalled("some avg10=0.22 avg60=0.55 avg300=0.43 total=21098681943"),
        Some(("some", 21_098_681_943))
    );
}

#[test]
fn a_line_without_a_total_is_skipped() {
    assert_eq!(stalled("some avg10=0.22"), None);
}

#[test]
fn without_pressure_files_the_card_says_the_kernel_lacks_psi() {
    let root = TempDir::new().unwrap();
    let devices = Pressure::new(root.path()).collect(SECOND);
    assert_eq!(devices.len(), 1);
    let reading = &devices[0].readings[0];
    assert_eq!(reading.value, Value::Unavailable(Unavailable::NotSupported));
    assert!(
        reading
            .origin
            .ends_with("this kernel has no PSI, or was booted with psi=0")
    );
}

#[test]
fn stall_is_the_share_of_the_last_second() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("io"), psi(0, 0)).unwrap();
    let mut pressure = Pressure::new(root.path());
    let first = pressure.collect(SECOND);
    assert_eq!(
        first[0].readings[0].value,
        Value::Unavailable(Unavailable::Pending)
    );
    fs::write(root.path().join("io"), psi(250_000, 50_000)).unwrap();
    let second = pressure.collect(SECOND);
    assert_eq!(
        second[0].readings[0].value,
        Value::plain(Measure::Fraction(0.25))
    );
    assert_eq!(
        second[0].readings[1].value,
        Value::plain(Measure::Fraction(0.05))
    );
    assert_eq!(second[0].readings[1].label, "io: all tasks stalled");
}

#[test]
fn cpu_full_is_left_out_when_the_kernel_never_counts_it() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("cpu"), psi(10, 0)).unwrap();
    let devices = Pressure::new(root.path()).collect(SECOND);
    assert_eq!(devices[0].readings.len(), 1);
}
