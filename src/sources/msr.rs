use std::{path::Path, time::Duration};

use sysglance_core::{
    counter::{Wrap, per_second},
    measure::Measure,
    msr::{
        CORE_PERF_LIMIT_REASONS, CORE_RESIDENCY, LIMIT_REASONS, LIMIT_REASONS_LOGGED,
        PACKAGE_RESIDENCY, PACKAGE_THERM_STATUS, PERF_STATUS, PKG_CST_CONFIG_CONTROL,
        PLATFORM_INFO, SKYLAKE_CLIENT_MODELS, SMI_COUNT, TEMPERATURE_TARGET, THERMAL_FLAGS,
        THERMAL_FLAGS_LOGGED, TSC, TURBO_RATIO_LIMIT, base_ratio, core_volts, flags,
        package_cstate_limit, ratio_hz, residency, tcc_offset, tj_max, turbo_ratios,
    },
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::{cores, helper::Registers};

const PACKAGE_CPU: u32 = 0;

type Residency = (u32, &'static str);

pub fn decodable(cpuinfo: &str) -> bool {
    let field = |name: &str| {
        cpuinfo.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim())
        })
    };
    field("vendor_id") == Some("GenuineIntel")
        && field("cpu family") == Some("6")
        && field("model")
            .and_then(|model| model.parse::<u32>().ok())
            .is_some_and(|model| SKYLAKE_CLIENT_MODELS.contains(&model))
}

fn package_device(readings: Vec<Reading>) -> Device {
    Device {
        key: "msr:package".to_owned(),
        title: "CPU package registers".to_owned(),
        detail: "MSRs · sysglance-helper".to_owned(),
        category: Category::Processor,
        readings,
    }
}

fn unavailable(why: Unavailable) -> Device {
    package_device(vec![Reading {
        key: "msr:package/registers".to_owned(),
        label: "registers".to_owned(),
        value: Value::Unavailable(why),
        origin: "/dev/cpu/<n>/msr".to_owned(),
    }])
}

pub fn undecodable() -> Device {
    unavailable(Unavailable::NotSupported)
}

fn named(names: &[&str]) -> Value {
    Value::Text(if names.is_empty() {
        "nothing".to_owned()
    } else {
        names.join(", ")
    })
}

fn ladder(ratios: &[u64]) -> Value {
    let mut runs: Vec<String> = Vec::new();
    let mut first: usize = 1;
    for run in ratios.chunk_by(|a, b| a == b) {
        let last = first + run.len() - 1;
        let cores = if first == last {
            first.to_string()
        } else {
            format!("{first}-{last}")
        };
        runs.push(format!("{cores}:{:.1}", ratio_hz(run[0]) / 1e9));
        first = last + 1;
    }
    Value::Text(if runs.is_empty() {
        "none".to_owned()
    } else {
        format!("{} GHz", runs.join(" "))
    })
}

fn pair(previous: Option<&Registers>, latest: &Registers, at: (u32, u32)) -> Option<(u64, u64)> {
    Some((*previous?.get(&at)?, *latest.get(&at)?))
}

fn share(previous: Option<&Registers>, latest: &Registers, cpu: u32, address: u32) -> Value {
    pair(previous, latest, (cpu, address))
        .zip(pair(previous, latest, (cpu, TSC)))
        .and_then(|(counter, tsc)| residency(counter, tsc))
        .map_or(Value::Unavailable(Unavailable::Pending), |fraction| {
            Value::plain(Measure::Fraction(fraction))
        })
}

fn smi_rate(previous: Option<&Registers>, latest: &Registers) -> Value {
    let rate = || {
        let (before, after) = pair(previous, latest, (PACKAGE_CPU, SMI_COUNT))?;
        let (start, end) = pair(previous, latest, (PACKAGE_CPU, TSC))?;
        let hz = ratio_hz(base_ratio(*latest.get(&(PACKAGE_CPU, PLATFORM_INFO))?));
        #[expect(
            clippy::cast_precision_loss,
            reason = "tick deltas between samples are far below 2^52"
        )]
        let seconds = end.checked_sub(start)? as f64 / hz;
        per_second(
            before,
            after,
            Wrap::Never,
            Duration::try_from_secs_f64(seconds).ok()?,
        )
    };
    rate().map_or(Value::Unavailable(Unavailable::Pending), |rate| {
        Value::plain(Measure::PerSecond(rate))
    })
}

