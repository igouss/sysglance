mod sampler;
mod sources;
mod view;

use std::time::Duration;

use sources::{Source, cpu::Cpu, hwmon::Hwmon, powercap::Powercap};

fn main() {
    let sources: Vec<Box<dyn Source>> = vec![
        Box::new(Cpu::new("/proc", "/sys")),
        Box::new(Powercap::new("/sys/class/powercap")),
        Box::new(Hwmon::new("/sys/class/hwmon")),
    ];
    view::run(sampler::spawn(sources, Duration::from_secs(1)));
}
