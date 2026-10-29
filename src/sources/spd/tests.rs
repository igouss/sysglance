use std::{fs, path::Path, time::Duration};

use sysglance_core::reading::{Unavailable, Value};
use tempfile::TempDir;

use super::{Module, Spd, ddr4};
use crate::sources::Source;

const CORSAIR: &[u8] = include_bytes!("corsair-cmw32gx4m2z2933c16.bin");

fn module(root: &Path, address: &str, name: &str, eeprom: &[u8]) {
    let dir = root.join(address);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("name"), format!("{name}\n")).unwrap();
    fs::write(dir.join("eeprom"), eeprom).unwrap();
}

#[test]
fn a_real_ddr4_spd_decodes() {
    assert_eq!(
        ddr4(CORSAIR),
        Some(Module {
            manufacturer: "Corsair".to_owned(),
            part: "CMW32GX4M2Z2933C16".to_owned(),
            bytes: 16 << 30,
            transfers: 2666,
            ranks: 2,
            width: 8,
        })
    );
}

#[test]
fn a_short_spd_is_not_ddr4() {
    assert_eq!(ddr4(&CORSAIR[..100]), None);
}

#[test]
fn another_memory_type_is_not_ddr4() {
    let mut ddr5 = CORSAIR.to_vec();
    ddr5[2] = 0x12;
    assert_eq!(ddr4(&ddr5), None);
}

#[test]
fn a_negative_fine_offset_shortens_the_cycle() {
    let mut faster = CORSAIR.to_vec();
    faster[18] = 0x06;
    faster[125] = 0x83;
    assert_eq!(ddr4(&faster).unwrap().transfers, 3200);
}

#[test]
fn an_unknown_manufacturer_names_its_jedec_code() {
    let mut unknown = CORSAIR.to_vec();
    unknown[320] = 0x83;
    unknown[321] = 0x11;
    assert_eq!(
        ddr4(&unknown).unwrap().manufacturer,
        "JEDEC bank 4 code 0x11"
    );
}

#[test]
fn modules_are_listed_by_address_and_other_chips_skipped() {
    let root = TempDir::new().unwrap();
    module(root.path(), "19-0051", "ee1004", CORSAIR);
    module(root.path(), "19-0050", "ee1004", CORSAIR);
    module(root.path(), "19-0018", "jc42", b"");
    let devices = Spd::new(root.path()).collect(Duration::ZERO);
    let labels: Vec<&str> = devices[0]
        .readings
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(labels, ["module 19-0050", "module 19-0051"]);
    assert_eq!(
        devices[0].readings[0].value,
        Value::Text("Corsair CMW32GX4M2Z2933C16 · 16 GiB DDR4-2666 · 2R×8".to_owned())
    );
}

#[test]
fn an_undecodable_module_is_not_supported() {
    let root = TempDir::new().unwrap();
    module(root.path(), "19-0050", "spd5118", &[0; 1024]);
    let devices = Spd::new(root.path()).collect(Duration::ZERO);
    assert_eq!(
        devices[0].readings[0].value,
        Value::Unavailable(Unavailable::NotSupported)
    );
}

#[test]
fn no_modules_means_no_device() {
    let root = TempDir::new().unwrap();
    assert_eq!(Spd::new(root.path()).collect(Duration::ZERO).len(), 0);
}

#[test]
fn modules_are_read_once() {
    let root = TempDir::new().unwrap();
    module(root.path(), "19-0050", "ee1004", CORSAIR);
    let mut spd = Spd::new(root.path());
    let first = spd.collect(Duration::ZERO);
    fs::remove_dir_all(root.path().join("19-0050")).unwrap();
    assert_eq!(spd.collect(Duration::ZERO), first);
}