fn package(previous: Option<&Registers>, latest: &Registers) -> Device {
    let mut readings: Vec<Reading> = Vec::new();
    let mut push = |address: u32, key: &str, label: &str, value: Value| {
        readings.push(Reading {
            key: format!("msr:package/{key}"),
            label: label.to_owned(),
            value,
            origin: format!("MSR {address:#X} on cpu{PACKAGE_CPU}"),
        });
    };
    let at = |address: u32| latest.get(&(PACKAGE_CPU, address)).copied();
    if let Some(raw) = at(CORE_PERF_LIMIT_REASONS) {
        let address = CORE_PERF_LIMIT_REASONS;
        let now = named(&flags(raw, &LIMIT_REASONS, 0));
        let logged = named(&flags(raw, &LIMIT_REASONS, LIMIT_REASONS_LOGGED));
        push(address, "limits/now", "limited by", now);
        push(address, "limits/logged", "limits since boot", logged);
    }
    if let Some(raw) = at(PACKAGE_THERM_STATUS) {
        let address = PACKAGE_THERM_STATUS;
        let now = named(&flags(raw, &THERMAL_FLAGS, 0));
        let logged = named(&flags(raw, &THERMAL_FLAGS, THERMAL_FLAGS_LOGGED));
        push(address, "thermal/now", "thermal events", now);
        push(address, "thermal/logged", "thermal since boot", logged);
    }
    if let Some(raw) = at(PERF_STATUS) {
        let volts = Value::plain(Measure::Volts(core_volts(raw)));
        push(PERF_STATUS, "vid", "core voltage (VID)", volts);
    }
    if let Some(raw) = at(TEMPERATURE_TARGET) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "an 8-bit temperature is exact in an f64"
        )]
        let celsius = |degrees: u64| Value::plain(Measure::Celsius(degrees as f64));
        push(TEMPERATURE_TARGET, "tjmax", "TjMax", celsius(tj_max(raw)));
        push(
            TEMPERATURE_TARGET,
            "tcc-offset",
            "TCC offset",
            celsius(tcc_offset(raw)),
        );
    }
    if let Some(raw) = at(TURBO_RATIO_LIMIT) {
        let ladder = ladder(&turbo_ratios(raw));
        push(TURBO_RATIO_LIMIT, "turbo", "turbo by cores", ladder);
    }
    if at(SMI_COUNT).is_some() {
        push(SMI_COUNT, "smi", "SMIs", smi_rate(previous, latest));
    }
    if let Some(raw) = at(PKG_CST_CONFIG_CONTROL) {
        let limit = Value::Text(package_cstate_limit(raw).unwrap_or("reserved").to_owned());
        push(
            PKG_CST_CONFIG_CONTROL,
            "cstate-limit",
            "package C-state limit",
            limit,
        );
    }
    for (address, name) in PACKAGE_RESIDENCY {
        if at(address).is_some_and(|counter| counter != 0) {
            let value = share(previous, latest, PACKAGE_CPU, address);
            push(address, name, &format!("{name} residency"), value);
        }
    }
    package_device(readings)
}

fn core_states(sys: &Path, previous: Option<&Registers>, latest: &Registers) -> Option<Device> {
    let cores = cores(sys);
    let (entered, never): (Vec<Residency>, Vec<Residency>) =
        CORE_RESIDENCY.into_iter().partition(|(address, _)| {
            cores.iter().any(|core| {
                latest
                    .get(&(core.cpu, *address))
                    .is_some_and(|counter| *counter != 0)
            })
        });
    if entered.is_empty() {
        return None;
    }
    let readings: Vec<Reading> = cores
        .iter()
        .flat_map(|core| {
            entered.iter().map(move |(address, name)| Reading {
                key: format!("msr:cores/core{}/{name}", core.id),
                label: format!("core {} {name}", core.id),
                value: share(previous, latest, core.cpu, *address),
                origin: format!("MSR {address:#X} on cpu{}", core.cpu),
            })
        })
        .collect();
    let unused: Vec<&str> = never.iter().map(|(_, name)| *name).collect();
    let detail = if unused.is_empty() {
        "hardware residency".to_owned()
    } else {
        format!("hardware residency · never entered: {}", unused.join(", "))
    };
    Some(Device {
        key: "msr:cores".to_owned(),
        title: "CPU core C-states".to_owned(),
        detail,
        category: Category::Processor,
        readings,
    })
}

pub fn devices(sys: &Path, previous: Option<&Registers>, latest: &Registers) -> Vec<Device> {
    if latest.is_empty() {
        return vec![unavailable(Unavailable::ModuleNotLoaded("msr"))];
    }
    let mut devices = vec![package(previous, latest)];
    devices.extend(core_states(sys, previous, latest));
    devices
}

#[cfg(test)]
mod tests;
