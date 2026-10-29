#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    low: Option<f64>,
    high: Option<f64>,
    critical: Option<f64>,
}

fn set(limit: Option<f64>) -> Option<f64> {
    limit.filter(|value| value.is_finite() && *value != 0.0)
}

impl Limits {
    pub const NONE: Self = Self {
        low: None,
        high: None,
        critical: None,
    };

    pub fn new(low: Option<f64>, high: Option<f64>, critical: Option<f64>) -> Self {
        Self {
            low: set(low),
            high: set(high),
            critical: set(critical),
        }
    }

    pub const fn low(self) -> Option<f64> {
        self.low
    }

    pub const fn high(self) -> Option<f64> {
        self.high
    }

    pub const fn critical(self) -> Option<f64> {
        self.critical
    }

    pub const fn is_rated(self) -> bool {
        self.low.is_some() || self.high.is_some() || self.critical.is_some()
    }

    pub fn trusted(self, alarms: Alarms) -> bool {
        let guarded =
            |flag: Option<bool>, limit: Option<f64>| flag == Some(true) && limit.is_some();
        guarded(alarms.low, self.low)
            || guarded(alarms.high, self.high)
            || guarded(alarms.critical, self.critical)
            || (alarms.any == Some(true) && self.is_rated())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Alarms {
    pub low: Option<bool>,
    pub high: Option<bool>,
    pub critical: Option<bool>,
    pub any: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Unrated,
    Normal,
    Low,
    High,
    Critical,
}

pub fn level(value: f64, limits: Limits, alarms: Alarms) -> Level {
    if limits.critical.is_some_and(|critical| value >= critical) {
        return Level::Critical;
    }
    if limits.high.is_some_and(|high| value >= high) {
        return Level::High;
    }
    if limits.low.is_some_and(|low| value <= low) {
        return Level::Low;
    }
    if limits.trusted(alarms) {
        return Level::High;
    }
    if limits.is_rated() {
        Level::Normal
    } else {
        Level::Unrated
    }
}

#[cfg(test)]
mod tests {
    use super::{Alarms, Level, Limits, level};

    const CPU: Limits = Limits {
        low: None,
        high: Some(82.0),
        critical: Some(100.0),
    };

    const TRIPPED: Alarms = Alarms {
        low: Some(true),
        high: Some(true),
        critical: Some(true),
        any: Some(true),
    };

    #[test]
    fn zero_limits_are_unset() {
        assert_eq!(Limits::new(Some(0.0), Some(0.0), Some(0.0)), Limits::NONE);
    }

    #[test]
    fn non_finite_limits_are_unset() {
        assert_eq!(
            Limits::new(Some(f64::NAN), Some(f64::INFINITY), None),
            Limits::NONE
        );
    }

    #[test]
    fn non_zero_limits_are_kept() {
        assert_eq!(
            Limits::new(Some(-5.0), Some(80.0), Some(85.0)).high(),
            Some(80.0)
        );
    }

    #[test]
    fn a_reading_without_limits_is_unrated() {
        assert_eq!(level(59.0, Limits::NONE, Alarms::default()), Level::Unrated);
    }

    #[test]
    fn an_alarm_on_an_unset_limit_is_not_trusted() {
        assert_eq!(
            level(59.0, Limits::new(Some(0.0), Some(0.0), Some(0.0)), TRIPPED),
            Level::Unrated
        );
    }

    #[test]
    fn an_alarm_on_a_set_limit_is_trusted() {
        let alarms = Alarms {
            any: Some(true),
            ..Alarms::default()
        };
        assert_eq!(
            level(1.1, Limits::new(None, Some(1.744), None), alarms),
            Level::High
        );
    }

    #[test]
    fn a_high_alarm_needs_the_high_limit() {
        let alarms = Alarms {
            high: Some(true),
            ..Alarms::default()
        };
        assert!(!Limits::new(Some(1.0), None, None).trusted(alarms));
    }

    #[test]
    fn a_low_alarm_on_a_set_low_limit_is_trusted() {
        let alarms = Alarms {
            low: Some(true),
            ..Alarms::default()
        };
        assert!(Limits::new(Some(1.0), None, None).trusted(alarms));
    }

    #[test]
    fn a_critical_alarm_on_a_set_critical_limit_is_trusted() {
        let alarms = Alarms {
            critical: Some(true),
            ..Alarms::default()
        };
        assert!(Limits::new(None, None, Some(1.0)).trusted(alarms));
    }

    #[test]
    fn below_high_is_normal() {
        assert_eq!(level(81.9, CPU, Alarms::default()), Level::Normal);
    }

    #[test]
    fn at_high_is_high() {
        assert_eq!(level(82.0, CPU, Alarms::default()), Level::High);
    }

    #[test]
    fn at_critical_is_critical() {
        assert_eq!(level(100.0, CPU, Alarms::default()), Level::Critical);
    }

    #[test]
    fn at_low_is_low() {
        assert_eq!(
            level(
                300.0,
                Limits::new(Some(300.0), None, None),
                Alarms::default()
            ),
            Level::Low
        );
    }

    #[test]
    fn above_low_is_normal() {
        assert_eq!(
            level(
                301.0,
                Limits::new(Some(300.0), None, None),
                Alarms::default()
            ),
            Level::Normal
        );
    }

    #[test]
    fn levels_order_by_severity() {
        assert!(
            Level::Critical > Level::High
                && Level::High > Level::Low
                && Level::Normal > Level::Unrated
        );
    }
}
