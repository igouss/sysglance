use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    mem,
    os::unix::fs::FileExt,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use sysglance_core::msr::{PACKAGE, PER_CPU};

const INTERVAL: Duration = Duration::from_secs(1);
const SMART_INTERVAL: Duration = Duration::from_secs(30);
const SMART_TIMEOUT: Duration = Duration::from_secs(20);
const SMARTCTL: [&str; 2] = ["/usr/sbin/smartctl", "/usr/bin/smartctl"];

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

fn kind_allowed(kind: &str) -> bool {
    !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ',' | '+' | '_'))
}

fn scanned(stdout: &str) -> Vec<(&str, &str)> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let path = fields.next().filter(|path| path.starts_with("/dev/"))?;
            let kind = match (fields.next(), fields.next()) {
                (Some("-d"), Some(kind)) => kind,
                _ => "auto",
            };
            kind_allowed(kind).then_some((path, kind))
        })
        .collect()
}

fn one_line(stdout: &str) -> Option<&str> {
    let json = stdout.trim();
    (!json.is_empty() && !json.contains('\n')).then_some(json)
}

fn run(program: &str, arguments: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).map(|_| text)
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!(
                    "smartctl gave no answer within {} s",
                    timeout.as_secs()
                ));
            }
        }
    }
    reader
        .join()
        .map_err(|_| "reading smartctl failed".to_owned())?
        .map_err(|e| e.to_string())
}

fn round(smartctl: Option<&str>, timeout: Duration) -> Vec<String> {
    let Some(smartctl) = smartctl else {
        return vec![format!(
            "smart-unavailable smartctl is not installed (looked for {})",
            SMARTCTL.join(" and ")
        )];
    };
    let scan = match run(smartctl, &["--scan-open"], timeout) {
        Ok(scan) => scan,
        Err(why) => return vec![format!("smart-unavailable {why}")],
    };
    let mut lines = vec!["smart-round".to_owned()];
    for (path, kind) in scanned(&scan) {
        let report = run(
            smartctl,
            &["--json=c", "-a", "-n", "standby", "-d", kind, path],
            timeout,
        );
        lines.push(match report.as_deref().map(one_line) {
            Ok(Some(json)) => format!("smart {path} {kind} {json}"),
            Ok(None) => format!("smart-error {path} {kind} smartctl printed no report"),
            Err(why) => format!("smart-error {path} {kind} {why}"),
        });
    }
    lines
}

fn main() -> io::Result<()> {
    let cpus = cpus(Path::new("/dev/cpu"));
    let pending: Arc<Mutex<Vec<String>>> = Arc::default();
    let producer = Arc::clone(&pending);
    let smartctl = SMARTCTL.into_iter().find(|path| Path::new(path).is_file());
    thread::spawn(move || {
        loop {
            let started = Instant::now();
            let lines = round(smartctl, SMART_TIMEOUT);
            producer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(lines);
            thread::sleep(SMART_INTERVAL.saturating_sub(started.elapsed()));
        }
    });
    let mut out = BufWriter::new(io::stdout().lock());
    loop {
        let started = Instant::now();
        registers(&mut out, &cpus)?;
        let health = mem::take(&mut *pending.lock().unwrap_or_else(PoisonError::into_inner));
        for line in health {
            writeln!(out, "{line}")?;
        }
        writeln!(out, "end")?;
        out.flush()?;
        thread::sleep(INTERVAL.saturating_sub(started.elapsed()));
    }
}

#[cfg(test)]
mod tests;
