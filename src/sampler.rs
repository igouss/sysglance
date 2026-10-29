use std::{
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use sysglance_core::reading::{Device, arrange};

use crate::sources::Source;

#[derive(Debug, Default)]
pub struct Published {
    pub generation: u64,
    pub devices: Vec<Device>,
    pub took: Duration,
}

pub type Shared = Arc<Mutex<Published>>;

pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Published> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn spawn(mut sources: Vec<Box<dyn Source>>, interval: Duration) -> Shared {
    let shared = Shared::default();
    let producer = Arc::clone(&shared);
    thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            let started = Instant::now();
            let elapsed = started.duration_since(last);
            last = started;
            let mut devices: Vec<Device> = sources
                .iter_mut()
                .flat_map(|source| source.collect(elapsed))
                .collect();
            arrange(&mut devices);
            let took = started.elapsed();
            {
                let mut published = lock(&producer);
                published.generation += 1;
                published.devices = devices;
                published.took = took;
            }
            thread::sleep(interval.saturating_sub(started.elapsed()));
        }
    });
    shared
}
