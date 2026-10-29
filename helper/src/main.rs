use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    os::unix::fs::FileExt,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use sysglance_core::msr::{PACKAGE, PER_CPU};

const INTERVAL: Duration = Duration::from_secs(1);
const SMART_EVERY: u32 = 30;
const SMARTCTL: &str = "/usr/bin/smartctl";

fn cpus(root: &Path) -> Vec<(u32, File)> {
    let mut cpus: Vec<(u32, File)> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|cpu| {
            let file = File::open(root.join(cpu.to_string()).join("msr")).ok()?;
            Some((cpu, file))
        })
        .collect();
    cpus.sort_by_key(|(cpu, _)| *cpu);
    cpus
}

fn read(file: &File, address: u32) -> Option<u64> {
    let mut bytes = [0; 8];
    file.read_exact_at(&mut bytes, u64::from(address)).ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn registers(out: &mut impl Write, cpus: &[(u32, File)]) -> io::Result<()> {
    for (index, (cpu, file)) in cpus.iter().enumerate() {
        let package: &[u32] = if index == 0 { &PACKAGE } else { &[] };
        for address in PER_CPU.iter().chain(package) {
            if let Some(value) = read(file, *address) {
                writeln!(out, "msr {cpu} {address:#x} {value:#x}")?;
            }
        }
    }
    Ok(())
}

fn scanned(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|drive| drive.starts_with("/dev/"))
        .collect()
}

fn health_line(drive: &str, stdout: &str) -> Option<String> {
    let json = stdout.trim();
    (!json.is_empty() && !json.contains('\n')).then(|| format!("smart {drive} {json}"))
}

fn health(out: &mut impl Write) -> io::Result<()> {
    let Ok(scan) = Command::new(SMARTCTL).arg("--scan").output() else {
        return Ok(());
    };
    for drive in scanned(&String::from_utf8_lossy(&scan.stdout)) {
        let Ok(report) = Command::new(SMARTCTL)
            .args(["--json=c", "-a", "-n", "standby", drive])
            .output()
        else {
            continue;
        };
        if let Some(line) = health_line(drive, &String::from_utf8_lossy(&report.stdout)) {
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let cpus = cpus(Path::new("/dev/cpu"));
    let mut out = BufWriter::new(io::stdout().lock());
    for tick in (0..SMART_EVERY).cycle() {
        let started = Instant::now();
        registers(&mut out, &cpus)?;
        if tick == 0 {
            health(&mut out)?;
        }
        writeln!(out, "end")?;
        out.flush()?;
        thread::sleep(INTERVAL.saturating_sub(started.elapsed()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::FileExt};

    use sysglance_core::msr::{PACKAGE, PER_CPU, SMI_COUNT, TSC};

    use super::{cpus, health_line, registers, scanned};

    fn fake_msr(root: &std::path::Path, cpu: u32, values: &[(u32, u64)]) {
        let dir = root.join(cpu.to_string());
        fs::create_dir_all(&dir).expect("cpu dir");
        let file = fs::File::create(dir.join("msr")).expect("msr file");
        file.set_len(0x700).expect("sparse msr file");
        for (address, value) in values {
            file.write_all_at(&value.to_le_bytes(), u64::from(*address))
                .expect("register");
        }
    }

    fn streamed(root: &std::path::Path) -> String {
        let mut out = Vec::new();
        registers(&mut out, &cpus(root)).expect("written");
        String::from_utf8(out).expect("utf-8")
    }

    #[test]
    fn no_cpus_stream_no_registers() {
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(streamed(root.path()), "");
    }

    #[test]
    fn the_first_cpu_streams_package_and_per_cpu_registers() {
        let root = tempfile::tempdir().expect("tempdir");
        fake_msr(root.path(), 0, &[(TSC, 0xabc), (SMI_COUNT, 0xf6)]);
        let lines = streamed(root.path());
        assert_eq!(lines.lines().count(), PER_CPU.len() + PACKAGE.len());
        assert!(lines.contains("msr 0 0x10 0xabc\n"));
        assert!(lines.contains("msr 0 0x34 0xf6\n"));
    }

    #[test]
    fn other_cpus_stream_only_per_cpu_registers_in_cpu_order() {
        let root = tempfile::tempdir().expect("tempdir");
        for cpu in [10, 0, 2] {
            fake_msr(root.path(), cpu, &[(TSC, u64::from(cpu))]);
        }
        fs::create_dir_all(root.path().join("microcode")).expect("non-cpu entry");
        let lines = streamed(root.path());
        assert_eq!(lines.lines().count(), PER_CPU.len() * 3 + PACKAGE.len());
        let tscs: Vec<&str> = lines.lines().filter(|l| l.contains(" 0x10 ")).collect();
        assert_eq!(
            tscs,
            vec!["msr 0 0x10 0x0", "msr 2 0x10 0x2", "msr 10 0x10 0xa"]
        );
    }

    #[test]
    fn a_register_that_cannot_be_read_is_left_out() {
        let root = tempfile::tempdir().expect("tempdir");
        fake_msr(root.path(), 0, &[]);
        let file = fs::OpenOptions::new()
            .write(true)
            .open(root.path().join("0/msr"))
            .expect("msr file");
        file.set_len(0x20).expect("truncated");
        assert_eq!(streamed(root.path()), "msr 0 0x10 0x0\n");
    }

    #[test]
    fn a_scan_lists_device_paths_and_skips_the_rest() {
        let scan = "/dev/sda -d scsi # /dev/sda, SCSI device\n# comment\n\n/dev/nvme0 -d nvme # /dev/nvme0, NVMe device\n";
        assert_eq!(scanned(scan), vec!["/dev/sda", "/dev/nvme0"]);
        assert_eq!(scanned(""), Vec::<&str>::new());
    }

    #[test]
    fn a_compact_report_becomes_one_line() {
        assert_eq!(
            health_line("/dev/sda", "{\"a\":1}\n"),
            Some("smart /dev/sda {\"a\":1}".to_owned())
        );
    }

    #[test]
    fn an_empty_or_multi_line_report_is_dropped() {
        assert_eq!(health_line("/dev/sda", "  \n"), None);
        assert_eq!(health_line("/dev/sda", "{\n\"a\":1}"), None);
    }
}
