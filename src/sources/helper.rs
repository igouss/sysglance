use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    mem,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use sysglance_core::reading::{Category, Device, Reading, Unavailable, Value};

use super::{Source, msr, smart, text};

pub const INSTALLED: &str = "/usr/local/libexec/sysglance-helper";
pub const GRANT: &str = "run install.sh: it lets your admin group run sysglance-helper";
pub const SILENCE: Duration = Duration::from_secs(5);

pub type Registers = BTreeMap<(u32, u32), u64>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Drive {
    pub path: String,
    pub kind: String,
}

pub type Reports = BTreeMap<Drive, Result<String, String>>;

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Health {
    #[default]
    Waiting,
    Drives(Reports),
    Unavailable(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub registers: Registers,
    pub health: Option<Health>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Register {
        cpu: u32,
        address: u32,
        value: u64,
    },
    Round,
    Report {
        drive: Drive,
        report: Result<String, String>,
    },
    HealthUnavailable(String),
    End,
}

fn hex(field: &str) -> Option<u64> {
    u64::from_str_radix(field.strip_prefix("0x")?, 16).ok()
}

fn report(rest: &str, ok: bool) -> Option<Line> {
    let (path, rest) = rest.split_once(' ')?;
    let (kind, body) = rest.split_once(' ')?;
    let body = body.to_owned();
    Some(Line::Report {
        drive: Drive {
            path: path.to_owned(),
            kind: kind.to_owned(),
        },
        report: if ok { Ok(body) } else { Err(body) },
    })
}

pub fn parse(line: &str) -> Option<Line> {
    let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
    match kind {
        "msr" => {
            let mut fields = rest.split(' ');
            let cpu = fields.next()?.parse().ok()?;
            let address = u32::try_from(hex(fields.next()?)?).ok()?;
            let value = hex(fields.next()?)?;
            fields.next().is_none().then_some(Line::Register {
                cpu,
                address,
                value,
            })
        }
        "smart-round" if rest.is_empty() => Some(Line::Round),
        "smart" => report(rest, true),
        "smart-error" => report(rest, false),
        "smart-unavailable" if !rest.is_empty() => Some(Line::HealthUnavailable(rest.to_owned())),
        "end" if rest.is_empty() => Some(Line::End),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Starting,
    Streaming {
        previous: Option<Registers>,
        latest: Registers,
        health: Health,
    },
    Refused(String),
    Lost(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Read(Frame),
    Ended(String),
}

pub fn evolve(link: Link, event: Event) -> Link {
    match (link, event) {
        (Link::Starting, Event::Read(frame)) => Link::Streaming {
            previous: None,
            latest: frame.registers,
            health: frame.health.unwrap_or_default(),
        },
        (Link::Streaming { latest, health, .. }, Event::Read(frame)) => Link::Streaming {
            previous: Some(latest),
            latest: frame.registers,
            health: frame.health.unwrap_or(health),
        },
        (Link::Starting, Event::Ended(why)) => Link::Refused(why),
        (Link::Streaming { .. }, Event::Ended(why)) => Link::Lost(why),
        (ended @ (Link::Refused(_) | Link::Lost(_)), Event::Read(_) | Event::Ended(_)) => ended,
    }
}

#[derive(Debug)]
pub struct Watched {
    pub link: Link,
    pub heard: Instant,
}

impl Watched {
    pub fn new(link: Link) -> Self {
        Self {
            link,
            heard: Instant::now(),
        }
    }
}

fn advance(watched: &Mutex<Watched>, event: Event) {
    let mut guard = watched.lock().unwrap_or_else(PoisonError::into_inner);
    if matches!(event, Event::Read(_)) {
        guard.heard = Instant::now();
    }
    guard.link = evolve(mem::take(&mut guard.link), event);
}

fn add(frame: &mut Frame, line: Line) -> bool {
    match line {
        Line::Register {
            cpu,
            address,
            value,
        } => {
            frame.registers.insert((cpu, address), value);
        }
        Line::Round => frame.health = Some(Health::Drives(Reports::new())),
        Line::Report { drive, report } => {
            if let Some(Health::Drives(reports)) = &mut frame.health {
                reports.insert(drive, report);
            }
        }
        Line::HealthUnavailable(why) => frame.health = Some(Health::Unavailable(why)),
        Line::End => return true,
    }
    false
}

pub fn frames(reader: impl BufRead, watched: &Mutex<Watched>) {
    let mut frame = Frame::default();
    for line in reader.lines().map_while(Result::ok) {
        if parse(&line).is_some_and(|line| add(&mut frame, line)) {
            advance(watched, Event::Read(mem::take(&mut frame)));
        }
    }
}

pub fn run(command: &[String], watched: &Mutex<Watched>) -> String {
    let Some((program, arguments)) = command.split_first() else {
        return "no helper command".to_owned();
    };
    let mut child = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return format!("{program}: {e}"),
    };
    if let Some(stdout) = child.stdout.take() {
        frames(BufReader::new(stdout), watched);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).ok();
    }
    let status = child
        .wait()
        .map_or_else(|e| e.to_string(), |status| status.to_string());
    stderr
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map_or(status, str::to_owned)
}

pub struct Helper {
    watched: Arc<Mutex<Watched>>,
    command: String,
    registers_decodable: bool,
    sys: PathBuf,
}

impl Helper {
    pub fn spawn(helper: &Path, proc: impl AsRef<Path>, sys: impl Into<PathBuf>) -> Self {
        let command: Vec<String> = vec![
            "sudo".to_owned(),
            "-n".to_owned(),
            helper.display().to_string(),
        ];
        let watched = if helper.is_file() {
            let watched = Arc::new(Mutex::new(Watched::new(Link::Starting)));
            let shared = Arc::clone(&watched);
            let owned = command.clone();
            thread::spawn(move || {
                let why = run(&owned, &shared);
                advance(&shared, Event::Ended(why));
            });
            watched
        } else {
            Arc::new(Mutex::new(Watched::new(Link::Refused(format!(
                "{} is not installed",
                helper.display()
            )))))
        };
        Self {
            watched,
            command: command.join(" "),
            registers_decodable: text(&proc.as_ref().join("cpuinfo"))
                .is_ok_and(|cpuinfo| msr::decodable(&cpuinfo)),
            sys: sys.into(),
        }
    }

    fn status(&self, value: Value, origin: String) -> Vec<Device> {
        let origin = if origin.is_empty() {
            self.command.clone()
        } else {
            origin
        };
        vec![Device {
            key: "helper".to_owned(),
            title: "Privileged helper".to_owned(),
            detail: "MSRs and SMART".to_owned(),
            category: Category::System,
            readings: vec![Reading {
                key: "helper/status".to_owned(),
                label: "status".to_owned(),
                value,
                origin,
            }],
        }]
    }

    fn streaming(
        &self,
        previous: Option<&Registers>,
        latest: &Registers,
        health: &Health,
    ) -> Vec<Device> {
        let mut devices = if self.registers_decodable {
            msr::devices(&self.sys, previous, latest)
        } else {
            vec![msr::undecodable()]
        };
        match health {
            Health::Waiting => {}
            Health::Drives(reports) => devices.extend(reports.iter().map(|(drive, report)| {
                smart::device(
                    drive,
                    report.as_ref().map(String::as_str).map_err(String::as_str),
                )
            })),
            Health::Unavailable(why) => devices.push(smart::unavailable(why)),
        }
        devices
    }
}

impl Source for Helper {
    #[expect(
        clippy::significant_drop_tightening,
        reason = "devices are decoded from the link in place; the reader waits at most one decode"
    )]
    fn collect(&mut self, _elapsed: Duration) -> Vec<Device> {
        let watched = self.watched.lock().unwrap_or_else(PoisonError::into_inner);
        let silent = watched.heard.elapsed() > SILENCE;
        match &watched.link {
            Link::Starting | Link::Streaming { .. } if silent => self.status(
                Value::Unavailable(Unavailable::Unreadable(format!(
                    "the helper has sent nothing for {} s",
                    watched.heard.elapsed().as_secs()
                ))),
                String::new(),
            ),
            Link::Starting => self.status(Value::Unavailable(Unavailable::Pending), String::new()),
            Link::Refused(why) => self.status(
                Value::Unavailable(Unavailable::NeedsPrivilege(GRANT)),
                why.clone(),
            ),
            Link::Lost(why) => self.status(
                Value::Unavailable(Unavailable::Unreadable(format!("helper exited: {why}"))),
                String::new(),
            ),
            Link::Streaming {
                previous,
                latest,
                health,
            } => self.streaming(previous.as_ref(), latest, health),
        }
    }
}

#[cfg(test)]
mod tests;
