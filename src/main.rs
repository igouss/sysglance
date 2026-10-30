mod bench;
mod layout;
mod omarchy;
mod sampler;
mod sources;
mod spark;
mod view;

use std::{env, path::Path, process, time::Duration};

use sources::{
    Source,
    cpu::Cpu,
    helper::{self, Helper},
    hwmon::Hwmon,
    memory::Memory,
    powercap::Powercap,
    pressure::Pressure,
    spd::Spd,
};

fn main() {
    if env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("sysglance: needs a Wayland session, and WAYLAND_DISPLAY is not set");
        process::exit(1);
    }
    let sources: Vec<Box<dyn Source>> = vec![
        Box::new(Cpu::new("/proc", "/sys")),
        Box::new(Powercap::new("/sys/class/powercap")),
        Box::new(Memory::new("/proc", "/sys")),
        Box::new(Hwmon::new("/sys/class/hwmon")),
        Box::new(Pressure::new("/proc/pressure")),
        Box::new(Spd::new("/sys/bus/i2c/devices")),
        Box::new(Helper::spawn(Path::new(helper::INSTALLED), "/proc", "/sys")),
    ];
    let bench = env::args().any(|argument| argument == "--bench-scroll");
    view::run(sampler::spawn(sources, Duration::from_secs(1)), bench);
}
