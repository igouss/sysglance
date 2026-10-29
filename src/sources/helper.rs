use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    mem,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::Duration,
};

use sysglance_core::reading::{Category, Device, Reading, Unavailable, Value};

use super::{Source, msr, smart, text};

pub const COMMAND: [&str; 3] = ["sudo", "-n", "/usr/local/libexec/sysglance-helper"];
pub const GRANT: &str = "run install.sh: it installs sysglance-helper and lets wheel run it";

pub type Registers = BTreeMap<(u32, u32), u64>;
pub type Reports = BTreeMap<String, String>;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub registers: Registers,
    pub reports: Reports,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Register { cpu: u32, address: u32, value: u64 },
    Report { drive: String, json: String },
    End,
}

fn hex(field: &str) -> Option<u64> {
    u64::from_str_radix(field.strip_prefix("0x")?, 16).ok()
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
        "smart" => {
            let (drive, json) = rest.split_once(' ')?;
            Some(Line::Report {
                drive: drive.to_owned(),
                json: json.to_owned(),
            })
        }
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
        reports: Reports,
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
            reports: frame.reports,
        },
        (
            Link::Streaming {
                latest,
                mut reports,
                ..
            },
            Event::Read(frame),
        ) => {
            reports.extend(frame.reports);
            Link::Streaming {
                previous: Some(latest),
                latest: frame.registers,
                reports,
            }
        }
        (Link::Starting, Event::Ended(why)) => Link::Refused(why),
        (Link::Streaming { .. }, Event::Ended(why)) => Link::Lost(why),
        (ended @ (Link::Refused(_) | Link::Lost(_)), Event::Read(_) | Event::Ended(_)) => ended,
    }
}

fn advance(link: &Mutex<Link>, event: Event) {
    let mut guard = link.lock().unwrap_or_else(PoisonError::into_inner);
    *guard = evolve(mem::take(&mut *guard), event);
}

pub fn frames(reader: impl BufRead, link: &Mutex<Link>) {
    let mut frame = Frame::default();
    for line in reader.lines().map_while(Result::ok) {
        match parse(&line) {
            Some(Line::Register {
                cpu,
                address,
                value,
            }) => {
                frame.registers.insert((cpu, address), value);
            }
            Some(Line::Report { drive, json }) => {
                frame.reports.insert(drive, json);
            }
            Some(Line::End) => advance(link, Event::Read(mem::take(&mut frame))),
            None => {}
        }
    }
}

pub fn run(command: &[String], link: &Mutex<Link>) -> String {
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
        frames(BufReader::new(stdout), link);
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
    link: Arc<Mutex<Link>>,
    command: String,
    registers_decodable: bool,
    sys: PathBuf,
}

impl Helper {
    pub fn spawn(command: &[&str], proc: impl AsRef<Path>, sys: impl Into<PathBuf>) -> Self {
        let link: Arc<Mutex<Link>> = Arc::default();
        let shared = Arc::clone(&link);
        let owned: Vec<String> = command.iter().map(|part| (*part).to_owned()).collect();
        thread::spawn(move || {
            let why = run(&owned, &shared);
            advance(&shared, Event::Ended(why));
        });
        Self {
            link,
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
}

impl Source for Helper {
    fn collect(&mut self, _elapsed: Duration) -> Vec<Device> {
        let link = self.link.lock().unwrap_or_else(PoisonError::into_inner);
        match &*link {
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
                reports,
            } => {
                let mut devices = if self.registers_decodable {
                    msr::devices(&self.sys, previous.as_ref(), latest)
                } else {
                    vec![msr::undecodable()]
                };
                devices.extend(
                    reports
                        .iter()
                        .map(|(drive, json)| smart::device(drive, json)),
                );
                devices
            }
        }
    }
}

#[cfg(test)]
mod tests;
