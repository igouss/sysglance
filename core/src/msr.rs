pub const TSC: u32 = 0x10;
pub const SMI_COUNT: u32 = 0x34;
pub const PLATFORM_INFO: u32 = 0xCE;
pub const PKG_CST_CONFIG_CONTROL: u32 = 0xE2;
pub const PERF_STATUS: u32 = 0x198;
pub const TEMPERATURE_TARGET: u32 = 0x1A2;
pub const TURBO_RATIO_LIMIT: u32 = 0x1AD;
pub const PACKAGE_THERM_STATUS: u32 = 0x1B1;
pub const CORE_PERF_LIMIT_REASONS: u32 = 0x64F;

pub const PACKAGE_RESIDENCY: [(u32, &str); 7] = [
    (0x60D, "PC2"),
    (0x3F8, "PC3"),
    (0x3F9, "PC6"),
    (0x3FA, "PC7"),
    (0x630, "PC8"),
    (0x631, "PC9"),
    (0x632, "PC10"),
];

pub const CORE_RESIDENCY: [(u32, &str); 3] = [(0x3FC, "C3"), (0x3FD, "C6"), (0x3FE, "C7")];

pub const PACKAGE: [u32; 15] = [
    SMI_COUNT,
    PLATFORM_INFO,
    PKG_CST_CONFIG_CONTROL,
    PERF_STATUS,
    TEMPERATURE_TARGET,
    TURBO_RATIO_LIMIT,
    PACKAGE_THERM_STATUS,
    CORE_PERF_LIMIT_REASONS,
    PACKAGE_RESIDENCY[0].0,
    PACKAGE_RESIDENCY[1].0,
    PACKAGE_RESIDENCY[2].0,
    PACKAGE_RESIDENCY[3].0,
    PACKAGE_RESIDENCY[4].0,
    PACKAGE_RESIDENCY[5].0,
    PACKAGE_RESIDENCY[6].0,
];

pub const PER_CPU: [u32; 4] = [
    TSC,
    CORE_RESIDENCY[0].0,
    CORE_RESIDENCY[1].0,
    CORE_RESIDENCY[2].0,
];

pub const SKYLAKE_CLIENT_MODELS: [u32; 6] = [0x4E, 0x5E, 0x8E, 0x9E, 0xA5, 0xA6];

pub const LIMIT_REASONS: [(u32, &str); 11] = [
    (0, "PROCHOT"),
    (1, "thermal"),
    (4, "residency regulation"),
    (5, "average thermal"),
    (6, "VR thermal"),
    (7, "VR current"),
    (8, "electrical"),
    (10, "PL1"),
    (11, "PL2"),
    (12, "max turbo"),
    (13, "transitions"),
];

pub const LIMIT_REASONS_LOGGED: u32 = 16;

pub const THERMAL_FLAGS: [(u32, &str); 6] = [
    (0, "thermal"),
    (2, "PROCHOT"),
    (4, "critical"),
    (10, "power limit"),
    (12, "current limit"),
    (14, "cross-domain"),
];

pub const THERMAL_FLAGS_LOGGED: u32 = 1;

const PACKAGE_CSTATE_LIMITS: [&str; 9] = [
    "PC0",
    "PC2",
    "PC3",
    "PC6",
    "PC7",
    "PC7s",
    "PC8",
    "PC9",
    "unlimited",
];

const fn field(raw: u64, low: u32, width: u32) -> u64 {
    (raw >> low) & ((1 << width) - 1)
}

const BUS_HZ: f64 = 100e6;

#[expect(
    clippy::cast_precision_loss,
    reason = "an 8-bit ratio is exact in an f64"
)]
pub const fn ratio_hz(ratio: u64) -> f64 {
    ratio as f64 * BUS_HZ
}

pub const fn base_ratio(platform_info: u64) -> u64 {
    field(platform_info, 8, 8)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a 16-bit voltage code is exact in an f64"
)]
pub const fn core_volts(perf_status: u64) -> f64 {
    field(perf_status, 32, 16) as f64 / 8192.0
}

pub const fn tj_max(temperature_target: u64) -> u64 {
    field(temperature_target, 16, 8)
}

pub const fn tcc_offset(temperature_target: u64) -> u64 {
    field(temperature_target, 24, 6)
}

pub fn turbo_ratios(turbo_ratio_limit: u64) -> Vec<u64> {
    (0..8)
        .map(|index| field(turbo_ratio_limit, index * 8, 8))
        .take_while(|ratio| *ratio != 0)
        .collect()
}

pub fn package_cstate_limit(config: u64) -> Option<&'static str> {
    usize::try_from(field(config, 0, 4))
        .ok()
        .and_then(|code| PACKAGE_CSTATE_LIMITS.get(code).copied())
}

