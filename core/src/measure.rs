use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measure {
    Celsius(f64),
    Volts(f64),
    Amps(f64),
    Watts(f64),
    Joules(f64),
    Rpm(f64),
    Hertz(f64),
    Fraction(f64),
    Bytes(f64),
    BytesPerSecond(f64),
    PerSecond(f64),
    Count(u64),
    Scalar(f64),
    Seconds(f64),
    Dbm(f64),
    Tripped(bool),
}

impl Measure {
    pub const fn magnitude(self) -> f64 {
        match self {
            Self::Celsius(v)
            | Self::Volts(v)
            | Self::Amps(v)
            | Self::Watts(v)
            | Self::Joules(v)
            | Self::Rpm(v)
            | Self::Hertz(v)
            | Self::Fraction(v)
            | Self::Bytes(v)
            | Self::BytesPerSecond(v)
            | Self::PerSecond(v)
            | Self::Scalar(v)
            | Self::Seconds(v)
            | Self::Dbm(v) => v,
            #[expect(
                clippy::cast_precision_loss,
                reason = "a magnitude only feeds a sparkline"
            )]
            Self::Count(n) => n as f64,
            Self::Tripped(on) => {
                if on {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

const BINARY: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
const DECIMAL: [&str; 5] = ["", "k", "M", "G", "T"];

fn binary(value: f64, suffix: &str) -> String {
    let mut scaled = value;
    let mut unit = 0;
    while scaled.abs() >= 1024.0 && unit + 1 < BINARY.len() {
        scaled /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{scaled:.0} {}{suffix}", BINARY[unit])
    } else {
        format!("{scaled:.1} {}{suffix}", BINARY[unit])
    }
}

fn decimal(value: f64, unit: &str, places: usize) -> String {
    let mut scaled = value;
    let mut prefix = 0;
    while scaled.abs() >= 1000.0 && prefix + 1 < DECIMAL.len() {
        scaled /= 1000.0;
        prefix += 1;
    }
    let places = if prefix == 0 { places } else { places.max(2) };
    format!("{scaled:.places$} {}{unit}", DECIMAL[prefix])
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Celsius(v) => write!(f, "{v:.1} °C"),
            Self::Volts(v) => write!(f, "{v:.3} V"),
            Self::Amps(v) => write!(f, "{v:.2} A"),
            Self::Watts(v) => write!(f, "{v:.1} W"),
            Self::Joules(v) => f.write_str(&decimal(v, "J", 0)),
            Self::Rpm(v) => write!(f, "{v:.0} rpm"),
            Self::Hertz(v) => f.write_str(&decimal(v, "Hz", 0)),
            Self::Fraction(v) => write!(f, "{:.1} %", v * 100.0),
            Self::Bytes(v) => f.write_str(&binary(v, "")),
            Self::BytesPerSecond(v) => f.write_str(&binary(v, "/s")),
            Self::PerSecond(v) => f.write_str(&decimal(v, "/s", 0)),
            Self::Count(n) => write!(f, "{n}"),
            Self::Scalar(v) => write!(f, "{v:.2}"),
            Self::Seconds(v) => f.write_str(&duration(v)),
            Self::Dbm(v) => write!(f, "{v:.0} dBm"),
            Self::Tripped(true) => f.write_str("TRIPPED"),
            Self::Tripped(false) => f.write_str("ok"),
        }
    }
}

fn duration(seconds: f64) -> String {
    if seconds < 1.0 {
        return format!("{:.1} ms", seconds * 1000.0);
    }
    if seconds < 120.0 {
        return format!("{seconds:.1} s");
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "seconds is positive and far below u64::MAX here"
    )]
    let whole = seconds as u64;
    let (days, rest) = (whole / 86_400, whole % 86_400);
    let (hours, rest) = (rest / 3600, rest % 3600);
    let minutes = rest / 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::Measure;

    #[test]
    fn celsius_shows_one_place() {
        assert_eq!(Measure::Celsius(89.04).to_string(), "89.0 °C");
    }

    #[test]
    fn bytes_below_a_kibibyte_show_whole() {
        assert_eq!(Measure::Bytes(0.0).to_string(), "0 B");
    }

    #[test]
    fn bytes_scale_to_the_largest_whole_unit() {
        assert_eq!(Measure::Bytes(1536.0).to_string(), "1.5 KiB");
    }

    #[test]
    fn many_bytes_scale_through_several_units() {
        assert_eq!(
            Measure::Bytes(31.0 * 1024.0 * 1024.0 * 1024.0).to_string(),
            "31.0 GiB"
        );
    }

    #[test]
    fn bytes_past_the_largest_unit_stay_in_it() {
        assert_eq!(Measure::Bytes(1024.0_f64.powi(6)).to_string(), "1024.0 PiB");
    }

    #[test]
    fn hertz_past_the_largest_prefix_stay_in_it() {
        assert_eq!(Measure::Hertz(1e15).to_string(), "1000.00 THz");
    }

    #[test]
    fn rates_carry_a_per_second_suffix() {
        assert_eq!(Measure::BytesPerSecond(2048.0).to_string(), "2.0 KiB/s");
    }

    #[test]
    fn hertz_scale_by_thousands() {
        assert_eq!(Measure::Hertz(4_700_000_000.0).to_string(), "4.70 GHz");
    }

    #[test]
    fn hertz_below_a_kilohertz_show_whole() {
        assert_eq!(Measure::Hertz(999.0).to_string(), "999 Hz");
    }

    #[test]
    fn fraction_shows_as_percent() {
        assert_eq!(Measure::Fraction(0.392).to_string(), "39.2 %");
    }

    #[test]
    fn scalars_show_two_places() {
        assert_eq!(Measure::Scalar(7.567).to_string(), "7.57");
    }

    #[test]
    fn short_durations_show_milliseconds() {
        assert_eq!(Measure::Seconds(0.25).to_string(), "250.0 ms");
    }

    #[test]
    fn one_second_shows_seconds() {
        assert_eq!(Measure::Seconds(1.0).to_string(), "1.0 s");
    }

    #[test]
    fn a_minute_shows_seconds() {
        assert_eq!(Measure::Seconds(60.0).to_string(), "60.0 s");
    }

    #[test]
    fn two_minutes_show_as_hours_and_minutes() {
        assert_eq!(Measure::Seconds(120.0).to_string(), "0h 2m");
    }

    #[test]
    fn minutes_show_as_hours_and_minutes() {
        assert_eq!(Measure::Seconds(3_725.0).to_string(), "1h 2m");
    }

    #[test]
    fn days_lead_long_durations() {
        assert_eq!(Measure::Seconds(660_000.0).to_string(), "7d 15h 20m");
    }

    #[test]
    fn a_tripped_flag_has_magnitude_one() {
        assert!((Measure::Tripped(true).magnitude() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_clear_flag_has_magnitude_zero() {
        assert!(Measure::Tripped(false).magnitude().abs() < f64::EPSILON);
    }
}
