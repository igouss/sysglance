use crate::{
    level::{Alarms, Level, Limits, level},
    measure::Measure,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unavailable {
    Pending,
    NeedsPrivilege(&'static str),
    ModuleNotLoaded(&'static str),
    NotSupported,
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Measured {
        measure: Measure,
        limits: Limits,
        level: Level,
    },
    Text(String),
    Unavailable(Unavailable),
}

impl Value {
    pub fn measured(measure: Measure, limits: Limits, alarms: Alarms) -> Self {
        Self::Measured {
            measure,
            limits,
            level: level(measure.magnitude(), limits, alarms),
        }
    }

    pub fn plain(measure: Measure) -> Self {
        Self::measured(measure, Limits::NONE, Alarms::default())
    }

    pub const fn level(&self) -> Level {
        match self {
            Self::Measured { level, .. } => *level,
            Self::Text(_) | Self::Unavailable(_) => Level::Unrated,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub key: String,
    pub label: String,
    pub value: Value,
    pub origin: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Processor,
    Power,
    Memory,
    Graphics,
    Board,
    Cooling,
    Storage,
    Network,
    System,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub category: Category,
    pub readings: Vec<Reading>,
}

impl Device {
    pub fn worst(&self) -> Level {
        self.readings
            .iter()
            .map(|reading| reading.value.level())
            .max()
            .unwrap_or(Level::Unrated)
    }
}

pub fn arrange(devices: &mut [Device]) {
    devices.sort_by(|a, b| (a.category, &a.title, &a.key).cmp(&(b.category, &b.title, &b.key)));
}

#[cfg(test)]
mod tests {
    use super::{Category, Device, Reading, Unavailable, Value, arrange};
    use crate::{
        level::{Alarms, Level, Limits},
        measure::Measure,
    };

    fn reading(value: Value) -> Reading {
        Reading {
            key: "k".to_owned(),
            label: "l".to_owned(),
            value,
            origin: "o".to_owned(),
        }
    }

    fn device(key: &str, category: Category, readings: Vec<Reading>) -> Device {
        Device {
            key: key.to_owned(),
            title: key.to_owned(),
            detail: String::new(),
            category,
            readings,
        }
    }

    #[test]
    fn a_measured_value_is_levelled_against_its_limits() {
        let value = Value::measured(
            Measure::Celsius(90.0),
            Limits::new(None, Some(82.0), Some(100.0)),
            Alarms::default(),
        );
        assert_eq!(value.level(), Level::High);
    }

    #[test]
    fn text_is_unrated() {
        assert_eq!(Value::Text("auto".to_owned()).level(), Level::Unrated);
    }

    #[test]
    fn unavailable_is_unrated() {
        assert_eq!(
            Value::Unavailable(Unavailable::NotSupported).level(),
            Level::Unrated
        );
    }

    #[test]
    fn a_device_without_readings_is_unrated() {
        assert_eq!(device("d", Category::Board, vec![]).worst(), Level::Unrated);
    }

    #[test]
    fn a_device_is_as_bad_as_its_worst_reading() {
        let hot = Value::measured(
            Measure::Celsius(101.0),
            Limits::new(None, Some(82.0), Some(100.0)),
            Alarms::default(),
        );
        let cool = Value::measured(
            Measure::Celsius(40.0),
            Limits::new(None, Some(82.0), Some(100.0)),
            Alarms::default(),
        );
        assert_eq!(
            device("d", Category::Processor, vec![reading(cool), reading(hot)]).worst(),
            Level::Critical
        );
    }

    #[test]
    fn devices_arrange_by_category_then_title() {
        let mut devices = vec![
            device("b", Category::Storage, vec![]),
            device("z", Category::Processor, vec![]),
            device("a", Category::Storage, vec![]),
        ];
        arrange(&mut devices);
        let order: Vec<&str> = devices.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(order, ["z", "a", "b"]);
    }
}
