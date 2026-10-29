mod sampler;
mod sources;
mod view;

use std::time::Duration;

use sources::{Source, hwmon::Hwmon};

fn main() {
    let sources: Vec<Box<dyn Source>> = vec![Box::new(Hwmon::new("/sys/class/hwmon"))];
    view::run(sampler::spawn(sources, Duration::from_secs(1)));
}
