mod sampler;
mod sources;
mod view;

use std::time::Duration;

use sources::{
    Source, cpu::Cpu, hwmon::Hwmon, memory::Memory, powercap::Powercap, pressure::Pressure,
    spd::Spd,
};

fn main() {
    let sources: Vec<Box<dyn Source>> = vec![
        Box::new(Cpu::new("/proc", "/sys")),
        Box::new(Powercap::new("/sys/class/powercap")),
        Box::new(Memory::new("/proc", "/sys")),
        Box::new(Hwmon::new("/sys/class/hwmon")),
        Box::new(Pressure::new("/proc/pressure")),
        Box::new(Spd::new("/sys/bus/i2c/devices")),
    ];
    view::run(sampler::spawn(sources, Duration::from_secs(1)));
}