pub fn flags(raw: u64, table: &[(u32, &'static str)], shift: u32) -> Vec<&'static str> {
    table
        .iter()
        .filter(|(bit, _)| field(raw, bit + shift, 1) == 1)
        .map(|(_, name)| *name)
        .collect()
}

pub fn residency(counter: (u64, u64), tsc: (u64, u64)) -> Option<f64> {
    let elapsed = tsc.1.checked_sub(tsc.0).filter(|ticks| *ticks > 0)?;
    let resident = counter.1.checked_sub(counter.0)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "tick deltas between samples are far below 2^52"
    )]
    let fraction = resident as f64 / elapsed as f64;
    Some(fraction.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::{
        LIMIT_REASONS, LIMIT_REASONS_LOGGED, THERMAL_FLAGS, THERMAL_FLAGS_LOGGED, base_ratio,
        core_volts, flags, package_cstate_limit, ratio_hz, residency, tcc_offset, tj_max,
        turbo_ratios,
    };

    #[test]
    fn the_base_ratio_is_bits_15_to_8_of_platform_info() {
        assert_eq!(base_ratio(0x0808_083a_f101_2600), 38);
    }

    #[test]
    fn a_ratio_is_a_multiple_of_the_100_mhz_bus() {
        assert!((ratio_hz(38) - 3.8e9).abs() < 1.0);
    }

    #[test]
    fn core_voltage_is_bits_47_to_32_in_units_of_1_8192_volt() {
        assert!((core_volts(0x0000_284a_0000_2f00) - 10314.0 / 8192.0).abs() < 1e-9);
    }

    #[test]
    fn tj_max_and_its_offset_come_from_the_temperature_target() {
        assert_eq!(tj_max(0x0064_1200), 100);
        assert_eq!(tcc_offset(0x0064_1200), 0);
        assert_eq!(tcc_offset(0x0564_1200), 5);
        assert_eq!(tj_max(0x3f64_1200), 100);
    }

    #[test]
    fn turbo_ratios_list_one_active_core_first() {
        assert_eq!(
            turbo_ratios(0x2f2f_2f30_3033_3333),
            vec![51, 51, 51, 48, 48, 47, 47, 47]
        );
    }

    #[test]
    fn turbo_ratios_stop_at_the_first_unset_byte() {
        assert_eq!(turbo_ratios(0x0000_0000_0000_2d2f), vec![47, 45]);
        assert_eq!(turbo_ratios(0), Vec::<u64>::new());
    }

    #[test]
    fn the_package_cstate_limit_decodes_the_low_nibble() {
        assert_eq!(package_cstate_limit(0x7400_0000), Some("PC0"));
        assert_eq!(package_cstate_limit(0x7400_0003), Some("PC6"));
        assert_eq!(package_cstate_limit(0x7400_0008), Some("unlimited"));
        assert_eq!(package_cstate_limit(0x7400_0009), None);
    }

    #[test]
    fn limit_reasons_separate_status_from_the_log() {
        let raw: u64 = 0x3000_1000;
        assert_eq!(flags(raw, &LIMIT_REASONS, 0), vec!["max turbo"]);
        assert_eq!(
            flags(raw, &LIMIT_REASONS, LIMIT_REASONS_LOGGED),
            vec!["max turbo", "transitions"]
        );
    }

    #[test]
    fn no_flag_set_names_nothing() {
        assert_eq!(flags(0, &LIMIT_REASONS, 0), Vec::<&str>::new());
    }

    #[test]
    fn thermal_logs_sit_one_bit_above_their_status() {
        let raw: u64 = 0x881f_0280;
        assert_eq!(flags(raw, &THERMAL_FLAGS, 0), Vec::<&str>::new());
        assert_eq!(
            flags(raw, &THERMAL_FLAGS, THERMAL_FLAGS_LOGGED),
            Vec::<&str>::new()
        );
        assert_eq!(
            flags(0x0000_0c05, &THERMAL_FLAGS, 0),
            vec!["thermal", "PROCHOT", "power limit"]
        );
        assert_eq!(
            flags(0x0000_0c05, &THERMAL_FLAGS, THERMAL_FLAGS_LOGGED),
            vec!["power limit"]
        );
    }

    #[test]
    fn residency_is_the_counter_share_of_elapsed_ticks() {
        assert_eq!(residency((100, 400), (1000, 2000)), Some(0.3));
    }

    #[test]
    fn residency_is_capped_at_whole() {
        assert_eq!(residency((0, 3000), (1000, 2000)), Some(1.0));
    }

    #[test]
    fn residency_needs_ticks_to_pass() {
        assert_eq!(residency((0, 10), (1000, 1000)), None);
        assert_eq!(residency((0, 10), (2000, 1000)), None);
    }

    #[test]
    fn a_counter_that_went_backwards_has_no_residency() {
        assert_eq!(residency((10, 0), (1000, 2000)), None);
    }
}
